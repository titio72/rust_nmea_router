use std::fmt;

use super::sentinel::Available;

#[derive(Debug, Clone)]
pub struct Humidity {
    #[allow(dead_code)]
    pub pgn: u32,
    #[allow(dead_code)]
    sid: u8,
    pub instance: u8,
    pub source: u8,
    pub actual_humidity: f64,         // percent (0-100%)
    pub set_humidity: Option<f64>,    // percent (0-100%)
}

impl Humidity {
    // Constructor
    // instance: sensor instance
    // source: sensor source
    // actual_humidity: percent (0-100%)
    // set_humidity: percent (0-100%) (optional)
    pub fn new(instance: u8, source: u8, actual_humidity: f64, set_humidity: Option<f64>) -> Self {
        Self {
            pgn: 130313,
            sid: 0,
            instance,
            source,
            actual_humidity,
            set_humidity,
        }
    }

    pub fn get_source_description(&self) -> &'static str {
        match self.source {
            0 => "Inside",
            1 => "Outside",
            2 => "User",
            _ => "Unknown",
        }
    }

    /// Returns `None` when the actual humidity is not available.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 6 {
            return None;
        }

        // Both humidities are signed 16-bit, 0.004 % per bit: "not available" is 0x7FFF.
        let actual = i16::from_le_bytes([data[3], data[4]]).available()? as f64 * 0.004;

        let set_hum = if data.len() >= 7 {
            i16::from_le_bytes([data[5], data[6]]).available().map(|r| r as f64 * 0.004)
        } else {
            None
        };

        Some(Self {
            pgn: 130313,
            sid: data[0],
            instance: data[1],
            source: data[2],
            actual_humidity: actual,
            set_humidity: set_hum,
        })
    }
}

impl fmt::Display for Humidity {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "      Humidity: {:.1}% (Source: {}, Instance: {})",
            self.actual_humidity,
            self.source,
            self.instance
        )?;
        if let Some(set) = self.set_humidity {
            write!(f, " | Set: {:.1}%", set)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_humidity_from_bytes() {
        // canboat sample: outside 60.116 %, set humidity not available (0x7FFF)
        let data = [0x17, 0x00, 0x01, 0xB5, 0x3A, 0xFF, 0x7F, 0xFF];
        let hum = Humidity::from_bytes(&data).unwrap();
        assert!((hum.actual_humidity - 60.116).abs() < 1e-9);
        assert_eq!(hum.set_humidity, None);
    }

    #[test]
    fn test_humidity_not_available() {
        let data = [0x17, 0x00, 0x01, 0xFF, 0x7F, 0xFF, 0x7F, 0xFF];
        assert!(Humidity::from_bytes(&data).is_none());
    }
}
