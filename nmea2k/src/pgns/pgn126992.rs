use std::fmt;

use super::nmea2000_date_time::N2kDateTime;

#[derive(Debug, Clone)]
pub struct NMEASystemTime {
    #[allow(dead_code)]
    pub pgn: u32,
    #[allow(dead_code)]
    pub sid: u8,
    #[allow(dead_code)]
    pub source: u8,
    pub date_time: N2kDateTime,
}

impl NMEASystemTime {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }

        let sid = data[0];
        let source = data[1];
        let date = u16::from_le_bytes([data[2], data[3]]);
        let time = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as f64;

        Some(NMEASystemTime {
            pgn: 126992,
            sid,
            source,
            date_time: N2kDateTime {
                date,
                time,
            },
        })
    }
}

impl fmt::Display for NMEASystemTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let timestamp = self.date_time.to_unix_timestamp();
        let ms = self.date_time.milliseconds();
        
        // Convert to date/time components
        let days_since_epoch = self.date_time.date as i64;
        let seconds_since_midnight = (self.date_time.time as f64 * 0.0001) as i64;
        
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
            0x80, 0x51, 0x01, 0x00, // Time = 86400 (in 0.0001 second units = 8.64 seconds)
        ];
        
        let time = NMEASystemTime::from_bytes(&data).unwrap();
        assert_eq!(time.sid, 1);
        assert_eq!(time.source, 2);
        assert_eq!(time.date_time.date, 10);
        assert_eq!(time.date_time.time as i64, 86400); // Raw time value
    }

    #[test]
    fn test_system_time_insufficient_data() {
        let data = vec![0x01, 0x02, 0x03]; // Only 3 bytes
        let time = NMEASystemTime::from_bytes(&data);
        assert!(time.is_none());
    }

    #[test]
    fn test_system_time_to_unix_timestamp_epoch() {
        // Day 0, time 0 should be Unix epoch
        let time = NMEASystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date_time: N2kDateTime { date: 0, time: 0.0 },
        };
        
        let timestamp = time.date_time.to_unix_timestamp();
        assert_eq!(timestamp, 0);
    }

    #[test]
    fn test_system_time_to_unix_timestamp_one_day() {
        // Day 1, time 0 should be 86400 seconds
        let time = NMEASystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date_time: N2kDateTime { date: 1, time: 0.0 },
        };
        
        let timestamp = time.date_time.to_unix_timestamp();
        assert_eq!(timestamp, 86400);
    }

    #[test]
    fn test_system_time_to_unix_timestamp_with_time() {
        // Day 1, 1 hour (3600 seconds = 36000000 in 0.0001 units)
        let time = NMEASystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date_time: N2kDateTime { date: 1, time: 36000000.0 },
        };
        
        let timestamp = time.date_time.to_unix_timestamp();
        assert_eq!(timestamp, 86400 + 3600); // 1 day + 1 hour
    }

    #[test]
    fn test_system_time_milliseconds_zero() {
        let time = NMEASystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date_time: N2kDateTime { date: 0, time: 0.0 },
        };
        
        let ms = time.date_time.milliseconds();
        assert_eq!(ms, 0);
    }

    #[test]
    fn test_system_time_milliseconds_extraction() {
        // 1.5 seconds = 15000 in 0.0001 second units
        let time = NMEASystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date_time: N2kDateTime { date: 0, time: 15000.0 },
        };
        
        let ms = time.date_time.milliseconds();
        assert_eq!(ms, 500); // 500 milliseconds
    }

    #[test]
    fn test_system_time_milliseconds_full_second() {
        // Exactly 1 second = 10000 in 0.0001 second units
        let time = NMEASystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date_time: N2kDateTime { date: 0, time: 10000.0 },
        };
        
        let ms = time.date_time.milliseconds();
        assert_eq!(ms, 0); // 0 milliseconds (full second)
    }

    #[test]
    fn test_system_time_milliseconds_complex() {
        // 3.75 seconds = 37500 in 0.0001 second units
        let time = NMEASystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date_time: N2kDateTime { date: 0, time: 37500.0 },
        };
        
        let ms = time.date_time.milliseconds();
        assert_eq!(ms, 750); // 750 milliseconds
    }

    #[test]
    fn test_system_time_realistic_date() {
        // January 1, 2024 00:00:00 UTC
        // Days since 1970-01-01: 19723 days (leap years included)
        let time = NMEASystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date_time: N2kDateTime { date: 19723, time: 0.0 },
        };
        
        let timestamp = time.date_time.to_unix_timestamp();
        // 19723 days * 86400 seconds/day = 1704067200 seconds
        assert_eq!(timestamp, 1704067200);
    }
}
