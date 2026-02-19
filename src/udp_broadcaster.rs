use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::Instant;
use tracing::{debug, warn, error};
use nmea2k::pgns::N2kMessage;
use nmea2k::pgns::{HeadingReference, WindReference, GnssMethod};
use nmea2k::{MessageHandler, N2kFrame};
use chrono::Datelike;

/// Aggregated state used to build RMC and GGA sentences
#[derive(Debug, Clone, Default)]
struct RmcState {
    latitude: Option<f64>,           // decimal degrees
    longitude: Option<f64>,          // decimal degrees
    sog_knots: Option<f64>,          // speed over ground in knots
    cog_true_deg: Option<f64>,       // course over ground in degrees
    altitude_m: Option<f64>,         // altitude in meters (from GNSS)
    num_svs: Option<u8>,             // number of satellites
    hdop: Option<f64>,               // horizontal dilution of precision
    n2k_date: Option<u16>,           // N2K date (days since 1970-01-01)
    n2k_time_secs: Option<f64>,      // N2K time (seconds since midnight)
    fix_quality: Option<u8>,         // 0=invalid,1=GPS,2=DGPS,etc.
}

/// UDP broadcaster for NMEA2000 messages
/// 
/// Converts incoming NMEA2000 messages to NMEA0183 format and broadcasts them
/// over UDP to a configured destination address. Rate-limited to 1 message per
/// second per topic.
pub struct UdpBroadcaster {
    socket: Arc<Mutex<Option<UdpSocket>>>,
    destination: String,
    enabled: bool,
    error_count: u64,
    message_count: u64,
    rate_limiter: HashMap<String, Instant>,
    rmc_state: RmcState,
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
            rate_limiter: HashMap::new(),
            rmc_state: RmcState::default(),
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

    /// Send an NMEA0183 sentence if rate limit permits
    fn maybe_send(&mut self, topic: &str, sentence: &str) {
        if !self.enabled {
            return;
        }

        let now = Instant::now();
        let last_send = self.rate_limiter.get(topic).copied();
        
        // Check if we should send (1 second rate limit per topic)
        if let Some(last) = last_send {
            if now.duration_since(last).as_millis() < 900 /* account for some wiggle room, other wise we end up with 2s period instead of 1s */ {
                return;
            }
        }

        let socket_guard = match self.socket.lock() {
            Ok(guard) => {
                if guard.is_none() {
                    return;
                }
                guard
            }
            Err(e) => {
                if self.error_count < 10 {
                    warn!("Failed to acquire UDP socket lock: {}", e);
                }
                self.error_count += 1;
                return;
            }
        };

        if let Some(ref socket) = *socket_guard {
            match socket.send_to(sentence.as_bytes(), &self.destination) {
                Ok(_) => {
                    self.message_count += 1;
                    self.rate_limiter.insert(topic.to_string(), now);
                    if self.message_count % 1000 == 0 {
                        debug!("Broadcasted {} NMEA0183 messages via UDP", self.message_count);
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

    /// Get statistics - for future uses
    /// Returns (message_count, error_count)
    #[allow(dead_code)]
    pub fn stats(&self) -> (u64, u64) {
        (self.message_count, self.error_count)
    }
}



// ============================================================================
// NMEA0183 Helper Functions
// ============================================================================

/// Calculate the NMEA0183 checksum (XOR of all bytes in the sentence body)
#[allow(dead_code)]
fn nmea0183_checksum(sentence: &str) -> String {
    let mut checksum = 0u8;
    for byte in sentence.as_bytes() {
        checksum ^= byte;
    }
    format!("{:02X}", checksum)
}

/// Convert N2K date (days since 1970-01-01) to NMEA0183 date string (DDMMYY)
fn format_n2k_date(days_since_epoch: u16) -> Result<String, Box<dyn std::error::Error>> {
    use chrono::Duration;
    let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1)
        .ok_or("Invalid epoch date")?;
    let date = epoch + Duration::days(days_since_epoch as i64);
    Ok(format!("{:02}{:02}{:02}", date.day(), date.month(), date.year() % 100))
}

/// Convert N2K time (seconds since midnight) to NMEA0183 time string (HHMMSS.ss)
fn format_n2k_time(seconds_since_midnight: f64) -> String {
    let total_secs = seconds_since_midnight as u32;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    let frac = (seconds_since_midnight - total_secs as f64) * 100.0;
    format!("{:02}{:02}{:02}.{:02}", hours, minutes, secs, frac as u8)
}

/// Format decimal degrees to NMEA0183 position format (DDMM.mmmm)
fn format_position(degrees: f64) -> String {
    let abs_deg = degrees.abs();
    let deg = abs_deg.trunc() as u32;
    let min = (abs_deg - deg as f64) * 60.0;
    format!("{:02}{:08.4}", deg, min)
}

/// Determine N or S for latitude, E or W for longitude
fn lat_direction(lat: f64) -> &'static str {
    if lat >= 0.0 { "N" } else { "S" }
}

fn lon_direction(lon: f64) -> &'static str {
    if lon >= 0.0 { "E" } else { "W" }
}

/// Format an RMC sentence (Recommended Minimum Navigation Information)
/// Aggregates position, COG, SOG, and date/time
fn format_rmc(state: &RmcState) -> Option<String> {
    let lat = state.latitude?;
    let lon = state.longitude?;
    let sog_kn = state.sog_knots?;
    let cog = state.cog_true_deg?;
    let date_n2k = state.n2k_date?;
    let time_n2k = state.n2k_time_secs?;

    let time_str = format_n2k_time(time_n2k);
    let lat_str = format_position(lat);
    let lon_str = format_position(lon);
    let date_str = format_n2k_date(date_n2k).ok()?;

    // $IIRMC,HHMMSS.ss,A,DDMM.mmmm,N,DDDMM.mmmm,W,SOG,COG,T,,,DDMMYY
    let sentence_body = format!(
        "IIRMC,{},A,{},{},{},{},{},{:.0},T,,,{}",
        time_str,
        lat_str, lat_direction(lat),
        lon_str, lon_direction(lon),
        sog_kn,
        cog,
        date_str
    );
    Some(format!("${}\r\n", sentence_body))
}

/// Format a GGA sentence (Global Positioning System Fix Data)
fn format_gga(state: &RmcState) -> Option<String> {
    let lat = state.latitude?;
    let lon = state.longitude?;
    let time_n2k = state.n2k_time_secs?;
    let altitude = state.altitude_m?;
    let fix_quality = state.fix_quality?;
    let num_sats = state.num_svs?;
    let hdop = state.hdop?;

    let time_str = format_n2k_time(time_n2k);
    let lat_str = format_position(lat);
    let lon_str = format_position(lon);

    // $IIGGA,HHMMSS.ss,DDMM.mmmm,N,DDDMM.mmmm,E,fix_quality,num_sats,hdop,altitude,M,0.0,M,,*checksum
    let sentence_body = format!(
        "IIGGA,{},{},{},{},{},{},{},{:.1},{:.1},M,,M,,",
        time_str,
        lat_str, lat_direction(lat),
        lon_str, lon_direction(lon),
        fix_quality,
        num_sats,
        hdop,
        altitude
    );
    Some(format!("${}\r\n", sentence_body))
}

/// Format a ZDA sentence (Time & Date)
fn format_zda(date_n2k: u16, time_n2k: f64) -> Option<String> {
    let time_str = format_n2k_time(time_n2k);
    let date_str = format_n2k_date(date_n2k).ok()?;

    // Parse date string DDMMYY
    let day: u32 = date_str[0..2].parse().ok()?;
    let month: u32 = date_str[2..4].parse().ok()?;
    let year_short: u32 = date_str[4..6].parse().ok()?;
    let year = if year_short >= 70 { 1900 + year_short } else { 2000 + year_short };

    // $IIZDA,HHMMSS.ss,DD,MM,YYYY,00,00*checksum
    let sentence_body = format!(
        "IIZDA,{},{:02},{:02},{:04},00,00",
        time_str, day, month, year
    );
    Some(format!("${}\r\n", sentence_body))
}

/// Format an MWV sentence (Wind Speed and Angle)
fn format_mwv(angle_rad: f64, speed_ms: f64, reference: &WindReference) -> Option<String> {
    let angle_deg = angle_rad.to_degrees();
    let speed_kn = speed_ms * 1.94384;  // m/s to knots
    let ref_str = match reference {
        WindReference::Apparent => "R",
        WindReference::TrueBoat | WindReference::TrueWater | WindReference::TrueGroundNorth | WindReference::Magnetic => "T",
    };

    let sentence_body = format!(
        "IIMWV,{:.1},{},,{:.1},N",
        angle_deg, ref_str, speed_kn
    );
    Some(format!("${}\r\n", sentence_body))
}

/// Format HDT (True Heading) or HDM (Magnetic Heading) sentence
fn format_heading(heading_rad: f64, reference: &HeadingReference) -> (Option<String>, Option<String>) {
    let heading_deg = heading_rad.to_degrees() % 360.0;
    
    let hdt = if matches!(reference, HeadingReference::True) {
        let sentence_body = format!("IIHDT,{:.1},T", heading_deg);
        Some(format!("${}\r\n", sentence_body))
    } else {
        None
    };

    let hdm = if matches!(reference, HeadingReference::Magnetic) {
        let sentence_body = format!("IIHDM,{:.1},M", heading_deg);
        Some(format!("${}\r\n", sentence_body))
    } else {
        None
    };

    (hdt, hdm)
}

/// Format ROT (Rate of Turn) sentence
fn format_rot(rate_rad_s: f64) -> Option<String> {
    let rate_deg_min = rate_rad_s.to_degrees() * 60.0;
    let sentence_body = format!("IIROT,{:.1},A", rate_deg_min);
    Some(format!("${}\r\n", sentence_body))
}

/// Format XDR sentence for attitude (yaw/pitch/roll in degrees)
fn format_xdr_attitude(yaw: Option<f64>, pitch: Option<f64>, roll: Option<f64>) -> Option<String> {
    if yaw.is_none() && pitch.is_none() && roll.is_none() {
        return None;
    }
    let body_parts: Vec<String> = vec![
        yaw.map(|y| format!("A,{:.1},D,YAW", y.to_degrees()))
            .or_else(|| Some(String::new()))
            .filter(|s| !s.is_empty()),
        pitch.map(|p| format!("A,{:.1},D,PITCH", p.to_degrees()))
            .or_else(|| Some(String::new()))
            .filter(|s| !s.is_empty()),
        roll.map(|r| format!("A,{:.1},D,ROLL", r.to_degrees()))
            .or_else(|| Some(String::new()))
            .filter(|s| !s.is_empty()),
    ]
    .into_iter()
    .filter_map(|x| x)
    .collect();
    
    if body_parts.is_empty() {
        return None;
    }
    
    let sentence_body = format!("IIXDR,{}", body_parts.join(","));
    Some(format!("${}\r\n", sentence_body))
}

/// Format RPM sentence
fn format_rpm(engine_instance: u8, rpm: Option<f64>) -> Option<String> {
    let rpm_val = rpm?;
    let sentence_body = format!(
        "IIRPM,E,{},{:.0},,A",
        engine_instance, rpm_val
    );
    Some(format!("${}\r\n", sentence_body))
}

/// Format VHW sentence (Water Speed and Heading)
fn format_vhw(speed_ms: Option<f64>) -> Option<String> {
    let speed_kn = speed_ms? * 1.94384;
    let sentence_body = format!("IIVHW,,,,,{:.1},N,,,K", speed_kn);
    Some(format!("${}\r\n", sentence_body))
}

/// Format DPT sentence (Water Depth)
fn format_dpt(depth_m: f64, offset_m: Option<f64>) -> Option<String> {
    let offset = offset_m.unwrap_or(0.0);
    let sentence_body = format!("IIDPT,{:.1},{:.1},", depth_m, offset);
    Some(format!("${}\r\n", sentence_body))
}

/// Format XDR transducer sentence for temperature
fn format_xdr_temperature(instance: u8, source: u8, kelvin: f64) -> Option<String> {
    let celsius = kelvin - 273.15;
    
    // Map source to transducer name
    let transducer_name = match source {
        0 => "WATER",
        1 => "AIR",
        2 => "CABIN",
        _ => &format!("TEMP_{}", instance),
    };

    let sentence_body = format!(
        "IIXDR,C,{:.1},C,{}",
        celsius, transducer_name
    );
    Some(format!("${}\r\n", sentence_body))
}

/// Format XDR transducer sentence for humidity
fn format_xdr_humidity(instance: u8, humidity_pct: f64) -> Option<String> {
    let sentence_body = format!(
        "IIXDR,H,{:.1},P,HUMIDITY_{}",
        humidity_pct, instance
    );
    Some(format!("${}\r\n", sentence_body))
}

/// Format XDR transducer sentence for barometric pressure (Pa to bar)
fn format_xdr_pressure(instance: u8, pressure_pa: f64) -> Option<String> {
    let pressure_bar = pressure_pa / 100000.0;
    let sentence_body = format!(
        "IIXDR,P,{:.4},B,BARO_{}",
        pressure_bar, instance
    );
    Some(format!("${}\r\n", sentence_body))
}

impl MessageHandler for UdpBroadcaster {
    fn handle_message(&mut self, frame: &N2kFrame, _timestamp: std::time::Instant) {
        match &frame.message {
            // Position - update state only
            N2kMessage::PositionRapidUpdate(msg) => {
                self.rmc_state.latitude = Some(msg.latitude);
                self.rmc_state.longitude = Some(msg.longitude);
            }

            // COG/SOG - update state only (convert from m/s to knots, rad to degrees)
            N2kMessage::CogSogRapidUpdate(msg) => {
                self.rmc_state.sog_knots = Some(msg.sog * 1.94384); // m/s to knots
                self.rmc_state.cog_true_deg = Some(msg.cog.to_degrees() % 360.0);
            }

            // System Time - update state, emit ZDA
            N2kMessage::NMEASystemTime(msg) => {
                self.rmc_state.n2k_date = Some(msg.date_time.date);
                self.rmc_state.n2k_time_secs = Some(msg.date_time.time);

                if let Some(zda) = format_zda(msg.date_time.date, msg.date_time.time) {
                    self.maybe_send("zda", &zda);
                }
            }

            // GNSS Position - update state, emit RMC and GGA
            N2kMessage::GnssPositionData(msg) => {
                self.rmc_state.latitude = Some(msg.latitude);
                self.rmc_state.longitude = Some(msg.longitude);
                self.rmc_state.altitude_m = Some(msg.altitude);
                self.rmc_state.num_svs = Some(msg.num_svs);
                self.rmc_state.hdop = Some(msg.hdop);
                self.rmc_state.n2k_date = Some(msg.date_time.date);
                self.rmc_state.n2k_time_secs = Some(msg.date_time.time);
                
                // Map GnssMethod to fix quality: 0=invalid, 1=GPS, 2=DGPS
                self.rmc_state.fix_quality = Some(match msg.method {
                    GnssMethod::NoGnss => 0,
                    GnssMethod::GnssFix => 1,
                    GnssMethod::DGnss => 2,
                    GnssMethod::PreciseGnss => 4,
                    GnssMethod::RtkFixed => 5,
                    GnssMethod::RtkFloat => 6,
                });

                if let Some(rmc) = format_rmc(&self.rmc_state) {
                    self.maybe_send("rmc", &rmc);
                }
                if let Some(gga) = format_gga(&self.rmc_state) {
                    self.maybe_send("gga", &gga);
                }
            }

            // Wind Data - PGN 130306
            N2kMessage::WindData(msg) => {
                let topic = match msg.reference {
                    WindReference::Apparent => "mwv_apparent",
                    _ => "mwv_true",
                };
                if let Some(mwv) = format_mwv(msg.angle, msg.speed, &msg.reference) {
                    self.maybe_send(topic, &mwv);
                }
            }

            // Vessel Heading - PGN 127250
            N2kMessage::VesselHeading(msg) => {
                let (hdt, hdm) = format_heading(msg.heading, &msg.reference);
                if let Some(sentence) = hdt {
                    self.maybe_send("hdt", &sentence);
                }
                if let Some(sentence) = hdm {
                    self.maybe_send("hdm", &sentence);
                }
            }

            // Rate of Turn - PGN 127251
            N2kMessage::RateOfTurn(msg) => {
                if let Some(rot) = format_rot(msg.rate) {
                    self.maybe_send("rot", &rot);
                }
            }

            // Attitude - PGN 127257 (yaw/pitch/roll)
            N2kMessage::Attitude(msg) => {
                if let Some(xdr) = format_xdr_attitude(msg.yaw, msg.pitch, msg.roll) {
                    self.maybe_send("xdr_attitude", &xdr);
                }
            }

            // Engine Rapid Update - PGN 127488
            N2kMessage::EngineRapidUpdate(msg) => {
                let instance = msg.engine_instance;
                if let Some(rpm_val) = msg.engine_speed {
                    let topic = format!("rpm_{}", instance);
                    if let Some(rpm_sentence) = format_rpm(instance, Some(rpm_val)) {
                        self.maybe_send(&topic, &rpm_sentence);
                    }
                }
            }

            // Speed Water Referenced - PGN 128259
            N2kMessage::SpeedWaterReferenced(msg) => {
                if let Some(vhw) = format_vhw(Some(msg.speed)) {
                    self.maybe_send("vhw", &vhw);
                }
            }

            // Water Depth - PGN 128267
            N2kMessage::WaterDepth(msg) => {
                if let Some(dpt) = format_dpt(msg.depth, Some(msg.offset)) {
                    self.maybe_send("dpt", &dpt);
                }
            }

            // Temperature - PGN 130312
            N2kMessage::Temperature(msg) => {
                let topic = format!("xdr_temp_{}", msg.instance);
                if let Some(xdr) = format_xdr_temperature(msg.instance, msg.source, msg.temperature) {
                    self.maybe_send(&topic, &xdr);
                }
            }

            // Humidity - PGN 130313
            N2kMessage::Humidity(msg) => {
                let topic = format!("xdr_hum_{}", msg.instance);
                if let Some(xdr) = format_xdr_humidity(msg.instance, msg.actual_humidity) {
                    self.maybe_send(&topic, &xdr);
                }
            }

            // Pressure - PGN 130314
            N2kMessage::ActualPressure(msg) => {
                let topic = format!("xdr_pres_{}", msg.instance);
                if let Some(xdr) = format_xdr_pressure(msg.instance, msg.pressure) {
                    self.maybe_send(&topic, &xdr);
                }
            }

            // Ignore all other message types
            _ => {}
        }
    }
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nmea0183_checksum() {
        // Example: IIHDT,10.5,T should produce a specific checksum
        let checksum = nmea0183_checksum("IIHDT,10.5,T");
        assert!(!checksum.is_empty());
        assert_eq!(checksum.len(), 2); // Two hex digits
    }

    #[test]
    fn test_format_position_north() {
        let lat = 52.373126; // degrees
        let formatted = format_position(lat);
        // Should be 52°22.3876' which is 5222.3876
        assert!(formatted.starts_with("52"));
    }

    #[test]
    fn test_lat_direction() {
        assert_eq!(lat_direction(52.5), "N");
        assert_eq!(lat_direction(-52.5), "S");
    }

    #[test]
    fn test_lon_direction() {
        assert_eq!(lon_direction(13.4), "E");
        assert_eq!(lon_direction(-13.4), "W");
    }

    #[test]
    fn test_format_n2k_time() {
        // 43200.0 seconds = 12:00:00
        let time_str = format_n2k_time(43200.0);
        assert!(time_str.contains("12"));
    }

    #[test]
    fn test_format_rmc_incomplete_state() {
        let state = RmcState::default();
        let result = format_rmc(&state);
        assert!(result.is_none(), "RMC should not format with missing data");
    }

    #[test]
    fn test_format_rmc_complete_state() {
        let mut state = RmcState::default();
        state.latitude = Some(52.373126);
        state.longitude = Some(13.403818);
        state.sog_knots = Some(5.2);
        state.cog_true_deg = Some(45.0);
        state.n2k_date = Some(19254); // Some date in N2K format
        state.n2k_time_secs = Some(43200.0);

        let result = format_rmc(&state);
        assert!(result.is_some(), "RMC should format with complete state");
        let rmc = result.unwrap();
        assert!(rmc.starts_with("$IIRMC"));
    }

    #[test]
    fn test_format_gga_incomplete_state() {
        let state = RmcState::default();
        let result = format_gga(&state);
        assert!(result.is_none(), "GGA should not format with missing data");
    }

    #[test]
    fn test_format_gga_complete_state() {
        let mut state = RmcState::default();
        state.latitude = Some(52.373126);
        state.longitude = Some(13.403818);
        state.n2k_time_secs = Some(43200.0);
        state.altitude_m = Some(100.0);
        state.fix_quality = Some(1);
        state.num_svs = Some(12);
        state.hdop = Some(1.5);

        let result = format_gga(&state);
        assert!(result.is_some(), "GGA should format with complete state");
        let gga = result.unwrap();
        assert!(gga.starts_with("$IIGGA"));
    }

    #[test]
    fn test_format_mwv_apparent() {
        let angle_rad = std::f64::consts::PI / 4.0; // 45 degrees
        let speed_ms = 5.0; // m/s
        let ref_type = WindReference::Apparent;

        let result = format_mwv(angle_rad, speed_ms, &ref_type);
        assert!(result.is_some());
        let mwv = result.unwrap();
        assert!(mwv.starts_with("$IIMWV"));
        assert!(mwv.contains(",R,"));
    }

    #[test]
    fn test_format_mwv_true() {
        let angle_rad = std::f64::consts::PI / 4.0;
        let speed_ms = 5.0;
        let ref_type = WindReference::TrueBoat;

        let result = format_mwv(angle_rad, speed_ms, &ref_type);
        assert!(result.is_some());
        let mwv = result.unwrap();
        assert!(mwv.contains(",T,"));
    }

    #[test]
    fn test_format_heading_true() {
        let heading_rad = std::f64::consts::PI / 4.0;
        let ref_type = HeadingReference::True;

        let (hdt, hdm) = format_heading(heading_rad, &ref_type);
        assert!(hdt.is_some());
        assert!(hdm.is_none());
        let hdt_sentence = hdt.unwrap();
        assert!(hdt_sentence.starts_with("$IIHDT"));
    }

    #[test]
    fn test_format_heading_magnetic() {
        let heading_rad = std::f64::consts::PI / 4.0;
        let ref_type = HeadingReference::Magnetic;

        let (hdt, hdm) = format_heading(heading_rad, &ref_type);
        assert!(hdt.is_none());
        assert!(hdm.is_some());
        let hdm_sentence = hdm.unwrap();
        assert!(hdm_sentence.starts_with("$IIHDM"));
    }

    #[test]
    fn test_format_rot() {
        let rate_rad_s = 0.1; // rad/s
        let result = format_rot(rate_rad_s);
        assert!(result.is_some());
        let rot = result.unwrap();
        assert!(rot.starts_with("$IIROT"));
    }

    #[test]
    fn test_format_xdr_attitude() {
        let yaw = Some(std::f64::consts::PI / 4.0);
        let pitch = Some(std::f64::consts::PI / 6.0);
        let roll = Some(std::f64::consts::PI / 8.0);

        let result = format_xdr_attitude(yaw, pitch, roll);
        assert!(result.is_some());
        let xdr = result.unwrap();
        assert!(xdr.starts_with("$IIXDR"));
    }

    #[test]
    fn test_format_rpm() {
        let result = format_rpm(0, Some(1500.0));
        assert!(result.is_some());
        let rpm = result.unwrap();
        assert!(rpm.starts_with("$IIRPM"));
        assert!(rpm.contains("1500"));
    }

    #[test]
    fn test_format_vhw() {
        let result = format_vhw(Some(2.5)); // m/s
        assert!(result.is_some());
        let vhw = result.unwrap();
        assert!(vhw.starts_with("$IIVHW"));
    }

    #[test]
    fn test_format_dpt() {
        let result = format_dpt(5.5, Some(0.0));
        assert!(result.is_some());
        let dpt = result.unwrap();
        assert!(dpt.starts_with("$IIDPT"));
    }

    #[test]
    fn test_format_xdr_temperature() {
        let result = format_xdr_temperature(0, 0, 293.15); // 20°C
        assert!(result.is_some());
        let xdr = result.unwrap();
        assert!(xdr.starts_with("$IIXDR"));
        assert!(xdr.contains("20"));
    }

    #[test]
    fn test_format_xdr_humidity() {
        let result = format_xdr_humidity(0, 65.0);
        assert!(result.is_some());
        let xdr = result.unwrap();
        assert!(xdr.starts_with("$IIXDR"));
        assert!(xdr.contains("65"));
    }

    #[test]
    fn test_format_xdr_pressure() {
        let result = format_xdr_pressure(0, 101325.0); // ~1 atm
        assert!(result.is_some());
        let xdr = result.unwrap();
        assert!(xdr.starts_with("$IIXDR"));
    }

    #[test]
    fn test_create_disabled_broadcaster() {
        let broadcaster = UdpBroadcaster::new("127.0.0.1:10110".to_string(), false);
        assert!(!broadcaster.enabled);
        assert!(broadcaster.socket.lock().unwrap().is_none());
    }

    #[test]
    fn test_broadcaster_initialization() {
        let broadcaster = UdpBroadcaster::new("127.0.0.1:10110".to_string(), false);
        assert_eq!(broadcaster.message_count, 0);
        assert_eq!(broadcaster.error_count, 0);
        assert!(broadcaster.rmc_state.latitude.is_none());
    }
}

