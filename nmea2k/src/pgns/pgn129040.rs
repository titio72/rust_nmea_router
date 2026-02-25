use std::fmt;
use super::ais_helpers::{read_bits, read_signed_bits, gnss_type_description, type_of_ship_description, repeat_indicator_description, time_stamp_description};

/// AIS Class B Extended Position Report (PGN 129040)
/// Fast Packet message with extended vessel information
/// Per NMEA 2000 specification
#[derive(Debug, Clone)]
pub struct AisClassBExtPositionReport {
    pub pgn: u32,
    pub message_id: u8,                       // 6 bits, offset 0
    pub repeat_indicator: u8,                 // 2 bits, offset 6
    pub mmsi: u32,                            // 32 bits, offset 8 (vessel ID)
    pub longitude_raw: i32,                   // 32 bits, signed, offset 40, × 1e-7 degrees
    pub latitude_raw: i32,                    // 32 bits, signed, offset 72, × 1e-7 degrees
    pub position_accuracy: bool,              // 1 bit, offset 104
    pub raim: bool,                           // 1 bit, offset 105
    pub time_stamp: u8,                       // 6 bits, offset 106
    pub cog_raw: u16,                         // 16 bits, offset 112, × 0.0001 radians
    pub sog_raw: u16,                         // 16 bits, offset 128, × 0.01 m/s
    pub regional_application: u8,             // 8 bits, offset 144
    pub regional_application_b: u8,           // 4 bits, offset 152
    pub type_of_ship: u8,                     // 8 bits, offset 160: vessel type code
    pub heading_raw: u16,                     // 16 bits, offset 168, × 0.0001 radians
    pub gnss_type: u8,                        // 4 bits, offset 188: GPS, GLONASS, etc.
    pub length_raw: u16,                      // 16 bits, offset 192, × 0.1 meters
    pub beam_raw: u16,                        // 16 bits, offset 208, × 0.1 meters
    pub position_ref_starboard_raw: u16,      // 16 bits, offset 224, × 0.1 meters
    pub position_ref_bow_raw: u16,            // 16 bits, offset 240, × 0.1 meters
}

impl AisClassBExtPositionReport {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 40 {
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
        let regional_application = read_bits(data, 144, 8) as u8;
        let regional_application_b = read_bits(data, 152, 4) as u8;
        let type_of_ship = read_bits(data, 160, 8) as u8;
        let heading_raw = read_bits(data, 168, 16) as u16;
        let gnss_type = read_bits(data, 188, 4) as u8;
        let length_raw = read_bits(data, 192, 16) as u16;
        let beam_raw = read_bits(data, 208, 16) as u16;
        let position_ref_starboard_raw = read_bits(data, 224, 16) as u16;
        let position_ref_bow_raw = read_bits(data, 240, 16) as u16;

        Some(Self {
            pgn: 129040,
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
            regional_application,
            regional_application_b,
            type_of_ship,
            heading_raw,
            gnss_type,
            length_raw,
            beam_raw,
            position_ref_starboard_raw,
            position_ref_bow_raw,
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
        self.sog_raw as f64 * 0.01 * 1.94384
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

    pub fn get_length_meters(&self) -> f64 {
        self.length_raw as f64 * 0.1
    }

    pub fn get_beam_meters(&self) -> f64 {
        self.beam_raw as f64 * 0.1
    }

    pub fn get_position_ref_starboard_meters(&self) -> f64 {
        self.position_ref_starboard_raw as f64 * 0.1
    }

    pub fn get_position_ref_bow_meters(&self) -> f64 {
        self.position_ref_bow_raw as f64 * 0.1
    }

    pub fn get_gnss_type_description(&self) -> &'static str {
        gnss_type_description(self.gnss_type)
    }

    pub fn get_unit_type_description(&self) -> &'static str {
        match self.regional_application_b & 0x1 {
            0 => "SOTDMA",
            1 => "CS",
            _ => "Unknown",
        }
    }

    pub fn get_type_of_ship_description(&self) -> &'static str {
        type_of_ship_description(self.type_of_ship)
    }

    pub fn get_repeat_indicator_description(&self) -> &'static str {
        repeat_indicator_description(self.repeat_indicator)
    }

    pub fn get_time_stamp_description(&self) -> String {
        time_stamp_description(self.time_stamp)
    }
}

impl fmt::Display for AisClassBExtPositionReport {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "AIS Class B Extended (ID:{}|{}): MMSI:{} Lat:{:.6}° Lon:{:.6}° SOG:{:.2}kn COG:{:.1}° HDG:{:.1}° Type:{} Len:{:.1}m Beam:{:.1}m",
            self.message_id,
            self.repeat_indicator,
            self.mmsi,
            self.get_latitude_degrees(),
            self.get_longitude_degrees(),
            self.get_sog_knots(),
            self.get_cog_degrees(),
            self.get_heading_degrees(),
            self.type_of_ship,
            self.get_length_meters(),
            self.get_beam_meters()
        )
    }
}

#[cfg(test)]
mod tests {



}
