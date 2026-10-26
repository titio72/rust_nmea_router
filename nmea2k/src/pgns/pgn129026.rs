use std::fmt;

use super::sentinel::Available;

#[derive(Debug, Clone)]
pub struct CogSogRapidUpdate {
    #[allow(dead_code)]
    pub pgn: u32,
    #[allow(dead_code)]
    sid: u8,
    pub cog_reference: bool, // true = True, false = Magnetic
    pub cog: Option<f64>, // radians; commonly not available when stationary
    pub sog: Option<f64>, // m/s
}

impl CogSogRapidUpdate {

    // Constructor
    // cog_reference: true = True, false = Magnetic
    // cog: radians
    // sog: m/s
    // 
    pub fn new(cog_reference: bool, cog: f64, sog: f64) -> Self {
        Self {
            pgn: 129026,
            sid: 0,
            cog_reference,
            cog: Some(cog),
            sog: Some(sog),
        }
    }

    /// Returns `None` when neither COG nor SOG is available.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        let cog = u16::from_le_bytes([data[2], data[3]]).available().map(|r| r as f64 * 0.0001);
        let sog = u16::from_le_bytes([data[4], data[5]]).available().map(|r| r as f64 * 0.01);
        if cog.is_none() && sog.is_none() {
            return None;
        }
        Some(Self {
            pgn: 129026,
            sid: data[0],
            cog_reference: (data[1] & 0x03) == 0,
            cog,
            sog,
        })
    }

    pub fn get_cog_reference_description(&self) -> &'static str {
        if self.cog_reference { "True North" } else { "Magnetic" }
    }

    pub fn sog_knots(&self) -> Option<f64> {
        self.sog.map(|s| s * 1.94384)
    }

    pub fn cog_degrees(&self) -> Option<f64> {
        self.cog.map(|c| c.to_degrees())
    }
}

impl fmt::Display for CogSogRapidUpdate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.cog_degrees() {
            Some(cog) => write!(f, "      COG: {:.2}°", cog)?,
            None => write!(f, "      COG: N/A")?,
        }
        write!(f, " ({}) | ", if self.cog_reference { "True" } else { "Mag" })?;
        match self.sog {
            Some(sog) => write!(f, "SOG: {:.2} m/s ({:.2} knots)", sog, sog * 1.94384),
            None => write!(f, "SOG: N/A"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cog_sog_from_bytes() {
        // COG 1.0 rad true, SOG 3.00 m/s
        let data = [0x00, 0xFC, 0x10, 0x27, 0x2C, 0x01, 0xFF, 0xFF];
        let cs = CogSogRapidUpdate::from_bytes(&data).unwrap();
        assert!(cs.cog_reference);
        assert!((cs.cog.unwrap() - 1.0).abs() < 1e-9);
        assert!((cs.sog.unwrap() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn test_cog_not_available_when_stationary() {
        // SOG 0, COG not available: the SOG reading must survive
        let data = [0x00, 0xFC, 0xFF, 0xFF, 0x00, 0x00, 0xFF, 0xFF];
        let cs = CogSogRapidUpdate::from_bytes(&data).unwrap();
        assert_eq!(cs.cog, None);
        assert_eq!(cs.sog, Some(0.0));
    }

    #[test]
    fn test_cog_sog_both_not_available() {
        let data = [0x00, 0xFC, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        assert!(CogSogRapidUpdate::from_bytes(&data).is_none());
    }
}
