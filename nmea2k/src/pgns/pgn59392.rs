/// PGN 59392 ISO Acknowledgement (single frame, encode only), canboat isoAcknowledgement.
///
/// Layout, 8 bytes: control (0 ACK, 1 NAK, 2 access denied, 3 address busy), group function,
/// three reserved bytes (0xFF), acknowledged PGN as 24-bit little-endian.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IsoAck {
    pub control: u8,
    pub group_function: u8,
    pub pgn: u32,
}

impl IsoAck {
    /// A negative acknowledgement (control 1) with group function "not available" (0xFF).
    pub fn nak(pgn: u32) -> Self {
        Self { control: 1, group_function: 0xFF, pgn }
    }

    pub fn to_bytes(&self) -> [u8; 8] {
        let p = self.pgn.to_le_bytes();
        [self.control, self.group_function, 0xFF, 0xFF, 0xFF, p[0], p[1], p[2]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn nak_layout() { assert_eq!(IsoAck::nak(130306).to_bytes(), [1, 0xFF, 0xFF, 0xFF, 0xFF, 0x02, 0xFD, 0x01]); }
    // canboat database/pgns/059392-isoAcknowledgement.yaml sample 3: NAK of PGN 126998
    #[test] fn nak_matches_canboat_sample() { assert_eq!(IsoAck::nak(126998).to_bytes(), [0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0x16, 0xF0, 0x01]); }
}
