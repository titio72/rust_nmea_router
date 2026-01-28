use std::fmt;

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

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 6 {
            return None;
        }
        
        // Parse actual humidity (bytes 3-4)
        let actual = u16::from_le_bytes([data[3], data[4]]) as f64 * 0.004;
        
        // Parse set humidity if available (bytes 5-6)
        let set_hum = if data.len() >= 7 {
            let val = u16::from_le_bytes([data[5], data.get(6).copied().unwrap_or(0xFF)]);
            if val == 0xFFFF {
                None
            } else {
                Some(val as f64 * 0.004)
            }
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
