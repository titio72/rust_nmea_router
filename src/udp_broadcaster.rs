use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::Instant;
use tracing::{debug, warn};
use nmea2k::pgns::N2kMessage;
use nmea2k::pgns::{HeadingReference, WindReference, GnssMethod};
use nmea2k::pgns::{AisClassAPositionReport, AisClassBPositionReport,
    AisClassAStaticData, AisClassBStaticDataPartA, AisClassBStaticDataPartB};
use nmea2k::{MessageHandler, N2kFrame};
use chrono::{Datelike, TimeZone, Timelike, Utc};

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
    /// * `bind_address` - Local interface/address to bind to (e.g., "192.168.1.10:0" or "0.0.0.0:0")
    /// * `enabled` - Whether UDP broadcasting is enabled
    ///
    /// # Returns
    /// * `Ok(Self)` - Broadcaster created successfully (or disabled)
    /// * `Err(String)` - Socket creation failed when enabled
    pub fn new(destination: String, bind_address: String, enabled: bool) -> Result<Self, String> {
        let socket = if enabled {
            match Self::create_socket(&destination, &bind_address) {
                Ok(sock) => {
                    debug!("UDP broadcaster initialized: {} -> {}", bind_address, destination);
                    Some(sock)
                }
                Err(e) => {
                    return Err(format!("Failed to bind UDP socket to {}: {}", bind_address, e));
                }
            }
        } else {
            debug!("UDP broadcaster disabled in configuration");
            None
        };

        Ok(Self {
            socket: Arc::new(Mutex::new(socket)),
            destination,
            enabled,
            error_count: 0,
            message_count: 0,
            rate_limiter: HashMap::new(),
            rmc_state: RmcState::default(),
        })
    }

    /// Create and configure a UDP socket
    fn create_socket(destination: &str, bind_address: &str) -> Result<UdpSocket, std::io::Error> {
        let socket = UdpSocket::bind(bind_address)?;
        
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

        // Cleanup stale entries if map is getting large (prevents unbounded growth from AIS MMSIs)
        if self.rate_limiter.len() > 500 {
            self.cleanup_stale_rate_limiter_entries(now);
        }

        let last_send = self.rate_limiter.get(topic).copied();

        // Check if we should send (1 second rate limit per topic)
        if let Some(last) = last_send {
            if now.duration_since(last).as_millis() < 900 /* account for some wiggle room, otherwise we end up with 2s period instead of 1s */ {
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

    /// Remove rate limiter entries older than 5 minutes to prevent unbounded growth
    /// Called when the map exceeds 500 entries (primarily from AIS MMSI-based topics)
    fn cleanup_stale_rate_limiter_entries(&mut self, now: Instant) {
        let stale_threshold = std::time::Duration::from_secs(300); // 5 minutes
        let before_count = self.rate_limiter.len();

        self.rate_limiter.retain(|_, last_send| {
            now.duration_since(*last_send) < stale_threshold
        });

        let removed = before_count - self.rate_limiter.len();
        if removed > 0 {
            debug!("Cleaned up {} stale rate limiter entries, {} remaining", removed, self.rate_limiter.len());
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
//    let frac = (seconds_since_midnight - total_secs as f64) * 100.0;
//    format!("{:02}{:02}{:02}.{:02}", hours, minutes, secs, frac as u8)
    format!("{:02}{:02}{:02}", hours, minutes, secs)
}

/// Format decimal degrees to NMEA0183 position format (DDMM.mmmm)
fn format_latitude(degrees: f64) -> String {
    let abs_deg = degrees.abs();
    let deg = abs_deg.trunc() as u32;
    let min = (abs_deg - deg as f64) * 60.0;
    format!("{:02}{:07.4}", deg, min)
}

fn format_longitude(degrees: f64) -> String {
    let abs_deg = degrees.abs();
    let deg = abs_deg.trunc() as u32;
    let min = (abs_deg - deg as f64) * 60.0;
    format!("{:03}{:07.4}", deg, min)
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
    let lat_str = format_latitude(lat);
    let lon_str = format_longitude(lon);
    let date_str = format_n2k_date(date_n2k).ok()?;

    // $IIRMC,HHMMSS.ss,A,DDMM.mmmm,N,DDDMM.mmmm,W,SOG,COG,T,,,DDMMYY
    let sentence_body = format!(
        "IIRMC,{},A,{},{},{},{},{},{:.0},{}",
        time_str,
        lat_str, lat_direction(lat),
        lon_str, lon_direction(lon),
        sog_kn,
        cog,
        date_str
    );
    let checksum = nmea0183_checksum(&sentence_body);
    Some(format!("${}*{}\r\n", sentence_body, checksum))
}

/// Format a GGA sentence (Global Positioning System Fix Data)
fn format_gga(state: &RmcState) -> Option<String> {
    let lat = state.latitude?;
    let lon = state.longitude?;
    let time_n2k = state.n2k_time_secs?;
    let fix_quality = state.fix_quality?;
    let num_sats = state.num_svs?;
    let hdop = state.hdop?;

    let time_str = format_n2k_time(time_n2k);
    let lat_str = format_latitude(lat);
    let lon_str = format_longitude(lon);

    // $IIGGA,HHMMSS.ss,DDMM.mmmm,N,DDDMM.mmmm,E,fix_quality,num_sats,hdop,altitude,M,0.0,M,,*checksum
    let sentence_body = format!(
        "IIGGA,{},{},{},{},{},{},{},{:.1},{:.1},M,0.0,M,,",
        time_str,
        lat_str, lat_direction(lat),
        lon_str, lon_direction(lon),
        fix_quality,
        num_sats,
        hdop,
        0.0
    );
    let checksum = nmea0183_checksum(&sentence_body);
    Some(format!("${}*{}\r\n", sentence_body, checksum))
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
    let checksum = nmea0183_checksum(&sentence_body);
    Some(format!("${}*{}\r\n", sentence_body, checksum))
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
        "IIMWV,{:.1},{},{:.1},N,A",
        angle_deg, ref_str, speed_kn
    );
    let checksum = nmea0183_checksum(&sentence_body);
    Some(format!("${}*{}\r\n", sentence_body, checksum))
}

/// Format HDT (True Heading) or HDM (Magnetic Heading) sentence
fn format_heading(heading_rad: f64, reference: &HeadingReference) -> (Option<String>, Option<String>) {
    let heading_deg = heading_rad.to_degrees() % 360.0;
    
    let hdt = if matches!(reference, HeadingReference::True) {
        let sentence_body = format!("IIHDT,{:.1},T", heading_deg);
        let checksum = nmea0183_checksum(&sentence_body);
        Some(format!("${}*{}\r\n", sentence_body, checksum))
    } else {
        None
    };

    let hdm = if matches!(reference, HeadingReference::Magnetic) {
        let sentence_body = format!("IIHDM,{:.1},M", heading_deg);
        let checksum = nmea0183_checksum(&sentence_body);
        Some(format!("${}*{}\r\n", sentence_body, checksum))
    } else {
        None
    };

    (hdt, hdm)
}

/// Format ROT (Rate of Turn) sentence
fn format_rot(rate_rad_s: f64) -> Option<String> {
    let rate_deg_min = rate_rad_s.to_degrees() * 60.0;
    let sentence_body = format!("IIROT,{:.1},A", rate_deg_min);
    let checksum = nmea0183_checksum(&sentence_body);
    Some(format!("${}*{}\r\n", sentence_body, checksum))
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
    let checksum = nmea0183_checksum(&sentence_body);
    Some(format!("${}*{}\r\n", sentence_body, checksum))
}

/// Format RPM sentence
fn format_rpm(engine_instance: u8, rpm: Option<f64>) -> Option<String> {
    let rpm_val = rpm?;
    let sentence_body = format!(
        "IIRPM,E,{},{:.0},,A",
        engine_instance, rpm_val
    );
    let checksum = nmea0183_checksum(&sentence_body);
    Some(format!("${}*{}\r\n", sentence_body, checksum))
}

/// Format VHW sentence (Water Speed and Heading)
fn format_vhw(speed_ms: Option<f64>) -> Option<String> {
    let speed_kn = speed_ms? * 1.94384;
    let sentence_body = format!("IIVHW,,,,,{:.1},N,,,K", speed_kn);
    let checksum = nmea0183_checksum(&sentence_body);
    Some(format!("${}*{}\r\n", sentence_body, checksum))
}

/// Format DPT sentence (Water Depth)
fn format_dpt(depth_m: f64, offset_m: f64) -> Option<String> {
    let sentence_body = format!("IIDPT,{:.1},{:.1},", depth_m, offset_m);
    let checksum = nmea0183_checksum(&sentence_body);
    Some(format!("${}*{}\r\n", sentence_body, checksum))
}

/// Format DBT sentence (Water Depth)
#[allow(dead_code)]
fn format_dbt(depth_m: f64) -> Option<String> {
    let sentence_body = format!("IIDBT,{:.1},f,{:.1},M,{:.1},F", depth_m * 3.28084, depth_m, depth_m * 0.546807);
    let checksum = nmea0183_checksum(&sentence_body);
    Some(format!("${}*{}\r\n", sentence_body, checksum))
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
    let checksum = nmea0183_checksum(&sentence_body);
    Some(format!("${}*{}\r\n", sentence_body, checksum))
}

/// Format XDR transducer sentence for humidity
fn format_xdr_humidity(instance: u8, humidity_pct: f64) -> Option<String> {
    let sentence_body = format!(
        "IIXDR,H,{:.1},P,HUMIDITY_{}",
        humidity_pct, instance
    );
    let checksum = nmea0183_checksum(&sentence_body);
    Some(format!("${}*{}\r\n", sentence_body, checksum))
}

/// Format XDR transducer sentence for barometric pressure (Pa to bar)
fn format_xdr_pressure(instance: u8, pressure_pa: f64) -> Option<String> {
    let pressure_bar = pressure_pa / 100000.0;
    let sentence_body = format!(
        "IIXDR,P,{:.4},B,BARO_{}",
        pressure_bar, instance
    );
    let checksum = nmea0183_checksum(&sentence_body);
    Some(format!("${}*{}\r\n", sentence_body, checksum))
}

// ============================================================================
// AIS VDM Encoding Helpers
// ============================================================================

/// Bit-writer for packing AIS binary payloads
struct BitWriter {
    bits: Vec<u8>,
}

impl BitWriter {
    fn new() -> Self {
        Self { bits: Vec::new() }
    }

    /// Write `num_bits` MSB-first from `value`
    fn write_u(&mut self, value: u64, num_bits: u8) {
        for i in (0..num_bits).rev() {
            self.bits.push(((value >> i) & 1) as u8);
        }
    }

    /// Write `num_bits` MSB-first from a signed value (two's complement)
    fn write_i(&mut self, value: i64, num_bits: u8) {
        for i in (0..num_bits).rev() {
            self.bits.push(((value >> i) & 1) as u8);
        }
    }

    /// Write an AIS 6-bit text field of exactly `num_chars` characters.
    /// Names/callsigns are AIS uppercase ASCII: '@'-'_' → 0-31, ' '-'?' → 32-63.
    fn write_text(&mut self, text: &str, num_chars: usize) {
        let bytes: Vec<u8> = text.bytes().collect();
        for i in 0..num_chars {
            let b = if i < bytes.len() { bytes[i] } else { b'@' };
            let v: u8 = if b >= 0x40 { b - 0x40 } else { b } & 0x3F;
            self.write_u(v as u64, 6);
        }
    }

    /// Encode accumulated bits as an AIS armoured payload string.
    /// Returns `(payload, fill_bits)` where `fill_bits` pads to a 6-bit boundary.
    fn to_payload(&self) -> (String, u8) {
        let total = self.bits.len();
        let padded = (total + 5) / 6 * 6;
        let fill = (padded - total) as u8;
        let mut payload = String::new();
        let mut i = 0usize;
        while i < padded {
            let mut v = 0u8;
            for _ in 0..6 {
                v = (v << 1) | if i < total { self.bits[i] } else { 0 };
                i += 1;
            }
            // AIS armoring: 0-39 → ASCII 48-87, 40-63 → ASCII 96-119
            payload.push(if v < 40 { (v + 48) as char } else { (v + 56) as char });
        }
        (payload, fill)
    }
}

/// Wrap an AIS payload in a !AIVDM sentence with NMEA checksum
fn wrap_ais_vdm(payload: &str, fill_bits: u8) -> String {
    let body = format!("AIVDM,1,1,,A,{},{}", payload, fill_bits);
    let checksum = nmea0183_checksum(&body);
    format!("!{}*{}\r\n", body, checksum)
}

/// Convert N2K COG raw (0.0001 rad units) to AIS COG (1/10 degree, 0-3599; 3600=not available)
fn n2k_cog_to_ais(cog_raw: u16) -> u16 {
    if cog_raw == 0xFFFF {
        return 3600;
    }
    let deg_tenths = cog_raw as f64 * 0.0001 * (180.0 / std::f64::consts::PI) * 10.0;
    (deg_tenths.round() as u16).min(3599)
}

/// Convert N2K SOG raw (0.01 m/s units) to AIS SOG (1/10 knot; 1023=not available)
fn n2k_sog_to_ais(sog_raw: u16) -> u16 {
    if sog_raw == 0xFFFF {
        return 1023;
    }
    (sog_raw as f64 * 0.194384).round() as u16
}

/// Convert N2K heading raw (0.0001 rad units) to AIS heading (integer degrees 0-359; 511=not available)
fn n2k_heading_to_ais(heading_raw: u16) -> u16 {
    if heading_raw == 0xFFFF {
        return 511;
    }
    ((heading_raw as f64 * 0.0001 * (180.0 / std::f64::consts::PI)).round() as u16) % 360
}

/// Convert N2K ROT raw (3.125e-5 rad/s, signed 16-bit) to AIS ROT (signed 8-bit, 4.733×sqrt units)
fn n2k_rot_to_ais(rot_raw: i16) -> i8 {
    if rot_raw == i16::MAX || rot_raw == i16::MIN {
        return -128i8; // not available
    }
    let rot_deg_min = rot_raw as f64 * 3.125e-5 * (180.0 / std::f64::consts::PI) * 60.0;
    if rot_deg_min.abs() < 0.0001 {
        return 0;
    }
    let ais = 4.733 * rot_deg_min.abs().sqrt() * rot_deg_min.signum();
    ais.clamp(-127.0, 127.0) as i8
}

/// Encode N2K position (1e-7 degrees) to AIS position integer (1/10000 min, 28-bit for lon, 27-bit for lat)
fn n2k_lon_to_ais(lon_raw: i32) -> i32 {
    (lon_raw as f64 * 1e-7 * 600_000.0).round() as i32
}

fn n2k_lat_to_ais(lat_raw: i32) -> i32 {
    (lat_raw as f64 * 1e-7 * 600_000.0).round() as i32
}

/// Format !AIVDM sentence for AIS Type 1 (Class A position report)
fn format_vdm_class_a_position(msg: &AisClassAPositionReport) -> String {
    let mut bw = BitWriter::new();
    bw.write_u(msg.message_id as u64, 6);
    bw.write_u(msg.repeat_indicator as u64, 2);
    bw.write_u(msg.mmsi as u64, 30);
    bw.write_u(msg.nav_status as u8 as u64, 4);
    bw.write_i(n2k_rot_to_ais(msg.rate_of_turn_raw) as i64, 8);
    bw.write_u(n2k_sog_to_ais(msg.sog_raw) as u64, 10);
    bw.write_u(msg.position_accuracy as u64, 1);
    bw.write_i(n2k_lon_to_ais(msg.longitude_raw) as i64, 28);
    bw.write_i(n2k_lat_to_ais(msg.latitude_raw) as i64, 27);
    bw.write_u(n2k_cog_to_ais(msg.cog_raw) as u64, 12);
    bw.write_u(n2k_heading_to_ais(msg.heading_raw) as u64, 9);
    bw.write_u(msg.time_stamp as u64, 6);
    bw.write_u(msg.special_maneuver_indicator as u64, 2);
    bw.write_u(0, 3); // spare
    bw.write_u(msg.raim as u64, 1);
    bw.write_u(msg.communication_state as u64, 19);
    let (payload, fill) = bw.to_payload();
    wrap_ais_vdm(&payload, fill)
}

/// Format !AIVDM sentence for AIS Type 18 (Class B position report)
fn format_vdm_class_b_position(msg: &AisClassBPositionReport) -> String {
    let mut bw = BitWriter::new();
    bw.write_u(msg.message_id as u64, 6);
    bw.write_u(msg.repeat_indicator as u64, 2);
    bw.write_u(msg.mmsi as u64, 30);
    bw.write_u(0, 8); // reserved
    bw.write_u(n2k_sog_to_ais(msg.sog_raw) as u64, 10);
    bw.write_u(msg.position_accuracy as u64, 1);
    bw.write_i(n2k_lon_to_ais(msg.longitude_raw) as i64, 28);
    bw.write_i(n2k_lat_to_ais(msg.latitude_raw) as i64, 27);
    bw.write_u(n2k_cog_to_ais(msg.cog_raw) as u64, 12);
    bw.write_u(n2k_heading_to_ais(msg.heading_raw) as u64, 9);
    bw.write_u(msg.time_stamp as u64, 6);
    bw.write_u(msg.regional_application as u64, 2);
    bw.write_u(msg.unit_type as u64, 1);
    bw.write_u(msg.integrated_display as u64, 1);
    bw.write_u(msg.dsc as u64, 1);
    bw.write_u(msg.band as u64, 1);
    bw.write_u(msg.can_handle_msg22 as u64, 1);
    bw.write_u(msg.assigned as u64, 1);
    bw.write_u(msg.raim as u64, 1);
    bw.write_u(msg.communication_state as u64 & 0xFFFFF, 20);
    let (payload, fill) = bw.to_payload();
    wrap_ais_vdm(&payload, fill)
}

/// Format !AIVDM sentence for AIS Type 5 (Class A static & voyage data)
fn format_vdm_class_a_static(msg: &AisClassAStaticData) -> String {
    // Derive ETA fields from eta_date_time when available
    let (eta_month, eta_day, eta_hour, eta_minute) = if let Some(ref dt) = msg.eta_date_time {
        let utc = Utc.timestamp_opt(
            (dt.date as i64) * 86400 + dt.time as i64, 0
        ).single();
        if let Some(t) = utc {
            (t.month() as u8, t.day() as u8, t.hour() as u8, t.minute() as u8)
        } else {
            (0u8, 0u8, 24u8, 60u8) // not available
        }
    } else {
        (0u8, 0u8, 24u8, 60u8) // not available
    };

    let to_bow = ((msg.position_ref_bow as f64 * 0.1).round() as u16).min(511);
    let to_stern = ((msg.length_raw as f64 * 0.1 - to_bow as f64).max(0.0).round() as u16).min(511);
    let to_starboard = ((msg.position_ref_starboard as f64 * 0.1).round() as u16).min(63);
    let to_port = ((msg.beam_raw as f64 * 0.1 - to_starboard as f64).max(0.0).round() as u16).min(63);
    let draught_ais = ((msg.get_draft_meters() * 10.0).round() as u8).min(255);

    let mut bw = BitWriter::new();
    bw.write_u(msg.message_id as u64, 6);
    bw.write_u(msg.repeat_indicator as u64, 2);
    bw.write_u(msg.mmsi as u64, 30);
    bw.write_u(msg.ais_version as u64, 2);
    bw.write_u(msg.imo_number as u64, 30);
    bw.write_text(&msg.callsign, 7);
    bw.write_text(&msg.name, 20);
    bw.write_u(msg.type_of_ship as u64, 8);
    bw.write_u(to_bow as u64, 9);
    bw.write_u(to_stern as u64, 9);
    bw.write_u(to_port as u64, 6);
    bw.write_u(to_starboard as u64, 6);
    bw.write_u(msg.gnss_type as u64, 4);
    bw.write_u(eta_month as u64, 4);
    bw.write_u(eta_day as u64, 5);
    bw.write_u(eta_hour as u64, 5);
    bw.write_u(eta_minute as u64, 6);
    bw.write_u(draught_ais as u64, 8);
    bw.write_text(&msg.destination, 20);
    bw.write_u(!msg.dte as u64, 1); // AIS DTE: 0=available, 1=not available
    bw.write_u(0, 1); // spare
    let (payload, fill) = bw.to_payload();
    wrap_ais_vdm(&payload, fill)
}

/// Format !AIVDM sentence for AIS Type 24 Part A (Class B static, vessel name)
fn format_vdm_class_b_static_a(msg: &AisClassBStaticDataPartA) -> String {
    let mut bw = BitWriter::new();
    bw.write_u(msg.message_id as u64, 6);
    bw.write_u(msg.repeat_indicator as u64, 2);
    bw.write_u(msg.mmsi as u64, 30);
    bw.write_u(0, 2); // part number = 0
    bw.write_text(&msg.name, 20);
    bw.write_u(0, 8); // spare
    let (payload, fill) = bw.to_payload();
    wrap_ais_vdm(&payload, fill)
}

/// Format !AIVDM sentence for AIS Type 24 Part B (Class B static, vessel details)
fn format_vdm_class_b_static_b(msg: &AisClassBStaticDataPartB) -> String {
    let to_bow = ((msg.position_ref_bow as f64 * 0.1).round() as u16).min(511);
    let to_stern = ((msg.length_raw as f64 * 0.1 - to_bow as f64).max(0.0).round() as u16).min(511);
    let to_starboard = ((msg.position_ref_starboard as f64 * 0.1).round() as u16).min(63);
    let to_port = ((msg.beam_raw as f64 * 0.1 - to_starboard as f64).max(0.0).round() as u16).min(63);

    let mut bw = BitWriter::new();
    bw.write_u(msg.message_id as u64, 6);
    bw.write_u(msg.repeat_indicator as u64, 2);
    bw.write_u(msg.mmsi as u64, 30);
    bw.write_u(1, 2); // part number = 1
    bw.write_u(msg.type_of_ship as u64, 8);
    bw.write_text(&msg.vendor_id, 7);
    bw.write_text(&msg.callsign, 7);
    bw.write_u(to_bow as u64, 9);
    bw.write_u(to_stern as u64, 9);
    bw.write_u(to_port as u64, 6);
    bw.write_u(to_starboard as u64, 6);
    bw.write_u(0, 6); // spare
    let (payload, fill) = bw.to_payload();
    wrap_ais_vdm(&payload, fill)
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
                if let Some(dpt) = format_dpt(msg.depth, msg.offset) {
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

            // AIS Class A Position Report (PGN 129038) - Type 1/2/3
            N2kMessage::AisClassAPositionReport(msg) => {
                let topic = format!("ais_a_pos_{}", msg.mmsi);
                let sentence = format_vdm_class_a_position(msg);
                self.maybe_send(&topic, &sentence);
            }

            // AIS Class B Position Report (PGN 129039) - Type 18
            N2kMessage::AisClassBPositionReport(msg) => {
                let topic = format!("ais_b_pos_{}", msg.mmsi);
                let sentence = format_vdm_class_b_position(msg);
                self.maybe_send(&topic, &sentence);
            }

            // AIS Class A Static & Voyage Data (PGN 129794) - Type 5
            N2kMessage::AisClassAStaticData(msg) => {
                let topic = format!("ais_a_static_{}", msg.mmsi);
                let sentence = format_vdm_class_a_static(msg);
                self.maybe_send(&topic, &sentence);
            }

            // AIS Class B Static Data Part A (PGN 129809) - Type 24A
            N2kMessage::AisClassBStaticDataPartA(msg) => {
                let topic = format!("ais_b_static_a_{}", msg.mmsi);
                let sentence = format_vdm_class_b_static_a(msg);
                self.maybe_send(&topic, &sentence);
            }

            // AIS Class B Static Data Part B (PGN 129810) - Type 24B
            N2kMessage::AisClassBStaticDataPartB(msg) => {
                let topic = format!("ais_b_static_b_{}", msg.mmsi);
                let sentence = format_vdm_class_b_static_b(msg);
                self.maybe_send(&topic, &sentence);
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
        let formatted = format_latitude(lat);
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
        let result = format_dpt(5.5, 0.0);
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
        let broadcaster = UdpBroadcaster::new("127.0.0.1:10110".to_string(), "0.0.0.0:0".to_string(), false).unwrap();
        assert!(!broadcaster.enabled);
        assert!(broadcaster.socket.lock().unwrap().is_none());
    }

    #[test]
    fn test_broadcaster_initialization() {
        let broadcaster = UdpBroadcaster::new("127.0.0.1:10110".to_string(), "0.0.0.0:0".to_string(), false).unwrap();
        assert_eq!(broadcaster.message_count, 0);
        assert_eq!(broadcaster.error_count, 0);
        assert!(broadcaster.rmc_state.latitude.is_none());
    }

    // ---- AIS encoding helpers ----

    #[test]
    fn test_bit_writer_payload_length() {
        // Type 1 / 18 are 168 bits = 28 payload chars with 0 fill bits
        let mut bw = BitWriter::new();
        for _ in 0..168 {
            bw.write_u(0, 1);
        }
        let (payload, fill) = bw.to_payload();
        assert_eq!(payload.len(), 28);
        assert_eq!(fill, 0);
    }

    #[test]
    fn test_bit_writer_text_encoding() {
        // 'A' = ASCII 65, AIS 6-bit = 65-64 = 1
        // '@' = ASCII 64, AIS 6-bit = 0
        let mut bw = BitWriter::new();
        bw.write_text("A@", 2);
        let (payload, _) = bw.to_payload();
        // First char: bits 000001 = 1 → ASCII 49 = '1'
        // Second char: bits 000000 = 0 → ASCII 48 = '0'
        assert_eq!(&payload, "10");
    }

    #[test]
    fn test_n2k_sog_to_ais_zero() {
        assert_eq!(n2k_sog_to_ais(0), 0);
    }

    #[test]
    fn test_n2k_sog_to_ais_not_available() {
        assert_eq!(n2k_sog_to_ais(0xFFFF), 1023);
    }

    #[test]
    fn test_n2k_cog_to_ais_not_available() {
        assert_eq!(n2k_cog_to_ais(0xFFFF), 3600);
    }

    #[test]
    fn test_n2k_heading_to_ais_not_available() {
        assert_eq!(n2k_heading_to_ais(0xFFFF), 511);
    }

    #[test]
    fn test_n2k_lon_to_ais_zero() {
        assert_eq!(n2k_lon_to_ais(0), 0);
    }

    #[test]
    fn test_n2k_lon_to_ais_positive() {
        // 10.0 degrees east = 10.0 * 600000 = 6000000
        let raw = (10.0 / 1e-7) as i32; // = 100_000_000
        let ais = n2k_lon_to_ais(raw);
        assert!((ais - 6_000_000).abs() < 10, "expected ~6000000, got {}", ais);
    }

    #[test]
    fn test_format_vdm_class_a_position_sentence_prefix() {
        use nmea2k::pgns::pgn129038::AisNavStatus;
        let msg = AisClassAPositionReport {
            pgn: 129038,
            message_id: 1,
            repeat_indicator: 0,
            mmsi: 247123456,
            longitude_raw: (10.161950 / 1e-7) as i32,
            latitude_raw: (43.922298 / 1e-7) as i32,
            position_accuracy: true,
            raim: false,
            time_stamp: 15,
            cog_raw: (316.8_f64.to_radians() / 0.0001) as u16,
            sog_raw: (5.29 / 1.94384 / 0.01) as u16,
            communication_state: 0,
            ais_transceiver_info: 0,
            heading_raw: (315.0_f64.to_radians() / 0.0001) as u16,
            rate_of_turn_raw: 0,
            nav_status: AisNavStatus::UnderWayEngine,
            special_maneuver_indicator: 0,
        };
        let sentence = format_vdm_class_a_position(&msg);
        assert!(sentence.starts_with("!AIVDM,1,1,,A,"), "unexpected prefix: {}", sentence);
        assert!(sentence.contains("*"), "missing checksum");
    }

    #[test]
    fn test_format_vdm_class_b_position_sentence_prefix() {
        let msg = AisClassBPositionReport {
            pgn: 129039,
            message_id: 18,
            repeat_indicator: 0,
            mmsi: 247363720,
            longitude_raw: (10.161950 / 1e-7) as i32,
            latitude_raw: (43.922298 / 1e-7) as i32,
            position_accuracy: false,
            raim: false,
            time_stamp: 15,
            cog_raw: (316.8_f64.to_radians() / 0.0001) as u16,
            sog_raw: (5.29 / 1.94384 / 0.01) as u16,
            communication_state: 0,
            ais_transceiver_info: 0,
            heading_raw: 0xFFFF,
            regional_application: 0,
            regional_application_b: 0,
            unit_type: 1,
            integrated_display: false,
            dsc: true,
            band: true,
            can_handle_msg22: false,
            assigned: false,
        };
        let sentence = format_vdm_class_b_position(&msg);
        assert!(sentence.starts_with("!AIVDM,1,1,,A,"), "unexpected prefix: {}", sentence);
    }

    #[test]
    fn test_vdm_checksum_valid() {
        // Verify the wrap_ais_vdm checksum is correct by re-computing it inline
        let payload = "15M67N0000G?Ijk`E`FepT?vN0<";
        let fill = 0u8;
        let sentence = wrap_ais_vdm(payload, fill);
        // Extract body between '!' and '*'
        let body_start = sentence.find('!').unwrap() + 1;
        let body_end = sentence.find('*').unwrap();
        let body = &sentence[body_start..body_end];
        let expected_cs = nmea0183_checksum(body);
        let actual_cs = &sentence[body_end + 1..body_end + 3];
        assert_eq!(actual_cs, expected_cs, "checksum mismatch");
    }
}

