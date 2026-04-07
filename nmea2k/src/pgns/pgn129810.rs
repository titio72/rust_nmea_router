use std::fmt;
use super::ais_helpers::{read_bits, extract_text_from_bytes, type_of_ship_description, repeat_indicator_description};

/// AIS Class B Static Data Report Part B (PGN 129810)
/// Reports vessel type, dimensions, callsign, and other details (Part B of Class B static data)
#[derive(Debug, Clone)]
pub struct AisClassBStaticDataPartB {
    pub pgn: u32,
    pub message_id: u8,
    pub repeat_indicator: u8,
    pub mmsi: u32,
    pub type_of_ship: u8,
    pub vendor_id: String,         // 7 chars
    pub callsign: String,          // 7 chars
    pub length_raw: u16,           // × 0.1 meters
    pub beam_raw: u16,             // × 0.1 meters
    pub position_ref_starboard: u16, // × 0.1 meters
    pub position_ref_bow: u16,     // × 0.1 meters
    pub mothership_mmsi: u32,
    pub sequence_id: u8,
    pub class: String,               // "A" or "B"
}

impl AisClassBStaticDataPartB {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        // mothership_mmsi occupies bits 224-255 → bytes 28-31; 32 bytes is the minimum.
        // sequence_id at bit 272 (byte 34) is read as 0 when absent - that's fine.
        if data.len() < 32 {
            return None;
        }

        let message_id = read_bits(data, 0, 6) as u8;
        let repeat_indicator = read_bits(data, 6, 2) as u8;
        let mmsi = read_bits(data, 8, 32) as u32;
        let type_of_ship = read_bits(data, 40, 8) as u8;
        
        // vendor_id is 7 bytes of ASCII starting at byte 6
        let vendor_id = extract_text_from_bytes(data, 6, 7);
        
        // callsign is 7 bytes of ASCII starting at byte 13
        let callsign = extract_text_from_bytes(data, 13, 7);
        let length_raw = read_bits(data, 160, 16) as u16;
        let beam_raw = read_bits(data, 176, 16) as u16;
        let position_ref_starboard = read_bits(data, 192, 16) as u16;
        let position_ref_bow = read_bits(data, 208, 16) as u16;
        let mothership_mmsi = read_bits(data, 224, 32) as u32;
        let sequence_id = read_bits(data, 272, 8) as u8;

        Some(Self {
            pgn: 129810,
            message_id,
            repeat_indicator,
            mmsi,
            type_of_ship,
            vendor_id,
            callsign,
            length_raw,
            beam_raw,
            position_ref_starboard,
            position_ref_bow,
            mothership_mmsi,
            sequence_id,
            class: "B".to_string(),
        })
    }

    pub fn get_length_meters(&self) -> f64 {
        self.length_raw as f64 * 0.1
    }

    pub fn get_beam_meters(&self) -> f64 {
        self.beam_raw as f64 * 0.1
    }

    pub fn get_type_of_ship_description(&self) -> &'static str {
        type_of_ship_description(self.type_of_ship)
    }

    pub fn get_repeat_indicator_description(&self) -> &'static str {
        repeat_indicator_description(self.repeat_indicator)
    }
}

impl fmt::Display for AisClassBStaticDataPartB {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "AIS Class B Static (Part B) - MMSI: {}, Call: {}, Type: {}, Length: {:.1}m",
            self.mmsi,
            self.callsign.trim(),
            self.type_of_ship,
            self.get_length_meters()
        )
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_bytes() {
        let b = [
                0x18, 0x88, 0x78, 0xbe, 0x0e, 0x24, 0x28, 0xc8, 0x22, 0x94,
                0x0c, 0x00, 0x38, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40,
                0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0xeb, 0x00, 0x00];  // 35 bytes total
        let ais = AisClassBStaticDataPartB::from_bytes(&b).unwrap();
        assert_eq!("B", ais.class);
        assert_eq!("", ais.callsign.trim());
        assert_eq!(247363720, ais.mmsi);
        assert_eq!(36, ais.type_of_ship);  // AIS ship type code (36 = Sailing)
    }
}   