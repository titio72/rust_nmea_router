use std::fmt;

#[derive(Debug, Clone)]
pub struct RateOfTurn {
    pub pgn: u32,
    sid: u8,
    pub rate: f64, // radians per second
}

impl RateOfTurn {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 5 {
            return None;
        }
        Some(Self {
            pgn: 127251,
            sid: data[0],
            rate: i32::from_le_bytes([data[1], data[2], data[3], data[4]]) as f64 * 1e-6,
        })
    }
}

impl fmt::Display for RateOfTurn {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "      Rate of Turn: {:.4}°/s", self.rate.to_degrees())
    }
}
