use std::time::{SystemTime as StdSystemTime, UNIX_EPOCH};
use nmea2k::pgns::NMEASystemTime;

pub struct TimeMonitor {
    last_warning_time: Option<StdSystemTime>,
    warning_cooldown_secs: u64,
    has_time_skew: bool,
    time_skew_threshold_ms: i64,
    last_measured_skew_ms: i64,
    is_initialized: bool,
}

impl TimeMonitor {
    pub fn new(time_skew_threshold_ms: i64) -> Self {
        Self {
            last_warning_time: None,
            warning_cooldown_secs: 10, // Only warn once every 10 seconds
            has_time_skew: false,
            time_skew_threshold_ms,
            last_measured_skew_ms: 0,
            is_initialized: false,
        }
    }

    pub fn is_initialized(&self) -> bool {
        self.is_initialized
    }

    pub fn last_measured_skew_ms(&self) -> i64 {
        self.last_measured_skew_ms
    }

    pub fn is_valid_and_synced(&self) -> bool {
        self.is_initialized() && self.is_time_synchronized()
    }

    /// Process a system time message and check for time skew
    pub fn process_system_time(&mut self, nmea_time: &NMEASystemTime) {
        // Get current system time
        let now = StdSystemTime::now();
        let system_timestamp = match now.duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_secs() as i64,
            Err(_) => {
                log::warn!("WARNING: System time is before Unix epoch!");
                return;
            }
        };

        // Calculate time skew in milliseconds
        let nmea_system_time = nmea_time.to_system_time();
        let time_skew_ms = match now.duration_since(nmea_system_time) {
            Ok(duration) => duration.as_millis() as i64,
            Err(e) => -(e.duration().as_millis() as i64), // Negative if NMEA is ahead
        };
        let abs_skew = time_skew_ms.abs();

        if abs_skew > self.time_skew_threshold_ms {
            self.has_time_skew = true;
            
            // Check if we should print a warning (respect cooldown period)
            let should_warn = if let Some(last_warn) = self.last_warning_time {
                match now.duration_since(last_warn) {
                    Ok(duration) => duration.as_secs() >= self.warning_cooldown_secs,
                    Err(_) => true,
                }
            } else {
                true
            };

            if should_warn {
                self.print_time_skew_warning(time_skew_ms, system_timestamp, nmea_time.to_unix_timestamp());
                self.last_warning_time = Some(now);
            }
        } else {
            self.has_time_skew = false;
        }
        self.is_initialized = true;
        self.last_measured_skew_ms = time_skew_ms;
    }

    /// Check if time is synchronized (no skew above threshold)
    /// Returns true if it's safe to write to database
    pub fn is_time_synchronized(&self) -> bool {
        !self.has_time_skew
    }

    fn print_time_skew_warning(&self, skew_ms: i64, system_ts: i64, nmea_ts: i64) {
        println!("\n╔════════════════════════════════════════════════════════════╗");
        println!("║  ⚠️  TIME SKEW WARNING                                     ║");
        println!("╠════════════════════════════════════════════════════════════╣");
        
        if skew_ms > 0 {
            println!("║  NMEA2000 time is BEHIND system time by {} ms       ", skew_ms);
        } else {
            println!("║  NMEA2000 time is AHEAD of system time by {} ms      ", skew_ms.abs());
        }
        
        println!("║                                                            ║");
        println!("║  System Time:  {} (Unix timestamp)              ║", system_ts);
        println!("║  NMEA2000 Time: {} (Unix timestamp)             ║", nmea_ts);
        println!("║                                                            ║");
        println!("║  Threshold: {} ms                                       ║", self.time_skew_threshold_ms);
        println!("║  ⚠️  DATABASE WRITES DISABLED UNTIL TIME SYNC              ║");
        println!("╚════════════════════════════════════════════════════════════╝\n");
    }
}

impl Default for TimeMonitor {
    fn default() -> Self {
        Self::new(500)
    }
}

impl nmea2k::MessageHandler for TimeMonitor {
    fn handle_message(&mut self, message: &nmea2k::N2kMessage) {
        match message {
            nmea2k::pgns::N2kMessage::NMEASystemTime(sys_time) => {
                self.process_system_time(sys_time);
            }
            _ => {} // Ignore messages we're not interested in
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmea2k::pgns::NMEASystemTime;

    #[test]
    fn test_time_monitor_default() {
        let monitor = TimeMonitor::default();
        assert_eq!(monitor.time_skew_threshold_ms, 500);
        assert!(!monitor.has_time_skew);
    }

    #[test]
    fn test_time_monitor_custom_threshold() {
        let monitor = TimeMonitor::new(1000);
        assert_eq!(monitor.time_skew_threshold_ms, 1000);
    }

    #[test]
    fn test_is_time_synchronized_initially() {
        let monitor = TimeMonitor::new(500);
        assert!(monitor.is_time_synchronized());
    }

    #[test]
    fn test_time_skew_detection_within_threshold() {
        // Use a larger threshold to account for processing delays in tests
        let mut monitor = TimeMonitor::new(2000);
        
        // Create a system time close to current time (within threshold)
        let now = StdSystemTime::now();
        let duration = now.duration_since(UNIX_EPOCH).unwrap();
        let current_days = (duration.as_secs() / 86400) as u16;
        let current_seconds = (duration.as_secs() % 86400) as u32;
        
        // Convert to NMEA2000 units (0.0001 seconds)
        let nmea_time_units = current_seconds * 10000;
        
        let nmea_time = NMEASystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date: current_days,
            time: nmea_time_units,
        };
        
        monitor.process_system_time(&nmea_time);
        
        // Time should be synchronized (skew within threshold)
        assert!(monitor.is_time_synchronized());
    }

    #[test]
    fn test_time_skew_detection_beyond_threshold() {
        let mut monitor = TimeMonitor::new(500);
        
        // Create a system time far in the past (definitely beyond threshold)
        let old_date = 10000; // Days since 1970 (way in the past)
        let nmea_time = NMEASystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date: old_date,
            time: 0,
        };
        
        monitor.process_system_time(&nmea_time);
        
        // Time should NOT be synchronized (large skew)
        assert!(!monitor.is_time_synchronized());
    }

    #[test]
    fn test_system_time_to_unix_timestamp() {
        // Test a known date/time
        // January 2, 1970, 00:01:00 UTC
        let nmea_time = NMEASystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date: 1, // 1 day since epoch
            time: 600000, // 60 seconds * 10000 (0.0001 second units)
        };
        
        let timestamp = nmea_time.to_unix_timestamp();
        // 1 day (86400 seconds) + 60 seconds = 86460
        assert_eq!(timestamp, 86460);
    }

    #[test]
    fn test_system_time_milliseconds() {
        // Test milliseconds extraction
        let nmea_time = NMEASystemTime {
            pgn: 126992,
            sid: 0,
            source: 0,
            date: 0,
            time: 12345, // 1.2345 seconds = 1234.5 ms
        };
        
        let ms = nmea_time.milliseconds();
        // 12345 * 0.0001 * 1000 = 1234.5 -> 1234 ms (integer part)
        assert_eq!(ms, 234); // 234 ms within the current second
    }
}
