// Mooring correction — fix a mislabeled `is_moored` period and, when the correction
// switches the window to moored, resample the (now dense, underway-cadence) rows down
// to the moored reporting cadence. See DB_ANALYST.md for the manual protocol this
// automates and AGENTS.md for the REST endpoint.
use crate::db::types::VesselDatabase;
use crate::error::AppError;
use crate::position_utils::Position;
use crate::utilities::average_angle;
use chrono::{DateTime, Utc};
use mysql::params;
use mysql::prelude::Queryable;
use tracing::warn;

const MYSQL_TS_FMT: &str = "%Y-%m-%d %H:%M:%S%.3f";

#[derive(Debug, Clone, serde::Serialize)]
pub struct FixMooringReport {
    pub trip_id: u32,
    pub rows_matched: u64,
    pub rows_after: u64,
    pub resampled: bool,
}

struct RawSample {
    ts: DateTime<Utc>,
    position: Position,
    max_speed_kn: f64,
    engine_on: u8,
    average_heading_deg: Option<f64>,
    average_wind_speed_kn: Option<f64>,
    average_wind_angle_deg: Option<f64>,
}

struct ResampledRow {
    timestamp: DateTime<Utc>,
    position: Position,
    average_speed_kn: f64,
    max_speed_kn: f64,
    cog_deg: f64,
    engine_on: u8,
    average_heading_deg: Option<f64>,
    total_distance_nm: f64,
    total_time_ms: u64,
    average_wind_speed_kn: Option<f64>,
    average_wind_angle_deg: Option<f64>,
}

fn parse_mysql_dt(s: &str) -> Result<DateTime<Utc>, AppError> {
    let ndt = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f")
        .map_err(|e| AppError::Parse(format!("Invalid timestamp '{}': {}", s, e)))?;
    Ok(DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc))
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = values.len();
    let mid = n / 2;
    if n % 2 == 0 {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

fn median_position(samples: &[&RawSample]) -> Position {
    let mut lats: Vec<f64> = samples.iter().map(|s| s.position.latitude).collect();
    let mut lons: Vec<f64> = samples.iter().map(|s| s.position.longitude).collect();
    Position {
        latitude: median(&mut lats),
        longitude: median(&mut lons),
    }
}

/// Most frequent `engine_on` value in the bucket (ties broken by the last sample —
/// the most recent state wins, same tie-break the live monitor would settle on).
fn mode_engine_on(bucket: &[&RawSample]) -> u8 {
    let mut counts = [0usize; 3]; // 0=off, 1=on, 2=unknown
    for s in bucket {
        if (s.engine_on as usize) < 3 {
            counts[s.engine_on as usize] += 1;
        }
    }
    let max_count = *counts.iter().max().unwrap_or(&0);
    bucket
        .iter()
        .rev()
        .map(|s| s.engine_on)
        .find(|v| counts.get(*v as usize).copied().unwrap_or(0) == max_count)
        .unwrap_or(2)
}

/// Bucket dense samples into `interval_secs`-sized groups (cumulative from `seed_ts`),
/// producing one resampled row per bucket. Mirrors the moored-interval reporting cadence
/// `VesselMonitor` would have produced had mooring been detected correctly at the time:
/// median position per bucket (a stable reference point while at anchor), vector
/// distance/time/course computed from the *previous bucket's* position instead of
/// summing the dense samples' own jittery per-sample distances (which is what turns GPS
/// noise into a spurious "sailed" distance in the first place), circular-mean
/// heading/wind-angle (project rule: never a plain arithmetic mean of angles),
/// arithmetic-mean wind speed, and the max of `max_speed_kn` seen in the bucket. The
/// final bucket keeps the exact position of the true last sample (not a median) so the
/// row immediately after the corrected window — whose own total_distance_nm/
/// total_time_ms already reference that exact position — stays internally consistent
/// without a separate fix-up.
fn resample_buckets(
    samples: &[RawSample],
    seed_position: Position,
    seed_ts: DateTime<Utc>,
    interval_secs: i64,
) -> Vec<ResampledRow> {
    if samples.is_empty() {
        return Vec::new();
    }

    let mut buckets: Vec<Vec<&RawSample>> = Vec::new();
    let mut current: Vec<&RawSample> = Vec::new();
    let mut next_boundary = seed_ts + chrono::Duration::seconds(interval_secs);
    for s in samples {
        if s.ts > next_boundary && !current.is_empty() {
            buckets.push(std::mem::take(&mut current));
            while s.ts > next_boundary {
                next_boundary += chrono::Duration::seconds(interval_secs);
            }
        }
        current.push(s);
    }
    if !current.is_empty() {
        buckets.push(current);
    }

    let last_bucket_idx = buckets.len() - 1;
    let mut rows = Vec::with_capacity(buckets.len());
    let mut prev_position = seed_position;
    let mut prev_ts = seed_ts;

    for (i, bucket) in buckets.iter().enumerate() {
        let position = if i == last_bucket_idx {
            bucket.last().unwrap().position
        } else {
            median_position(bucket)
        };
        let ts = bucket.last().unwrap().ts;
        let total_distance_nm = prev_position.distance_to_nm(&position);
        let total_time_ms = (ts - prev_ts).num_milliseconds().max(0) as u64;
        let average_speed_kn = if total_time_ms > 0 {
            total_distance_nm / (total_time_ms as f64 / 3_600_000.0)
        } else {
            0.0
        };
        let cog_deg = prev_position.course_from_deg(&position);
        let max_speed_kn = bucket
            .iter()
            .map(|s| s.max_speed_kn)
            .fold(0.0_f64, f64::max);
        let engine_on = mode_engine_on(bucket);
        let headings: Vec<f64> = bucket
            .iter()
            .filter_map(|s| s.average_heading_deg)
            .collect();
        let average_heading_deg = if headings.is_empty() {
            None
        } else {
            Some(average_angle(&headings))
        };
        let wind_speeds: Vec<f64> = bucket
            .iter()
            .filter_map(|s| s.average_wind_speed_kn)
            .collect();
        let average_wind_speed_kn = if wind_speeds.is_empty() {
            None
        } else {
            Some(wind_speeds.iter().sum::<f64>() / wind_speeds.len() as f64)
        };
        let wind_angles: Vec<f64> = bucket
            .iter()
            .filter_map(|s| s.average_wind_angle_deg)
            .collect();
        let average_wind_angle_deg = if wind_angles.is_empty() {
            None
        } else {
            Some(average_angle(&wind_angles))
        };

        rows.push(ResampledRow {
            timestamp: ts,
            position,
            average_speed_kn,
            max_speed_kn,
            cog_deg,
            engine_on,
            average_heading_deg,
            total_distance_nm,
            total_time_ms,
            average_wind_speed_kn,
            average_wind_angle_deg,
        });

        prev_position = position;
        prev_ts = ts;
    }

    rows
}

impl VesselDatabase {
    /// Correct a mislabeled mooring period: set `is_moored` for every `vessel_status`
    /// row in `[start, end]` (clamped to the covering trip's own window) to `is_moored`.
    /// When the target is `true`, also resample the window down to `moored_interval_secs`
    /// cadence (see `resample_buckets`) — dense underway-rate rows left under a moored
    /// label would otherwise accumulate GPS jitter into a spurious travelled distance.
    /// Recomputes the trip's aggregate totals (never its start/end timestamps) and
    /// invalidates `trip_legs_cache` / `heatmap_cache` for the affected range.
    pub fn fix_mooring_status(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        is_moored: bool,
        moored_interval_secs: u64,
    ) -> Result<FixMooringReport, AppError> {
        if start >= end {
            return Err(AppError::Parse(
                "start_timestamp must be before end_timestamp".to_string(),
            ));
        }

        let mut conn = self.pool.get_conn()?;

        // Find the trip that covers this window; aggregates are recomputed against its
        // own start/end below.
        let trip_row: Option<mysql::Row> = conn.exec_first(
            r"SELECT id,
                     DATE_FORMAT(start_timestamp, '%Y-%m-%d %H:%i:%S.%f') AS start_ts,
                     DATE_FORMAT(end_timestamp,   '%Y-%m-%d %H:%i:%S.%f') AS end_ts
              FROM trips
              WHERE start_timestamp <= :end AND end_timestamp >= :start
              ORDER BY id DESC LIMIT 1",
            params! {
                "start" => start.format(MYSQL_TS_FMT).to_string(),
                "end" => end.format(MYSQL_TS_FMT).to_string(),
            },
        )?;

        let mut trip_row = trip_row.ok_or_else(|| {
            AppError::Database("No trip covers the requested time range".to_string())
        })?;
        let trip_id: u32 = trip_row
            .take("id")
            .ok_or(AppError::Database("missing id".to_string()))?;
        let trip_start_str: String = trip_row
            .take("start_ts")
            .ok_or(AppError::Database("missing start_ts".to_string()))?;
        let trip_end_str: String = trip_row
            .take("end_ts")
            .ok_or(AppError::Database("missing end_ts".to_string()))?;
        let trip_start_dt = parse_mysql_dt(&trip_start_str)?;
        let trip_end_dt = parse_mysql_dt(&trip_end_str)?;

        let clamped_start = start.max(trip_start_dt);
        let clamped_end = end.min(trip_end_dt);
        if clamped_start >= clamped_end {
            return Err(AppError::Parse(
                "Requested range does not overlap the trip".to_string(),
            ));
        }

        // fetch_track serves timestamps with the milliseconds zeroed and callers echo
        // that string back as the end bound; treat a whole-second end bound as covering
        // that entire second so the point actually selected isn't excluded. Mirrors
        // correct_engine_status.
        let query_end = if clamped_end.timestamp_subsec_millis() == 0 {
            clamped_end + chrono::Duration::milliseconds(999)
        } else {
            clamped_end
        };

        let start_str = clamped_start.format(MYSQL_TS_FMT).to_string();
        let end_str = query_end.format(MYSQL_TS_FMT).to_string();

        let mut tx = conn.start_transaction(mysql::TxOpts::default())?;

        let rows_matched: u64 = tx
            .exec_first(
                "SELECT COUNT(*) FROM vessel_status WHERE timestamp BETWEEN :start AND :end",
                params! { "start" => &start_str, "end" => &end_str },
            )?
            .unwrap_or(0);
        if rows_matched == 0 {
            // Dropping `tx` rolls back; nothing has been written yet.
            return Err(AppError::Database(
                "No vessel_status rows found in the requested range".to_string(),
            ));
        }

        let mut rows_after = rows_matched;

        if is_moored {
            let seed: Option<mysql::Row> = tx.exec_first(
                r"SELECT latitude, longitude, DATE_FORMAT(timestamp, '%Y-%m-%d %H:%i:%S.%f') AS ts
                  FROM vessel_status WHERE timestamp < :start ORDER BY timestamp DESC LIMIT 1",
                params! { "start" => &start_str },
            )?;

            let sample_rows: Vec<mysql::Row> = tx.exec(
                r"SELECT DATE_FORMAT(timestamp, '%Y-%m-%d %H:%i:%S.%f') AS ts,
                         latitude, longitude, max_speed_kn, engine_on,
                         average_heading_deg, average_wind_speed_kn, average_wind_angle_deg
                  FROM vessel_status WHERE timestamp BETWEEN :start AND :end ORDER BY timestamp ASC",
                params! { "start" => &start_str, "end" => &end_str },
            )?;

            let mut samples = Vec::with_capacity(sample_rows.len());
            for mut row in sample_rows {
                let ts_str: String = row
                    .take("ts")
                    .ok_or(AppError::Database("missing ts".to_string()))?;
                let latitude: f64 = row
                    .take("latitude")
                    .ok_or(AppError::Database("missing latitude".to_string()))?;
                let longitude: f64 = row
                    .take("longitude")
                    .ok_or(AppError::Database("missing longitude".to_string()))?;
                let max_speed_kn: f64 = row.take("max_speed_kn").unwrap_or(0.0);
                let engine_on: u8 = row.take("engine_on").unwrap_or(2);
                let average_heading_deg: Option<f64> = row
                    .get_opt("average_heading_deg")
                    .and_then(|r| r.ok());
                let average_wind_speed_kn: Option<f64> = row
                    .get_opt("average_wind_speed_kn")
                    .and_then(|r| r.ok());
                let average_wind_angle_deg: Option<f64> = row
                    .get_opt("average_wind_angle_deg")
                    .and_then(|r| r.ok());
                samples.push(RawSample {
                    ts: parse_mysql_dt(&ts_str)?,
                    position: Position { latitude, longitude },
                    max_speed_kn,
                    engine_on,
                    average_heading_deg,
                    average_wind_speed_kn,
                    average_wind_angle_deg,
                });
            }

            let (seed_position, seed_ts) = match seed {
                Some(mut r) => {
                    let latitude: f64 = r.take("latitude").unwrap_or(samples[0].position.latitude);
                    let longitude: f64 = r.take("longitude").unwrap_or(samples[0].position.longitude);
                    let ts_str: String = r
                        .take("ts")
                        .ok_or(AppError::Database("missing ts".to_string()))?;
                    (Position { latitude, longitude }, parse_mysql_dt(&ts_str)?)
                }
                // No prior row exists at all (this is the very first vessel_status ever
                // recorded) — seed from the window's own first sample so the first
                // bucket reports zero distance/time instead of an artificial jump.
                None => (samples[0].position, samples[0].ts),
            };

            let resampled_rows =
                resample_buckets(&samples, seed_position, seed_ts, moored_interval_secs as i64);
            rows_after = resampled_rows.len() as u64;

            tx.exec_drop(
                "DELETE FROM vessel_status WHERE timestamp BETWEEN :start AND :end",
                params! { "start" => &start_str, "end" => &end_str },
            )?;

            for row in &resampled_rows {
                tx.exec_drop(
                    r"INSERT INTO vessel_status
                        (timestamp, latitude, longitude, average_speed_kn, max_speed_kn,
                         is_moored, engine_on, total_distance_nm, total_time_ms,
                         average_wind_speed_kn, average_wind_angle_deg, cog_deg, average_heading_deg)
                      VALUES
                        (:timestamp, :latitude, :longitude, :avg_speed, :max_speed,
                         1, :engine_on, :total_distance, :total_time,
                         :avg_wind_speed, :avg_wind_angle, :cog_deg, :avg_heading_deg)",
                    params! {
                        "timestamp" => row.timestamp.format(MYSQL_TS_FMT).to_string(),
                        "latitude" => row.position.latitude,
                        "longitude" => row.position.longitude,
                        "avg_speed" => row.average_speed_kn,
                        "max_speed" => row.max_speed_kn,
                        "engine_on" => row.engine_on,
                        "total_distance" => row.total_distance_nm,
                        "total_time" => row.total_time_ms,
                        "avg_wind_speed" => row.average_wind_speed_kn,
                        "avg_wind_angle" => row.average_wind_angle_deg,
                        "cog_deg" => row.cog_deg,
                        "avg_heading_deg" => row.average_heading_deg,
                    },
                )?;
            }
        } else {
            tx.exec_drop(
                "UPDATE vessel_status SET is_moored = 0 WHERE timestamp BETWEEN :start AND :end",
                params! { "start" => &start_str, "end" => &end_str },
            )?;
        }

        // Recompute the trip's aggregates over its own full window (same CASE semantics
        // as correct_engine_status / recalculate_and_update_trip), never touching
        // trips.start_timestamp / end_timestamp.
        let agg_row: Option<mysql::Row> = tx.exec_first(
            r"SELECT
                  SUM(CASE WHEN is_moored = 1 THEN total_time_ms  ELSE 0 END) AS time_moored,
                  SUM(CASE WHEN is_moored = 0 AND engine_on = 1 THEN total_time_ms  ELSE 0 END) AS time_motoring,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 THEN total_time_ms ELSE 0 END) AS time_sailing,
                  SUM(CASE WHEN is_moored = 0 AND engine_on = 1 THEN total_distance_nm ELSE 0 END) AS dist_motoring,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 THEN total_distance_nm ELSE 0 END) AS dist_sailed
              FROM vessel_status
              WHERE timestamp BETWEEN :start AND :end",
            params! { "start" => &trip_start_str, "end" => &trip_end_str },
        )?;

        if let Some(row) = agg_row {
            let time_moored: u64 = row.get("time_moored").unwrap_or(0);
            let time_motoring: u64 = row.get("time_motoring").unwrap_or(0);
            let time_sailing: u64 = row.get("time_sailing").unwrap_or(0);
            let dist_motoring: f64 = row.get("dist_motoring").unwrap_or(0.0);
            let dist_sailed: f64 = row.get("dist_sailed").unwrap_or(0.0);

            tx.exec_drop(
                r"UPDATE trips
                  SET total_time_moored       = :time_moored,
                      total_time_motoring     = :time_motoring,
                      total_time_sailing      = :time_sailing,
                      total_distance_motoring = :dist_motoring,
                      total_distance_sailed   = :dist_sailed
                  WHERE id = :trip_id",
                params! {
                    "time_moored"   => time_moored,
                    "time_motoring" => time_motoring,
                    "time_sailing"  => time_sailing,
                    "dist_motoring" => dist_motoring,
                    "dist_sailed"   => dist_sailed,
                    "trip_id"       => trip_id,
                },
            )?;
        }

        tx.commit()?;

        // The correction itself is already committed; a cache-invalidation failure must
        // not report the whole operation as failed (same pattern as correct_engine_status).
        if let Err(e) = self.invalidate_trip_legs_cache(trip_id) {
            warn!(
                "Failed to invalidate trip_legs_cache after fix_mooring_status({}): {}",
                trip_id, e
            );
        }
        if let Err(e) =
            self.invalidate_heatmap_cache(clamped_start.date_naive(), clamped_end.date_naive())
        {
            warn!(
                "Failed to invalidate heatmap_cache after fix_mooring_status({}): {}",
                trip_id, e
            );
        }

        Ok(FixMooringReport {
            trip_id,
            rows_matched,
            rows_after,
            resampled: is_moored,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(ts: DateTime<Utc>, lat: f64, lon: f64, heading: f64, wind_speed: f64, wind_angle: f64) -> RawSample {
        RawSample {
            ts,
            position: Position { latitude: lat, longitude: lon },
            max_speed_kn: 0.5,
            engine_on: 0,
            average_heading_deg: Some(heading),
            average_wind_speed_kn: Some(wind_speed),
            average_wind_angle_deg: Some(wind_angle),
        }
    }

    #[test]
    fn resample_buckets_collapses_dense_samples_into_interval_sized_groups() {
        let base = DateTime::parse_from_rfc3339("2026-08-03T22:20:42Z")
            .unwrap()
            .with_timezone(&Utc);
        let seed_ts = base - chrono::Duration::seconds(500);
        let seed_position = Position { latitude: 42.92500, longitude: 9.35640 };

        // 120 samples at 10s cadence = 1200s span -> should collapse into buckets
        // roughly 600s apart, not stay as 120 rows.
        let samples: Vec<RawSample> = (0..120)
            .map(|i| {
                let ts = base + chrono::Duration::seconds(i * 10);
                sample(ts, 42.92500 + (i as f64 % 3.0) * 0.00001, 9.35640, 90.0, 10.0, 45.0)
            })
            .collect();

        let rows = resample_buckets(&samples, seed_position, seed_ts, 600);

        assert!(
            rows.len() < samples.len(),
            "resampling must reduce row count, got {} rows from {} samples",
            rows.len(),
            samples.len()
        );
        assert!(rows.len() >= 2, "1200s span at 600s cadence should yield at least 2 buckets");

        // The final row must be anchored to the exact last sample's position, not a
        // median, so a downstream row referencing that position stays consistent.
        let last = rows.last().unwrap();
        let last_sample = samples.last().unwrap();
        assert_eq!(last.position.latitude, last_sample.position.latitude);
        assert_eq!(last.position.longitude, last_sample.position.longitude);
        assert_eq!(last.timestamp, last_sample.ts);
    }

    #[test]
    fn resample_buckets_uses_vector_distance_not_summed_jitter() {
        // Simulates GPS jitter around a fixed anchor point: real net displacement is
        // ~0, but naively summing consecutive-pair distances would be large.
        let base = DateTime::parse_from_rfc3339("2026-08-03T22:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let seed_position = Position { latitude: 42.9250, longitude: 9.35640 };
        let seed_ts = base;

        let jitter_offsets = [0.00003, -0.00003, 0.00002, -0.00002, 0.00001, -0.00001];
        let samples: Vec<RawSample> = (0..60)
            .map(|i| {
                let ts = base + chrono::Duration::seconds(i * 10);
                let offset = jitter_offsets[i as usize % jitter_offsets.len()];
                sample(ts, 42.9250 + offset, 9.35640, 90.0, 10.0, 45.0)
            })
            .collect();

        let rows = resample_buckets(&samples, seed_position, seed_ts, 600);
        assert_eq!(rows.len(), 1, "600s span at 600s cadence should yield exactly one bucket");

        // Sum of naive consecutive-pair distances would be roughly 60 * ~0.002nm; the
        // bucketed vector distance must be far smaller since the net displacement from
        // the anchor is close to zero.
        assert!(
            rows[0].total_distance_nm < 0.01,
            "bucketed distance should reflect net displacement, not summed jitter, got {}",
            rows[0].total_distance_nm
        );
    }

    #[test]
    fn resample_buckets_uses_circular_mean_for_headings_crossing_0_360() {
        let base = DateTime::parse_from_rfc3339("2026-08-03T22:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let seed_position = Position { latitude: 42.9250, longitude: 9.35640 };
        let seed_ts = base;

        // Headings straddling the 0/360 wrap: a plain arithmetic mean would give ~180°
        // (exactly wrong); the circular mean should land near 0°/360°.
        let samples = vec![
            sample(base + chrono::Duration::seconds(10), 42.9250, 9.35640, 350.0, 10.0, 10.0),
            sample(base + chrono::Duration::seconds(20), 42.9250, 9.35640, 10.0, 10.0, 10.0),
        ];

        let rows = resample_buckets(&samples, seed_position, seed_ts, 600);
        assert_eq!(rows.len(), 1);
        let heading = rows[0].average_heading_deg.unwrap();
        assert!(
            !(90.0..=270.0).contains(&heading),
            "circular mean of 350°/10° must stay near the 0°/360° wrap, got {}",
            heading
        );
    }

    #[test]
    fn mode_engine_on_breaks_ties_with_most_recent_sample() {
        let base = DateTime::parse_from_rfc3339("2026-08-03T22:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut off = sample(base, 42.925, 9.3564, 90.0, 10.0, 45.0);
        off.engine_on = 0;
        let mut on = sample(base + chrono::Duration::seconds(10), 42.925, 9.3564, 90.0, 10.0, 45.0);
        on.engine_on = 1;

        // Tied 1-1: the last sample (engine on) should win.
        assert_eq!(mode_engine_on(&[&off, &on]), 1);
        // Tied 1-1 the other way: the last sample (engine off) should win.
        assert_eq!(mode_engine_on(&[&on, &off]), 0);
    }
}
