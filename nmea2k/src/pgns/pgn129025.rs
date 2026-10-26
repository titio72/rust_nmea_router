use std::fmt;

use super::sentinel::{latitude, longitude, Available};

#[derive(Debug, Clone)]
pub struct PositionRapidUpdate {
    #[allow(dead_code)]
    pub pgn: u32,
    pub latitude: f64,  // degrees
    pub longitude: f64, // degrees
}

impl PositionRapidUpdate {
    pub fn new(latitude: f64, longitude: f64) -> Self {
        Self {
            pgn: 129025,
            latitude,
            longitude,
        }
    }

    /// Returns `None` unless both latitude and longitude are available: half a position is none.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        let lat_raw = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let lon_raw = i32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        Some(Self {
            pgn: 129025,
            latitude: latitude(lat_raw.available().map(|r| r as f64 * 1e-7))?,
            longitude: longitude(lon_raw.available().map(|r| r as f64 * 1e-7))?,
        })
    }
}

impl fmt::Display for PositionRapidUpdate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "      Position: {:.6}°, {:.6}°", self.latitude, self.longitude)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(lat_raw: i32, lon_raw: i32) -> [u8; 8] {
        let lat = lat_raw.to_le_bytes();
        let lon = lon_raw.to_le_bytes();
        [lat[0], lat[1], lat[2], lat[3], lon[0], lon[1], lon[2], lon[3]]
    }

    #[test]
    fn test_position_rapid_update_from_bytes() {
        let pos = PositionRapidUpdate::from_bytes(&frame(430_530_014, -100_111_665)).unwrap();
        assert!((pos.latitude - 43.0530014).abs() < 1e-9);
        assert!((pos.longitude + 10.0111665).abs() < 1e-9);
    }

    #[test]
    fn test_position_rapid_update_not_available() {
        assert!(PositionRapidUpdate::from_bytes(&frame(i32::MAX, i32::MAX)).is_none());
        assert!(PositionRapidUpdate::from_bytes(&frame(430_530_014, i32::MAX)).is_none());
        assert!(PositionRapidUpdate::from_bytes(&frame(i32::MAX, 100_111_665)).is_none());
    }

    #[test]
    fn test_position_rapid_update_out_of_range() {
        // 91° latitude is not a position
        assert!(PositionRapidUpdate::from_bytes(&frame(910_000_000, 100_111_665)).is_none());
    }
}
