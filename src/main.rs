// NMEA2000 Router - Main Application Entry Point
// For architectural guidance, coding conventions, and testing strategies, see: AGENTS.md
//
use std::{
    error::Error,
    sync::{Arc, RwLock},
    time::Duration,
};
use tracing::{info, warn};

mod app_metrics;
mod config;
mod db;
mod environmental_monitor;
mod environmental_status_handler;
mod error;
mod frame_filter;
mod mooring_detection;
mod position_utils;
mod router_loop;
mod signalk_broadcaster;
mod time_monitor;
mod trip;
mod udp_broadcaster;
pub mod utilities;
mod vessel_monitor;
mod vessel_status_handler;
mod web;

use app_metrics::{AppMetrics, MetricsLogger};
use config::Config;
use db::{HealthCheckManager, VesselDatabase};
use environmental_monitor::EnvironmentalMonitor;
use router_loop::RouterLoop;
use signalk_broadcaster::SignalKBroadcaster;
use time_monitor::TimeMonitor;
use udp_broadcaster::UdpBroadcaster;
use vessel_monitor::VesselMonitor;
// Import from nmea2k crate
use nmea2k::{CanBus, N2kStreamReader};

// ========== Logging Setup ==========

fn init_logging(log_config: &config::LogConfig) -> Result<(), Box<dyn Error>> {
    use tracing_appender::rolling;
    use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    // Create log directory if it doesn't exist
    std::fs::create_dir_all(&log_config.directory)?;

    // Create daily rolling file appender
    let file_appender = rolling::daily(&log_config.directory, &log_config.file_prefix);

    // Build subscriber with both console and file output
    let file_layer = fmt::layer()
        .with_writer(file_appender)
        .with_ansi(false)
        .with_timer(fmt::time::OffsetTime::local_rfc_3339().unwrap_or_else(|_| {
            fmt::time::OffsetTime::new(
                time::UtcOffset::UTC,
                time::format_description::well_known::Rfc3339,
            )
        }));

    let console_layer = fmt::layer().with_writer(std::io::stdout).with_timer(
        fmt::time::OffsetTime::local_rfc_3339().unwrap_or_else(|_| {
            fmt::time::OffsetTime::new(
                time::UtcOffset::UTC,
                time::format_description::well_known::Rfc3339,
            )
        }),
    );

    // Parse log level from config
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&log_config.level));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .with(file_layer)
        .init();

    Ok(())
}

// ========== Main Application ==========

fn main() -> Result<(), Box<dyn Error>> {
    // Check for command-line arguments
    let args: Vec<String> = std::env::args().collect();

    // Check for help flag
    if args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        println!("NMEA2000 Router");
        println!();
        println!("USAGE:");
        println!("    nmea_router [OPTIONS]");
        println!();
        println!("OPTIONS:");
        println!("    --config <path>                      Path to configuration file");
        println!("    --validate-config, --validate, -v    Validate configuration and exit");
        println!("    --help, -h                           Show this help message");
        println!();
        println!("Configuration file:");
        println!("  Checked in order: --config path, ./config.json, /etc/nmea_router/config.json");
        std::process::exit(0);
    }

    let config_path = args
        .windows(2)
        .find(|w| w[0] == "--config")
        .map(|w| w[1].as_str());

    let validate_only = args.contains(&"--validate-config".to_string())
        || args.contains(&"--validate".to_string())
        || args.contains(&"-v".to_string());

    // Load configuration
    let mut config = match Config::load_for_context(config_path) {
        Ok(cfg) => {
            if validate_only {
                println!("✓ Configuration validation successful");
                println!("  CAN enabled: {}", cfg.can.enabled);
                println!("  CAN interface: {}", cfg.can.interface);
                println!("  Time skew threshold: {} ms", cfg.time.skew_threshold_ms);
                println!(
                    "  Database: {}@{}",
                    cfg.database.connection.username, cfg.database.connection.host
                );
                println!(
                    "  Vessel status intervals: moored={}s, underway={}s",
                    cfg.database.vessel_status.interval_moored_seconds,
                    cfg.database.vessel_status.interval_underway_seconds
                );
                println!(
                    "  PGN source filters: {} entries",
                    cfg.can.source_filter.pgn_source_map.len()
                );
                std::process::exit(0);
            }
            cfg
        }
        Err(e) => {
            if validate_only {
                eprintln!("✗ Configuration validation failed: {}", e);
            } else {
                eprintln!("Fatal: Failed to load configuration: {}", e);
                eprintln!("Checked: ./config.json, /etc/nmea_router/config.json");
            }
            std::process::exit(1);
        }
    };

    // Initialize logging
    init_logging(&config.logging)?;
    info!("==================================================================");
    info!("NMEA2000 Router starting...");
    info!("Logging initialized");
    info!("Configuration {:#?}", config);
    info!("Loaded configuration");

    // When CAN is disabled the app runs as a read-only web viewer
    if !config.can.enabled {
        config.web.read_only = true;
        info!("CAN disabled — forcing read_only mode");
    }

    // Create database connection using config
    let db_url = config.database.connection.connection_url();

    let vessel_db = match VesselDatabase::new(
        &db_url,
        config.database.connection.pool_min,
        config.database.connection.pool_max,
    ) {
        Ok(db) => {
            info!("Database connection established");
            Arc::new(RwLock::new(db))
        }
        Err(e) => {
            eprintln!("Fatal: Failed to connect to database: {}", e);
            eprintln!(
                "Database host: {}:{}",
                config.database.connection.host, config.database.connection.port
            );
            std::process::exit(1);
        }
    };

    // Start web server if enabled
    if config.web.enabled {
        let db_arc = vessel_db.clone(); // Clone the Arc, not the database
        let config_arc = Arc::new(config.clone());
        let web_port = config.web.port;

        // Use channel to confirm web server started successfully
        let (startup_tx, startup_rx) = std::sync::mpsc::channel::<Result<(), String>>();

        // Spawn web server in a separate thread
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ =
                            startup_tx.send(Err(format!("Failed to create tokio runtime: {}", e)));
                        return;
                    }
                };
                rt.block_on(async {
                    match web::start_web_server(db_arc, config_arc, web_port, startup_tx).await {
                        Ok(()) => {}
                        Err(e) => {
                            warn!("Web server error: {}", e);
                        }
                    }
                });
            }));
            if let Err(e) = result {
                let msg = e
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| e.downcast_ref::<String>().map(|s| s.as_str()))
                    .unwrap_or("unknown panic");
                warn!("Web server thread panicked: {}", msg);
            }
        });

        // Wait for startup confirmation (with timeout)
        match startup_rx.recv_timeout(std::time::Duration::from_secs(10)) {
            Ok(Ok(())) => {
                info!("Web server started on port {}", config.web.port);
            }
            Ok(Err(e)) => {
                eprintln!("Fatal: Web server failed to start: {}", e);
                eprintln!("Port {} may already be in use", config.web.port);
                std::process::exit(1);
            }
            Err(_) => {
                eprintln!("Fatal: Web server startup timed out");
                std::process::exit(1);
            }
        }
    } else {
        info!("Web server disabled in configuration");
    }

    if !config.can.enabled {
        info!("Running in web-only mode (CAN disabled)");
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }

    // CAN is enabled: open socket and run the NMEA2000 processing loop
    let interface = &config.can.interface;
    info!("Opening CAN interface: {}", interface);

    let mut socket = CanBus::open_can_socket_with_retry(interface);
    if let Err(e) = CanBus::configure_nmea2k_socket(&mut socket) {
        eprintln!("Fatal: Failed to configure CAN socket: {}", e);
        eprintln!("CAN interface: {}", interface);
        std::process::exit(1);
    }

    info!("Listening for NMEA2000 messages");

    // Create NMEA2000 stream reader
    let reader = N2kStreamReader::new();

    // Create vessel monitor with config
    info!(
        "Creating vessel monitor with underway interval: {} seconds",
        config.database.vessel_status.interval_underway_seconds
    );
    let vessel_monitor = VesselMonitor::new(
        config.database.vessel_status.interval_underway(),
        config.database.vessel_status.interval_moored(),
    );

    // Create time monitor
    let time_monitor = TimeMonitor::new(config.time.skew_threshold_ms, config.time.set_system_time);

    // Create environmental monitor with config
    let env_monitor = EnvironmentalMonitor::new();

    // Create vessel status handler
    let mut vessel_status_handler = vessel_status_handler::VesselStatusHandler::new();

    // Create environmental status handler
    let environmental_status_handler =
        environmental_status_handler::EnvironmentalStatusHandler::new(
            &config.database.environmental,
        );

    // Create UDP broadcaster with config
    let udp_broadcaster = match UdpBroadcaster::new(
        config.udp.address.clone(),
        config.udp.bind_address.clone(),
        config.udp.enabled,
    ) {
        Ok(broadcaster) => broadcaster,
        Err(e) => {
            eprintln!("Fatal: {}", e);
            eprintln!("UDP destination: {}", config.udp.address);
            std::process::exit(1);
        }
    };

    if config.udp.enabled {
        info!("UDP broadcaster enabled: {}", config.udp.address);
    }

    // Load the last trip from database
    {
        let db = vessel_db
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        vessel_status_handler.load_last_trip(&db);
        vessel_status_handler.load_last_vessel_status(&db);
    }

    // Application metrics tracking
    let metrics = AppMetrics::new();
    let metrics_logger = MetricsLogger::new(Duration::from_secs(60));

    // Database health check manager
    let db_health_check = HealthCheckManager::new(
        Duration::from_secs(60),
        config.database.connection.pool_min,
        config.database.connection.pool_max,
    );

    // Create SignalK broadcaster (if enabled)
    let signalk_broadcaster = SignalKBroadcaster::new(
        config.signalk.rate_limit_ms,
        config.signalk.vessel_uuid.clone(),
    );

    RouterLoop::new(
        socket,
        reader,
        config,
        vessel_monitor,
        time_monitor,
        env_monitor,
        vessel_status_handler,
        environmental_status_handler,
        udp_broadcaster,
        signalk_broadcaster,
        vessel_db,
        metrics,
        metrics_logger,
        db_health_check,
    )
    .run()
}
