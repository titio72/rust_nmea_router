/// PGN 126993 Heartbeat (single frame, encode only), canboat heartbeat.
///
/// Layout, 8 bytes:
/// - bytes 0-1: data transmit offset (the heartbeat interval), u16 LE, 1 ms resolution
///   (valid range 0..65532)
/// - byte 2: sequence counter
/// - byte 3, bits 0-1: controller 1 state = 0 (Error Active)
/// - byte 3, bits 2-3: controller 2 state = 3 (not available)
/// - byte 3, bits 4-5: equipment status = 0 (Operational)
/// - byte 3, bits 6-7 and bytes 4-7: reserved, all ones
///
/// Byte 3 is therefore 0xCC, as in the canboat samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Heartbeat {
    pub interval_ms: u32,
    pub sequence: u8,
}

impl Heartbeat {
    /// The interval is clamped to the largest valid raw value (65532 ms).
    pub fn to_bytes(&self) -> [u8; 8] {
        let interval = self.interval_ms.min(u16::MAX as u32 - 3) as u16;
        let i = interval.to_le_bytes();
        [i[0], i[1], self.sequence, 0xCC, 0xFF, 0xFF, 0xFF, 0xFF]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // canboat database/pgns/126993-heartbeat.yaml sample 2: offset 60.000 s, sequence 155
    #[test] fn heartbeat_matches_canboat_sample() {
        assert_eq!(Heartbeat { interval_ms: 60_000, sequence: 155 }.to_bytes(), [0x60, 0xEA, 0x9B, 0xCC, 0xFF, 0xFF, 0xFF, 0xFF]);
    }
    #[test] fn heartbeat_interval_is_clamped_to_valid_range() {
        let b = Heartbeat { interval_ms: 1_000_000, sequence: 0 }.to_bytes();
        assert_eq!(u16::from_le_bytes([b[0], b[1]]), 65532);
    }
}
