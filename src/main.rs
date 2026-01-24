use socketcan::{CanSocket, EmbeddedFrame, ExtendedId, Frame, Socket};
use std::{error::Error, ops::ControlFlow, time::{Instant}};
use tracing::{info, warn, debug};

mod pgns;
mod stream_reader;
mod vessel_monitor;
mod time_monitor;
mod environmental_monitor;
mod db;
mod config;
mod trip;

use stream_reader::N2kStreamReader;
use vessel_monitor::VesselMonitor;
use time_monitor::TimeMonitor;
use environmental_monitor::EnvironmentalMonitor;
use db::VesselDatabase;
use config::Config;
use trip::Trip;

use crate::vessel_monitor::VesselStatus;

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
    
    let mut last_vessel_status: Option<VesselStatus> = None;
    let mut last_reported_max_speed: f64 = 0.0;
    let mut current_trip: Option<Trip> = None;
    
    // Load the last trip from database if available
    if let Some(ref db) = vessel_db {
        match db.get_last_trip() {
            Ok(trip) => {
                if let Some(t) = trip {
                    info!("Loaded last trip from database: {} (ID: {})", t.description, t.id.unwrap_or(0));
                    current_trip = Some(t);
                } else {
                    info!("No existing trip found in database");
                }
            }
            Err(e) => {
                warn!("Failed to load last trip from database: {}", e);
            }
        }
    }

    // Read CAN frames in a loop
    loop {
        match socket.read_frame() {
            Ok(frame) => {
                // NMEA2000 uses 29-bit extended CAN identifiers
                let can_id = frame.can_id();
                let extended_id = ExtendedId::new(can_id.as_raw()).expect("Invalid CAN ID for NMEA2000");
                let data = frame.data();
                
                // Process the frame through the stream reader
                if let Some(n2k_frame) = reader.process_frame(extended_id, data) {
                    if let ControlFlow::Break(_) = filter_frame(&config, &n2k_frame) {
                        continue;
                    }
                    
                    handle_message(&mut vessel_monitor, &mut time_monitor, &mut env_monitor, n2k_frame);
                    
                    handle_vessel_status(&vessel_db, &mut vessel_monitor, &time_monitor, &mut last_vessel_status, &mut last_reported_max_speed, &mut current_trip);
                                        
                    handle_environment_status(&vessel_db, &time_monitor, &mut env_monitor);
                    
                }
            }
            Err(e) => {
                warn!("Error reading CAN frame: {}", e);
                warn!("CAN bus connection lost. Attempting to reconnect...");
                
                // Try to reconnect
                socket = open_can_socket_with_retry(interface);
                info!("Reconnected to CAN bus. Resuming operation");
            }
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

fn handle_environment_status(vessel_db: &Option<VesselDatabase>, time_monitor: &TimeMonitor, env_monitor: &mut EnvironmentalMonitor) {
    // Write to database if connected, time to persist, and time is synchronized
    if let Some(ref db) = *vessel_db {
        let metrics_to_persist = env_monitor.get_metrics_to_persist();
        if !metrics_to_persist.is_empty() {
            if time_monitor.is_valid_and_synced() {
                for metricid in metrics_to_persist.iter() {
                    debug!("Persisting environmental metric: {}", metricid.name());
                    let data = env_monitor.calculate_metric_data(*metricid);
                    if let Some(metric_data) = data {
                        debug!("Metric Data for {}: avg={:?}, max={:?}, min={:?}, count={:?}", 
                            metricid.name(), 
                            metric_data.avg, 
                            metric_data.max, 
                            metric_data.min,
                            metric_data.count);
                        if let Err(e) = db.insert_environmental_metrics(&metric_data, *metricid) {
                            warn!("Error writing {} data to database: {}", metricid.name(), e);
                        } else {
                        env_monitor.mark_metric_persisted(*metricid);
                        env_monitor.cleanup_all_samples(*metricid);
                            debug!("Environmental metric {} written to database", metricid.name());
                        }
                    } else {
                        debug!("No data available for metric: {}", metricid.name());
                    }
                }
            } else {
                warn!("Skipping environmental metrics DB write - time skew detected {} ms", time_monitor.last_measured_skew_ms());
            }
        }
    }
}

fn handle_vessel_status(vessel_db: &Option<VesselDatabase>, vessel_monitor: &mut VesselMonitor, time_monitor: &TimeMonitor, last_vessel_status: &mut Option<VesselStatus>, last_reported_max_speed: &mut f64, current_trip: &mut Option<Trip>) {
    // Check if it's time to generate a vessel status report
    if let Some(status) = vessel_monitor.generate_status() {
        let effective_position = status.get_effective_position();
        debug!("Vessel Status: latitude={:.6}, longitude={:.6}, avg_speed={:.2} m/s, max_speed={:.2} m/s, moored={}", 
            effective_position.latitude,
            effective_position.longitude,
            status.average_speed, status.max_speed, status.is_moored);
    
        // Write to database if connected, time to persist, and time is synchronized
        if let Some(ref db) = *vessel_db && vessel_monitor.should_persist_to_db(status.is_moored) {
            if time_monitor.is_valid_and_synced() {
                let position = status.get_effective_position();
                let latitude = position.latitude;
                let longitude = position.longitude;
                let (total_distance_nm, total_time_ms) = status.get_total_distance_and_time_from_last_report(last_vessel_status);
                let time: Instant = status.timestamp;
                let average_speed = if total_time_ms > 0 { total_distance_nm / (total_time_ms as f64 / 1000.0) } else { 0.0 };
                let max_speed = if *last_reported_max_speed > status.max_speed { *last_reported_max_speed } else { status.max_speed };
                *last_reported_max_speed = max_speed;


                if let Err(e) = db.insert_status(time, latitude, longitude, average_speed, max_speed, status.is_moored, status.engine_on, total_distance_nm, total_time_ms) {
                    warn!("Error writing to database: {}", e);
                } else {
                    debug!("Vessel status written to database: lat={:.6}, lon={:.6}, avg_speed={:.2} m/s, distance={:.3} nm, time={} ms, moored={}", 
                        position.latitude, position.longitude, status.average_speed, total_distance_nm, total_time_ms, status.is_moored);
                    vessel_monitor.mark_db_persisted();
                    *last_vessel_status = Some(status.clone());
                    *last_reported_max_speed = 0.0;
                    
                    // Update or create trip
                    handle_trip_update(db, current_trip, &status, total_distance_nm, total_time_ms);
                }
            } else {
                warn!("Skipping vessel status DB write - time skew detected {} ms", time_monitor.last_measured_skew_ms());
            }
        }
    }
}

fn handle_trip_update(db: &VesselDatabase, current_trip: &mut Option<Trip>, status: &VesselStatus, distance: f64, time_ms: u64) {
    let report_time = status.timestamp;
    
    // Check if we need to create a new trip or update existing
    let should_create_new = if let Some(ref trip) = *current_trip {
        !trip.is_active(report_time)
    } else {
        true // No current trip, create new one
    };
    
    if should_create_new {
        // Create new trip
        let start_time = report_time;
        
        // Format description with date
        let delta = Instant::now().duration_since(start_time);
        let system_time = std::time::SystemTime::now().checked_sub(delta).unwrap_or(std::time::UNIX_EPOCH);
        let datetime = chrono::DateTime::<chrono::Utc>::from(system_time);
        let description = format!("Trip {}", datetime.format("%Y-%m-%d"));
        
        let mut new_trip = Trip::new(start_time, description);
        new_trip.update(report_time, distance, time_ms, status.engine_on, status.is_moored);
        
        match db.insert_trip(&new_trip) {
            Ok(id) => {
                new_trip.id = Some(id);
                info!("Created new trip: {} (ID: {})", new_trip.description, id);
                *current_trip = Some(new_trip);
            }
            Err(e) => {
                warn!("Failed to create new trip: {}", e);
            }
        }
    } else {
        // Update existing trip
        if let Some(ref mut trip) = *current_trip {
            trip.update(report_time, distance, time_ms, status.engine_on, status.is_moored);
            
            match db.update_trip(trip) {
                Ok(_) => {
                    debug!("Updated trip: {} (ID: {}), total_distance={:.3}nm, total_time={}ms", 
                           trip.description, trip.id.unwrap_or(0), trip.total_distance(), trip.total_time());
                }
                Err(e) => {
                    warn!("Failed to update trip: {}", e);
                }
            }
        }
    }
}

fn handle_message(vessel_monitor: &mut VesselMonitor, time_monitor: &mut TimeMonitor, env_monitor: &mut EnvironmentalMonitor, n2k_frame: stream_reader::N2kFrame) {
    // Update monitors with incoming messages
    match &n2k_frame.message {
        pgns::N2kMessage::PositionRapidUpdate(pos) => {
            vessel_monitor.process_position(pos);
        }
        pgns::N2kMessage::CogSogRapidUpdate(cog_sog) => {
            vessel_monitor.process_cog_sog(cog_sog);    
        }
        pgns::N2kMessage::NMEASystemTime(sys_time) => {
            time_monitor.process_system_time(sys_time);
        }
        pgns::N2kMessage::Temperature(temp) => {
            env_monitor.process_temperature(temp);
        }
        pgns::N2kMessage::WindData(wind) => {
            env_monitor.process_wind(wind);
        }
        pgns::N2kMessage::Humidity(hum) => {
            env_monitor.process_humidity(hum);
        }
        pgns::N2kMessage::ActualPressure(pressure) => {
            env_monitor.process_actual_pressure(pressure);
        }
        pgns::N2kMessage::Attitude(attitude) => {
            env_monitor.process_attitude(attitude);
        }
        pgns::N2kMessage::EngineRapidUpdate(engine) => {
            vessel_monitor.process_engine(engine);
        }
        _ => {}
    }
}
