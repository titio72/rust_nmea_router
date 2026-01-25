use std::fmt;

#[derive(Debug, Clone)]
pub struct EngineRapidUpdate {
    #[allow(dead_code)]
    pub pgn: u32,
    pub engine_instance: u8,
    pub engine_speed: Option<f64>,  // RPM
    pub engine_boost_pressure: Option<f64>,  // Pa
    pub engine_tilt_trim: Option<i8>,  // %
}

impl EngineRapidUpdate {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }

        let engine_instance = data[0];
        
        // Engine speed in 0.25 RPM per bit
        let speed_raw = u16::from_le_bytes([data[1], data[2]]);
        let engine_speed = if speed_raw == 0xFFFF {
            None
        } else {
            Some(speed_raw as f64 * 0.25)
        };
        
        // Engine boost pressure in 100 Pa per bit
        let boost_raw = u16::from_le_bytes([data[3], data[4]]);
        let engine_boost_pressure = if boost_raw == 0xFFFF {
            None
        } else {
            Some(boost_raw as f64 * 100.0)
        };
        
        // Engine tilt/trim in 1% per bit
        let tilt_trim = data[5] as i8;
        let engine_tilt_trim = if tilt_trim == -128 {
            None
        } else {
            Some(tilt_trim)
        };

        Some(EngineRapidUpdate {
            pgn: 127488,
            engine_instance,
            engine_speed,
            engine_boost_pressure,
            engine_tilt_trim,
        })
    }
    
    /// Check if engine is running based on RPM
    /// Considers engine running if RPM > 0
    pub fn is_engine_running(&self) -> bool {
        self.engine_speed.map(|rpm| rpm > 0.0).unwrap_or(false)
    }
}

impl fmt::Display for EngineRapidUpdate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Engine #{}: ", self.engine_instance)?;
        
        if let Some(rpm) = self.engine_speed {
            write!(f, "RPM: {:.0}", rpm)?;
        } else {
            write!(f, "RPM: N/A")?;
        }
        
        if let Some(boost) = self.engine_boost_pressure {
            write!(f, " | Boost: {:.0} Pa", boost)?;
        }
        
        if let Some(tilt) = self.engine_tilt_trim {
            write!(f, " | Tilt/Trim: {}%", tilt)?;
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_rapid_update_from_bytes() {
        // Engine instance 0, 1500 RPM (6000 * 0.25), boost 150000 Pa (1500 * 100), tilt 10%
        let data = [0x00, 0x70, 0x17, 0xDC, 0x05, 0x0A, 0xFF, 0xFF];
        let engine = EngineRapidUpdate::from_bytes(&data).unwrap();
        
        assert_eq!(engine.pgn, 127488);
        assert_eq!(engine.engine_instance, 0);
        assert_eq!(engine.engine_speed, Some(6000.0 * 0.25));
        assert_eq!(engine.engine_boost_pressure, Some(1500.0 * 100.0));
        assert_eq!(engine.engine_tilt_trim, Some(10));
    }

    #[test]
    fn test_engine_rapid_update_invalid_values() {
        // All invalid values (0xFF markers)
        let data = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x80, 0xFF, 0xFF];
        let engine = EngineRapidUpdate::from_bytes(&data).unwrap();
        
        assert_eq!(engine.engine_speed, None);
        assert_eq!(engine.engine_boost_pressure, None);
        assert_eq!(engine.engine_tilt_trim, None);
    }

    #[test]
    fn test_engine_running_detection() {
        // Engine running at 800 RPM
        let data = [0x00, 0x80, 0x0C, 0x00, 0x00, 0x00, 0xFF, 0xFF];
        let engine = EngineRapidUpdate::from_bytes(&data).unwrap();
        assert!(engine.is_engine_running());
        
        // Engine stopped (0 RPM)
        let data = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF];
        let engine = EngineRapidUpdate::from_bytes(&data).unwrap();
        assert!(!engine.is_engine_running());
        
        // Engine RPM not available
        let data = [0x00, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0xFF, 0xFF];
        let engine = EngineRapidUpdate::from_bytes(&data).unwrap();
        assert!(!engine.is_engine_running());
    }

    #[test]
    fn test_engine_rapid_update_short_data() {
        let data = [0x00, 0x01];
        assert!(EngineRapidUpdate::from_bytes(&data).is_none());
    }
}
