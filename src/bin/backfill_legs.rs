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

use config::Config;
use db::VesselDatabase;
use tracing::info;

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let config_path = std::env::args().nth(1);

    let config = match config_path {
        Some(path) => {
            let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
                eprintln!("Cannot read config file '{}': {}", path, e);
                std::process::exit(1);
            });
            serde_json::from_str::<Config>(&content).unwrap_or_else(|e| {
                eprintln!("Cannot parse config file '{}': {}", path, e);
                std::process::exit(1);
            })
        }
        None => Config::load_for_context(None).unwrap_or_else(|e| {
            eprintln!("Cannot load config: {}", e);
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

    info!("Starting trip legs cache backfill…");

    match db.backfill_trip_legs_cache() {
        Ok(count) => {
            info!("Done. Backfilled legs cache for {} trip(s).", count);
        }
        Err(e) => {
            eprintln!("Backfill failed: {}", e);
            std::process::exit(1);
        }
    }
}
