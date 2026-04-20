// Gap Filler — standalone binary to backfill vessel_status and environmental_data
// for periods when the main nmea_router service was not running.
//
// Usage:
//   gap_filler --logs <dir> --from <date> --to <date> [--dry-run] [--config <path>]
//
// Dates are in YYYY-MM-DD format (UTC, inclusive).
//
// Only the modules actually required by the gap filler are declared here.
// Each `#[path]` attribute points to the shared source file in src/.
#[path = "../utilities.rs"]
pub mod utilities;

#[path = "../position_utils.rs"]
pub mod position_utils;

#[path = "../config.rs"]
pub mod config;

#[path = "../trip.rs"]
pub mod trip;

#[path = "../environmental_monitor.rs"]
pub mod environmental_monitor;

#[path = "../db/mod.rs"]
pub mod db;

#[path = "../gap_filler/mod.rs"]
pub mod gap_filler;

#[path = "../error.rs"]
pub mod error;

use std::path::PathBuf;
use std::time::SystemTime;

use chrono::{TimeZone, Timelike, Utc};
use config::Config;
use db::VesselDatabase;
use tracing::info;

struct CliArgs {
    log_dir: PathBuf,
    from: SystemTime,
    to: SystemTime,
    dry_run: bool,
    config_path: Option<String>,
}

fn parse_args() -> Result<CliArgs, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut log_dir: Option<PathBuf> = None;
    let mut from_date: Option<SystemTime> = None;
    let mut to_date: Option<SystemTime> = None;
    let mut dry_run = false;
    let mut config_path: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--logs" => {
                i += 1;
                log_dir = Some(PathBuf::from(
                    args.get(i).ok_or("--logs requires a value")?,
                ));
            }
            "--from" => {
                i += 1;
                from_date = Some(parse_date(
                    args.get(i).ok_or("--from requires a value")?,
                    false)?);
            }
            "--to" => {
                i += 1;
                to_date = Some(parse_date(
                    args.get(i).ok_or("--to requires a value")?,
                    true)?);
            }
            "--dry-run" => {
                dry_run = true;
            }
            "--config" => {
                i += 1;
                config_path = Some(
                    args.get(i).ok_or("--config requires a value")?.clone(),
                );
            }
            other => {
                return Err(format!("Unknown argument: {}", other));
            }
        }
        i += 1;
    }

    Ok(CliArgs {
        log_dir: log_dir.ok_or("--logs is required")?,
        from: from_date.ok_or("--from is required")?,
        to: to_date.ok_or("--to is required")?,
        dry_run,
        config_path,
    })
}

/// Parse a YYYY-MM-DD date string into a `SystemTime`.
/// `end_of_day` → 23:59:59.999 UTC; otherwise 00:00:00 UTC.
fn parse_date(s: &str, end_of_day: bool) -> Result<SystemTime, String> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return Err(format!("Invalid date '{}'. Expected YYYY-MM-DD", s));
    }
    let year: i32 = parts[0].parse().map_err(|_| format!("Invalid year in '{}'", s))?;
    let month: u32 = parts[1].parse().map_err(|_| format!("Invalid month in '{}'", s))?;
    let day: u32 = parts[2].parse().map_err(|_| format!("Invalid day in '{}'", s))?;

    let (h, min, sec, nano) = if end_of_day {
        (23, 59, 59, 999_000_000u32)
    } else {
        (0, 0, 0, 0u32)
    };

    let dt = Utc
        .with_ymd_and_hms(year, month, day, h, min, sec)
        .single()
        .ok_or_else(|| format!("Invalid date '{}'", s))?;

    let dt_with_ns = dt
        .with_nanosecond(nano)
        .unwrap_or(dt);

    Ok(SystemTime::from(dt_with_ns))
}

fn main() {
    // Minimal console logger (no file rotation needed for a one-shot tool)
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {}", e);
            eprintln!();
            eprintln!("Usage:");
            eprintln!("  gap_filler --logs <dir> --from YYYY-MM-DD --to YYYY-MM-DD [--dry-run] [--config <path>]");
            std::process::exit(1);
        }
    };

    let config = match &args.config_path {
        Some(path) => {
            let content = std::fs::read_to_string(path).unwrap_or_else(|e| {
                eprintln!("Cannot read config file '{}': {}", path, e);
                std::process::exit(1);
            });
            serde_json::from_str::<Config>(&content).unwrap_or_else(|e| {
                eprintln!("Cannot parse config file '{}': {}", path, e);
                std::process::exit(1);
            })
        }
        None => Config::load_for_context().unwrap_or_else(|e| {
            eprintln!("Cannot load configuration: {}", e);
            std::process::exit(1);
        }),
    };

    let conn_url = format!(
        "mysql://{}:{}@{}:{}/{}",
        config.database.connection.username,
        config.database.connection.password,
        config.database.connection.host,
        config.database.connection.port,
        config.database.connection.database_name,
    );
    let db = VesselDatabase::new(&conn_url, config.database.connection.pool_min, config.database.connection.pool_max).unwrap_or_else(|e| {
        eprintln!("Cannot connect to database: {}", e);
        std::process::exit(1);
    });

    info!(
        "Scanning for gaps between {} and {} (threshold = {}× moored interval = {} s)",
        format_st(args.from),
        format_st(args.to),
        2,
        config.database.vessel_status.interval_moored_seconds,
    );

    if args.dry_run {
        info!("DRY-RUN mode: no changes will be made to the database");
    }

    let report = gap_filler::fill_gaps(
        &db,
        &config,
        &args.log_dir,
        args.from,
        args.to,
        args.dry_run,
    )
    .unwrap_or_else(|e| {
        eprintln!("Gap fill failed: {}", e);
        std::process::exit(1);
    });

    println!();
    println!("=== Gap Filler Report ===");
    println!("Mode            : {}", if args.dry_run { "DRY RUN" } else { "live" });
    println!("Gaps found      : {}", report.gaps_found);
    println!("Gaps filled     : {}", report.gaps_filled);
    println!("vessel_status   : {} records inserted", report.synthetic_vessel_status_inserted);
    println!("env metrics     : {} records inserted", report.environmental_metrics_inserted);
    println!("trips updated   : {}", report.trips_updated);
}

fn format_st(st: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Utc> = st.into();
    dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}


