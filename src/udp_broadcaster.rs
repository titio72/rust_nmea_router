use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use tracing::{debug, warn, error};
use nmea2k::pgns::N2kMessage;
use nmea2k::MessageHandler;
use serde::Serialize;

/// Wrapper struct for serializing NMEA2000 messages to JSON
#[derive(Debug, Serialize)]
struct N2kMessageWrapper {
    /// Message type identifier
    message_type: String,
    /// PGN (Parameter Group Number)
    pgn: u32,
    /// Source address
    source: u8,
    /// Priority
    priority: u8,
    /// Message data serialized as JSON
    data: serde_json::Value,
}

/// UDP broadcaster for NMEA2000 messages
/// 
/// Serializes incoming NMEA2000 messages to JSON and broadcasts them
/// over UDP to a configured destination address.
pub struct UdpBroadcaster {
    socket: Arc<Mutex<Option<UdpSocket>>>,
    destination: String,
    enabled: bool,
    error_count: u64,
    message_count: u64,
}

impl UdpBroadcaster {
    /// Create a new UDP broadcaster
    /// 
    /// # Arguments
    /// * `destination` - UDP destination address (e.g., "192.168.1.255:10110")
    /// * `enabled` - Whether UDP broadcasting is enabled
    pub fn new(destination: String, enabled: bool) -> Self {
        let socket = if enabled {
            match Self::create_socket(&destination) {
                Ok(sock) => {
                    debug!("UDP broadcaster initialized: {}", destination);
                    Some(sock)
                }
                Err(e) => {
                    error!("Failed to create UDP socket: {}. Broadcasting disabled.", e);
                    None
                }
            }
        } else {
            debug!("UDP broadcaster disabled in configuration");
            None
        };

        Self {
            socket: Arc::new(Mutex::new(socket)),
            destination,
            enabled,
            error_count: 0,
            message_count: 0,
        }
    }

    /// Create and configure a UDP socket
    fn create_socket(destination: &str) -> Result<UdpSocket, std::io::Error> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        
        // Enable broadcast if destination is a broadcast address
        if destination.contains(".255") {
            socket.set_broadcast(true)?;
        }
        
        // Set non-blocking mode to prevent blocking the main loop
        socket.set_nonblocking(true)?;
        
        Ok(socket)
    }

    /// Serialize and broadcast an NMEA2000 message
    fn broadcast_message(&mut self, message: &N2kMessage, source: u8, priority: u8) {
        if !self.enabled {
            return;
        }

        let socket_guard = self.socket.lock().unwrap();
        if socket_guard.is_none() {
            return;
        }

        // Serialize message to JSON
        let wrapper = match self.serialize_message(message, source, priority) {
            Ok(w) => w,
            Err(e) => {
                if self.error_count < 10 {
                    warn!("Failed to serialize message: {}", e);
                }
                self.error_count += 1;
                return;
            }
        };

        let json = match serde_json::to_string(&wrapper) {
            Ok(j) => j,
            Err(e) => {
                if self.error_count < 10 {
                    warn!("Failed to convert message to JSON: {}", e);
                }
                self.error_count += 1;
                return;
            }
        };

        // Send UDP packet
        if let Some(ref socket) = *socket_guard {
            match socket.send_to(json.as_bytes(), &self.destination) {
                Ok(_) => {
                    self.message_count += 1;
                    if self.message_count % 1000 == 0 {
                        debug!("Broadcasted {} messages via UDP", self.message_count);
                    }
                }
                Err(e) => {
                    if self.error_count < 10 {
                        warn!("Failed to send UDP packet: {}", e);
                    }
                    self.error_count += 1;
                }
            }
        }
    }

    /// Serialize an NMEA2000 message to the wrapper format
    fn serialize_message(
        &self,
        message: &N2kMessage,
        source: u8,
        priority: u8,
    ) -> Result<N2kMessageWrapper, serde_json::Error> {
        let (message_type, pgn, data) = match message {
            N2kMessage::NMEASystemTime(msg) => {
                let data = serde_json::json!({
                    "date": format!("{:?}", msg.date_time.date),
                    "time": format!("{:?}", msg.date_time.time)
                });
                ("NMEASystemTime", 126992, data)
            }
            N2kMessage::PositionRapidUpdate(msg) => {
                let data = serde_json::json!({
                    "latitude": msg.latitude,
                    "longitude": msg.longitude,
                });
                ("PositionRapidUpdate", 129025, data)
            }
            N2kMessage::CogSogRapidUpdate(msg) => {
                let data = serde_json::json!({
                    "sog": msg.sog,
                    "cog": msg.cog,
                    "cog_reference": msg.cog_reference
                });
                ("CogSogRapidUpdate", 129026, data)
            }
            N2kMessage::GnssPositionData(msg) => {
                let data = serde_json::json!({
                    "date": format!("{:?}", msg.date_time.date),
                    "time": format!("{:?}", msg.date_time.time),
                    "latitude": msg.latitude,
                    "longitude": msg.longitude,
                    "altitude": msg.altitude,
                });
                ("GnssPositionData", 129029, data)
            }
            N2kMessage::WindData(msg) => {
                let data = serde_json::json!({
                    "speed": msg.speed,
                    "angle": msg.angle,
                    "reference": format!("{:?}", msg.reference)
                });
                ("WindData", 130306, data)
            }
            N2kMessage::Temperature(msg) => {
                let data = serde_json::json!({
                    "instance": msg.instance,
                    "source": msg.source,
                    "temperature": msg.temperature,
                    "set_temperature": msg.set_temperature,
                });
                ("Temperature", 130312, data)
            }
            N2kMessage::Humidity(msg) => {
                let data = serde_json::json!({
                    "instance": msg.instance,
                    "source": msg.source,
                    "actual_humidity": msg.actual_humidity,
                    "set_humidity": msg.set_humidity,
                });
                ("Humidity", 130313, data)
            }
            N2kMessage::ActualPressure(msg) => {
                let data = serde_json::json!({
                    "instance": msg.instance,
                    "source": msg.source,
                    "pressure": msg.pressure,
                });
                ("ActualPressure", 130314, data)
            }
            N2kMessage::EngineRapidUpdate(msg) => {
                let data = serde_json::json!({
                    "engine_instance": msg.engine_instance,
                    "engine_speed": msg.engine_speed,
                    "engine_boost_pressure": msg.engine_boost_pressure,
                    "engine_tilt_trim": msg.engine_tilt_trim,
                });
                ("EngineRapidUpdate", 127488, data)
            }
            N2kMessage::Attitude(msg) => {
                let data = serde_json::json!({
                    "yaw": msg.yaw,
                    "pitch": msg.pitch,
                    "roll": msg.roll,
                });
                ("Attitude", 127257, data)
            }
            N2kMessage::VesselHeading(msg) => {
                let data = serde_json::json!({
                    "heading": msg.heading,
                    "reference": format!("{:?}", msg.reference),
                });
                ("VesselHeading", 127250, data)
            }
            N2kMessage::RateOfTurn(msg) => {
                let data = serde_json::json!({
                    "rate": msg.rate,
                });
                ("RateOfTurn", 127251, data)
            }
            N2kMessage::SpeedWaterReferenced(msg) => {
                let data = serde_json::json!({
                    "speed": msg.speed,
                });
                ("SpeedWaterReferenced", 128259, data)
            }
            N2kMessage::WaterDepth(msg) => {
                let data = serde_json::json!({
                    "depth": msg.depth,
                    "offset": msg.offset,
                });
                ("WaterDepth", 128267, data)
            }
            N2kMessage::Unknown(pgn, raw_data) => {
                let data = serde_json::json!({
                    "raw": raw_data
                });
                ("Unknown", *pgn, data)
            }
        };

        Ok(N2kMessageWrapper {
            message_type: message_type.to_string(),
            pgn,
            source,
            priority,
            data,
        })
    }

    /// Get statistics - for future uses
    /// /// Returns (message_count, error_count)
    #[allow(dead_code)]
    pub fn stats(&self) -> (u64, u64) {
        (self.message_count, self.error_count)
    }
}

impl MessageHandler for UdpBroadcaster {
    fn handle_message(&mut self, message: &N2kMessage) {
        // For now, use dummy source and priority values
        // These will be passed from the actual frame in the main loop
        self.broadcast_message(message, 0, 0);
    }
}

impl UdpBroadcaster {
    /// Handle message with frame metadata (source and priority)
    /// This is the preferred method to call from the main loop
    pub fn handle_message_with_metadata(&mut self, message: &N2kMessage, source: u8, priority: u8) {
        self.broadcast_message(message, source, priority);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmea2k::pgns::NMEASystemTime;

    #[test]
    fn test_create_disabled_broadcaster() {
        let broadcaster = UdpBroadcaster::new("127.0.0.1:10110".to_string(), false);
        assert!(!broadcaster.enabled);
        assert!(broadcaster.socket.lock().unwrap().is_none());
    }

    #[test]
    fn test_serialize_system_time() {
        let broadcaster = UdpBroadcaster::new("127.0.0.1:10110".to_string(), false);


        let msg = NMEASystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date_time: nmea2k::pgns::nmea2000_date_time::N2kDateTime {
                date: 19000,
                time: 43200.0,
            },
        };

        let wrapper = broadcaster.serialize_message(&N2kMessage::NMEASystemTime(msg), 1, 3).unwrap();
        assert_eq!(wrapper.message_type, "NMEASystemTime");
        assert_eq!(wrapper.pgn, 126992);
        assert_eq!(wrapper.source, 1);
        assert_eq!(wrapper.priority, 3);
    }
}
