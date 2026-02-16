use std::time::{Instant, Duration};
use nmea2k::{MessageHandler, N2kFrame};
use nmea2k::pgns::{N2kMessage, HeadingReference};
use crate::web::websocket::RealtimeMessage;
use crate::web::get_broadcast_channels;
use crate::utilities::{instant_to_unix_millis, calculate_true_wind};

/// Broadcasts NMEA2000 messages as realtime data via WebSocket
/// Each message type is serialized independently and sent directly to connected clients
/// Caches SOG to calculate true wind from apparent wind data
/// Also caches time sync status to provide with GNSS time broadcasts
/// Rate-limits each message type to at most 1 message per second
pub struct BroadcastMonitor {
    last_sog_kn: Option<f64>,
    last_time_sync_status: Option<String>,
    last_time_skew_ms: Option<i64>,
    // Rate limiting: track last broadcast time for each message type
    last_position_broadcast: Option<Instant>,
    last_course_speed_broadcast: Option<Instant>,
    last_heading_broadcast: Option<Instant>,
    last_wind_broadcast: Option<Instant>,
    last_temperature_broadcast: Option<Instant>,
    last_humidity_broadcast: Option<Instant>,
    last_pressure_broadcast: Option<Instant>,
    last_system_time_broadcast: Option<Instant>,
}

impl BroadcastMonitor {
    pub fn new() -> Self {
        Self {
            last_sog_kn: None,
            last_time_sync_status: Some("not_synced".to_string()),
            last_time_skew_ms: Some(0),
            last_position_broadcast: None,
            last_course_speed_broadcast: None,
            last_heading_broadcast: None,
            last_wind_broadcast: None,
            last_temperature_broadcast: None,
            last_humidity_broadcast: None,
            last_pressure_broadcast: None,
            last_system_time_broadcast: None,
        }
    }
    
    /// Update the cached time sync status and skew
    /// This should be called from the main loop with the latest values from TimeMonitor
    pub fn update_time_sync_status(&mut self, status: String, skew_ms: i64) {
        self.last_time_sync_status = Some(status);
        self.last_time_skew_ms = Some(skew_ms);
    }
    
    /// Check if enough time has passed to broadcast a message of this type
    fn should_broadcast(&self, last_broadcast: Option<Instant>, now: Instant) -> bool {
        match last_broadcast {
            None => true, // First message always broadcasts
            Some(last) => now.duration_since(last) >= Duration::from_millis(900),
        }
    }
}

impl MessageHandler for BroadcastMonitor {
    fn handle_message(&mut self, frame: &N2kFrame, now: std::time::Instant) {
        let channels = get_broadcast_channels();
        let timestamp = instant_to_unix_millis(now);
        
        match &frame.message {
            // Position data - PGN 129025
            N2kMessage::PositionRapidUpdate(pos) => {
                if self.should_broadcast(self.last_position_broadcast, now) {
                    let msg = RealtimeMessage::Position {
                        latitude: pos.latitude,
                        longitude: pos.longitude,
                        timestamp,
                    };
                    let _ = channels.send(msg);
                    self.last_position_broadcast = Some(now);
                }
            },
            
            // COG/SOG data - PGN 129026
            N2kMessage::CogSogRapidUpdate(cog_sog) => {
                let sog_kn = cog_sog.sog_knots();
                // Cache the SOG for true wind calculations
                self.last_sog_kn = Some(sog_kn);
                
                if self.should_broadcast(self.last_course_speed_broadcast, now) {
                    let msg = RealtimeMessage::CourseSpeed {
                        cog_deg: cog_sog.cog_degrees(),
                        sog_kn,
                        timestamp,
                    };
                    let _ = channels.send(msg);
                    self.last_course_speed_broadcast = Some(now);
                }
            },
            
            // Heading - PGN 127250
            N2kMessage::VesselHeading(heading) => {
                let heading_deg = if heading.reference == HeadingReference::Magnetic {
                    if let Some(variation) = heading.variation {
                        (heading.heading + variation).to_degrees()
                    } else {
                        heading.heading.to_degrees()
                    }
                } else {
                    return;
                };
                
                if self.should_broadcast(self.last_heading_broadcast, now) {
                    let msg = RealtimeMessage::Heading {
                        heading_deg,
                        timestamp,
                    };
                    let _ = channels.send(msg);
                    self.last_heading_broadcast = Some(now);
                }
            },
            
            // Wind data - PGN 130306
            N2kMessage::WindData(wind) => {
                let is_apparent = matches!(
                    wind.reference,
                    nmea2k::pgns::pgn130306::WindReference::Apparent
                );
                
                if is_apparent {
                    let aws_kn = wind.speed_knots();
                    let awa_deg = wind.angle.to_degrees();
                    
                    // Calculate true wind if we have a cached SOG
                    let (tws_kn, twa_deg) = if let Some(sog_kn) = self.last_sog_kn {
                        calculate_true_wind(aws_kn, awa_deg, sog_kn)
                    } else {
                        // No SOG available yet, use apparent as true
                        (aws_kn, awa_deg)
                    };
                    
                    if self.should_broadcast(self.last_wind_broadcast, now) {
                        let msg = RealtimeMessage::Wind {
                            true_wind_speed_kn: Some(tws_kn),
                            true_wind_angle_deg: Some(twa_deg),
                            apparent_wind_speed_kn: Some(aws_kn),
                            apparent_wind_angle_deg: Some(awa_deg),
                            timestamp,
                        };
                        let _ = channels.send(msg);
                        self.last_wind_broadcast = Some(now);
                    }
                }
                // Skip true wind messages to avoid confusion
            },
            
            // Temperature sensors - PGN 130312
            N2kMessage::Temperature(temp) => {
                if self.should_broadcast(self.last_temperature_broadcast, now) {
                    let msg = RealtimeMessage::Temperature {
                        temperature_c: temp.temperature - 273.15,
                        instance: temp.instance,
                        timestamp,
                    };
                    let _ = channels.send(msg);
                    self.last_temperature_broadcast = Some(now);
                }
            },
            
            // Humidity sensor - PGN 130313
            N2kMessage::Humidity(humidity) => {
                if self.should_broadcast(self.last_humidity_broadcast, now) {
                    let msg = RealtimeMessage::Humidity {
                        humidity_percent: humidity.actual_humidity,
                        timestamp,
                    };
                    let _ = channels.send(msg);
                    self.last_humidity_broadcast = Some(now);
                }
            },
            
            // Pressure sensor - PGN 130314
            N2kMessage::ActualPressure(pressure) => {
                if self.should_broadcast(self.last_pressure_broadcast, now) {
                    let msg = RealtimeMessage::Pressure {
                        pressure_pa: pressure.pressure,
                        timestamp,
                    };
                    let _ = channels.send(msg);
                    self.last_pressure_broadcast = Some(now);
                }
            },
            
            // System Time - PGN 126992
            N2kMessage::NMEASystemTime(system_time) => {
                if self.should_broadcast(self.last_system_time_broadcast, now) {
                    // Extract GNSS time from the system time message
                    let gnss_timestamp_secs = system_time.date_time.to_unix_timestamp();
                    let gnss_timestamp_ms = gnss_timestamp_secs * 1000 + system_time.date_time.milliseconds() as i64;
                    
                    let msg = RealtimeMessage::SystemTime {
                        time_sync_status: self.last_time_sync_status.clone().unwrap_or_else(|| "unknown".to_string()),
                        time_skew_ms: self.last_time_skew_ms.unwrap_or(0),
                        timestamp: gnss_timestamp_ms, // Use actual GNSS time from the NMEASystemTime message
                    };
                    let _ = channels.send(msg);
                    self.last_system_time_broadcast = Some(now);
                }
            },
            
            // All other messages - ignore
            _ => {}
        }
    }
}
