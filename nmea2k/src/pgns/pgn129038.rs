use std::fmt;
use super::ais_helpers::{read_bits, read_signed_bits, ais_transceiver_info_description, repeat_indicator_description, time_stamp_description};

/// AIS Class A Position Report (PGN 129038)
/// Fast Packet message from Class A transponders reporting vessel position and motion
/// Per NMEA 2000 specification
#[derive(Debug, Clone)]
pub struct AisClassAPositionReport {
    pub pgn: u32,
    pub message_id: u8,                       // 6 bits, offset 0
    pub repeat_indicator: u8,                 // 2 bits, offset 6
    pub mmsi: u32,                            // 32 bits, offset 8 (vessel ID)
    pub longitude_raw: i32,                   // 32 bits, signed, offset 40, × 1e-7 degrees
    pub latitude_raw: i32,                    // 32 bits, signed, offset 72, × 1e-7 degrees
    pub position_accuracy: bool,              // 1 bit, offset 104 (false = low, true = high)
    pub raim: bool,                           // 1 bit, offset 105
    pub time_stamp: u8,                       // 6 bits, offset 106 (UTC second, 60-63 = special)
    pub cog_raw: u16,                         // 16 bits, offset 112, × 0.0001 radians
    pub sog_raw: u16,                         // 16 bits, offset 128, × 0.01 m/s
    pub communication_state: u32,             // 19 bits, offset 144 (TDMA info)
    pub ais_transceiver_info: u8,             // 5 bits, offset 163
    pub heading_raw: u16,                     // 16 bits, offset 168, × 0.0001 radians
    pub rate_of_turn_raw: i16,                // 16 bits, offset 184, × 3.125e-5 rad/s, signed
    pub nav_status: AisNavStatus,             // 4 bits, offset 200
    pub special_maneuver_indicator: u8,       // 2 bits, offset 204
}

/// AIS Navigation Status (4-bit field)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AisNavStatus {
    UnderWayEngine = 0,
    AtAnchor = 1,
    NotUnderCommand = 2,
    RestrictedManeuverability = 3,
    ConstrainedByDraught = 4,
    Moored = 5,
    Aground = 6,
    Fishing = 7,
    UnderWaySailing = 8,
    HazmatWIG = 9,
    HazmatWIG2 = 10,
    TowingAstern = 11,
    PushingAhead = 12,
    PlaceholderStatus = 13,
    AisSart = 14,
    UnknownStatus = 15,
}

impl From<u8> for AisNavStatus {
    fn from(val: u8) -> Self {
        match val & 0x0F {
            0 => AisNavStatus::UnderWayEngine,
            1 => AisNavStatus::AtAnchor,
            2 => AisNavStatus::NotUnderCommand,
            3 => AisNavStatus::RestrictedManeuverability,
            4 => AisNavStatus::ConstrainedByDraught,
            5 => AisNavStatus::Moored,
            6 => AisNavStatus::Aground,
            7 => AisNavStatus::Fishing,
            8 => AisNavStatus::UnderWaySailing,
            9 => AisNavStatus::HazmatWIG,
            10 => AisNavStatus::HazmatWIG2,
            11 => AisNavStatus::TowingAstern,
            12 => AisNavStatus::PushingAhead,
            13 => AisNavStatus::PlaceholderStatus,
            14 => AisNavStatus::AisSart,
            _ => AisNavStatus::UnknownStatus,
        }
    }
}

impl fmt::Display for AisNavStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", match self {
            AisNavStatus::UnderWayEngine => "Under way engine",
            AisNavStatus::AtAnchor => "At anchor",
            AisNavStatus::NotUnderCommand => "Not under command",
            AisNavStatus::RestrictedManeuverability => "Restricted maneuverability",
            AisNavStatus::ConstrainedByDraught => "Constrained by draught",
            AisNavStatus::Moored => "Moored",
            AisNavStatus::Aground => "Aground",
            AisNavStatus::Fishing => "Fishing",
            AisNavStatus::UnderWaySailing => "Under way sailing",
            AisNavStatus::HazmatWIG => "HSC hazmat",
            AisNavStatus::HazmatWIG2 => "WIG hazmat",
            AisNavStatus::TowingAstern => "Towing astern",
            AisNavStatus::PushingAhead => "Pushing ahead",
            AisNavStatus::PlaceholderStatus => "Placeholder",
            AisNavStatus::AisSart => "AIS-SART",
            AisNavStatus::UnknownStatus => "Unknown",
        })
    }
}

impl AisClassAPositionReport {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        // Last field (special_maneuver_indicator) sits at bits 204-205 → byte 25.
        // 26 bytes are the minimum needed to read all fields.
        if data.len() < 26 {
            return None;
        }

        let message_id = read_bits(data, 0, 6) as u8;
        let repeat_indicator = read_bits(data, 6, 2) as u8;
        let mmsi = read_bits(data, 8, 32) as u32;
        let longitude_raw = read_signed_bits(data, 40, 32) as i32;
        let latitude_raw = read_signed_bits(data, 72, 32) as i32;
        let position_accuracy = read_bits(data, 104, 1) != 0;
        let raim = read_bits(data, 105, 1) != 0;
        let time_stamp = read_bits(data, 106, 6) as u8;
        let cog_raw = read_bits(data, 112, 16) as u16;
        let sog_raw = read_bits(data, 128, 16) as u16;
        let communication_state = read_bits(data, 144, 19) as u32;
        let ais_transceiver_info = read_bits(data, 163, 5) as u8;
        let heading_raw = read_bits(data, 168, 16) as u16;
        let rate_of_turn_raw = read_signed_bits(data, 184, 16) as i16;
        let nav_status_bits = read_bits(data, 200, 4) as u8;
        let special_maneuver_indicator = read_bits(data, 204, 2) as u8;
        Some(Self {
            pgn: 129038,
            message_id,
            repeat_indicator,
            mmsi,
            longitude_raw,
            latitude_raw,
            position_accuracy,
            raim,
            time_stamp,
            cog_raw,
            sog_raw,
            communication_state,
            ais_transceiver_info,
            heading_raw,
            rate_of_turn_raw,
            nav_status: AisNavStatus::from(nav_status_bits),
            special_maneuver_indicator,
        })
    }

    pub fn get_longitude_degrees(&self) -> f64 {
        self.longitude_raw as f64 * 1e-7
    }

    pub fn get_latitude_degrees(&self) -> f64 {
        self.latitude_raw as f64 * 1e-7
    }

    pub fn get_cog_degrees(&self) -> f64 {
        self.cog_raw as f64 * 0.0001 * 180.0 / std::f64::consts::PI
    }

    pub fn get_cog_radians(&self) -> f64 {
        self.cog_raw as f64 * 0.0001
    }

    pub fn get_sog_knots(&self) -> f64 {
        self.sog_raw as f64 * 0.01 * 1.94384  // m/s to knots
    }

    pub fn get_sog_ms(&self) -> f64 {
        self.sog_raw as f64 * 0.01
    }

    pub fn get_heading_degrees(&self) -> f64 {
        self.heading_raw as f64 * 0.0001 * 180.0 / std::f64::consts::PI
    }

    pub fn get_heading_radians(&self) -> f64 {
        self.heading_raw as f64 * 0.0001
    }

    pub fn get_rate_of_turn_rad_per_sec(&self) -> f64 {
        self.rate_of_turn_raw as f64 * 3.125e-5
    }

    pub fn get_rate_of_turn_degrees_per_min(&self) -> f64 {
        self.get_rate_of_turn_rad_per_sec() * 180.0 / std::f64::consts::PI * 60.0
    }

    pub fn get_special_maneuver_description(&self) -> &'static str {
        match self.special_maneuver_indicator {
            0 => "Not available",
            1 => "Not engaged in special maneuver",
            2 => "Engaged in special maneuver",
            _ => "Reserved",
        }
    }

    pub fn get_ais_transceiver_info_description(&self) -> &'static str {
        ais_transceiver_info_description(self.ais_transceiver_info)
    }

    pub fn get_repeat_indicator_description(&self) -> &'static str {
        repeat_indicator_description(self.repeat_indicator)
    }

    pub fn get_time_stamp_description(&self) -> String {
        time_stamp_description(self.time_stamp)
    }
}

impl fmt::Display for AisClassAPositionReport {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "AIS Class A (ID:{}|{}): MMSI:{} Lat:{:.6}° Lon:{:.6}° SOG:{:.2}kn COG:{:.1}° HDG:{:.1}° Status:{} Accuracy:{}",
            self.message_id,
            self.repeat_indicator,
            self.mmsi,
            self.get_latitude_degrees(),
            self.get_longitude_degrees(),
            self.get_sog_knots(),
            self.get_cog_degrees(),
            self.get_heading_degrees(),
            self.nav_status,
            if self.position_accuracy { "HIGH" } else { "LOW" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ais_position_report_parsing() {
        let payload = [
            0xc1, 0xb8, 0x68, 0xbc, 0x0e, 0x31, 0x95, 0xf7, 0x05,
            0xde, 0x5d, 0xa9, 0x19, 0x98, 0x16, 0x13, 0x60, 0x03,
            0x00, 0x00, 0x00, 0x68, 0x12, 0x00, 0x00, 0xf0, 0xfe, 0x00];
        let p = AisClassAPositionReport::from_bytes(&payload).unwrap();
        println!("{}", p);
        assert_eq!(247228600, p.mmsi);
        assert!((43.0530014 - p.get_latitude_degrees()).abs() < 0.00001);
        assert!((10.0111665 - p.get_longitude_degrees()).abs() < 0.00001);
        assert!((27.0 - p.get_heading_degrees()).abs() < 0.01);
        assert!((16.8 - p.get_sog_knots()).abs() < 0.01);
        assert!((28.0 - p.get_cog_degrees()).abs() < 0.01);
        assert_eq!(AisNavStatus::UnderWayEngine, p.nav_status);
    }

    #[test]
    fn test_ais_position_report_parsing1() {
        let payload = [0xc1, 0xdc, 0x9a, 0xbb, 0x0e, 0xff,
            0xff, 0xff, 0x7f,0xff,0xff,0xff,0x7f,
            0xfc,0xff,0xff,0xff,0xff,0x01,0x00,
            0x08,0xff,0xff,0xff,0x7f,0xf5,0xfe, 0x00];
        let p = AisClassAPositionReport::from_bytes(&payload).unwrap();
        println!("{:?}", p);
    }
}