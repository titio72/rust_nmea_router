use socketcan::{CanSocket, EmbeddedFrame, ExtendedId, Frame, Socket};
use std::{error::Error, ops::ControlFlow, time::{Duration, Instant}};
use tracing::{info, warn};

mod pgns;
mod stream_reader;
mod vessel_monitor;
mod time_monitor;
mod environmental_monitor;
mod db;
mod config;
mod trip;
mod vessel_status_handler;
mod environmental_status_handler;
mod message_handler;

use stream_reader::N2kStreamReader;
use vessel_monitor::VesselMonitor;
use time_monitor::TimeMonitor;
use environmental_monitor::EnvironmentalMonitor;
use db::VesselDatabase;
use config::Config;
use message_handler::MessageHandler;

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

fn open_can_socket_with_retry(interface: &str) -> CanSocket {
    loop {
        match CanSocket::open(interface) {
            Ok(socket) => {
                info!("Successfully opened CAN interface: {}", interface);
                return socket;
            }
            Err(e) => {
                warn!("Failed to open CAN interface '{}': {}", interface, e);
                warn!("Retrying in 10 seconds...");
                std::thread::sleep(std::time::Duration::from_secs(10));
            }
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    // Load configuration
    let config = Config::from_file("config.json").unwrap_or_else(|e| {
        eprintln!("Warning: Could not load config.json: {}", e);
        eprintln!("Using default configuration");
        Config::default()
    });
    
    // Initialize logging
    init_logging(&config.logging)?;
    info!("NMEA2000 Router starting...");
    info!("Loaded configuration");
    
    // Open CAN socket with retry
    let interface = &config.can_interface;
    info!("Opening CAN interface: {}", interface);
    
    let mut socket = open_can_socket_with_retry(interface);
    info!("Listening for NMEA2000 messages");
    
    // Create database connection using config
    let db_url = config.database.connection.connection_url();
    
    let vessel_db = match VesselDatabase::new(&db_url) {
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

    // Metrics tracking
    struct Metrics {
        can_frames: u64,
        nmea_messages: u64,
        vessel_reports: u64,
        env_reports: u64,
        can_errors: u64,
    }
    
    let mut metrics = Metrics {
        can_frames: 0,
        nmea_messages: 0,
        vessel_reports: 0,
        env_reports: 0,
        can_errors: 0,
    };
    let mut last_metrics_log = Instant::now();
    let metrics_interval = Duration::from_secs(60);

    // Read CAN frames in a loop
    loop {
        match socket.read_frame() {
            Ok(frame) => {
                metrics.can_frames += 1;
                
                // NMEA2000 uses 29-bit extended CAN identifiers
                let can_id = frame.can_id();
                let extended_id = ExtendedId::new(can_id.as_raw()).expect("Invalid CAN ID for NMEA2000");
                let data = frame.data();
                
                // Process the frame through the stream reader
                if let Some(n2k_frame) = reader.process_frame(extended_id, data) {
                    metrics.nmea_messages += 1;
                    
                    if let ControlFlow::Break(_) = filter_frame(&config, &n2k_frame) {
                        continue;
                    }
                    time_monitor.handle_message(&n2k_frame.message);
                    vessel_monitor.handle_message(&n2k_frame.message);
                    env_monitor.handle_message(&n2k_frame.message);
                    
                    if vessel_status_handler.handle_vessel_status(&vessel_db, &time_monitor, &mut vessel_monitor) {
                        metrics.vessel_reports += 1;
                    }
                    
                    let env_writes = environmental_status_handler.handle_environment_status(&vessel_db, &time_monitor, &mut env_monitor);
                    metrics.env_reports += env_writes as u64;
                }
            }
            Err(e) => {
                metrics.can_errors += 1;
                warn!("Error reading CAN frame: {}", e);
                warn!("CAN bus connection lost. Attempting to reconnect...");
                
                // Try to reconnect
                socket = open_can_socket_with_retry(interface);
                info!("Reconnected to CAN bus. Resuming operation");
                
                // Wait before resuming to allow bus to stabilize
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
        
        // Log metrics every minute
        if last_metrics_log.elapsed() >= metrics_interval {
            info!("[Metrics] CAN frames: {}, NMEA messages: {}, Vessel reports: {}, Env reports: {}, CAN errors: {}",
                metrics.can_frames, metrics.nmea_messages, metrics.vessel_reports, metrics.env_reports, metrics.can_errors);
            
            // Reset metrics
            metrics.can_frames = 0;
            metrics.nmea_messages = 0;
            metrics.vessel_reports = 0;
            metrics.env_reports = 0;
            metrics.can_errors = 0;
            last_metrics_log = Instant::now();
        }
    }
}

fn filter_frame(config: &Config, n2k_frame: &stream_reader::N2kFrame) -> ControlFlow<()> {
    let pgn = n2k_frame.identifier.pgn();
    let source = n2k_frame.identifier.source();
                    
    // Apply source filter - skip messages that don't match the configured source
    if !config.source_filter.should_accept(pgn, source) {
        return ControlFlow::Break(());
    }
    ControlFlow::Continue(())
}