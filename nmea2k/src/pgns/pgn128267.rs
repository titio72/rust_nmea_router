use std::fmt;

use super::sentinel::Available;

#[derive(Debug, Clone)]
pub struct WaterDepth {
    #[allow(dead_code)]
    pub pgn: u32,
    #[allow(dead_code)]
    sid: u8,
    pub depth: f64, // meters
    pub offset: Option<f64>, // meters
}

impl WaterDepth {
    /// Returns `None` when the depth is not available (e.g. bottom lost in deep water).
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 7 {
            return None;
        }
        Some(Self {
            pgn: 128267,
            sid: data[0],
            depth: u32::from_le_bytes([data[1], data[2], data[3], data[4]]).available()? as f64 * 0.01,
            offset: i16::from_le_bytes([data[5], data[6]]).available().map(|r| r as f64 * 0.001),
        })
    }
}

impl fmt::Display for WaterDepth {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "      Depth: {:.2} m", self.depth)?;
        if let Some(offset) = self.offset {
            write!(f, " | Offset: {:.3} m", offset)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_water_depth_from_bytes() {
        // 12.34 m, offset -0.5 m
        let offset = (-500i16).to_le_bytes();
        let data = [0x00, 0xD2, 0x04, 0x00, 0x00, offset[0], offset[1], 0xFF];
        let depth = WaterDepth::from_bytes(&data).unwrap();
        assert!((depth.depth - 12.34).abs() < 1e-9);
        assert!((depth.offset.unwrap() + 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_water_depth_offset_not_available() {
        let data = [0x00, 0xD2, 0x04, 0x00, 0x00, 0xFF, 0x7F, 0xFF];
        let depth = WaterDepth::from_bytes(&data).unwrap();
        assert!((depth.depth - 12.34).abs() < 1e-9);
        assert_eq!(depth.offset, None);
    }

    #[test]
    fn test_water_depth_not_available() {
        let data = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0xFF];
        assert!(WaterDepth::from_bytes(&data).is_none());
    }
}
