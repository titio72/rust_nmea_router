use std::fmt;

use super::sentinel::Available;

#[derive(Debug, Clone)]
pub struct SpeedWaterReferenced {
    #[allow(dead_code)]
    pub pgn: u32,
    #[allow(dead_code)]
    sid: u8,
    pub speed: f64, // m/s
}

impl SpeedWaterReferenced {
    /// Returns `None` when the water speed is not available: it is the only field decoded.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 3 {
            return None;
        }
        Some(Self {
            pgn: 128259,
            sid: data[0],
            speed: u16::from_le_bytes([data[1], data[2]]).available()? as f64 * 0.01,
        })
    }

    pub fn speed_knots(&self) -> f64 {
        self.speed * 1.94384
    }
}

impl fmt::Display for SpeedWaterReferenced {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "      Speed: {:.2} m/s ({:.2} knots)", self.speed, self.speed * 1.94384)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_speed_water_referenced_from_bytes() {
        // 2.50 m/s
        let data = [0x00, 0xFA, 0x00, 0xFF, 0xFF, 0x00, 0xFF, 0xFF];
        let stw = SpeedWaterReferenced::from_bytes(&data).unwrap();
        assert!((stw.speed - 2.5).abs() < 1e-9);
    }

    #[test]
    fn test_speed_water_referenced_not_available() {
        // Paddle wheel not reporting; ground speed present but not decoded
        let data = [0x00, 0xFF, 0xFF, 0x10, 0x00, 0x00, 0xFF, 0xFF];
        assert!(SpeedWaterReferenced::from_bytes(&data).is_none());
    }
}
