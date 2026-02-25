/// AIS bit-parsing helpers
/// 
/// AIS messages use bit-aligned fields, so standard byte-aligned parsing doesn't work.
/// These helpers support extracting arbitrary-width fields from a little-endian bit stream.

/// Read an unsigned integer from a bit-aligned position
/// 
/// # Arguments
/// * `data` - The data buffer
/// * `bit_offset` - Bit position (0-indexed) where the field starts
/// * `bit_len` - Number of bits to extract (1-64)
/// 
/// # Returns
/// The extracted value as u64
pub(crate) fn read_bits(data: &[u8], bit_offset: usize, bit_len: usize) -> u64 {
    let mut value: u64 = 0;
    for i in 0..bit_len {
        let bit_pos = bit_offset + i;
        let byte_idx = bit_pos / 8;
        let bit_in_byte = bit_pos % 8;
        if byte_idx < data.len() {
            value |= (((data[byte_idx] >> bit_in_byte) & 1) as u64) << i;
        }
    }
    value
}

/// Read a signed integer from a bit-aligned position with sign extension
/// 
/// # Arguments
/// * `data` - The data buffer
/// * `bit_offset` - Bit position (0-indexed) where the field starts
/// * `bit_len` - Number of bits to extract (1-64)
/// 
/// # Returns
/// The extracted value as i64, sign-extended if bit_len < 64
pub(crate) fn read_signed_bits(data: &[u8], bit_offset: usize, bit_len: usize) -> i64 {
    let unsigned = read_bits(data, bit_offset, bit_len);
    
    // Sign-extend if the high bit is set
    if bit_len < 64 && (unsigned & (1u64 << (bit_len - 1))) != 0 {
        // Sign extend: set all higher bits to 1
        let mask = !((1u64 << bit_len) - 1);
        (unsigned | mask) as i64
    } else {
        unsigned as i64
    }
}

/// Extract and clean text from raw bytes, removing padding and filtering characters
/// 
/// This function:
/// 1. Removes trailing padding characters (0xFF, space, 0x00, '@')
/// 2. Escapes special control characters (\b, \n, \r, \t, ", \, /)
/// 3. Filters to only include printable ASCII (space to tilde, 32-126)
/// 
/// # Arguments
/// * `data` - The data buffer
/// * `byte_start` - Starting byte offset in the buffer
/// * `byte_length` - Maximum number of bytes to extract
/// 
/// # Returns
/// A cleaned String with padding removed and special characters escaped
pub(crate) fn extract_text_from_bytes(data: &[u8], byte_start: usize, byte_length: usize) -> String {
    if byte_start >= data.len() {
        return String::new();
    }

    let max_length = data.len() - byte_start;
    let actual_length = byte_length.min(max_length);

    if actual_length == 0 {
        return String::new();
    }

    // Remove trailing padding characters (0xFF, space, 0x00, '@')
    let mut end = actual_length;
    while end > 0 {
        let byte = data[byte_start + end - 1];
        if byte == 0xFF || byte == b' ' || byte == 0x00 || byte == b'@' {
            end -= 1;
        } else {
            break;
        }
    }

    if end == 0 {
        return String::new();
    }

    // Extract and escape/filter characters
    let mut result = String::new();
    for i in 0..end {
        let c = data[byte_start + i] as char;
        match c {
            '\x08' => result.push_str("\\b"),      // backspace
            '\n' => result.push_str("\\n"),        // newline
            '\r' => result.push_str("\\r"),        // carriage return
            '\t' => result.push_str("\\t"),        // tab
            '"' => result.push_str("\\\""),        // quote
            '\\' => result.push_str("\\\\"),       // backslash
            '/' => result.push_str("\\/"),         // forward slash
            c if c >= ' ' && c <= '~' => result.push(c), // printable ASCII
            _ => {}  // skip all other characters (control chars, non-ASCII)
        }
    }

    result
}

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
    use super::*;

    #[test]
    fn test_read_bits_byte_aligned() {
        let data = [0b11010101u8, 0b01010011u8];
        
        // First byte, first 3 bits = 0b101 (reversed binary) = 5
        assert_eq!(read_bits(&data, 0, 3), 5);
        
        // First byte, bits 3-5 = 0b010 = 2
        assert_eq!(read_bits(&data, 3, 3), 2);
    }

    #[test]
    fn test_read_bits_span_bytes() {
        // Bits: 01010011 11010101
        //       ^^^^^^^^ byte[0], next 8 bits from byte[1]
        let data = [0b11010101u8, 0b01010011u8];
        
        // Bits 5-12 (8 bits) should give us bits [5..13)
        // Byte 0, bits 5-7: 010
        // Byte 1, bits 0-4: 10011 (reversed)
        // Total: 010 + 10011 (LSB first) = 10011010 = 26? Let me recalculate.
        // Actually LE means bit 0 is LSB. So:
        // Byte 0: bit 5 = 1, bit 6 = 1, bit 7 = 0
        // Byte 1: bit 0 = 1, bit 1 = 1, bit 2 = 0, bit 3 = 0, bit 4 = 1
        // Combined (5-12): 1, 1, 0, 1, 1, 0, 0, 1 = 0b10011011 = 0x9B = 155
        // Let's verify with the actual bit pattern
        let result = read_bits(&data, 5, 8);
        // Manually: data[0] = 0b11010101, bits 5-7 are: (data[0] >> 5) & 0b111 = 0b010 = 2
        // data[1] = 0b01010011, bits 0-4 are: data[1] & 0b11111 = 0b10011 = 19
        // Combined: 2 | (19 << 3) = 2 + 152 = 154
        assert_eq!(result, 154);
    }

    #[test]
    fn test_read_signed_bits_positive() {
        let data = [0b01010101u8]; // 0x55
        let result = read_signed_bits(&data, 0, 7);
        assert_eq!(result, 0b0101010); // 42, positive
    }

    #[test]
    fn test_read_signed_bits_negative() {
        let data = [0b10101010u8]; // 0xAA
        let _result = read_signed_bits(&data, 0, 7);
        // Bit 6 = 1 (sign bit), so negative
        // Value = 0b0101010 = 42, but with bit 6 set, so sign-extend to -22
        // Actually: lower 7 bits = 0b0101010 = 42
        // Bit 6 is at position 6 (0-indexed), so (0b10101010 & 0b01111111) = 0b00101010 = 42
        // But the sign bit (bit 6) is 1, so result is -(2^6 - 42) = -(64 - 42) = -22
        // Wait, let me recalculate: 0xAA = 0b10101010
        // Bits 0-6 (7 bits): take bits 0,1,0,1,0,1,0 = 0b0101010 = 42
        // Bit 6 (position 6) = 0 in this case... let me recount.
        // 10101010: bit 0=0, bit 1=1, bit 2=0, bit 3=1, bit 4=0, bit 5=1, bit 6=0, bit 7=1
        // 7 bits = 0b0101010 = 42, no sign bit set. So result = 42.
        
        // Let's use a simpler example: 0b10000000 should give -1 in 7 bits
        let data = [0b10000000u8];
        let result = read_signed_bits(&data, 0, 7);
        // Bits 0-6: 0,0,0,0,0,0,1 = 0b1000000 = 64? No wait.
        // 0b10000000: bit 0=0, bit 1=0, ..., bit 6=1
        // So lower 7 bits = 0b1000000 = 64, and sign bit (bit 6) is 1
        // Using 7 bits: mask = (1u64 << 7) - 1 = 0x7F = 0b01111111
        // unsigned = 0b1000000 & 0x7F = 0b1000000 = 64
        // Then we check bit 6: (64 & (1 << 6)) = (64 & 64) = 64, which is != 0
        // So we sign-extend: mask = !(0x7F) = all bits except 0-6
        // (64 | mask) as i64 = ... actually this should give -64 + 64 = 0? No.
        // Let me think again. If the sign bit is set in the 7-bit value 0b1000000,
        // then in 2's complement, this represents -64.
        // So: (64 | !(0x7F)) as i64 should be -64.
        // !(0x7F) = 0xFFFF...FF80 (all 1s except bits 0-6)
        // 64 | 0xFFFF...FF80 = 0xFFFF...FFC0 = -64 in 2's complement. YES!
        
        // So the test should be: 0b1000000 in 7 bits = -64
        assert_eq!(result, -64);
    }

    #[test]
    fn test_extract_text_basic() {
        // "NASHORN" followed by spaces
        let data = [
            0x4e, 0x41, 0x53, 0x48, 0x4f, 0x52, 0x4e, 0x20, 0x20, 0x20,
            0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20,
        ];
        let result = extract_text_from_bytes(&data, 0, 20);
        assert_eq!(result, "NASHORN");
    }

    #[test]
    fn test_extract_text_with_0xff_padding() {
        // "TEST" followed by 0xFF padding
        let data = [0x54, 0x45, 0x53, 0x54, 0xff, 0xff, 0xff];
        let result = extract_text_from_bytes(&data, 0, 7);
        assert_eq!(result, "TEST");
    }

    #[test]
    fn test_extract_text_mixed_padding() {
        // "HELLO" followed by mix of space, @, and 0xFF
        let data = [0x48, 0x45, 0x4c, 0x4c, 0x4f, 0x20, 0x40, 0xff, 0x00];
        let result = extract_text_from_bytes(&data, 0, 9);
        assert_eq!(result, "HELLO");
    }

    #[test]
    fn test_extract_text_filters_control_chars() {
        // "AB\nCD" should escape newline character to \\n
        let data = [0x41, 0x42, 0x0a, 0x43, 0x44];
        let result = extract_text_from_bytes(&data, 0, 5);
        assert_eq!(result, "AB\\nCD");
    }

    #[test]
    fn test_extract_text_escapes_special_chars() {
        // Test escaping of special characters
        let data = [0x41, 0x08, 0x42, 0x09, 0x43]; // A, \b, B, \t, C
        let result = extract_text_from_bytes(&data, 0, 5);
        assert_eq!(result, "A\\bB\\tC");
    }

    #[test]
    fn test_extract_text_filters_non_ascii() {
        // High bytes (non-ASCII) should be filtered out
        let data = [0x41, 0x42, 0xff, 0x43, 0x44]; // AB<0xff>CD
        let result = extract_text_from_bytes(&data, 0, 5);
        // 0xff as char is non-printable, should be skipped
        assert_eq!(result, "ABCD");
    }

    #[test]
    fn test_extract_text_only_printable_ascii() {
        // Only printable ASCII (space to tilde) should be included
        let data = [0x20, 0x41, 0x7e, 0x1f, 0x42]; // space, A, ~, DEL, B
        let result = extract_text_from_bytes(&data, 0, 5);
        // 0x1f (31) is below space (32), should be filtered
        assert_eq!(result, " A~B");
    }

    #[test]
    fn test_extract_text_offset_and_length() {
        // Test byte offset and length limiting
        let data = [0x41, 0x42, 0x43, 0x44, 0x45]; // ABCDE
        let result = extract_text_from_bytes(&data, 1, 3); // start at 'B', take 3
        assert_eq!(result, "BCD");
    }

    #[test]
    fn test_extract_text_invalid_offset() {
        // Invalid offset should return empty string
        let data = [0x41, 0x42, 0x43];
        let result = extract_text_from_bytes(&data, 10, 5);
        assert_eq!(result, "");
    }

    #[test]
    fn test_extract_text_all_padding() {
        // All padding should return empty string
        let data = [0x20, 0x20, 0xff, 0x00, 0x40];
        let result = extract_text_from_bytes(&data, 0, 5);
        assert_eq!(result, "");
    }
}
