use std::fmt;

use super::sentinel::{fmt_opt, Available};

/// NMEA2000 PGN 130310 — Environmental Parameters (obsolete).
///
/// Superseded by PGNs 130312..130315, but still sent by older instruments. Each reading is
/// independent and commonly not available (e.g. a barometer that has no water sensor).
#[derive(Debug, Clone)]
pub struct EnvironmentalParameters {
    #[allow(dead_code)]
    pub pgn: u32,
    #[allow(dead_code)]
    sid: Option<u8>,
    pub water_temperature: Option<f64>,       // Kelvin
    pub outside_air_temperature: Option<f64>, // Kelvin
    pub atmospheric_pressure: Option<f64>,    // Pascals
}

impl EnvironmentalParameters {
    /// Returns `None` when none of the three readings is available.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        let water_temperature = u16::from_le_bytes([data[1], data[2]]).available().map(|r| r as f64 * 0.01);
        let outside_air_temperature = u16::from_le_bytes([data[3], data[4]]).available().map(|r| r as f64 * 0.01);
        let atmospheric_pressure = u16::from_le_bytes([data[5], data[6]]).available().map(|r| r as f64 * 100.0);
        if water_temperature.is_none() && outside_air_temperature.is_none() && atmospheric_pressure.is_none() {
            return None;
        }
        Some(Self {
            pgn: 130310,
            sid: data[0].available(),
            water_temperature,
            outside_air_temperature,
            atmospheric_pressure,
        })
    }
}

impl fmt::Display for EnvironmentalParameters {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "      Environment: Water {} K | Air {} K | Pressure {} Pa",
            fmt_opt(self.water_temperature, 2),
            fmt_opt(self.outside_air_temperature, 2),
            fmt_opt(self.atmospheric_pressure, 0)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_environmental_parameters_air_and_pressure() {
        // canboat database/pgns/130310-environmentalParametersObsolete.yaml
        let data = [0xFF, 0xFF, 0xFF, 0xC5, 0x72, 0xF6, 0x03, 0xFF];
        let env = EnvironmentalParameters::from_bytes(&data).unwrap();
        assert_eq!(env.sid, None);
        assert_eq!(env.water_temperature, None);
        assert!((env.outside_air_temperature.unwrap() - 293.81).abs() < 1e-9);
        assert!((env.atmospheric_pressure.unwrap() - 101400.0).abs() < 1e-9);
    }

    #[test]
    fn test_environmental_parameters_water_only() {
        // canboat database/pgns/130310-environmentalParametersObsolete.yaml
        let data = [0x04, 0x88, 0x71, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let env = EnvironmentalParameters::from_bytes(&data).unwrap();
        assert_eq!(env.sid, Some(4));
        assert!((env.water_temperature.unwrap() - 290.64).abs() < 1e-9);
        assert_eq!(env.outside_air_temperature, None);
        assert_eq!(env.atmospheric_pressure, None);
    }

    #[test]
    fn test_environmental_parameters_nothing_available() {
        // canboat database/pgns/130310-environmentalParametersObsolete.yaml
        let data = [0xFF; 8];
        assert!(EnvironmentalParameters::from_bytes(&data).is_none());
    }

    #[test]
    fn test_environmental_parameters_too_short() {
        let data = vec![0u8; 7];
        assert!(EnvironmentalParameters::from_bytes(&data).is_none());
    }
}
