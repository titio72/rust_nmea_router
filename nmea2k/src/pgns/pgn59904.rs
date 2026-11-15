use std::fmt;

/// PGN 59904 ISO Request: asks a node to transmit the requested PGN.
/// Layout (canboat isoRequest): 3 bytes, PGN as 24-bit little-endian. Trailing bytes are ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsoRequest {
    pub requested_pgn: u32,
}

impl IsoRequest {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 3 {
            return None;
        }
        Some(Self {
            requested_pgn: u32::from_le_bytes([data[0], data[1], data[2], 0]),
        })
    }

    pub fn to_bytes(&self) -> [u8; 3] {
        let b = self.requested_pgn.to_le_bytes();
        [b[0], b[1], b[2]]
    }
}

impl fmt::Display for IsoRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ISO Request: PGN {}", self.requested_pgn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip() {
        assert_eq!(IsoRequest { requested_pgn: 126996 }.to_bytes(), [0x14, 0xF0, 0x01]);
        assert_eq!(IsoRequest::from_bytes(&[0x00, 0xEE, 0x00]).unwrap().requested_pgn, 60928);
        assert!(IsoRequest::from_bytes(&[0x00, 0xEE]).is_none());
    }

    // canboat database/pgns/059904-isoRequest.yaml: second sample carries trailing 0xFF padding
    #[test]
    fn request_with_padding_ignores_extra_bytes() {
        let data = [0x00, 0xEE, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        assert_eq!(IsoRequest::from_bytes(&data).unwrap().requested_pgn, 60928);
    }

    // canboat sample [14,f0,01]
    #[test]
    fn request_canboat_sample() {
        assert_eq!(IsoRequest::from_bytes(&[0x14, 0xF0, 0x01]).unwrap().requested_pgn, 126996);
    }

    #[test]
    fn empty_request_rejected() {
        assert!(IsoRequest::from_bytes(&[]).is_none());
    }
}
