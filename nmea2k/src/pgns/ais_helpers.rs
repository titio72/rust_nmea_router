/// AIS domain lookup helpers.
///
/// Generic bit-parsing utilities (`read_bits`, `read_signed_bits`, `extract_text_from_bytes`)
/// live in [`super::bit_utils`] and are re-imported here for use by AIS PGN decoders.
pub(crate) use super::bit_utils::{read_bits, read_signed_bits, extract_text_from_bytes};

/// Return a human-readable description for the AIS repeat indicator field (2 bits)
pub fn repeat_indicator_description(v: u8) -> &'static str {
    match v & 0x03 {
        0 => "Initial",
        1 => "First retransmission",
        2 => "Second retransmission",
        3 => "Do not retransmit",
        _ => "Unknown",
    }
}

/// Return a human-readable description for the AIS time stamp field (6 bits).
/// Returns a formatted String because values 0–59 embed the actual UTC second.
pub fn time_stamp_description(v: u8) -> String {
    match v {
        0..=59 => format!("UTC second {}", v),
        60 => "No electronic fix".to_string(),
        61 => "Manual input mode".to_string(),
        62 => "Dead reckoning mode".to_string(),
        63 => "Positioning system inoperative".to_string(),
        _ => format!("Reserved ({})", v),
    }
}

/// Return a human-readable description for the AIS transceiver info field (5 bits)
pub fn ais_transceiver_info_description(v: u8) -> &'static str {
    match v {
        0 => "Channel A VDL reception",
        1 => "Channel B VDL reception",
        2 => "Channel A VDL transmission",
        3 => "Channel B VDL transmission",
        4 => "Own information not broadcast",
        5 => "Reserved",
        _ => "Unknown",
    }
}

/// Return a human-readable description for a GNSS type field (4 bits)
pub fn gnss_type_description(v: u8) -> &'static str {
    match v {
        0 => "Undefined",
        1 => "GPS",
        2 => "GLONASS",
        3 => "Combined GPS/GLONASS",
        4 => "Loran-C",
        5 => "Chayka",
        6 => "Integrated navigation system",
        7 => "Surveyed",
        8 => "Galileo",
        15 => "Internal GNSS",
        _ => "Reserved",
    }
}

/// Return a human-readable description for the AIS type-of-ship field (8 bits).
/// Based on ITU-R M.1371 vessel type codes.
pub fn type_of_ship_description(v: u8) -> &'static str {
    match v {
        0 => "Not available",
        1..=19 => "Reserved",
        20 => "Wing in ground (WIG)",
        21 => "WIG – Hazardous category A",
        22 => "WIG – Hazardous category B",
        23 => "WIG – Hazardous category C",
        24 => "WIG – Hazardous category D",
        25..=29 => "WIG – Reserved",
        30 => "Fishing",
        31 => "Towing",
        32 => "Towing (length >200m or breadth >25m)",
        33 => "Dredging/underwater ops",
        34 => "Diving ops",
        35 => "Military ops",
        36 => "Sailing",
        37 => "Pleasure craft",
        38..=39 => "Reserved",
        40 => "High speed craft (HSC)",
        41 => "HSC – Hazardous category A",
        42 => "HSC – Hazardous category B",
        43 => "HSC – Hazardous category C",
        44 => "HSC – Hazardous category D",
        45..=48 => "HSC – Reserved",
        49 => "HSC – No additional information",
        50 => "Pilot vessel",
        51 => "Search and rescue vessel",
        52 => "Tug",
        53 => "Port tender",
        54 => "Anti-pollution equipment",
        55 => "Law enforcement",
        56..=57 => "Spare",
        58 => "Medical transport",
        59 => "Noncombatant ship",
        60 => "Passenger",
        61 => "Passenger – Hazardous category A",
        62 => "Passenger – Hazardous category B",
        63 => "Passenger – Hazardous category C",
        64 => "Passenger – Hazardous category D",
        65..=68 => "Passenger – Reserved",
        69 => "Passenger – No additional information",
        70 => "Cargo",
        71 => "Cargo – Hazardous category A",
        72 => "Cargo – Hazardous category B",
        73 => "Cargo – Hazardous category C",
        74 => "Cargo – Hazardous category D",
        75..=78 => "Cargo – Reserved",
        79 => "Cargo – No additional information",
        80 => "Tanker",
        81 => "Tanker – Hazardous category A",
        82 => "Tanker – Hazardous category B",
        83 => "Tanker – Hazardous category C",
        84 => "Tanker – Hazardous category D",
        85..=88 => "Tanker – Reserved",
        89 => "Tanker – No additional information",
        90 => "Other type",
        91 => "Other type – Hazardous category A",
        92 => "Other type – Hazardous category B",
        93 => "Other type – Hazardous category C",
        94 => "Other type – Hazardous category D",
        95..=98 => "Other type – Reserved",
        99 => "Other type – No additional information",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    // Tests for read_bits, read_signed_bits, and extract_text_from_bytes
    // live in bit_utils.rs where those functions are defined.
}
