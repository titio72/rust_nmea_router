use std::fmt;

use super::nmea2000_date_time::N2kDateTime;
use super::sentinel::{fmt_opt, latitude, longitude, Available};

#[derive(Debug, Clone)]
pub struct GnssPositionData {
    #[allow(dead_code)]
    pub pgn: u32,
    #[allow(dead_code)]
    sid: u8,
    pub date_time: Option<N2kDateTime>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    #[allow(dead_code)]
    pub altitude: Option<f64>,
    pub gnss_type: GnssType,
    pub method: GnssMethod,
    #[allow(dead_code)]
    integrity: u8,
    pub num_svs: Option<u8>,
    pub hdop: Option<f64>,
    pub pdop: Option<f64>,
    #[allow(dead_code)]
    geoidal_separation: Option<f64>,
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

impl fmt::Display for GnssType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", match self {
            GnssType::Gps => "GPS",
            GnssType::Glonass => "GLONASS",
            GnssType::GpsGlonass => "Combined GPS/GLONASS",
            GnssType::GpsSbasWaas => "GPS+SBAS/WAAS",
            GnssType::GpsSbasWaasDglonass => "GPS+SBAS/WAAS+DGLONASS",
            GnssType::Chayka => "Chayka",
            GnssType::Integrated => "Integrated navigation system",
            GnssType::Surveyed => "Surveyed",
            GnssType::Galileo => "Galileo",
        })
    }
}

impl fmt::Display for GnssMethod {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", match self {
            GnssMethod::NoGnss => "No GNSS",
            GnssMethod::GnssFix => "GNSS fix",
            GnssMethod::DGnss => "DGNSS fix",
            GnssMethod::PreciseGnss => "Precise GNSS",
            GnssMethod::RtkFixed => "RTK fixed integer",
            GnssMethod::RtkFloat => "RTK float",
        })
    }
}

impl GnssPositionData {
    /// Always decodes when long enough: a receiver without a fix still reports method and
    /// satellite count, with the position fields not available.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 43 {
            return None;
        }
        let date = u16::from_le_bytes([data[1], data[2]]).available();
        let time = u32::from_le_bytes([data[3], data[4], data[5], data[6]]).available();
        let lat_raw = i64::from_le_bytes([
            data[7], data[8], data[9], data[10], data[11], data[12], data[13], data[14]
        ]);
        let lon_raw = i64::from_le_bytes([
            data[15], data[16], data[17], data[18], data[19], data[20], data[21], data[22]
        ]);
        let alt_raw = i64::from_le_bytes([
            data[23], data[24], data[25], data[26], data[27], data[28], data[29], data[30]
        ]);
        Some(Self {
            pgn: 129029,
            sid: data[0],
            date_time: date.zip(time).map(|(date, time)| N2kDateTime {
                date,
                time: time as f64 * 0.0001,
            }),
            latitude: latitude(lat_raw.available().map(|r| r as f64 * 1e-16)),
            longitude: longitude(lon_raw.available().map(|r| r as f64 * 1e-16)),
            altitude: alt_raw.available().map(|r| r as f64 * 1e-6),
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
            num_svs: data[33].available(),
            hdop: i16::from_le_bytes([data[34], data[35]]).available().map(|r| r as f64 * 0.01),
            pdop: i16::from_le_bytes([data[36], data[37]]).available().map(|r| r as f64 * 0.01),
            geoidal_separation: i32::from_le_bytes([data[38], data[39], data[40], data[41]])
                .available()
                .map(|r| r as f64 * 0.01),
        })
    }
}

impl fmt::Display for GnssPositionData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "      Position: {}°, {}° Satellites: {} Type: {} Method: {} HDOP: {} PDOP: {}",
            fmt_opt(self.latitude, 6),
            fmt_opt(self.longitude, 6),
            self.num_svs.map_or_else(|| "N/A".to_string(), |n| n.to_string()),
            self.gnss_type,
            self.method,
            fmt_opt(self.hdop, 2),
            fmt_opt(self.pdop, 2)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        s.split(',').map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
    }

    #[test]
    fn test_gnss_position_data_from_bytes() {
        // canboat sample (merrimac-2022.raw)
        let data = hex("37,2d,4b,10,6d,ff,19,00,7a,5b,64,9d,56,61,07,b0,7c,b2,ca,79,db,c0,00,20,6e,bf,02,00,00,00,00,10,fc,0c,46,00,82,00,34,12,00,00,ff");
        let gnss = GnssPositionData::from_bytes(&data).unwrap();
        let dt = gnss.date_time.unwrap();
        assert_eq!(dt.date, 19245);
        assert!((dt.time - 43617.0).abs() < 1e-6);
        assert!((gnss.latitude.unwrap() - 53.1801465).abs() < 1e-7);
        assert!((gnss.longitude.unwrap() - 5.428451167).abs() < 1e-7);
        assert!((gnss.altitude.unwrap() - 46.1).abs() < 1e-6);
        assert!(matches!(gnss.method, GnssMethod::GnssFix));
        assert_eq!(gnss.num_svs, Some(12));
        assert!((gnss.hdop.unwrap() - 0.70).abs() < 1e-9);
        assert!((gnss.pdop.unwrap() - 1.30).abs() < 1e-9);
    }

    #[test]
    fn test_gnss_position_data_no_fix() {
        // canboat sample: receiver with no fix, 9 satellites in view, everything else unavailable
        let data = hex("c4,ff,ff,ff,ff,ff,ff,ff,ff,ff,ff,ff,ff,ff,7f,ff,ff,ff,ff,ff,ff,ff,7f,ff,ff,ff,ff,ff,ff,ff,7f,00,fc,09,ff,7f,ff,7f,ff,ff,ff,7f,ff");
        let gnss = GnssPositionData::from_bytes(&data).unwrap();
        assert!(gnss.date_time.is_none());
        assert_eq!(gnss.latitude, None);
        assert_eq!(gnss.longitude, None);
        assert_eq!(gnss.altitude, None);
        assert!(matches!(gnss.method, GnssMethod::NoGnss));
        assert_eq!(gnss.num_svs, Some(9));
        assert_eq!(gnss.hdop, None);
        assert_eq!(gnss.pdop, None);
    }
}
