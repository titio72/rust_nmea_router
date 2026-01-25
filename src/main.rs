use std::{error::Error, time::Duration};
use tracing::{info, warn};

mod vessel_monitor;
mod time_monitor;
mod environmental_monitor;
mod db;
mod config;
mod trip;
mod vessel_status_handler;
mod environmental_status_handler;
mod app_metrics;
mod frame_filter;

use vessel_monitor::VesselMonitor;
use time_monitor::TimeMonitor;
use environmental_monitor::EnvironmentalMonitor;
use db::{VesselDatabase, HealthCheckManager};
use config::Config;
use app_metrics::{AppMetrics, MetricsLogger};
use frame_filter::should_process_frame;

// Import from nmea2k crate
use nmea2k::{N2kStreamReader, CanBus, MessageHandler};

// ========== Logging Setup ==========

fn init_logging(log_config: &config::LogConfig) -> Result<(), Box<dyn Error>> {
    use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
    use tracing_appender::rolling;
    
    // Create log directory if it doesn't exist
    std::fs::create_dir_all(&log_config.directory)?;
    
    // Create daily rolling file appender
    let file_appender = rolling::daily(&log_config.directory, &log_config.file_prefix);
    
    // Build subscriber with both console and file output
    let file_layer = fmt::layer()
        .with_writer(file_appender)
        .with_ansi(false)
        .with_timer(fmt::time::OffsetTime::local_rfc_3339().unwrap_or_else(|_| fmt::time::OffsetTime::new(
            time::UtcOffset::UTC,
            time::format_description::well_known::Rfc3339,
        )));
    
    let console_layer = fmt::layer()
        .with_writer(std::io::stdout)
        .with_timer(fmt::time::OffsetTime::local_rfc_3339().unwrap_or_else(|_| fmt::time::OffsetTime::new(
            time::UtcOffset::UTC,
            time::format_description::well_known::Rfc3339,
        )));
    
    // Parse log level from config
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&log_config.level));
    
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
        println!("    --validate-config, --validate, -v    Validate configuration and exit");
        println!("    --help, -h                           Show this help message");
        println!();
        println!("Configuration file: config.json (in current directory)");
        std::process::exit(0);
    }
    
    let validate_only = args.contains(&"--validate-config".to_string()) 
                     || args.contains(&"--validate".to_string())
                     || args.contains(&"-v".to_string());
    
    // Load configuration
    let config = match Config::from_file("config.json") {
        Ok(cfg) => {
            if validate_only {
                println!("✓ Configuration validation successful");
                println!("  CAN interface: {}", cfg.can_interface);
                println!("  Time skew threshold: {} ms", cfg.time.skew_threshold_ms);
                println!("  Database: {}@{}", cfg.database.connection.username, cfg.database.connection.host);
                println!("  Vessel status intervals: moored={}s, underway={}s", 
                    cfg.database.vessel_status.interval_moored_seconds,
                    cfg.database.vessel_status.interval_underway_seconds);
                println!("  PGN source filters: {} entries", cfg.source_filter.pgn_source_map.len());
                std::process::exit(0);
            }
            cfg
        },
        Err(e) => {
            // Check if this is a CAN interface validation error
            let err_msg = e.to_string();
            if validate_only {
                eprintln!("✗ Configuration validation failed: {}", e);
                std::process::exit(1);
            }
            if err_msg.contains("CAN interface") {
                eprintln!("Fatal configuration error: {}", e);
                eprintln!("Please fix the CAN interface configuration and try again.");
                std::process::exit(1);
            }
            eprintln!("Warning: Could not load config.json: {}", e);
            eprintln!("Using default configuration");
            Config::default()
        }
    };
    
    // Initialize logging
    init_logging(&config.logging)?;
    info!("NMEA2000 Router starting...");
    info!("Loaded configuration");
    
    // Open CAN socket with retry
    let interface = &config.can_interface;
    info!("Opening CAN interface: {}", interface);
    
    let mut socket = CanBus::open_can_socket_with_retry(interface);
    CanBus::configure_nmea2k_socket(&mut socket).expect("Failed to configure CAN socket");
    
    info!("Listening for NMEA2000 messages");
    
    // Create database connection using config
    let db_url = config.database.connection.connection_url();
    
    let mut vessel_db = match VesselDatabase::new(&db_url) {
        Ok(db) => {
            info!("Database connection established");
            Some(db)
        }
        Err(e) => {
            warn!("Failed to connect to database: {}", e);
            warn!("Continuing without database logging...");
            None
        }
    };
    
    // Create NMEA2000 stream reader
    let mut reader = N2kStreamReader::new();
    
    // Create vessel monitor with config
    let mut vessel_monitor = VesselMonitor::new(config.database.vessel_status.clone());
    
    // Create time monitor
    let mut time_monitor = TimeMonitor::new(config.time.skew_threshold_ms);
    
    // Create environmental monitor with config
    let mut env_monitor = EnvironmentalMonitor::new(config.database.environmental.clone());
    
    // Create vessel status handler
    let mut vessel_status_handler = vessel_status_handler::VesselStatusHandler::new(config.database.vessel_status.clone());
    
    // Create environmental status handler
    let mut environmental_status_handler = environmental_status_handler::EnvironmentalStatusHandler::new(env_monitor.db_periods());
    
    // Load the last trip from database if available
    if let Some(ref db) = vessel_db {
        vessel_status_handler.load_last_trip(db);
    }

    // Application metrics tracking
    let mut metrics = AppMetrics::new();
    let mut metrics_logger = MetricsLogger::new(Duration::from_secs(60));
    
    // Database health check manager
    let mut db_health_check = HealthCheckManager::new(Duration::from_secs(60));

    // Read CAN frames in a loop
    loop {
        match CanBus::read_nmea2k_frame(&socket) {
            Ok((extended_id, data)) => {
                metrics.can_frames += 1;
                
                // Process the frame through the stream reader
                if let Some(n2k_frame) = reader.process_frame(extended_id, &data) {
                    metrics.nmea_messages += 1;
                    
                    if !should_process_frame(&config, &n2k_frame) {
                        continue;
                    }
                    time_monitor.handle_message(&n2k_frame.message);
                    if time_monitor.is_valid_and_synced() {
                        vessel_monitor.handle_message(&n2k_frame.message);
                        if let Some(vessel_status) = vessel_monitor.generate_status() && vessel_status.is_valid() {
                            match vessel_status_handler.handle_vessel_status(&vessel_db, vessel_status.clone()) {
                                Ok(true) => metrics.vessel_reports += 1,
                                Ok(false) => {},
                                Err(e) => {
                                    warn!("Database error during vessel status write: {}", e);
                                }
                            }
                        }

                        env_monitor.handle_message(&n2k_frame.message);
                        match environmental_status_handler.handle_environment_status(&vessel_db, &mut env_monitor) {
                            Ok(count) => metrics.env_reports += count as u64,
                            Err(e) => {
                                warn!("Database error during environmental write: {}", e);
                            }
                        }
                    } else {
                        warn!("Skipping message processing - time not synchronized - skew {} ms", time_monitor.last_measured_skew_ms());
                    }
                }
            }
            Err(e) => {
                // Check if this is just a timeout (no data available)
                if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut {
                    // Timeout is expected - just continue to allow metrics and health checks
                    // Don't log or count as error
                } else {
                    // Actual error - log and reconnect
                    metrics.can_errors += 1;
                    warn!("Error reading CAN frame: {}", e);
                    warn!("CAN bus connection lost. Attempting to reconnect...");
                    
                    // Try to reconnect
                    socket = CanBus::open_can_socket_with_retry(interface);
                    CanBus::configure_nmea2k_socket(&mut socket).expect("Failed to configure CAN socket");
                    
                    info!("Reconnected to CAN bus. Resuming operation");
                    
                    // Wait before resuming to allow bus to stabilize
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }
        }
        
        // Log metrics periodically
        metrics_logger.check_and_log(&mut metrics);
        
        // Database health check using manager
        if db_health_check.check_and_reconnect(&mut vessel_db, &db_url) {
            // Reload last trip if reconnection occurred
            if let Some(ref db) = vessel_db {
                vessel_status_handler.load_last_trip(db);
            }
        }
    }
}