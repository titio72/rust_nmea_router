use std::fmt;

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

#[derive(Debug, Clone)]
pub enum HeadingReference {
    True,
    Magnetic,
    Error,
    Null,
}

impl VesselHeading {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        Some(Self {
            pgn: 127250,
            sid: data[0],
            heading: u16::from_le_bytes([data[1], data[2]]) as f64 * 0.0001,
            deviation: Some(i16::from_le_bytes([data[3], data[4]]) as f64 * 0.0001),
            variation: Some(i16::from_le_bytes([data[5], data[6]]) as f64 * 0.0001),
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
        write!(f, "      Heading: {:.2}° ({:?})", self.heading.to_degrees(), self.reference)?;
        if let Some(dev) = self.deviation {
            write!(f, " | Deviation: {:.2}°", dev.to_degrees())?;
        }
        if let Some(var) = self.variation {
            write!(f, " | Variation: {:.2}°", var.to_degrees())?;
        }
        Ok(())
    }
}
