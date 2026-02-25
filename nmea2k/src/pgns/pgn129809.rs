use std::fmt;
use super::ais_helpers::{read_bits, extract_text_from_bytes, repeat_indicator_description};

/// AIS Class B Static Data Report Part A (PGN 129809)
/// Reports vessel name (Part A of Class B static data)
#[derive(Debug, Clone)]
pub struct AisClassBStaticDataPartA {
    pub pgn: u32,
    pub message_id: u8,
    pub repeat_indicator: u8,
    pub mmsi: u32,
    pub name: String,              // 20 chars of 6-bit ASCII
    pub sequence_id: u8,
    pub class: String,
}

impl AisClassBStaticDataPartA {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 27 {
            return None;
        }

        let message_id = read_bits(data, 0, 6) as u8;
        let repeat_indicator = read_bits(data, 6, 2) as u8;
        let mmsi = read_bits(data, 8, 32) as u32;
        
        // Name is stored as regular ASCII, 20 bytes starting at byte 5
        let name = extract_text_from_bytes(data, 5, 20);
        
        let sequence_id = read_bits(data, 208, 8) as u8;

        Some(Self {
            pgn: 129809,
            message_id,
            repeat_indicator,
            mmsi,
            name,
            sequence_id,
            class: "B".to_string(),
        })
    }

    pub fn get_repeat_indicator_description(&self) -> &'static str {
        repeat_indicator_description(self.repeat_indicator)
    }
}

impl fmt::Display for AisClassBStaticDataPartA {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "AIS Class B Static (Part A) - MMSI: {}, Name: {}, Class: {}",
            self.mmsi,
            self.name.trim(),
            self.class
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ais_class_b_static_data_part_a() {
        // Example raw data for PGN 129809 (20 bytes of name "TEST VESSEL" padded with spaces)
        let raw_data = [
                0x18, 0xc8, 0x53, 0xbc, 0x0e, 0x4e, 0x41, 0x53, 0x48,
            0x4f, 0x52, 0x4e, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20,
            0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0xff, 0xff];

        let m = AisClassBStaticDataPartA::from_bytes(&raw_data).unwrap();
        assert_eq!("NASHORN", m.name);
        assert_eq!(247223240, m.mmsi);
        assert_eq!("B", m.class);
    }
}