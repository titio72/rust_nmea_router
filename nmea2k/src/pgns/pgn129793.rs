use std::fmt;
use crate::pgns::nmea2000_date_time::N2kDateTime;

use super::ais_helpers::{read_bits, read_signed_bits, gnss_type_description, repeat_indicator_description};

/// AIS UTC and Date Report (PGN 129793)
/// Reports precise time and date synchronization from AIS receiver
#[derive(Debug, Clone)]
pub struct AisUtcDateReport {
    pub pgn: u32,
    pub message_id: u8,
    pub repeat_indicator: u8,
    pub mmsi: u32,
    pub longitude_raw: i32,
    pub latitude_raw: i32,
    pub position_accuracy: bool,
    pub raim: bool,
    pub position_time_raw: u32,    // × 0.0001 seconds since midnight
    pub position_date: u16,        // Days since epoch (1970-01-01)
    pub gnss_type: u8,             // GNSS position type
    pub date_time: N2kDateTime,
}

impl AisUtcDateReport {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 24 {
            return None;
        }

        let message_id = read_bits(data, 0, 6) as u8;
        let repeat_indicator = read_bits(data, 6, 2) as u8;
        let mmsi = read_bits(data, 8, 32) as u32;
        let longitude_raw = read_signed_bits(data, 40, 32) as i32;
        let latitude_raw = read_signed_bits(data, 72, 32) as i32;
        let position_accuracy = read_bits(data, 104, 1) != 0;
        let raim = read_bits(data, 105, 1) != 0;
        let position_time_raw = read_bits(data, 112, 32) as u32;
        let position_date = read_bits(data, 168, 16) as u16;
        let gnss_type = read_bits(data, 188, 4) as u8;

        let date_time = N2kDateTime::new(position_date, position_time_raw as f64)?;

        Some(Self {
            pgn: 129793,
            message_id,
            repeat_indicator,
            mmsi,
            longitude_raw,
            latitude_raw,
            position_accuracy,
            raim,
            position_time_raw,
            position_date,
            gnss_type,
            date_time,
        })
    }

    pub fn get_longitude_degrees(&self) -> f64 {
        self.longitude_raw as f64 * 1e-7
    }

    pub fn get_latitude_degrees(&self) -> f64 {
        self.latitude_raw as f64 * 1e-7
    }

    pub fn get_gnss_type_description(&self) -> &'static str {
        gnss_type_description(self.gnss_type)
    }

    pub fn get_repeat_indicator_description(&self) -> &'static str {
        repeat_indicator_description(self.repeat_indicator)
    }

}

impl fmt::Display for AisUtcDateReport {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "AIS UTC Report - MMSI: {}, Lat: {:.6}, Lon: {:.6}, Time: {:.2}s, Date: {} days",
            self.mmsi,
            self.get_latitude_degrees(),
            self.get_longitude_degrees(),
            self.date_time.time,
            self.date_time.date
        )
    }
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;

    use super::*;

    /*
    2026-02-22-14:10:25.050,7,129793,0,255,8,e0,1a,04,a6,b0,25,00,8a
    2026-02-22-14:10:25.051,7,129793,0,255,8,e1,e7,d9,05,20,0a,44,1a
    2026-02-22-14:10:25.053,7,129793,0,255,8,e2,fc,c0,08,69,1e,00,00
    2026-02-22-14:10:25.055,7,129793,0,255,8,e3,08,1a,50,7f,00,fc,ff

    {
        "timestamp":"2026-02-22-14:10:25.055",
        "prio":7,
        "src":0,
        "dst":255,
        "pgn":129793,
        "description":"AIS UTC and Date Report",
        "fields":{
            "Message ID":"Base station report",
            "Repeat Indicator":"Initial",
            "User ID":"002470054",
            "Longitude": 9.8166666,
            "Latitude":44.0666656,
            "Position Accuracy":"Low",
            "RAIM":"not in use",
            "Position Time":"14:10:20.0000",
            "Communication State":"00 00 00",
            "AIS Transceiver information":"Channel B VDL reception",
            "Position Date":"2026.02.22",
            "GNSS type":"Surveyed"}
        }
     */

    #[test]
    fn test_get_longitude_degrees() {
        let payload = [0x04, 0xa6, 0xb0, 0x25, 0x00, 0x8a, 0xe7, 0xd9, 0x05, 0x20, 0x0a, 0x44, 0x1a, 0xfc, 0xc0, 0x08, 0x69, 0x1e, 0x00, 0x00, 0x08, 0x1a, 0x50, 0x7f, 0x00, 0xfc, 0xff];
        let report = AisUtcDateReport::from_bytes(&payload).unwrap();
        assert_eq!(report.get_longitude_degrees(), 9.8166666);
        assert_eq!(report.get_latitude_degrees(), 44.0666656);
        assert_eq!(report.date_time.to_date_time(), DateTime::parse_from_rfc3339("2026-02-22T14:10:20+00:00").unwrap()); // 2026-02-22T14:10:20Z
        assert_eq!(report.gnss_type, 7); // Surveyed
        assert_eq!(report.position_accuracy, false); // Low accuracy
        assert_eq!(report.raim, false); // RAIM not in use
        assert_eq!(report.message_id, 4); // Base station report
        assert_eq!(report.repeat_indicator, 0); // Initial
        assert_eq!(report.mmsi, 2470054); // MMSI
    }
}
