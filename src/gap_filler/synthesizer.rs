/// Synthesize vessel_status and environmental_data records from raw backup log entries.
///
/// Each call to `synthesize_gap` produces one `SyntheticPeriod` per time interval
/// (moored interval or underway interval, as configured) within the gap window.
/// Intervals with fewer than `MIN_SAMPLES_FOR_VALIDATION` GNSS fixes are skipped.
use std::time::{Duration, SystemTime};
use std::time::Instant;

use crate::db::operations::gap_fill::{systemtime_to_millis, millis_to_systemtime};
use crate::db::types::VesselStatusOperation;
use crate::environmental_monitor::{MetricData, MetricId};
use crate::position_utils::Position;
use crate::utilities::{
    average_angle, calculate_true_wind, haversine_distance_nm, haversine_heading,
    normalize0_360, EngineStatus,
};

use super::log_parser::LogEntry;

/// A synthesised period ready to be written to the database.
pub struct SyntheticPeriod {
    pub vessel_status: VesselStatusOperation,
    /// Environmental metrics present in this interval. Only non-empty metrics are included.
    pub env_metrics: Vec<(MetricId, MetricData)>,
}

const MIN_SAMPLES_FOR_VALIDATION: usize = 10;
const MAX_VALID_SOG_KN: f64 = 25.0;
/// Mooring detection: fraction of positions that must be within `MOORING_RADIUS_NM` of the
/// median position for the vessel to be considered stationary.
const MOORING_ACCURACY: f64 = 0.85;
/// 50 m in nautical miles
const MOORING_RADIUS_NM: f64 = 50.0 / 1852.0;

/// Convert `SystemTime` → `Instant` (approximate, relative to now).
fn st_to_instant(st: SystemTime) -> Instant {
    st.elapsed()
        .map(|d| Instant::now() - d)
        .unwrap_or_else(|_| Instant::now())
}

// ---- Median position -------------------------------------------------------

fn median_position(positions: &[(f64, f64)]) -> Option<Position> {
    if positions.is_empty() {
        return None;
    }
    let mut lats: Vec<f64> = positions.iter().map(|p| p.0).collect();
    let mut lons: Vec<f64> = positions.iter().map(|p| p.1).collect();
    lats.sort_by(|a, b| a.partial_cmp(b).unwrap());
    lons.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mid = lats.len() / 2;
    let lat = if lats.len() % 2 == 0 {
        (lats[mid - 1] + lats[mid]) / 2.0
    } else {
        lats[mid]
    };
    let lon = if lons.len() % 2 == 0 {
        (lons[mid - 1] + lons[mid]) / 2.0
    } else {
        lons[mid]
    };
    Some(Position { latitude: lat, longitude: lon })
}

// ---- Simple statistics helpers --------------------------------------------

fn opt_avg(v: &[f64]) -> Option<f64> {
    if v.is_empty() {
        None
    } else {
        Some(v.iter().sum::<f64>() / v.len() as f64)
    }
}

fn opt_max(v: &[f64]) -> Option<f64> {
    v.iter().cloned().reduce(f64::max)
}

fn opt_min(v: &[f64]) -> Option<f64> {
    v.iter().cloned().reduce(f64::min)
}

fn metric_data(v: &[f64]) -> Option<MetricData> {
    if v.is_empty() {
        None
    } else {
        Some(MetricData {
            avg: opt_avg(v),
            max: opt_max(v),
            min: opt_min(v),
            count: Some(v.len()),
        })
    }
}

// ---- Per-interval synthesis -----------------------------------------------

/// Synthesize a single time interval `[t_start, t_end]` from the provided log entries
/// (which must already be filtered/sorted to the gap window).
///
/// `prev_position` is the effective position of the record that preceded this interval
/// (used for COG/distance/speed calculation).
/// `prev_heading` is the heading from the previous interval or the "before" record.
///
/// Returns `None` if the interval has fewer than `MIN_SAMPLES_FOR_VALIDATION` GNSS fixes.
fn synthesize_interval(
    entries: &[LogEntry],
    t_start: SystemTime,
    t_end: SystemTime,
    prev_position: Option<(Position, SystemTime)>,
    prev_heading: Option<f64>,
) -> Option<SyntheticPeriod> {
    // ---- Collect entries for this interval ---------------------------------
    let interval_entries: Vec<&LogEntry> = entries
        .iter()
        .filter(|e| {
            let ts = e.timestamp();
            ts >= t_start && ts < t_end
        })
        .collect();

    // ---- GNSS samples -----------------------------------------------------
    let gnss_samples: Vec<(f64, f64, SystemTime)> = interval_entries
        .iter()
        .filter_map(|e| {
            if let LogEntry::Gnss(g) = e {
                Some((g.latitude, g.longitude, g.timestamp))
            } else {
                None
            }
        })
        .collect();

    if gnss_samples.len() < MIN_SAMPLES_FOR_VALIDATION {
        return None;
    }

    // ---- Mooring detection ------------------------------------------------
    let positions_2d: Vec<(f64, f64)> =
        gnss_samples.iter().map(|g| (g.0, g.1)).collect();
    let median_pos = median_position(&positions_2d)?;

    let within_radius = gnss_samples.iter().filter(|g| {
        haversine_distance_nm(g.0, g.1, median_pos.latitude, median_pos.longitude)
            <= MOORING_RADIUS_NM
    }).count();
    let is_moored = (within_radius as f64 / gnss_samples.len() as f64) >= MOORING_ACCURACY;

    // ---- Effective position -----------------------------------------------
    let current_position = if is_moored {
        median_pos
    } else {
        // Last GNSS fix
        let last = gnss_samples.last().unwrap();
        Position { latitude: last.0, longitude: last.1 }
    };

    // ---- SOG per consecutive GNSS pair ------------------------------------
    let mut sog_values: Vec<f64> = Vec::new();
    let mut avg_sog_for_wind = 0.0_f64;
    for w in gnss_samples.windows(2) {
        let (lat1, lon1, ts1) = w[0];
        let (lat2, lon2, ts2) = w[1];
        let dt_ms = ts2.duration_since(ts1).unwrap_or(Duration::ZERO).as_millis() as f64;
        if dt_ms > 0.0 {
            let dist_nm = haversine_distance_nm(lat1, lon1, lat2, lon2);
            let sog = (dist_nm / (dt_ms / 3_600_000.0)).min(MAX_VALID_SOG_KN);
            sog_values.push(sog);
        }
    }
    if !sog_values.is_empty() {
        avg_sog_for_wind = sog_values.iter().sum::<f64>() / sog_values.len() as f64;
    }
    let max_speed_kn = opt_max(&sog_values).unwrap_or(0.0);

    // ---- Wind -------------------------------------------------------------
    let mut tws_values: Vec<f64> = Vec::new();
    let mut twa_values: Vec<f64> = Vec::new();
    for e in &interval_entries {
        if let LogEntry::Wind(w) = e {
            let (tws, twa) = calculate_true_wind(w.speed_kn, w.angle_deg, avg_sog_for_wind);
            tws_values.push(tws);
            twa_values.push(twa);
        }
    }
    let wind_speed_kn = opt_avg(&tws_values);
    let wind_angle_deg = if twa_values.is_empty() {
        None
    } else {
        Some(normalize0_360(average_angle(&twa_values)))
    };

    // ---- Engine status ----------------------------------------------------
    let rpm_values: Vec<f64> = interval_entries
        .iter()
        .filter_map(|e| if let LogEntry::Rpme(r) = e { Some(r.rpm) } else { None })
        .collect();
    let engine_on = if rpm_values.is_empty() {
        EngineStatus::Unknown
    } else if rpm_values.iter().any(|&r| r > 100.0) {
        EngineStatus::On
    } else {
        EngineStatus::Off
    };

    // ---- Timing & distance ------------------------------------------------
    let total_time_ms = duration_between_sys(t_start, t_end).as_millis() as u64;

    let (total_distance_nm, cog_deg, average_speed_kn) =
        if let Some((prev_pos, _prev_ts)) = prev_position {
            let dist = haversine_distance_nm(
                prev_pos.latitude, prev_pos.longitude,
                current_position.latitude, current_position.longitude,
            );
            let cog = haversine_heading(
                prev_pos.latitude, prev_pos.longitude,
                current_position.latitude, current_position.longitude,
            );
            let speed = if total_time_ms > 0 {
                dist / (total_time_ms as f64 / 3_600_000.0)
            } else {
                0.0
            };
            (dist, Some(cog), speed.min(MAX_VALID_SOG_KN))
        } else {
            (0.0, None, 0.0)
        };

    // ---- Heading ----------------------------------------------------------
    // Underway: approximate with COG.
    // Moored: carry forward prev_heading.
    let average_heading_deg = if !is_moored {
        cog_deg
    } else {
        prev_heading
    };

    // ---- Environmental data -----------------------------------------------
    // Pressure: mbar × 100 → Pa
    let pres_pa: Vec<f64> = interval_entries
        .iter()
        .filter_map(|e| if let LogEntry::Pres(p) = e { Some(p.pressure_mbar * 100.0) } else { None })
        .collect();

    let cabt_c: Vec<f64> = interval_entries
        .iter()
        .filter_map(|e| if let LogEntry::CabT(c) = e { Some(c.temp_c) } else { None })
        .collect();

    let humi_pct: Vec<f64> = interval_entries
        .iter()
        .filter_map(|e| if let LogEntry::HumI(h) = e { Some(h.humidity_pct) } else { None })
        .collect();

    // Roll: each log entry gives (min, max); we compute avg=(v1+v2)/2 per entry, then aggregate
    let mut roll_avgs: Vec<f64> = Vec::new();
    let mut roll_maxs: Vec<f64> = Vec::new();
    let mut roll_mins: Vec<f64> = Vec::new();
    for e in &interval_entries {
        if let LogEntry::Roll(r) = e {
            roll_avgs.push((r.val1 + r.val2) / 2.0);
            roll_maxs.push(r.val1.max(r.val2));
            roll_mins.push(r.val1.min(r.val2));
        }
    }
    let roll_metric = if roll_avgs.is_empty() {
        None
    } else {
        Some(MetricData {
            avg: opt_avg(&roll_avgs),
            max: opt_max(&roll_maxs),
            min: opt_min(&roll_mins),
            count: Some(roll_avgs.len()),
        })
    };

    let mut env_metrics: Vec<(MetricId, MetricData)> = Vec::new();
    if let Some(d) = metric_data(&pres_pa)    { env_metrics.push((MetricId::Pressure,  d)); }
    if let Some(d) = metric_data(&cabt_c)     { env_metrics.push((MetricId::CabinTemp, d)); }
    if let Some(d) = metric_data(&humi_pct)   { env_metrics.push((MetricId::Humidity,  d)); }
    if let Some(d) = metric_data(&tws_values) { env_metrics.push((MetricId::WindSpeed, d)); }
    if let Some(d) = roll_metric              { env_metrics.push((MetricId::Roll,       d)); }
    // WindDir is omitted: no heading available in backup logs.

    Some(SyntheticPeriod {
        vessel_status: VesselStatusOperation {
            timestamp: st_to_instant(t_end),
            position: current_position,
            average_speed_kn,
            max_speed_kn,
            is_moored,
            engine_on,
            total_distance_nm,
            total_time_ms,
            wind_speed_kn,
            wind_speed_variance: None,
            wind_angle_deg: wind_angle_deg,
            wind_angle_variance: None,
            cog_deg,
            average_heading_deg,
        },
        env_metrics,
    })
}

fn duration_between_sys(a: SystemTime, b: SystemTime) -> Duration {
    b.duration_since(a).unwrap_or(Duration::ZERO)
}

// ---- Public entry point ---------------------------------------------------

/// Split the gap window `[gap_start, gap_end]` into intervals of `interval` duration,
/// synthesise each interval from the provided log `entries`, and return the results.
///
/// `prev_position`: position+timestamp of the record immediately before the gap.
/// `prev_heading`:  heading of the record immediately before the gap (or `None`).
///
/// Intervals with fewer than `MIN_SAMPLES_FOR_VALIDATION` GNSS fixes are silently skipped.
pub fn synthesize_gap(
    entries: &[LogEntry],
    gap_start: SystemTime,
    gap_end: SystemTime,
    interval: Duration,
    prev_position: Option<(Position, SystemTime)>,
    prev_heading: Option<f64>,
) -> Vec<SyntheticPeriod> {
    let mut results = Vec::new();

    let mut current_prev_pos = prev_position;
    let mut current_prev_heading = prev_heading;

    let gap_start_ms = systemtime_to_millis(gap_start);
    let gap_end_ms = systemtime_to_millis(gap_end);
    let interval_ms = interval.as_millis() as u64;

    if interval_ms == 0 {
        return results;
    }

    let mut t_start_ms = gap_start_ms;
    while t_start_ms < gap_end_ms {
        let t_end_ms = (t_start_ms + interval_ms).min(gap_end_ms);
        let t_start = millis_to_systemtime(t_start_ms);
        let t_end = millis_to_systemtime(t_end_ms);

        if let Some(period) = synthesize_interval(
            entries,
            t_start,
            t_end,
            current_prev_pos,
            current_prev_heading,
        ) {
            // Update prev_position / prev_heading for the next interval
            current_prev_pos = Some((period.vessel_status.position, t_end));
            current_prev_heading = period.vessel_status.average_heading_deg;
            results.push(period);
        }

        t_start_ms = t_end_ms;
    }

    results
}

// ---- tests ----------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::gap_filler::log_parser::{GnssEntry, WindEntry, LogEntry};
    use std::time::UNIX_EPOCH;

    fn make_gnss(lat: f64, lon: f64, offset_secs: u64) -> LogEntry {
        LogEntry::Gnss(GnssEntry {
            timestamp: UNIX_EPOCH + Duration::from_secs(1_744_300_000 + offset_secs),
            latitude: lat,
            longitude: lon,
        })
    }

    fn make_wind(speed_kn: f64, angle_deg: f64, offset_secs: u64) -> LogEntry {
        LogEntry::Wind(WindEntry {
            timestamp: UNIX_EPOCH + Duration::from_secs(1_744_300_000 + offset_secs),
            speed_kn,
            angle_deg,
        })
    }

    #[test]
    fn test_synthesize_skips_insufficient_gnss() {
        let base = UNIX_EPOCH + Duration::from_secs(1_744_300_000);
        let entries: Vec<LogEntry> = (0..5).map(|i| make_gnss(42.96, 9.45, i)).collect();
        let result = synthesize_gap(
            &entries,
            base,
            base + Duration::from_secs(300),
            Duration::from_secs(300),
            None,
            None,
        );
        assert!(result.is_empty(), "should skip interval with < 10 GNSS fixes");
    }

    #[test]
    fn test_synthesize_produces_moored_period() {
        let base = UNIX_EPOCH + Duration::from_secs(1_744_300_000);
        // 15 GNSS entries clustered at ~same position (moored)
        let mut entries: Vec<LogEntry> = (0..15)
            .map(|i| make_gnss(42.9590 + i as f64 * 0.000001, 9.45537, i * 2))
            .collect();
        // Add some wind entries
        entries.push(make_wind(4.0, 330.0, 5));
        entries.push(make_wind(3.5, 320.0, 10));

        let prev_pos = Position { latitude: 42.9589, longitude: 9.4553 };

        let result = synthesize_gap(
            &entries,
            base,
            base + Duration::from_secs(300),
            Duration::from_secs(300),
            Some((prev_pos, base)),
            Some(270.0),
        );

        assert_eq!(result.len(), 1);
        let p = &result[0];
        assert!(p.vessel_status.is_moored, "expected moored");
        // Heading when moored should be carried from prev_heading
        assert_eq!(p.vessel_status.average_heading_deg, Some(270.0));
        // Wind should be present
        assert!(p.vessel_status.wind_speed_kn.is_some());
    }

    #[test]
    fn test_synthesize_heading_underway_uses_cog() {
        let base = UNIX_EPOCH + Duration::from_secs(1_744_300_000);
        // Spread positions widely → underway
        let mut entries: Vec<LogEntry> = (0..15)
            .map(|i| make_gnss(42.90 + i as f64 * 0.01, 9.40 + i as f64 * 0.01, i * 2))
            .collect();
        entries.push(make_wind(5.0, 45.0, 5));

        let prev_pos = Position { latitude: 42.85, longitude: 9.35 };

        let result = synthesize_gap(
            &entries,
            base,
            base + Duration::from_secs(300),
            Duration::from_secs(300),
            Some((prev_pos, base)),
            Some(90.0),
        );

        assert_eq!(result.len(), 1);
        let p = &result[0];
        assert!(!p.vessel_status.is_moored, "expected underway");
        // Heading should equal COG, not the previous 90°
        assert_eq!(p.vessel_status.average_heading_deg, p.vessel_status.cog_deg);
    }
}
