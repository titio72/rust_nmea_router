use std::fmt;

#[derive(Debug, Clone)]
pub struct Temperature {
    #[allow(dead_code)]
    pub pgn: u32,
    #[allow(dead_code)]
    sid: u8,
    pub instance: u8,
    pub source: u8,
    pub temperature: f64, // Kelvin
    pub set_temperature: Option<f64>,
}

impl Temperature {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 6 {
            return None;
        }
        let set_temp = if data.len() >= 8 {
            Some(u16::from_le_bytes([data[6], data[7]]) as f64 * 0.01)
        } else {
            None
        };
        Some(Self {
            pgn: 130312,
            sid: data[0],
            instance: data[1],
            source: data[2],
            temperature: u16::from_le_bytes([data[3], data[4]]) as f64 * 0.01,
            set_temperature: set_temp,
        })
    }
}

impl fmt::Display for Temperature {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "      Temperature: {:.2}°C (Source: {}, Instance: {})",
            self.temperature - 273.15,
            self.source,
            self.instance
        )?;
        if let Some(set) = self.set_temperature {
            write!(f, " | Set: {:.2}°C", set - 273.15)?;
        }
        Ok(())
    }
}
