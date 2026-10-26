use std::fmt;

use super::sentinel::Available;

#[derive(Debug, Clone)]
pub struct RateOfTurn {
    #[allow(dead_code)]
    pub pgn: u32,
    #[allow(dead_code)]
    sid: u8,
    pub rate: f64, // radians per second
}

impl RateOfTurn {
    /// Returns `None` when the rate is not available: the message carries nothing else.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 5 {
            return None;
        }
        Some(Self {
            pgn: 127251,
            sid: data[0],
            rate: i32::from_le_bytes([data[1], data[2], data[3], data[4]]).available()? as f64 * 1e-6,
        })
    }
}

impl fmt::Display for RateOfTurn {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "      Rate of Turn: {:.4}°/s", self.rate.to_degrees())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_of_turn_from_bytes() {
        // -0.01 rad/s = -10000 µrad/s
        let raw = (-10000i32).to_le_bytes();
        let data = [0x00, raw[0], raw[1], raw[2], raw[3], 0xFF, 0xFF, 0xFF];
        let rot = RateOfTurn::from_bytes(&data).unwrap();
        assert!((rot.rate + 0.01).abs() < 1e-9);
    }

    #[test]
    fn test_rate_of_turn_not_available() {
        let data = [0x00, 0xFF, 0xFF, 0xFF, 0x7F, 0xFF, 0xFF, 0xFF];
        assert!(RateOfTurn::from_bytes(&data).is_none());
    }
}
