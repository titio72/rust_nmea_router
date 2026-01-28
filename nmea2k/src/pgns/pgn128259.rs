use std::fmt;

#[derive(Debug, Clone)]
pub struct SpeedWaterReferenced {
    #[allow(dead_code)]
    pub pgn: u32,
    #[allow(dead_code)]
    sid: u8,
    pub speed: f64, // m/s
}

impl SpeedWaterReferenced {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 3 {
            return None;
        }
        Some(Self {
            pgn: 128259,
            sid: data[0],
            speed: u16::from_le_bytes([data[1], data[2]]) as f64 * 0.01,
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
