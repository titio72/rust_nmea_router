use std::fmt;

use super::nmea2000_date_time::N2kDateTime;

#[derive(Debug, Clone)]
pub struct GnssPositionData {
    #[allow(dead_code)]
    pub pgn: u32,
    #[allow(dead_code)]
    sid: u8,
    pub date_time: N2kDateTime,
    pub latitude: f64,
    pub longitude: f64,
    #[allow(dead_code)]
    pub altitude: f64,
    pub gnss_type: GnssType,
    pub method: GnssMethod,
    #[allow(dead_code)]
    integrity: u8,
    pub num_svs: u8,
    pub hdop: f64,
    pub pdop: f64,
    #[allow(dead_code)]
    geoidal_separation: f64,
}

#[derive(Debug, Clone)]
pub enum GnssType {
    Gps,
    Glonass,
    GpsGlonass,
    GpsSbasWaas,
    GpsSbasWaasDglonass,
    Chayka,
    Integrated,
    Surveyed,
    Galileo,
}

#[derive(Debug, Clone)]
pub enum GnssMethod {
    NoGnss,
    GnssFix,
    DGnss,
    PreciseGnss,
    RtkFixed,
    RtkFloat,
}

impl GnssPositionData {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 43 {
            return None;
        }
        Some(Self {
            pgn: 129029,
            sid: data[0],
            date_time: N2kDateTime {
                date: u16::from_le_bytes([data[1], data[2]]),
                time: u32::from_le_bytes([data[3], data[4], data[5], data[6]]) as f64 * 0.0001,
            },
            latitude: i64::from_le_bytes([
                data[7], data[8], data[9], data[10], data[11], data[12], data[13], data[14]
            ]) as f64 * 1e-16,
            longitude: i64::from_le_bytes([
                data[15], data[16], data[17], data[18], data[19], data[20], data[21], data[22]
            ]) as f64 * 1e-16,
            altitude: i64::from_le_bytes([
                data[23], data[24], data[25], data[26], data[27], data[28], data[29], data[30]
            ]) as f64 * 1e-6,
            gnss_type: match data[31] & 0x0F {
                0 => GnssType::Gps,
                1 => GnssType::Glonass,
                2 => GnssType::GpsGlonass,
                3 => GnssType::GpsSbasWaas,
                4 => GnssType::GpsSbasWaasDglonass,
                5 => GnssType::Chayka,
                6 => GnssType::Integrated,
                7 => GnssType::Surveyed,
                8 => GnssType::Galileo,
                _ => GnssType::Gps,
            },
            method: match (data[31] >> 4) & 0x0F {
                0 => GnssMethod::NoGnss,
                1 => GnssMethod::GnssFix,
                2 => GnssMethod::DGnss,
                3 => GnssMethod::PreciseGnss,
                4 => GnssMethod::RtkFixed,
                5 => GnssMethod::RtkFloat,
                _ => GnssMethod::NoGnss,
            },
            integrity: data[32] & 0x03,
            num_svs: data[33],
            hdop: i16::from_le_bytes([data[34], data[35]]) as f64 * 0.01,
            pdop: i16::from_le_bytes([data[36], data[37]]) as f64 * 0.01,
            geoidal_separation: i32::from_le_bytes([data[38], data[39], data[40], data[41]]) as f64 * 0.01,
        })
    }
}

impl fmt::Display for GnssPositionData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "      Position: {:.6}°, {:.6}° Satellites: {} Type: {:?} Method: {:?} HDOP: {:.2} PDOP: {:.2}", self.latitude, self.longitude, self.num_svs, self.gnss_type, self.method, self.hdop, self.pdop)
    }
}
