use nmea2000::Identifier;
use socketcan::{CanSocket, EmbeddedFrame, ExtendedId, Frame, Socket};
use std::{error::Error, time::Instant};

mod pgns;
mod stream_reader;
mod vessel_monitor;
mod time_monitor;
mod environmental_monitor;
mod db;
mod config;

use stream_reader::N2kStreamReader;
use vessel_monitor::VesselMonitor;
use time_monitor::TimeMonitor;
use environmental_monitor::EnvironmentalMonitor;
use db::VesselDatabase;
use config::Config;

use crate::pgns::SystemTime;

// ========== Display & Utility Functions ==========
/*
fn decode_pgn_name(pgn: u32) -> &'static str {
    match pgn {
        126992 => "System Time",
        126996 => "Product Information",
        127233 => "Man Overboard Notification",
        127237 => "Heading/Track Control",
        127245 => "Rudder",
        127250 => "Vessel Heading",
        127251 => "Rate of Turn",
        127257 => "Attitude",
        127258 => "Magnetic Variation",
        127488 => "Engine Parameters, Rapid Update",
        127489 => "Engine Parameters, Dynamic",
        127493 => "Transmission Parameters, Dynamic",
        127505 => "Fluid Level",
        127508 => "Battery Status",
        128259 => "Speed, Water Referenced",
        128267 => "Water Depth",
        128275 => "Distance Log",
        129025 => "Position, Rapid Update",
        129026 => "COG & SOG, Rapid Update",
        129029 => "GNSS Position Data",
        129033 => "Time & Date",
        129038 => "AIS Class A Position Report",
        129039 => "AIS Class B Position Report",
        129540 => "GNSS Sats in View",
        129794 => "AIS Class A Static and Voyage Related Data",
        129809 => "AIS Class B Static Data (Part A)",
        129810 => "AIS Class B Static Data (Part B)",
        130306 => "Wind Data",
        130311 => "Environmental Parameters",
        130312 => "Temperature",
        130313 => "Humidity",
        130314 => "Actual Pressure",
        130316 => "Temperature, Extended Range",
        _ => "Unknown PGN",
    }
}

fn format_data_bytes(data: &[u8]) -> String {
    data.iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode_message(identifier: &Identifier, message: &pgns::N2kMessage, is_fast_packet: bool, data_bytes: &[u8]) {
    let pgn = identifier.pgn();
    let priority = identifier.priority();
    let source = identifier.source();
    let pgn_name = decode_pgn_name(pgn);
    
    // Display message header
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("PGN: {} ({}){}", pgn, pgn_name, if is_fast_packet { " [Fast Packet]" } else { "" });
    println!("Priority: {} | Source: {}", priority, source);
    println!("Data: [{}]", format_data_bytes(data_bytes));
    
    // Display decoded message
    println!("{}", message);
}
*/
// ========== Main Application ==========

fn open_can_socket_with_retry(interface: &str) -> CanSocket {
    loop {
        match CanSocket::open(interface) {
            Ok(socket) => {
                println!("✓ Successfully opened CAN interface: {}", interface);
                return socket;
            }
            Err(e) => {
                eprintln!("⚠️  Failed to open CAN interface '{}': {}", interface, e);
                eprintln!("   Retrying in 10 seconds...");
                std::thread::sleep(std::time::Duration::from_secs(10));
            }
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("NMEA2000 Router - Starting...");
    
    // Load configuration
    let config = Config::from_file("config.json").unwrap_or_else(|e| {
        eprintln!("Warning: Could not load config.json: {}", e);
        eprintln!("Using default configuration");
        Config::default()
    });
    
    // Open CAN socket with retry
    let interface = &config.can_interface;
    println!("Opening CAN interface: {}", interface);
    
    let mut socket = open_can_socket_with_retry(interface);
    println!("Listening for NMEA2000 messages...\n");
    
    // Create database connection using config
    let db_url = config.database.connection.connection_url();
    
    let vessel_db = match VesselDatabase::new(&db_url) {
        Ok(db) => {
            println!("Database connection established");
            Some(db)
        }
        Err(e) => {
            eprintln!("Warning: Failed to connect to database: {}", e);
            eprintln!("Continuing without database logging...\n");
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
                    if n2k_frame.identifier.pgn() == 126992 {
                        let sys_time: SystemTime = pgns::pgn126992::SystemTime::from_bytes(&n2k_frame.data).expect("msg");
                        println!("System Time: {:?} {:?}", sys_time.to_date_time(), sys_time.to_total_milliseconds() - chrono::Utc::now().timestamp_millis());
                    }
                    // Update monitors with incoming messages
                    match &n2k_frame.message {
                        pgns::N2kMessage::PositionRapidUpdate(pos) => {
                            vessel_monitor.process_position(pos);
                        }
                        pgns::N2kMessage::CogSogRapidUpdate(cog_sog) => {
                            vessel_monitor.process_cog_sog(cog_sog);    
                        }
                        pgns::N2kMessage::SystemTime(sys_time) => {
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
                    
                    // Check if it's time to generate a vessel status report
                    if let Some(status) = vessel_monitor.generate_status() {
                        println!("\n{}", status);
                        
                        // Write to database if connected, time to persist, and time is synchronized
                        if let Some(ref db) = vessel_db {
                            if vessel_monitor.should_persist_to_db(status.is_moored) {
                                if time_monitor.is_time_synchronized() {
                                    if let Err(e) = db.insert_status(&status) {
                                        eprintln!("Error writing to database: {}", e);
                                    } else {
                                        vessel_monitor.mark_db_persisted();
                                    }
                                } else {
                                    eprintln!("⚠️  Skipping vessel status DB write - time skew detected");
                                }
                            }
                        }
                    }
                    
                    // Check if it's time to generate an environmental report
                    if let Some(env_report) = env_monitor.generate_report() {
                        println!("\n{}", env_report);
                        
                        // Write to database if connected, time to persist, and time is synchronized
                        if let Some(ref db) = vessel_db {
                            let metrics_to_persist = env_monitor.get_metrics_to_persist();
                            if !metrics_to_persist.is_empty() {
                                if time_monitor.is_time_synchronized() {
                                    if let Err(e) = db.insert_environmental_metrics(&env_report, &metrics_to_persist) {
                                        eprintln!("Error writing environmental data to database: {}", e);
                                    } else {
                                        env_monitor.mark_metrics_persisted(&metrics_to_persist);
                                    }
                                } else {
                                    eprintln!("⚠️  Skipping environmental metrics DB write - time skew detected");
                                }
                            }
                        }
                    }
                    
                    // Complete message available - decode and display
                    /*decode_message(
                        &n2k_frame.identifier,
                        &n2k_frame.message,
                        n2k_frame.is_fast_packet,
                        &n2k_frame.data,
                    );*/
                }
            }
            Err(e) => {
                eprintln!("⚠️  Error reading CAN frame: {}", e);
                eprintln!("⚠️  CAN bus connection lost. Attempting to reconnect...");
                
                // Try to reconnect
                socket = open_can_socket_with_retry(interface);
                eprintln!("✓ Reconnected to CAN bus. Resuming operation...\n");
            }
        }
    }
}
