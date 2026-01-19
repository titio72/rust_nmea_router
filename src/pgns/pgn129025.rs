use std::fmt;

#[derive(Debug, Clone)]
pub struct PositionRapidUpdate {
    pub pgn: u32,
    pub latitude: f64,  // degrees
    pub longitude: f64, // degrees
}

impl PositionRapidUpdate {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        Some(Self {
            pgn: 129025,
            latitude: i32::from_le_bytes([data[0], data[1], data[2], data[3]]) as f64 * 1e-7,
            longitude: i32::from_le_bytes([data[4], data[5], data[6], data[7]]) as f64 * 1e-7,
        })
    }
}

impl fmt::Display for PositionRapidUpdate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "      Position: {:.6}° N, {:.6}° E", self.latitude, self.longitude)
    }
}
