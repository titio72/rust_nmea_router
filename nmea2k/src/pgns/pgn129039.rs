use std::fmt;
use super::ais_helpers::{read_bits, read_signed_bits, ais_transceiver_info_description, repeat_indicator_description, time_stamp_description};

/// AIS Class B Position Report (PGN 129039)
/// Fast Packet message from Class B transponders reporting vessel position and motion
#[derive(Debug, Clone)]
pub struct AisClassBPositionReport {
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
    pub communication_state: u32,             // 19 bits, offset 144 (TDMA info)
    pub ais_transceiver_info: u8,             // 5 bits, offset 163
    pub heading_raw: u16,                     // 16 bits, offset 168, × 0.0001 radians
    pub regional_application: u8,             // 8 bits, offset 184
    pub regional_application_b: u8,           // 2 bits, offset 192
    pub unit_type: u8,                        // 1 bit, offset 194: 0 = SOTDMA, 1 = CS
    pub integrated_display: bool,             // 1 bit, offset 195
    pub dsc: bool,                            // 1 bit, offset 196: has DSC capability
    pub band: bool,                           // 1 bit, offset 197: 0 = top 525kHz, 1 = entire band
    pub can_handle_msg22: bool,               // 1 bit, offset 198
    pub assigned: bool,                       // 1 bit, offset 199: 0 = autonomous, 1 = assigned
}

impl AisClassBPositionReport {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 27 {
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
        let regional_application = read_bits(data, 184, 8) as u8;
        let regional_application_b = read_bits(data, 192, 2) as u8;
        let unit_type = read_bits(data, 194, 1) as u8;
        let integrated_display = read_bits(data, 195, 1) != 0;
        let dsc = read_bits(data, 196, 1) != 0;
        let band = read_bits(data, 197, 1) != 0;
        let can_handle_msg22 = read_bits(data, 198, 1) != 0;
        let assigned = read_bits(data, 199, 1) != 0;

        Some(Self {
            pgn: 129039,
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
            regional_application,
            regional_application_b,
            unit_type,
            integrated_display,
            dsc,
            band,
            can_handle_msg22,
            assigned,
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

    pub fn get_ais_transceiver_info_description(&self) -> &'static str {
        ais_transceiver_info_description(self.ais_transceiver_info)
    }

    pub fn get_unit_type_description(&self) -> &'static str {
        match self.unit_type {
            0 => "SOTDMA",
            1 => "CS",
            _ => "Unknown",
        }
    }

    pub fn get_repeat_indicator_description(&self) -> &'static str {
        repeat_indicator_description(self.repeat_indicator)
    }

    pub fn get_time_stamp_description(&self) -> String {
        time_stamp_description(self.time_stamp)
    }
}

impl fmt::Display for AisClassBPositionReport {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "AIS Class B Position - MMSI: {}, Lat: {:.6}, Lon: {:.6}, SOG: {:.2} kn, COG: {:.1}°",
            self.mmsi,
            self.get_latitude_degrees(),
            self.get_longitude_degrees(),
            self.get_sog_knots(),
            self.get_cog_degrees()
        )
    }
}


#[cfg(test)]
mod tests {
    use crate::pgns::AisClassBPositionReport;



    #[test]
    fn test_from_bytes() {
        let data = [
            0x12, 0xa2, 0x81, 0xbe, 0x0e, 0x2d,
            0x97, 0x0e, 0x06, 0xc8, 0x02, 0x2e, 0x1a,
            0xdf, 0xfc, 0xd7, 0x10, 0x01, 0x00, 0x00,
            0x08, 0xff, 0xff, 0x00, 0xfc, 0xfe, 0xff
        ];
        let report = AisClassBPositionReport::from_bytes(&data);
        assert!(report.is_some());
        let report = report.unwrap();
        println!("{}", report);
        assert_eq!(247366050, report.mmsi);
        assert!((report.get_latitude_degrees() - 43.922298).abs() < 0.000001);
        assert!((report.get_longitude_degrees() - 10.161950).abs() < 0.000001);
        assert!((report.get_sog_knots() - 5.29).abs() < 0.01);
        assert!((report.get_cog_degrees() - 316.8).abs() < 0.1);
    }


}