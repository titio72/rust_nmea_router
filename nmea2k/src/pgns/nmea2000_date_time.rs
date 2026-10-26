use chrono::{DateTime, Timelike};

#[derive(Debug, Clone)]
pub struct N2kDateTime {
    pub date: u16, // days since 1970-01-01
    pub time: f64, // seconds since midnight
}

impl N2kDateTime {
    pub fn new(date: u16, time: f64) -> Option<Self> {
        Some(Self { date, time })
    }

    pub fn from_date_time(date_time: &DateTime<chrono::Utc>) -> Self {
        let epoch = DateTime::<chrono::Utc>::from_timestamp(0, 0);
        let duration = date_time.signed_duration_since(epoch.unwrap());
        let total_days = duration.num_days() as u16;
        let seconds_since_midnight =
            (date_time.hour() * 3600 + date_time.minute() * 60 + date_time.second()) as f64
                + (date_time.timestamp_subsec_micros() as f64) * 0.000001;
        N2kDateTime {
            date: total_days,
            time: seconds_since_midnight,
        }
    }

    /// Time since midnight in whole milliseconds, rounded to the nearest one.
    ///
    /// Rounding absorbs the float error of the decoders' scaling, which can land just below the
    /// true value: truncating would turn 0.57 s into 569 ms, or 45297 s into 45296 s.
    fn millis_since_midnight(&self) -> i64 {
        (self.time * 1000.0).round() as i64
    }

    /// Convert NMEA2000 date/time to Unix timestamp (seconds since epoch)
    pub fn to_unix_timestamp(&self) -> i64 {
        // NMEA2000 date is days since January 1, 1970
        let days_since_epoch = self.date as i64;
        let seconds_from_date = days_since_epoch * 86400;

        let seconds_since_midnight = self.millis_since_midnight() / 1000;

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
        (self.millis_since_midnight() % 1000) as u32
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_date_time_round_trip() {
        let dt = DateTime::parse_from_rfc3339("2026-02-22T14:10:20.570Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let n2k = N2kDateTime::from_date_time(&dt);
        assert!((n2k.time - 51020.57).abs() < 1e-6);
        assert_eq!(n2k.to_date_time(), dt);
    }

    #[test]
    fn test_scaled_raw_time_does_not_lose_a_unit() {
        // Decoded as raw * 0.0001, these land just below the true value:
        // (raw, expected whole seconds, expected milliseconds)
        for (raw, seconds, ms) in [(5_700u32, 0, 570), (452_970_000, 45_297, 0), (863_999_990, 86_399, 999)] {
            let n2k = N2kDateTime { date: 0, time: raw as f64 * 0.0001 };
            assert_eq!(n2k.to_unix_timestamp(), seconds, "raw {}", raw);
            assert_eq!(n2k.milliseconds(), ms, "raw {}", raw);
        }
    }
}
