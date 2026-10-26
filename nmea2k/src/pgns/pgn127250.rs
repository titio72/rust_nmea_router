use std::fmt;

use super::sentinel::Available;

#[derive(Debug, Clone)]
pub struct VesselHeading {
    #[allow(dead_code)]
    pub pgn: u32,
    #[allow(dead_code)]
    sid: u8,
    pub heading: f64, // radians
    pub deviation: Option<f64>,
    pub variation: Option<f64>,
    pub reference: HeadingReference,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadingReference {
    True,
    Magnetic,
    Error,
    Null,
}

impl fmt::Display for HeadingReference {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", match self {
            HeadingReference::True => "True",
            HeadingReference::Magnetic => "Magnetic",
            HeadingReference::Error => "Error",
            HeadingReference::Null => "Null",
        })
    }
}

impl VesselHeading {

    pub fn new(heading: f64, reference: HeadingReference) -> Self {
        Self {
            pgn: 127250,
            sid: 0,
            heading,
            deviation: None,
            variation: None,
            reference,
        }
    }

    /// Returns `None` when the heading is not available. Some devices send this PGN with only
    /// variation filled in; nothing here consumes variation on its own, so such messages are dropped.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        Some(Self {
            pgn: 127250,
            sid: data[0],
            heading: u16::from_le_bytes([data[1], data[2]]).available()? as f64 * 0.0001,
            deviation: i16::from_le_bytes([data[3], data[4]]).available().map(|r| r as f64 * 0.0001),
            variation: i16::from_le_bytes([data[5], data[6]]).available().map(|r| r as f64 * 0.0001),
            reference: match data[7] & 0x03 {
                0 => HeadingReference::True,
                1 => HeadingReference::Magnetic,
                2 => HeadingReference::Error,
                _ => HeadingReference::Null,
            },
        })
    }
}

impl fmt::Display for VesselHeading {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "      Heading: {:.2}° ({})", self.heading.to_degrees(), self.reference)?;
        if let Some(dev) = self.deviation {
            write!(f, " | Deviation: {:.2}°", dev.to_degrees())?;
        }
        if let Some(var) = self.variation {
            write!(f, " | Variation: {:.2}°", var.to_degrees())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vessel_heading_from_bytes() {
        // 1.2345 rad magnetic, deviation not available, variation +0.0349 rad
        let data = [0x00, 0x39, 0x30, 0xFF, 0x7F, 0x5D, 0x01, 0xFD];
        let hdg = VesselHeading::from_bytes(&data).unwrap();
        assert!((hdg.heading - 1.2345).abs() < 1e-9);
        assert_eq!(hdg.deviation, None);
        assert!((hdg.variation.unwrap() - 0.0349).abs() < 1e-9);
        assert_eq!(hdg.reference, HeadingReference::Magnetic);
    }

    #[test]
    fn test_vessel_heading_not_available() {
        let data = [0x00, 0xFF, 0xFF, 0xFF, 0x7F, 0x5D, 0x01, 0xFD];
        assert!(VesselHeading::from_bytes(&data).is_none());
    }
}
