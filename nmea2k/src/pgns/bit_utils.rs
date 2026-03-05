/// Generic bit-manipulation utilities for little-endian bit-packed binary protocols.
///
/// NMEA2000 AIS payloads (and similar marine binary protocols) pack fields at arbitrary
/// bit boundaries. Standard byte-aligned `from_le_bytes` does not work for these;
/// instead, fields must be read bit-by-bit across byte boundaries.

/// Read an unsigned integer from a bit-aligned position in a little-endian bit stream.
///
/// Bits are indexed from the LSB of byte 0 outward: bit 0 is the LSB of `data[0]`,
/// bit 7 is the MSB of `data[0]`, bit 8 is the LSB of `data[1]`, etc.
///
/// # Arguments
/// * `data`       – The data buffer.
/// * `bit_offset` – Bit position (0-indexed) where the field starts.
/// * `bit_len`    – Number of bits to extract (1–64).
///
/// # Returns
/// The extracted value as `u64`, with bit 0 of the field in the LSB.
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

/// Read a signed integer from a bit-aligned position with 2's-complement sign extension.
///
/// Identical to [`read_bits`] but sign-extends the result when the MSB of the
/// extracted field is set.
///
/// # Arguments
/// * `data`       – The data buffer.
/// * `bit_offset` – Bit position (0-indexed) where the field starts.
/// * `bit_len`    – Number of bits to extract (1–64).
///
/// # Returns
/// The extracted value as `i64`, sign-extended to 64 bits if the field's MSB is 1.
pub(crate) fn read_signed_bits(data: &[u8], bit_offset: usize, bit_len: usize) -> i64 {
    let unsigned = read_bits(data, bit_offset, bit_len);

    // Sign-extend if the high bit of the extracted field is set
    if bit_len < 64 && (unsigned & (1u64 << (bit_len - 1))) != 0 {
        let mask = !((1u64 << bit_len) - 1);
        (unsigned | mask) as i64
    } else {
        unsigned as i64
    }
}

/// Extract and clean text from byte-aligned raw bytes.
///
/// The padding rules (`@`, `0xFF`, space, `0x00`) are common across marine binary
/// protocols (NMEA2000 string fields, AIS names, etc.), not just AIS.
///
/// This function:
/// 1. Removes trailing padding characters (`0xFF`, space, `0x00`, `@`).
/// 2. Escapes special control characters (`\b`, `\n`, `\r`, `\t`, `"`, `\`, `/`).
/// 3. Filters to only printable ASCII (codes 32–126).
///
/// # Arguments
/// * `data`        – The data buffer.
/// * `byte_start`  – Starting byte offset in the buffer.
/// * `byte_length` – Maximum number of bytes to read.
///
/// # Returns
/// A cleaned `String` with padding removed and special characters escaped.
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
            '\x08' => result.push_str("\\b"),   // backspace
            '\n'   => result.push_str("\\n"),   // newline
            '\r'   => result.push_str("\\r"),   // carriage return
            '\t'   => result.push_str("\\t"),   // tab
            '"'    => result.push_str("\\\""),  // double quote
            '\\'   => result.push_str("\\\\"),  // backslash
            '/'    => result.push_str("\\/"),   // forward slash
            c if c >= ' ' && c <= '~' => result.push(c), // printable ASCII
            _ => {} // skip control chars and non-ASCII
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- read_bits -----

    #[test]
    fn test_read_bits_byte_aligned() {
        let data = [0b11010101u8, 0b01010011u8];

        // First byte, first 3 bits (LSB first) = bits 0,1,2 of 0b11010101 = 1,0,1 → 0b101 = 5
        assert_eq!(read_bits(&data, 0, 3), 5);

        // First byte, bits 3–5 = bits 3,4,5 of 0b11010101 = 0,1,0 → 0b010 = 2
        assert_eq!(read_bits(&data, 3, 3), 2);
    }

    #[test]
    fn test_read_bits_span_bytes() {
        // data[0] = 0b11010101, data[1] = 0b01010011
        // Bits 5–12 (8 bits, little-endian):
        //   byte 0, bits 5,6,7 → (0b11010101 >> 5) & 7 = 0b110 = 6
        //   byte 1, bits 0–4  → 0b01010011 & 0b11111 = 0b10011 = 19
        //   combined: 6 | (19 << 3) = 6 + 152 = 158
        let data = [0b11010101u8, 0b01010011u8];
        assert_eq!(read_bits(&data, 5, 8), 158);
    }

    // ----- read_signed_bits -----

    #[test]
    fn test_read_signed_bits_positive() {
        // 0b00101010 = 42; 7-bit extraction: bits 0-6 = 0b0101010 = 42; bit 6 = 0 → positive
        // (0b01010101 would have bit 6 = 1, making it -43 in 7-bit 2's complement)
        let data = [0b00101010u8];
        assert_eq!(read_signed_bits(&data, 0, 7), 42);
    }

    #[test]
    fn test_read_signed_bits_negative() {
        // 0b10101010 = 170; lower 7 bits = 0b0101010 = 42; bit 6 = 0 → positive 42
        let data = [0b10101010u8];
        assert_eq!(read_signed_bits(&data, 0, 7), 42);

        // 0b01000000 = 64; lower 7 bits = 0b1000000 = 64; bit 6 = 1 → sign-extend → -64
        let data = [0b01000000u8];
        assert_eq!(read_signed_bits(&data, 0, 7), -64);
    }

    // ----- extract_text_from_bytes -----

    #[test]
    fn test_extract_text_basic() {
        // "NASHORN" followed by space padding
        let data = [
            0x4e, 0x41, 0x53, 0x48, 0x4f, 0x52, 0x4e,
            0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20,
            0x20, 0x20, 0x20, 0x20, 0x20,
        ];
        assert_eq!(extract_text_from_bytes(&data, 0, 20), "NASHORN");
    }

    #[test]
    fn test_extract_text_with_0xff_padding() {
        let data = [0x54, 0x45, 0x53, 0x54, 0xff, 0xff, 0xff]; // "TEST" + 0xFF padding
        assert_eq!(extract_text_from_bytes(&data, 0, 7), "TEST");
    }

    #[test]
    fn test_extract_text_mixed_padding() {
        // "HELLO" followed by mixed padding (space, @, 0xFF, 0x00)
        let data = [0x48, 0x45, 0x4c, 0x4c, 0x4f, 0x20, 0x40, 0xff, 0x00];
        assert_eq!(extract_text_from_bytes(&data, 0, 9), "HELLO");
    }

    #[test]
    fn test_extract_text_filters_control_chars() {
        // "AB\nCD" — newline should be escaped to \\n
        let data = [0x41, 0x42, 0x0a, 0x43, 0x44];
        assert_eq!(extract_text_from_bytes(&data, 0, 5), "AB\\nCD");
    }

    #[test]
    fn test_extract_text_escapes_special_chars() {
        // A, \b, B, \t, C
        let data = [0x41, 0x08, 0x42, 0x09, 0x43];
        assert_eq!(extract_text_from_bytes(&data, 0, 5), "A\\bB\\tC");
    }

    #[test]
    fn test_extract_text_filters_non_ascii() {
        // AB<0xFF>CD — 0xFF is non-printable, should be skipped (not trailing → filtered mid-string)
        let data = [0x41, 0x42, 0xff, 0x43, 0x44];
        assert_eq!(extract_text_from_bytes(&data, 0, 5), "ABCD");
    }

    #[test]
    fn test_extract_text_only_printable_ascii() {
        // space(0x20), A, ~(0x7E), DEL(0x1F), B — 0x1F is below space, should be filtered
        let data = [0x20, 0x41, 0x7e, 0x1f, 0x42];
        assert_eq!(extract_text_from_bytes(&data, 0, 5), " A~B");
    }

    #[test]
    fn test_extract_text_offset_and_length() {
        // ABCDE, start at B (offset 1), take 3 → "BCD"
        let data = [0x41, 0x42, 0x43, 0x44, 0x45];
        assert_eq!(extract_text_from_bytes(&data, 1, 3), "BCD");
    }

    #[test]
    fn test_extract_text_invalid_offset() {
        let data = [0x41, 0x42, 0x43];
        assert_eq!(extract_text_from_bytes(&data, 10, 5), "");
    }

    #[test]
    fn test_extract_text_all_padding() {
        let data = [0x20, 0x20, 0xff, 0x00, 0x40];
        assert_eq!(extract_text_from_bytes(&data, 0, 5), "");
    }
}
