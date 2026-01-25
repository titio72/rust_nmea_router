use chrono::DateTime;

#[derive(Debug, Clone)]
pub struct N2kDateTime {
    pub date: u16, // days since 1970-01-01
    pub time: f64, // seconds since midnight
}

impl N2kDateTime {
    pub fn new(date: u16, time: f64) -> Option<Self> {
        Some(Self {
            date,
            time,
        })    }

    /// Convert NMEA2000 date/time to Unix timestamp (seconds since epoch)
    pub fn to_unix_timestamp(&self) -> i64 {
        // NMEA2000 date is days since January 1, 1970
        let days_since_epoch = self.date as i64;
        let seconds_from_date = days_since_epoch * 86400;
        
        // NMEA2000 time is in units of 0.0001 seconds since midnight
        let seconds_since_midnight = (self.time as f64 * 0.0001) as i64;
        
        seconds_from_date + seconds_since_midnight
    }

    #[allow(dead_code)]
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

    #[allow(dead_code)]
    pub fn to_date_time(&self) -> DateTime<chrono::Utc> {
        let unix_timestamp = self.to_unix_timestamp();
        DateTime::<chrono::Utc>::from_timestamp(unix_timestamp, self.milliseconds() * 1_000_000)
            .expect("Invalid timestamp")
    }

    pub fn to_system_time(&self) -> std::time::SystemTime {
        let unix_timestamp = self.to_unix_timestamp();
        let duration = std::time::Duration::from_secs(unix_timestamp as u64)
            + std::time::Duration::from_millis(self.milliseconds() as u64);
        std::time::UNIX_EPOCH + duration
    }
}