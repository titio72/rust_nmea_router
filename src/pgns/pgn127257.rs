#[derive(Debug, Clone)]
pub struct Attitude {
    sid: u8,
    pub yaw: Option<f64>,   // radians
    pub pitch: Option<f64>, // radians
    pub roll: Option<f64>,  // radians
}

impl Attitude {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() >= 7 {
            let sid = data[0];
            
            // Yaw (bytes 1-2): int16, 0.0001 radians
            let yaw_raw = i16::from_le_bytes([data[1], data[2]]);
            let yaw = if yaw_raw == i16::MAX {
                None
            } else {
                Some(yaw_raw as f64 * 0.0001)
            };
            
            // Pitch (bytes 3-4): int16, 0.0001 radians
            let pitch_raw = i16::from_le_bytes([data[3], data[4]]);
            let pitch = if pitch_raw == i16::MAX {
                None
            } else {
                Some(pitch_raw as f64 * 0.0001)
            };
            
            // Roll (bytes 5-6): int16, 0.0001 radians
            let roll_raw = i16::from_le_bytes([data[5], data[6]]);
            let roll = if roll_raw == i16::MAX {
                None
            } else {
                Some(roll_raw as f64 * 0.0001)
            };
            
            return Some(Attitude {
                sid,
                yaw,
                pitch,
                roll,
            });
        }
        None
    }
    
    pub fn roll_degrees(&self) -> Option<f64> {
        self.roll.map(|r| r.to_degrees())
    }
}

impl std::fmt::Display for Attitude {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "      Yaw: ")?;
        if let Some(yaw) = self.yaw {
            write!(f, "{:.2}° ({:.4} rad)", yaw.to_degrees(), yaw)?;
        } else {
            write!(f, "N/A")?;
        }
        
        write!(f, ", Pitch: ")?;
        if let Some(pitch) = self.pitch {
            write!(f, "{:.2}° ({:.4} rad)", pitch.to_degrees(), pitch)?;
        } else {
            write!(f, "N/A")?;
        }
        
        write!(f, ", Roll: ")?;
        if let Some(roll) = self.roll {
            write!(f, "{:.2}° ({:.4} rad)", roll.to_degrees(), roll)?;
        } else {
            write!(f, "N/A")?;
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attitude_valid_data() {
        // Test data with valid roll, pitch, yaw values
        // Yaw = 0.1 rad, Pitch = 0.2 rad, Roll = 0.3 rad
        let data = vec![
            0x01, // SID
            0xE8, 0x03, // Yaw = 1000 * 0.0001 = 0.1 rad
            0xD0, 0x07, // Pitch = 2000 * 0.0001 = 0.2 rad
            0xB8, 0x0B, // Roll = 3000 * 0.0001 = 0.3 rad
        ];
        
        let attitude = Attitude::from_bytes(&data).unwrap();
        assert_eq!(attitude.yaw.unwrap(), 0.1);
        assert_eq!(attitude.pitch.unwrap(), 0.2);
        assert_eq!(attitude.roll.unwrap(), 0.3);
    }

    #[test]
    fn test_attitude_with_invalid_values() {
        // Test data with invalid (0x7FFF) values
        let data = vec![
            0x01, // SID
            0xFF, 0x7F, // Yaw = 0x7FFF (invalid)
            0xFF, 0x7F, // Pitch = 0x7FFF (invalid)
            0xFF, 0x7F, // Roll = 0x7FFF (invalid)
        ];
        
        let attitude = Attitude::from_bytes(&data).unwrap();
        assert!(attitude.yaw.is_none());
        assert!(attitude.pitch.is_none());
        assert!(attitude.roll.is_none());
    }

    #[test]
    fn test_attitude_roll_degrees() {
        let data = vec![
            0x01,
            0x00, 0x00, // Yaw = 0
            0x00, 0x00, // Pitch = 0
            0x9A, 0x27, // Roll = 10138 * 0.0001 = 1.0138 rad ≈ 58.09°
        ];
        
        let attitude = Attitude::from_bytes(&data).unwrap();
        let roll_deg = attitude.roll_degrees().unwrap();
        assert!((roll_deg - 58.09).abs() < 0.1);
    }

    #[test]
    fn test_attitude_negative_roll() {
        // Test negative roll (port side)
        let data = vec![
            0x01,
            0x00, 0x00,
            0x00, 0x00,
            0x18, 0xFC, // Roll = -1000 * 0.0001 = -0.1 rad ≈ -5.73°
        ];
        
        let attitude = Attitude::from_bytes(&data).unwrap();
        let roll_deg = attitude.roll_degrees().unwrap();
        assert!((roll_deg + 5.73).abs() < 0.1);
    }

    #[test]
    fn test_attitude_insufficient_data() {
        let data = vec![0x01, 0x00]; // Only 2 bytes
        let attitude = Attitude::from_bytes(&data);
        assert!(attitude.is_none());
    }

    #[test]
    fn test_attitude_mixed_valid_invalid() {
        // Valid roll, invalid pitch and yaw
        let data = vec![
            0x01,
            0xFF, 0x7F, // Yaw invalid
            0xFF, 0x7F, // Pitch invalid
            0xE8, 0x03, // Roll = 1000 * 0.0001 = 0.1 rad
        ];
        
        let attitude = Attitude::from_bytes(&data).unwrap();
        assert!(attitude.yaw.is_none());
        assert!(attitude.pitch.is_none());
        assert!(attitude.roll.is_some());
        assert_eq!(attitude.roll.unwrap(), 0.1);
    }

    #[test]
    fn test_attitude_zero_values() {
        let data = vec![
            0x01,
            0x00, 0x00, // Yaw = 0
            0x00, 0x00, // Pitch = 0
            0x00, 0x00, // Roll = 0
        ];
        
        let attitude = Attitude::from_bytes(&data).unwrap();
        assert_eq!(attitude.yaw.unwrap(), 0.0);
        assert_eq!(attitude.pitch.unwrap(), 0.0);
        assert_eq!(attitude.roll.unwrap(), 0.0);
    }
}
