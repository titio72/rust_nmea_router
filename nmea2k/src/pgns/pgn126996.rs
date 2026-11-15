use std::fmt;

use super::sentinel::Available;

/// PGN 126996 Product Information (fast packet, 134 bytes), canboat productInformation.
///
/// Layout: NMEA 2000 version u16 (x0.001), product code u16, model ID / software version /
/// model version / serial code as four 32-byte fixed strings, certification level u8,
/// load equivalency u8. `nmea2000_version` is kept as the raw u16 (2100 = version 2.100).
/// Strings lose their trailing `0xFF`/`@`/space/`0x00` padding and are decoded as lossy UTF-8
/// (invalid bytes become U+FFFD); no escaping is applied, so `/`, `"` and `\` survive intact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductInformation {
    pub nmea2000_version: Option<u16>,
    pub product_code: Option<u16>,
    pub model_id: String,
    pub software_version: String,
    pub model_version: String,
    pub serial_code: String,
    pub certification_level: Option<u8>,
    pub load_equivalency: Option<u8>,
}

const LENGTH: usize = 134;
const STRING_LEN: usize = 32;

/// A fixed-length string field: trailing padding stripped, invalid UTF-8 replaced lossily.
/// Deliberately not `bit_utils::extract_text_from_bytes`, which backslash-escapes `/`, `"` and `\`
/// and would corrupt identity strings such as "GPS/Compass".
fn fixed_string(data: &[u8], start: usize, len: usize) -> String {
    let field = &data[start..start + len];
    let end = field
        .iter()
        .rposition(|&b| !matches!(b, 0xFF | b'@' | 0x00 | b' '))
        .map_or(0, |i| i + 1);
    String::from_utf8_lossy(&field[..end]).into_owned()
}

impl ProductInformation {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < LENGTH {
            return None;
        }
        Some(Self {
            nmea2000_version: u16::from_le_bytes([data[0], data[1]]).available(),
            product_code: u16::from_le_bytes([data[2], data[3]]).available(),
            model_id: fixed_string(data, 4, STRING_LEN),
            software_version: fixed_string(data, 36, STRING_LEN),
            model_version: fixed_string(data, 68, STRING_LEN),
            serial_code: fixed_string(data, 100, STRING_LEN),
            certification_level: data[132].available(),
            load_equivalency: data[133].available(),
        })
    }

    /// Encodes exactly 134 bytes: strings truncated to 32 bytes and padded with 0xFF,
    /// `None` numerics as all-ones.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(LENGTH);
        out.extend_from_slice(&self.nmea2000_version.unwrap_or(u16::MAX).to_le_bytes());
        out.extend_from_slice(&self.product_code.unwrap_or(u16::MAX).to_le_bytes());
        for s in [&self.model_id, &self.software_version, &self.model_version, &self.serial_code] {
            let b = s.as_bytes();
            let n = b.len().min(STRING_LEN);
            out.extend_from_slice(&b[..n]);
            out.resize(out.len() + (STRING_LEN - n), 0xFF);
        }
        out.push(self.certification_level.unwrap_or(u8::MAX));
        out.push(self.load_equivalency.unwrap_or(u8::MAX));
        out
    }
}

impl fmt::Display for ProductInformation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Product Information: model \"{}\", software \"{}\", model version \"{}\", serial \"{}\"",
            self.model_id, self.software_version, self.model_version, self.serial_code
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn product_info_round_trip() {
        let p = ProductInformation { nmea2000_version: Some(2100), product_code: Some(1), model_id: "NMEA Router".into(),
            software_version: "0.1.0".into(), model_version: "".into(), serial_code: "271828".into(),
            certification_level: Some(0), load_equivalency: Some(1) };
        let bytes = p.to_bytes();
        assert_eq!(bytes.len(), 134);
        let d = ProductInformation::from_bytes(&bytes).unwrap();
        assert_eq!((d.model_id.as_str(), d.software_version.as_str(), d.serial_code.as_str()), ("NMEA Router", "0.1.0", "271828"));
        assert_eq!((d.nmea2000_version, d.product_code, d.certification_level, d.load_equivalency), (Some(2100), Some(1), Some(0), Some(1)));
    }
    #[test] fn product_info_cleans_padding() {
        let mut b = vec![0xFFu8; 134];
        b[4..4 + 5].copy_from_slice(b"i70s@");
        b[36..36 + 4].copy_from_slice(&[b'3', b'.', 0xC3, 0x28]);
        let d = ProductInformation::from_bytes(&b).unwrap();
        assert_eq!(d.model_id, "i70s");
        assert_eq!(d.software_version, "3.\u{FFFD}(");
        assert_eq!(d.nmea2000_version, None);
    }
    #[test] fn product_info_does_not_escape_text() {
        let p = ProductInformation { nmea2000_version: None, product_code: None, model_id: "GPS/Compass \"X\" a\\b".into(),
            software_version: "".into(), model_version: "".into(), serial_code: "".into(),
            certification_level: None, load_equivalency: None };
        let d = ProductInformation::from_bytes(&p.to_bytes()).unwrap();
        assert_eq!(d.model_id, "GPS/Compass \"X\" a\\b");
    }
    // canboat database/pgns/126996-productInformation.yaml sample 2 (CORTEX)
    #[test] fn product_info_canboat_sample() {
        let mut b = vec![0xFFu8; 134];
        b[0..4].copy_from_slice(&[0x34, 0x08, 0xc5, 0x79]);
        b[4..10].copy_from_slice(b"CORTEX");
        b[10..36].fill(0x00);
        b[36..45].copy_from_slice(b"1.08.1489");
        b[45] = 0x00;
        b[68..74].copy_from_slice(b"M1-GBL");
        b[74..100].fill(0x00);
        b[100..108].copy_from_slice(b"A300000B");
        b[108] = 0x00;
        b[132] = 2;
        b[133] = 2;
        let d = ProductInformation::from_bytes(&b).unwrap();
        assert_eq!((d.nmea2000_version, d.product_code), (Some(2100), Some(31173)));
        assert_eq!((d.model_id.as_str(), d.software_version.as_str(), d.model_version.as_str(), d.serial_code.as_str()),
                   ("CORTEX", "1.08.1489", "M1-GBL", "A300000B"));
        assert_eq!((d.certification_level, d.load_equivalency), (Some(2), Some(2)));
    }
    #[test] fn product_info_too_short() { assert!(ProductInformation::from_bytes(&[0u8; 133]).is_none()); }
}
