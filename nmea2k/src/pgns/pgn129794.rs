use std::fmt;
use crate::pgns::nmea2000_date_time::N2kDateTime;

use super::ais_helpers::{read_bits, extract_text_from_bytes, gnss_type_description, type_of_ship_description, repeat_indicator_description};

/// AIS Class A Static and Voyage Data (PGN 129794)
/// Reports vessel name, callsign, dimensions, type, and voyage-related information
#[derive(Debug, Clone)]
pub struct AisClassAStaticData {
    pub pgn: u32,
    pub message_id: u8,
    pub repeat_indicator: u8,
    pub mmsi: u32,
    pub imo_number: u32,
    pub callsign: String,          // 7 chars
    pub name: String,              // 20 chars
    pub type_of_ship: u8,
    pub length_raw: u16,           // × 0.1 meters
    pub beam_raw: u16,             // × 0.1 meters
    pub position_ref_starboard: u16, // × 0.1 meters
    pub position_ref_bow: u16,     // × 0.1 meters
    pub eta_date: u16,             // Days since epoch
    pub eta_time: u32,             // × 0.0001 seconds since midnight
    pub draft_raw: u16,            // × 0.01 meters
    pub destination: String,       // 20 chars
    pub ais_version: u8,
    pub gnss_type: u8,
    pub dte: bool,
    pub eta_date_time: Option<N2kDateTime>,
    pub class: String,
}

impl AisClassAStaticData {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 74 {
            return None;
        }

        let message_id = read_bits(data, 0, 6) as u8;
        let repeat_indicator = read_bits(data, 6, 2) as u8;
        let mmsi = read_bits(data, 8, 32) as u32;
        let imo_number = read_bits(data, 40, 32) as u32;
        
        // callsign is 7 bytes of ASCII starting at byte 9
        let callsign = extract_text_from_bytes(data, 9, 7);
        
        // name is 20 bytes of ASCII starting at byte 16
        let name = extract_text_from_bytes(data, 16, 20);
        let type_of_ship = read_bits(data, 288, 8) as u8;
        let length_raw = read_bits(data, 296, 16) as u16;
        let beam_raw = read_bits(data, 312, 16) as u16;
        let position_ref_starboard = read_bits(data, 328, 16) as u16;
        let position_ref_bow = read_bits(data, 344, 16) as u16;
        let eta_date = read_bits(data, 360, 16) as u16;
        let eta_time = read_bits(data, 376, 32) as u32;
        let draft_raw = read_bits(data, 408, 16) as u16;
        
        // destination is 20 bytes of ASCII starting at byte 53
        let destination = extract_text_from_bytes(data, 53, 20);
        let ais_version = read_bits(data, 584, 2) as u8;
        let gnss_type = read_bits(data, 586, 4) as u8;
        let dte = read_bits(data, 590, 1) != 0;

        Some(Self {
            pgn: 129794,
            message_id,
            repeat_indicator,
            mmsi,
            imo_number,
            callsign,
            name,
            type_of_ship,
            length_raw,
            beam_raw,
            position_ref_starboard,
            position_ref_bow,
            eta_date,
            eta_time,
            draft_raw,
            destination,
            ais_version,
            gnss_type,
            dte,
            eta_date_time: N2kDateTime::new(eta_date, eta_time as f64),
            class: "A".to_string(),
        })
    }

    pub fn get_length_meters(&self) -> f64 {
        self.length_raw as f64 * 0.1
    }

    pub fn get_beam_meters(&self) -> f64 {
        self.beam_raw as f64 * 0.1
    }

    pub fn get_draft_meters(&self) -> f64 {
        self.draft_raw as f64 * 0.01
    }

    pub fn get_eta_time_seconds(&self) -> f64 {
        self.eta_time as f64 * 0.0001
    }

    pub fn get_type_of_ship_description(&self) -> &'static str {
        type_of_ship_description(self.type_of_ship)
    }

    pub fn get_gnss_type_description(&self) -> &'static str {
        gnss_type_description(self.gnss_type)
    }

    pub fn get_ais_version_description(&self) -> &'static str {
        match self.ais_version {
            0 => "ITU-R M.1371-1",
            1 => "ITU-R M.1371-3",
            2 => "ITU-R M.1371-5",
            _ => "Unknown",
        }
    }

    pub fn get_repeat_indicator_description(&self) -> &'static str {
        repeat_indicator_description(self.repeat_indicator)
    }
}

impl fmt::Display for AisClassAStaticData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "AIS Class A Static - MMSI: {}, Name: {}, Call: {}, Type: {}, Length: {:.1}m, Draft: {:.2}m",
            self.mmsi,
            self.name.trim(),
            self.callsign.trim(),
            self.type_of_ship,
            self.get_length_meters(),
            self.get_draft_meters()
        )
    }
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;

    use crate::pgns::AisClassAStaticData;





    #[test]
    fn test_ais_class_a_static_data_parsing() {
        /*
            2026-02-22-14:10:36.421,6,129794,0,255,8,a0,4b,05,f0,82,dd,0e,f5
            2026-02-22-14:10:36.421,6,129794,0,255,8,a1,73,95,00,39,48,41,36
            2026-02-22-14:10:36.422,6,129794,0,255,8,a2,32,34,36,50,41,50,41
            2026-02-22-14:10:36.422,6,129794,0,255,8,a3,40,40,40,40,40,40,40
            2026-02-22-14:10:36.423,6,129794,0,255,8,a4,40,40,40,40,40,40,40
            2026-02-22-14:10:36.423,6,129794,0,255,8,a5,40,40,25,26,02,64,00
            2026-02-22-14:10:36.424,6,129794,0,255,8,a6,ff,ff,ff,ff,ef,50,00
            2026-02-22-14:10:36.424,6,129794,0,255,8,a7,d9,4f,13,7b,01,4c,49
            2026-02-22-14:10:36.425,6,129794,0,255,8,a8,56,4f,52,4e,4f,40,40
            2026-02-22-14:10:36.425,6,129794,0,255,8,a9,40,40,40,40,40,40,40
            2026-02-22-14:10:36.426,6,129794,0,255,8,aa,40,40,40,40,06,e1,ff

            {"timestamp":"2026-02-22-14:10:36.426","prio":6,"src":0,"dst":255,"pgn":129794,"description":"AIS Class A Static and Voyage Related Data","fields":{
                "Message ID":"Static and voyage related data",
                "Repeat Indicator":"Initial",
                "User ID":"249398000",
                "IMO number":9794549,
                "Callsign":"9HA6246",
                "Name":"PAPA",
                "Type of ship":"Pleasure",
                "Length":55.0,
                "Beam":10.0,
                "ETA Date":"2026.09.23",
                "ETA Time":"09:00:00.0000",
                "Draft":3.79,
                "Destination":"LIVORNO",
                "AIS version indicator":"ITU-R M.1371-5",
                "GNSS type":"GPS",
                "DTE":"Available",
                "Reserved":"00",
                "AIS Transceiver information":"Channel B VDL reception"
            }}

        */
        let payload = [0x05, 0xf0,0x82,0xdd,0x0e,0xf5,
                                    0x73,0x95,0x00,0x39,0x48,0x41,0x36,
                                    0x32,0x34,0x36,0x50,0x41,0x50,0x41,
                                    0x40,0x40,0x40,0x40,0x40,0x40,0x40,
                                    0x40,0x40,0x40,0x40,0x40,0x40,0x40,
                                    0x40,0x40,0x25,0x26,0x02,0x64,0x00,
                                    0xff,0xff,0xff,0xff,0xef,0x50,0x00,
                                    0xd9,0x4f,0x13,0x7b,0x01,0x4c,0x49,
                                    0x56,0x4f,0x52,0x4e,0x4f,0x40,0x40,
                                    0x40,0x40,0x40,0x40,0x40,0x40,0x40,
                                    0x40,0x40,0x40,0x40,0x06,0xe1,0xff];
        let static_data = AisClassAStaticData::from_bytes(&payload).unwrap();
        assert_eq!(static_data.mmsi, 249398000);
        assert_eq!(static_data.imo_number, 9794549);
        assert_eq!(static_data.callsign.trim(), "9HA6246");
        assert_eq!(static_data.name.trim(), "PAPA");
        assert_eq!(static_data.type_of_ship, 37); // Pleasure
        assert_eq!(static_data.get_length_meters(), 55.0);
        assert_eq!(static_data.get_beam_meters(), 10.0);
        assert_eq!(static_data.get_draft_meters(), 3.79);
        assert_eq!(static_data.get_eta_time_seconds(), 32400.0); // 09:00:00 in seconds
        assert_eq!(static_data.destination.trim(), "LIVORNO");
        assert_eq!(static_data.ais_version, 2); // ITU-R M.1371-5
        assert_eq!(static_data.gnss_type, 1); // GPS
        assert_eq!(static_data.dte, false); // DTE
        assert_eq!(static_data.type_of_ship, 37); // Pleasure
        assert_eq!(static_data.eta_date_time.unwrap().to_date_time(), DateTime::parse_from_rfc3339("2026-09-23T09:00:00Z").unwrap()); // ETA Date and Time
        assert_eq!(static_data.class, "A");
    
    }



}