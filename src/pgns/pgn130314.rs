use std::fmt;

#[derive(Debug, Clone)]
pub struct ActualPressure {
    pub pgn: u32,
    sid: u8,
    pub instance: u8,
    pub source: u8,
    pub pressure: f64, // Pascals
}

impl ActualPressure {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 6 {
            return None;
        }
        
        // Parse pressure (bytes 3-6) - 32-bit value
        let pressure_raw = if data.len() >= 7 {
            u32::from_le_bytes([
                data[3],
                data[4],
                data[5],
                data.get(6).copied().unwrap_or(0),
            ])
        } else {
            u32::from_le_bytes([data[3], data[4], data[5], 0])
        };
        
        // Resolution is 0.1 Pa, so divide by 10 to get Pascals
        // But actually the resolution is 1 Pa based on standard
        let pressure = pressure_raw as f64;
        
        Some(Self {
            pgn: 130314,
            sid: data[0],
            instance: data[1],
            source: data[2],
            pressure,
        })
    }
}

impl fmt::Display for ActualPressure {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "      Pressure: {:.0} Pa ({:.2} hPa) (Source: {}, Instance: {})",
            self.pressure,
            self.pressure / 100.0, // Convert to hectopascals (mbar)
            self.source,
            self.instance
        )
    }
}
