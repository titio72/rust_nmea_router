use std::fmt;

use chrono::DateTime;

#[derive(Debug, Clone)]
pub struct SystemTime {
    pub pgn: u32,
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
            pgn: 126992,
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

    pub fn to_total_milliseconds(&self) -> i64 {
        let unix_timestamp = self.to_unix_timestamp() as u64;
        let total_ms = unix_timestamp * 1000 + self.milliseconds() as u64;
        total_ms as i64
    }

    /// Get milliseconds component
    pub fn milliseconds(&self) -> u32 {
        // Time is in units of 0.0001 seconds (100 microseconds)
        let total_ms = (self.time as f64 * 0.0001 * 1000.0) as u32;
        total_ms % 1000
    }

    pub fn to_date_time(&self) -> DateTime<chrono::Utc> {
        let unix_timestamp = self.to_unix_timestamp();
        let naive = chrono::NaiveDateTime::from_timestamp_opt(unix_timestamp, self.milliseconds() * 1_000_000)
            .expect("Invalid timestamp");
        DateTime::<chrono::Utc>::from_utc(naive, chrono::Utc)
    }

    pub fn to_system_time(&self) -> std::time::SystemTime {
        let unix_timestamp = self.to_unix_timestamp();
        let duration = std::time::Duration::from_secs(unix_timestamp as u64)
            + std::time::Duration::from_millis(self.milliseconds() as u64);
        std::time::UNIX_EPOCH + duration
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_time_from_bytes() {
        let data = vec![
            0x01, // SID
            0x02, // Source
            0x0A, 0x00, // Date = 10 days
            0x80, 0x51, 0x01, 0x00, // Time = 86400 (1 day in 0.0001 second units)
        ];
        
        let time = SystemTime::from_bytes(&data).unwrap();
        assert_eq!(time.sid, 1);
        assert_eq!(time.source, 2);
        assert_eq!(time.date, 10);
        assert_eq!(time.time, 86400);
    }

    #[test]
    fn test_system_time_insufficient_data() {
        let data = vec![0x01, 0x02, 0x03]; // Only 3 bytes
        let time = SystemTime::from_bytes(&data);
        assert!(time.is_none());
    }

    #[test]
    fn test_system_time_to_unix_timestamp_epoch() {
        // Day 0, time 0 should be Unix epoch
        let time = SystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date: 0,
            time: 0,
        };
        
        let timestamp = time.to_unix_timestamp();
        assert_eq!(timestamp, 0);
    }

    #[test]
    fn test_system_time_to_unix_timestamp_one_day() {
        // Day 1, time 0 should be 86400 seconds
        let time = SystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date: 1,
            time: 0,
        };
        
        let timestamp = time.to_unix_timestamp();
        assert_eq!(timestamp, 86400);
    }

    #[test]
    fn test_system_time_to_unix_timestamp_with_time() {
        // Day 1, 1 hour (3600 seconds = 36000000 in 0.0001 units)
        let time = SystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date: 1,
            time: 36000000,
        };
        
        let timestamp = time.to_unix_timestamp();
        assert_eq!(timestamp, 86400 + 3600); // 1 day + 1 hour
    }

    #[test]
    fn test_system_time_milliseconds_zero() {
        let time = SystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date: 0,
            time: 0,
        };
        
        let ms = time.milliseconds();
        assert_eq!(ms, 0);
    }

    #[test]
    fn test_system_time_milliseconds_extraction() {
        // 1.5 seconds = 15000 in 0.0001 second units
        let time = SystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date: 0,
            time: 15000,
        };
        
        let ms = time.milliseconds();
        assert_eq!(ms, 500); // 500 milliseconds
    }

    #[test]
    fn test_system_time_milliseconds_full_second() {
        // Exactly 1 second = 10000 in 0.0001 second units
        let time = SystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date: 0,
            time: 10000,
        };
        
        let ms = time.milliseconds();
        assert_eq!(ms, 0); // 0 milliseconds (full second)
    }

    #[test]
    fn test_system_time_milliseconds_complex() {
        // 3.75 seconds = 37500 in 0.0001 second units
        let time = SystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date: 0,
            time: 37500,
        };
        
        let ms = time.milliseconds();
        assert_eq!(ms, 750); // 750 milliseconds
    }

    #[test]
    fn test_system_time_realistic_date() {
        // January 1, 2024 00:00:00 UTC
        // Days since 1970-01-01: 19723 days (leap years included)
        let time = SystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date: 19723,
            time: 0,
        };
        
        let timestamp = time.to_unix_timestamp();
        // 19723 days * 86400 seconds/day = 1704067200 seconds
        assert_eq!(timestamp, 1704067200);
    }
}
