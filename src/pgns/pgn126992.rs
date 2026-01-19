use std::fmt;

#[derive(Debug, Clone)]
pub struct SystemTime {
    pub sid: u8,
    pub source: u8,
    pub date: u16,      // Days since January 1, 1970
    pub time: u32,      // Seconds since midnight
}

impl SystemTime {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }

        let sid = data[0];
        let source = data[1];
        let date = u16::from_le_bytes([data[2], data[3]]);
        let time = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

        Some(SystemTime {
            sid,
            source,
            date,
            time,
        })
    }

    /// Convert NMEA2000 date/time to Unix timestamp (seconds since epoch)
    pub fn to_unix_timestamp(&self) -> i64 {
        // NMEA2000 date is days since January 1, 1970
        let days_since_epoch = self.date as i64;
        let seconds_from_date = days_since_epoch * 86400;
        
        // NMEA2000 time is in units of 0.0001 seconds since midnight
        let seconds_since_midnight = (self.time as f64 * 0.0001) as i64;
        
        seconds_from_date + seconds_since_midnight
    }

    /// Get milliseconds component
    pub fn milliseconds(&self) -> u32 {
        // Time is in units of 0.0001 seconds (100 microseconds)
        let total_ms = (self.time as f64 * 0.0001 * 1000.0) as u32;
        total_ms % 1000
    }
}

impl fmt::Display for SystemTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let timestamp = self.to_unix_timestamp();
        let ms = self.milliseconds();
        
        // Convert to date/time components
        let days_since_epoch = self.date as i64;
        let seconds_since_midnight = (self.time as f64 * 0.0001) as i64;
        
        let hours = seconds_since_midnight / 3600;
        let minutes = (seconds_since_midnight % 3600) / 60;
        let seconds = seconds_since_midnight % 60;
        
        write!(f, "System Time: Day {} from 1970-01-01, {:02}:{:02}:{:02}.{:03} UTC (Unix: {}.{:03}s)", 
               days_since_epoch, hours, minutes, seconds, ms, timestamp, ms)
    }
}
