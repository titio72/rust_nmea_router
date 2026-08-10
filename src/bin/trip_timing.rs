// Trip timing diagnostic — replays the exact sequence of DB calls that trip.html
// issues for a full-trip view (GET /api/trip, /api/trip_legs, /api/track,
// /api/metrics/batch) directly against the instrumented VesselDatabase functions,
// so their internal `tracing` timing spans (operation/phase/elapsed_ms) are visible
// without going through HTTP/axum.
//
// Usage:
//   trip_timing --trip-id <id> [--config <path>]
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

#[path = "../error.rs"]
pub mod error;

#[path = "../polars.rs"]
pub mod polars;

use chrono::{DateTime, Utc};
use config::Config;
use db::VesselDatabase;
use std::time::Instant;
use tracing::info;

fn parse_args() -> (u32, Option<String>) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut trip_id: Option<u32> = None;
    let mut config_path: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--trip-id" => {
                trip_id = args.get(i + 1).and_then(|s| s.parse().ok());
                i += 2;
            }
            "--config" => {
                config_path = args.get(i + 1).cloned();
                i += 2;
            }
            _ => {
                i += 1;
            }
        }
    }
    let trip_id = trip_id.unwrap_or_else(|| {
        eprintln!("Usage: trip_timing --trip-id <id> [--config <path>]");
        std::process::exit(1);
    });
    (trip_id, config_path)
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let (trip_id, config_path) = parse_args();

    let config = match &config_path {
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
        None => Config::load_for_context(None).unwrap_or_else(|e| {
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
    let db = VesselDatabase::new(
        &conn_url,
        config.database.connection.pool_min,
        config.database.connection.pool_max,
    )
    .unwrap_or_else(|e| {
        eprintln!("Cannot connect to database: {}", e);
        std::process::exit(1);
    });

    let polars = config
        .polars_file_path
        .as_deref()
        .and_then(|path| polars::PolarTable::from_csv(path).ok());

    info!(trip_id, "=== Starting trip_timing breakdown ===");
    let t_request_total = Instant::now();

    // 1. GET /api/trip?id=<id>
    let trip = db
        .fetch_trip(trip_id)
        .unwrap_or_else(|e| {
            eprintln!("fetch_trip failed: {}", e);
            std::process::exit(1);
        })
        .unwrap_or_else(|| {
            eprintln!("Trip {} not found", trip_id);
            std::process::exit(1);
        });
    info!(
        description = %trip.description,
        start = %trip.start_date,
        end = %trip.end_date,
        total_distance_nm = trip.total_distance_nm,
        "trip metadata"
    );

    let start: DateTime<Utc> = trip.start_date.parse().unwrap_or_else(|e| {
        eprintln!("Cannot parse trip start_date '{}': {}", trip.start_date, e);
        std::process::exit(1);
    });
    let end: DateTime<Utc> = trip.end_date.parse().unwrap_or_else(|e| {
        eprintln!("Cannot parse trip end_date '{}': {}", trip.end_date, e);
        std::process::exit(1);
    });

    // 2. GET /api/trip_legs?id=<id>  (parallel with #1/#3 in the browser)
    let _legs = db.fetch_trip_legs(trip_id);

    // 3. GET /api/track?trip_id=<id>&max_points=600  (parallel with #1/#2 in the browser)
    let track = db.fetch_track(Some(trip_id), None, None, Some(600));
    if let (Ok(mut track), Some(polars)) = (track, polars.as_ref()) {
        let t_polar = Instant::now();
        for point in &mut track {
            if let (Some(tws), Some(twa_360), Some(actual)) = (
                point.average_wind_speed_kn,
                point.average_wind_angle_deg,
                point.avg_speed_kn,
            ) {
                let twa = twa_360.min(360.0 - twa_360);
                if let Some(polar_spd) = polars.boat_speed(twa, tws) {
                    let _ = polar_spd;
                    let _ = actual;
                }
            }
        }
        info!(
            operation = "get_track",
            phase = "polar_enrichment",
            rows = track.len(),
            elapsed_ms = t_polar.elapsed().as_secs_f64() * 1000.0,
            "timing"
        );
    }

    // 5. GET /api/metrics/batch?metrics=1,2,4,5,6&start=..&end=..&max_points=1000
    let _batch = db.fetch_metrics_batch(&[1, 2, 4, 5, 6], None, Some(start), Some(end), Some(1000));

    info!(
        trip_id,
        elapsed_ms = t_request_total.elapsed().as_secs_f64() * 1000.0,
        "=== Full-trip-view request sequence complete (sum of DB-layer time; browser issues these with some overlap) ==="
    );
}
