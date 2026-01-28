use std::fmt;

#[derive(Debug, Clone)]
pub struct WindData {
    #[allow(dead_code)]
    pub pgn: u32,
    #[allow(dead_code)]
    sid: u8,
    pub speed: f64, // m/s
    pub angle: f64, // radians
    pub reference: WindReference,
}

#[derive(Debug, Clone)]
pub enum WindReference {
    TrueGroundNorth,
    Magnetic,
    Apparent,
    TrueBoat,
    TrueWater,
}

impl WindData {

    pub fn new_apparent(speed: f64, angle: f64) -> Self {
        Self {
            pgn: 130306,
            sid: 0,
            speed,
            angle,
            reference: WindReference::Apparent,
        }
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 6 {
            return None;
        }
        Some(Self {
            pgn: 130306,
            sid: data[0],
            speed: u16::from_le_bytes([data[1], data[2]]) as f64 * 0.01,
            angle: u16::from_le_bytes([data[3], data[4]]) as f64 * 0.0001,
            reference: match data[5] & 0x07 {
                0 => WindReference::TrueGroundNorth,
                1 => WindReference::Magnetic,
                2 => WindReference::Apparent,
                3 => WindReference::TrueBoat,
                4 => WindReference::TrueWater,
                _ => WindReference::Apparent,
            },
        })
    }

    pub fn speed_knots(&self) -> f64 {
        self.speed * 1.94384
    }
}

impl fmt::Display for WindData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "      Wind Speed: {:.2} m/s ({:.2} knots) | Angle: {:.2}° | Ref: {:?}",
            self.speed,
            self.speed * 1.94384,
            self.angle.to_degrees(),
            self.reference
        )
    }
}
