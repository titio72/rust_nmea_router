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
