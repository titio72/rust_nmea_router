use std::fmt;
use super::ais_helpers::{read_bits, read_signed_bits, extract_text_from_bytes, repeat_indicator_description, time_stamp_description};

/// AIS Aid-to-Navigation (AtoN) Report (PGN 129041)
/// Reports position and status of navigational aids (buoys, beacons, etc.)
#[derive(Debug, Clone)]
pub struct AisAtonReport {
    pub pgn: u32,
    pub message_id: u8,
    pub repeat_indicator: u8,
    pub mmsi: u32,                // ATON ID
    pub longitude_raw: i32,
    pub latitude_raw: i32,
    pub position_accuracy: bool,
    pub raim: bool,
    pub time_stamp: u8,
    pub length_raw: u16,          // × 0.1 meters
    pub beam_raw: u16,            // × 0.1 meters
    pub position_ref_starboard: u16, // × 0.1 meters
    pub position_ref_true_north: u16, // × 0.1 meters
    pub aton_type: AisAtonType,
    pub off_position: bool,
    pub virtual_aton: bool,
    pub assigned_mode: bool,
    pub name: String,             // AtoN name
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AisAtonType {
    Default = 0,
    ReferencePoint = 1,
    Racon = 2,
    FixedStructure = 3,
    UnknownType = 4,
    Light = 5,
    LightWithSectors = 6,
    LeadingLightFront = 7,
    LeadingLightRear = 8,
    BeaconCardinalN = 9,
    BeaconCardinalE = 10,
    BeaconCardinalS = 11,
    BeaconCardinalW = 12,
    BeaconPortHand = 13,
    BeaconStarboardHand = 14,
    BeaconPreferredChannelPortHand = 15,
    BeaconPreferredChannelStarboardHand = 16,
    BeaconIsolatedDanger = 17,
    BeaconSafeWater = 18,
    BeaconSpecialMark = 19,
    CardinalMarkN = 20,
    CardinalMarkE = 21,
    CardinalMarkS = 22,
    CardinalMarkW = 23,
    PortHandMark = 24,
    StarboardHandMark = 25,
    PreferredChannelPortHand = 26,
    PreferredChannelStarboardHand = 27,
    IsolatedDangerMark = 28,
    SafeWaterMark = 29,
    SpecialMark = 30,
    LightVessel = 31,
}

impl From<u8> for AisAtonType {
    fn from(val: u8) -> Self {
        match val & 0x1F {
            0 => AisAtonType::Default,
            1 => AisAtonType::ReferencePoint,
            2 => AisAtonType::Racon,
            3 => AisAtonType::FixedStructure,
            4 => AisAtonType::UnknownType,
            5 => AisAtonType::Light,
            6 => AisAtonType::LightWithSectors,
            7 => AisAtonType::LeadingLightFront,
            8 => AisAtonType::LeadingLightRear,
            9 => AisAtonType::BeaconCardinalN,
            10 => AisAtonType::BeaconCardinalE,
            11 => AisAtonType::BeaconCardinalS,
            12 => AisAtonType::BeaconCardinalW,
            13 => AisAtonType::BeaconPortHand,
            14 => AisAtonType::BeaconStarboardHand,
            15 => AisAtonType::BeaconPreferredChannelPortHand,
            16 => AisAtonType::BeaconPreferredChannelStarboardHand,
            17 => AisAtonType::BeaconIsolatedDanger,
            18 => AisAtonType::BeaconSafeWater,
            19 => AisAtonType::BeaconSpecialMark,
            20 => AisAtonType::CardinalMarkN,
            21 => AisAtonType::CardinalMarkE,
            22 => AisAtonType::CardinalMarkS,
            23 => AisAtonType::CardinalMarkW,
            24 => AisAtonType::PortHandMark,
            25 => AisAtonType::StarboardHandMark,
            26 => AisAtonType::PreferredChannelPortHand,
            27 => AisAtonType::PreferredChannelStarboardHand,
            28 => AisAtonType::IsolatedDangerMark,
            29 => AisAtonType::SafeWaterMark,
            30 => AisAtonType::SpecialMark,
            _ => AisAtonType::LightVessel,
        }
    }
}

impl fmt::Display for AisAtonType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", match self {
            AisAtonType::Default => "Default",
            AisAtonType::ReferencePoint => "Reference Point",
            AisAtonType::Racon => "Racon",
            AisAtonType::FixedStructure => "Fixed Structure",
            AisAtonType::UnknownType => "Unknown",
            AisAtonType::Light => "Light",
            AisAtonType::LightWithSectors => "Light with Sectors",
            AisAtonType::LeadingLightFront => "Leading Light Front",
            AisAtonType::LeadingLightRear => "Leading Light Rear",
            AisAtonType::BeaconCardinalN => "Beacon Cardinal N",
            AisAtonType::BeaconCardinalE => "Beacon Cardinal E",
            AisAtonType::BeaconCardinalS => "Beacon Cardinal S",
            AisAtonType::BeaconCardinalW => "Beacon Cardinal W",
            AisAtonType::BeaconPortHand => "Beacon Port Hand",
            AisAtonType::BeaconStarboardHand => "Beacon Starboard Hand",
            AisAtonType::BeaconPreferredChannelPortHand => "Beacon PC Port Hand",
            AisAtonType::BeaconPreferredChannelStarboardHand => "Beacon PC Starboard Hand",
            AisAtonType::BeaconIsolatedDanger => "Beacon Isolated Danger",
            AisAtonType::BeaconSafeWater => "Beacon Safe Water",
            AisAtonType::BeaconSpecialMark => "Beacon Special Mark",
            AisAtonType::CardinalMarkN => "Cardinal Mark N",
            AisAtonType::CardinalMarkE => "Cardinal Mark E",
            AisAtonType::CardinalMarkS => "Cardinal Mark S",
            AisAtonType::CardinalMarkW => "Cardinal Mark W",
            AisAtonType::PortHandMark => "Port Hand Mark",
            AisAtonType::StarboardHandMark => "Starboard Hand Mark",
            AisAtonType::PreferredChannelPortHand => "Preferred Channel Port Hand",
            AisAtonType::PreferredChannelStarboardHand => "Preferred Channel Starboard Hand",
            AisAtonType::IsolatedDangerMark => "Isolated Danger Mark",
            AisAtonType::SafeWaterMark => "Safe Water Mark",
            AisAtonType::SpecialMark => "Special Mark",
            AisAtonType::LightVessel => "Light Vessel",
        })
    }
}

impl AisAtonReport {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
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
        
        let length_raw = read_bits(data, 112, 16) as u16;
        let beam_raw = read_bits(data, 128, 16) as u16;
        let position_ref_starboard = read_bits(data, 144, 16) as u16;
        let position_ref_true_north = read_bits(data, 160, 16) as u16;
        
        let aton_type_bits = read_bits(data, 176, 5) as u8;
        let off_position = read_bits(data, 181, 1) != 0;
        let virtual_aton = read_bits(data, 182, 1) != 0;
        let assigned_mode = read_bits(data, 183, 1) != 0;
        
        // AtoN name is variable-length (max 20 bytes of ASCII) starting at byte 26
        // Format: byte 26=length, byte 27=control, bytes 28+=actual name
        let name = if data.len() > 28 {
            let length = data[26] as usize;
            let actual_len = length.min(data.len() - 28);
            extract_text_from_bytes(data, 28, actual_len)
        } else {
            String::new()
        };

        Some(Self {
            pgn: 129041,
            message_id,
            repeat_indicator,
            mmsi,
            longitude_raw,
            latitude_raw,
            position_accuracy,
            raim,
            time_stamp,
            length_raw,
            beam_raw,
            position_ref_starboard,
            position_ref_true_north,
            aton_type: AisAtonType::from(aton_type_bits),
            off_position,
            virtual_aton,
            assigned_mode,
            name,
        })
    }

    pub fn get_longitude_degrees(&self) -> f64 {
        self.longitude_raw as f64 * 1e-7
    }

    pub fn get_latitude_degrees(&self) -> f64 {
        self.latitude_raw as f64 * 1e-7
    }

    pub fn get_length_meters(&self) -> f64 {
        self.length_raw as f64 * 0.1
    }

    pub fn get_beam_meters(&self) -> f64 {
        self.beam_raw as f64 * 0.1
    }

    pub fn get_repeat_indicator_description(&self) -> &'static str {
        repeat_indicator_description(self.repeat_indicator)
    }

    pub fn get_time_stamp_description(&self) -> String {
        time_stamp_description(self.time_stamp)
    }
}

impl fmt::Display for AisAtonReport {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "AIS AtoN - ID: {}, Name: {}, Type: {}, Lat: {:.6}, Lon: {:.6}",
            self.mmsi,
            self.name.trim(),
            self.aton_type,
            self.get_latitude_degrees(),
            self.get_longitude_degrees()
        )
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ais_aton_report_parsing() {
        let secche_mel = [
            0x55, 0x6c, 0xe8, 0x27, 0x3b,
            0x6e, 0x52, 0x1a, 0x06, 0xee,
            0xe9, 0xf2, 0x19, 0xfc, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00,
            0xff, 0xff, 0x5e, 0xe0, 0xff,
            0xe0, 0x1c, 0x01, 0x4d, 0x50,
            0x41, 0x20, 0x53, 0x45, 0x43,
            0x43, 0x48, 0x45, 0x20, 0x44,
            0x45, 0x4c, 0x4c, 0x41, 0x20,
            0x4d, 0x45, 0x4c, 0x20, 0x20,
            0x49, 0x41, 0x2d, 0x44, 0xff];

        let aton_report = AisAtonReport::from_bytes(&secche_mel).unwrap();
        println!("{}", aton_report);
        assert_eq!(aton_report.name, "MPA SECCHE DELLA MEL  IA-D");
        assert_eq!(aton_report.aton_type, AisAtonType::SpecialMark);
        assert!((aton_report.get_latitude_degrees() - 43.5349998).abs() < 0.0000001);
        assert!((aton_report.get_longitude_degrees() - 10.2388334).abs() < 0.0000001);
    }

}













