/// Function code of PGN 126464 (canboat PGN_LIST_FUNCTION).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PgnListFunction {
    Transmit = 0,
    Receive = 1,
}

/// PGN 126464 PGN List (transmit and receive), fast packet, encode only.
/// Layout: function code byte, then each PGN as 24-bit little-endian.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgnList {
    pub function: PgnListFunction,
    pub pgns: Vec<u32>,
}

impl PgnList {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + 3 * self.pgns.len());
        out.push(self.function as u8);
        for pgn in &self.pgns {
            out.extend_from_slice(&pgn.to_le_bytes()[..3]);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn pgn_list_layout() {
        let l = PgnList { function: PgnListFunction::Transmit, pgns: vec![60928, 126996] };
        assert_eq!(l.to_bytes(), vec![0, 0x00, 0xEE, 0x00, 0x14, 0xF0, 0x01]);
    }
    #[test] fn pgn_list_receive_and_empty() {
        assert_eq!(PgnList { function: PgnListFunction::Receive, pgns: vec![] }.to_bytes(), vec![1]);
    }
}
