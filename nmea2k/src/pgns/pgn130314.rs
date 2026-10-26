use std::fmt;

use super::sentinel::Available;

#[derive(Debug, Clone)]
pub struct ActualPressure {
    #[allow(dead_code)]
    pub pgn: u32,
    #[allow(dead_code)]
    sid: u8,
    pub instance: u8,
    pub source: u8,
    pub pressure: f64, // Pascals
}

impl ActualPressure {
    // Constructor
    // instance: sensor instance
    // source: sensor source
    // pressure: Pascals
    pub fn new(instance: u8, source: u8, pressure: f64) -> Self {
        Self {
            pgn: 130314,
            sid: 0,
            instance,
            source,
            pressure,
        }
    }

    pub fn get_source_description(&self) -> &'static str {
        match self.source {
            0 => "Atmospheric",
            1 => "Water",
            2 => "Steam",
            3 => "Compressed air",
            4 => "Hydraulic",
            _ => "Unknown",
        }
    }

    /// Returns `None` when the pressure is not available.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 6 {
            return None;
        }

        // Pressure (bytes 3-6): signed 32-bit, 0.1 Pa per bit
        let pressure_raw = i32::from_le_bytes([
            data[3],
            data[4],
            data[5],
            data.get(6).copied().unwrap_or(0),
        ]);
        let pressure = pressure_raw.available()? as f64 / 10.0;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actual_pressure_from_bytes() {
        // canboat sample: atmospheric 101410.0 Pa
        let data = [0xFF, 0x00, 0x00, 0x54, 0x79, 0x0F, 0x00, 0xFF];
        let pres = ActualPressure::from_bytes(&data).unwrap();
        assert!((pres.pressure - 101410.0).abs() < 1e-9);
    }

    #[test]
    fn test_actual_pressure_not_available() {
        let data = [0xFF, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0x7F, 0xFF];
        assert!(ActualPressure::from_bytes(&data).is_none());
    }
}
