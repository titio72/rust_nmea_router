use std::fmt;

use super::sentinel::Available;

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

impl fmt::Display for WindReference {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", match self {
            WindReference::TrueGroundNorth => "True (ground, North ref.)",
            WindReference::Magnetic => "Magnetic",
            WindReference::Apparent => "Apparent",
            WindReference::TrueBoat => "True (boat referenced)",
            WindReference::TrueWater => "True (water referenced)",
        })
    }
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

    /// Returns `None` unless both speed and angle are available: every consumer needs the pair.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 6 {
            return None;
        }
        Some(Self {
            pgn: 130306,
            sid: data[0],
            speed: u16::from_le_bytes([data[1], data[2]]).available()? as f64 * 0.01,
            angle: u16::from_le_bytes([data[3], data[4]]).available()? as f64 * 0.0001,
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
            "      Wind Speed: {:.2} m/s ({:.2} knots) | Angle: {:.2}° | Ref: {}",
            self.speed,
            self.speed * 1.94384,
            self.angle.to_degrees(),
            self.reference
        )    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wind_data_from_bytes() {
        // canboat sample: 4.36 m/s at 2.27 rad, true (boat referenced)
        let data = [0x00, 0xB4, 0x01, 0xAC, 0x58, 0x03, 0xFA, 0xFF];
        let wind = WindData::from_bytes(&data).unwrap();
        assert!((wind.speed - 4.36).abs() < 1e-9);
        assert!((wind.angle - 2.27).abs() < 1e-4);
        assert!(matches!(wind.reference, WindReference::TrueBoat));
    }

    #[test]
    fn test_wind_data_not_available() {
        // Speed not available
        let data = [0x00, 0xFF, 0xFF, 0xAC, 0x58, 0x02, 0xFF, 0xFF];
        assert!(WindData::from_bytes(&data).is_none());
        // Angle not available
        let data = [0x00, 0xB4, 0x01, 0xFF, 0xFF, 0x02, 0xFF, 0xFF];
        assert!(WindData::from_bytes(&data).is_none());
    }
}