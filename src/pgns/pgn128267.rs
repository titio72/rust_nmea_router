use std::fmt;

#[derive(Debug, Clone)]
pub struct WaterDepth {
    pub pgn: u32,
    sid: u8,
    pub depth: f64, // meters
    pub offset: f64, // meters
}

impl WaterDepth {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 7 {
            return None;
        }
        Some(Self {
            pgn: 128267,
            sid: data[0],
            depth: u32::from_le_bytes([data[1], data[2], data[3], data[4]]) as f64 * 0.01,
            offset: i16::from_le_bytes([data[5], data[6]]) as f64 * 0.001,
        })
    }
}

impl fmt::Display for WaterDepth {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "      Depth: {:.2} m | Offset: {:.3} m", self.depth, self.offset)
    }
}
