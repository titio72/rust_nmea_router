/// Gap filler orchestration module.
///
/// Detects gaps in vessel_status, reads backup log files, synthesises records,
/// inserts them into the database, corrects the "restart" record, and
/// recalculates affected trip totals.
pub mod log_parser;
pub mod synthesizer;

use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::config::Config;
use crate::db::operations::gap_fill::{systemtime_to_millis, Gap};
use crate::db::types::VesselStatusOperation;
use crate::utilities::{haversine_distance_nm, haversine_heading};
use crate::db::VesselDatabase;

use log_parser::read_entries_in_range;
use synthesizer::synthesize_gap;

/// Summary of what the gap filler did (or would do in dry-run mode).
#[derive(Debug, Default)]
pub struct FillReport {
    pub gaps_found: usize,
    pub gaps_filled: usize,
    pub synthetic_vessel_status_inserted: usize,
    pub environmental_metrics_inserted: usize,
    pub trips_updated: usize,
}

/// Detect gaps and fill them from backup log files.
///
/// * `db`         — open database connection
/// * `config`     — application configuration
/// * `log_dir`    — directory containing backup log files
/// * `from` / `to` — inclusive UTC window to scan for gaps
/// * `dry_run`    — when `true`, log what would be done without modifying the database
pub fn fill_gaps(
    db: &VesselDatabase,
    config: &Config,
    log_dir: &Path,
    from: SystemTime,
    to: SystemTime,
    dry_run: bool,
) -> Result<FillReport, Box<dyn std::error::Error>> {
    let moored_interval_secs = config.database.vessel_status.interval_moored_seconds;
    let threshold = Duration::from_secs(moored_interval_secs * 2);

    let gaps = db.find_gaps(from, to, threshold)?;

    let mut report = FillReport {
        gaps_found: gaps.len(),
        ..Default::default()
    };

    for gap in &gaps {
        tracing::info!(
            "Gap detected: {} → {} ({:.1} min), ids {} → {}",
            format_systemtime(gap.before_timestamp),
            format_systemtime(gap.after_timestamp),
            gap.after_timestamp
                .duration_since(gap.before_timestamp)
                .unwrap_or(Duration::ZERO)
                .as_secs_f64() / 60.0,
            gap.before_id,
            gap.after_id,
        );

        // Read log entries spanning the gap
        let entries = read_entries_in_range(log_dir, gap.before_timestamp, gap.after_timestamp);
        tracing::info!("  Log entries found: {}", entries.len());

        let moored_interval = Duration::from_secs(moored_interval_secs);
        let underway_interval = config.database.vessel_status.interval_underway();

        // Synthesise synthetic periods
        let prev_position = Some((gap.before_position, gap.before_timestamp));
        let prev_heading = gap.before_heading;

        let periods = synthesize_gap(
            &entries,
            gap.before_timestamp,
            gap.after_timestamp,
            moored_interval,
            underway_interval,
            prev_position,
            prev_heading,
        );

        tracing::info!("  Synthetic periods synthesised: {}", periods.len());

        if dry_run {
            for (i, p) in periods.iter().enumerate() {
                tracing::info!(
                    "  [dry-run] period {}: ts={} moored={} engine={:?} dist_nm={:.4} sog={:.3}kn",
                    i + 1,
                    format_systemtime(std::time::UNIX_EPOCH
                        + std::time::Duration::from_millis(
                            crate::db::operations::gap_fill::systemtime_to_millis(
                                crate::utilities::dirty_instant_to_systemtime(p.vessel_status.timestamp)
                            )
                        )
                    ),
                    p.vessel_status.is_moored,
                    p.vessel_status.engine_on,
                    p.vessel_status.total_distance_nm,
                    p.vessel_status.average_speed_kn,
                );
            }
            continue;
        }

        // ---- Insert synthetic records -------------------------------------
        for period in &periods {
            db.insert_synthetic_vessel_status(&period.vessel_status)?;
            report.synthetic_vessel_status_inserted += 1;

            let ts_system = crate::utilities::dirty_instant_to_systemtime(period.vessel_status.timestamp);
            for (metric_id, metric_data) in &period.env_metrics {
                db.insert_environmental_metrics(metric_data, *metric_id, ts_system)?;
                report.environmental_metrics_inserted += 1;
            }
        }

        // ---- Correct the "restart" record ---------------------------------
        if let Some(last_period) = periods.last() {
            let corrected = build_corrected_restart_record(gap, last_period);

            tracing::info!(
                "  Correcting restart record id={}: total_time_ms {} → {}",
                gap.after_id,
                gap.after_record.total_time_ms,
                corrected.total_time_ms,
            );

            db.delete_vessel_status_by_id(gap.after_id)?;
            db.insert_synthetic_vessel_status(&corrected)?;
            report.synthetic_vessel_status_inserted += 1;
        }

        // ---- Update trip --------------------------------------------------
        if let Some(trip) = db.find_trip_for_gap(gap.before_timestamp, gap.after_timestamp)? {
            if let Some(trip_id) = trip.id {
                db.recalculate_and_update_trip(trip_id, trip.start_timestamp, trip.end_timestamp)?;
                report.trips_updated += 1;
                tracing::info!("  Trip id={} recalculated", trip_id);
            }
        }

        report.gaps_filled += 1;
    }

    Ok(report)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a corrected copy of the "after" (restart) record by recalculating
/// distance, speed, and COG from the last synthetic period's position.
fn build_corrected_restart_record(
    gap: &Gap,
    last_period: &synthesizer::SyntheticPeriod,
) -> VesselStatusOperation {
    let last_pos = last_period.vessel_status.position;
    let last_ts_millis = systemtime_to_millis(
        crate::utilities::dirty_instant_to_systemtime(last_period.vessel_status.timestamp)
    );
    let after_ts_millis = systemtime_to_millis(
        crate::utilities::dirty_instant_to_systemtime(gap.after_record.timestamp)
    );

    let total_time_ms = after_ts_millis.saturating_sub(last_ts_millis);

    let after_pos = gap.after_record.position;
    let total_distance_nm = haversine_distance_nm(
        last_pos.latitude, last_pos.longitude,
        after_pos.latitude, after_pos.longitude,
    );

    let average_speed_kn = if total_time_ms > 0 {
        (total_distance_nm / (total_time_ms as f64 / 3_600_000.0))
            .min(25.0) // MAX_VALID_SOG_KN
    } else {
        0.0
    };

    let cog_deg = if total_distance_nm > 1e-9 {
        Some(haversine_heading(
            last_pos.latitude, last_pos.longitude,
            after_pos.latitude, after_pos.longitude,
        ))
    } else {
        gap.after_record.cog_deg
    };

    VesselStatusOperation {
        timestamp: gap.after_record.timestamp,
        position: after_pos,
        average_speed_kn,
        max_speed_kn: gap.after_record.max_speed_kn,
        is_moored: gap.after_record.is_moored,
        engine_on: gap.after_record.engine_on,
        total_distance_nm,
        total_time_ms,
        wind_speed_kn: gap.after_record.wind_speed_kn,
        wind_speed_variance: None,
        wind_angle_deg: gap.after_record.wind_angle_deg,
        wind_angle_variance: None,
        cog_deg,
        average_heading_deg: gap.after_record.average_heading_deg,
    }
}

fn format_systemtime(st: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Utc> = st.into();
    dt.format("%Y-%m-%d %H:%M:%S%.3fZ").to_string()
}
