use std::time::{SystemTime as StdSystemTime, UNIX_EPOCH};
use crate::pgns::SystemTime;

pub struct TimeMonitor {
    last_warning_time: Option<StdSystemTime>,
    warning_cooldown_secs: u64,
    has_time_skew: bool,
    time_skew_threshold_ms: i64,
}

impl TimeMonitor {
    pub fn new(time_skew_threshold_ms: i64) -> Self {
        Self {
            last_warning_time: None,
            warning_cooldown_secs: 10, // Only warn once every 10 seconds
            has_time_skew: false,
            time_skew_threshold_ms,
        }
    }

    /// Process a system time message and check for time skew
    pub fn process_system_time(&mut self, nmea_time: &SystemTime) {
        // Get current system time
        let now = StdSystemTime::now();
        let system_timestamp = match now.duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_secs() as i64,
            Err(_) => {
                eprintln!("⚠️  WARNING: System time is before Unix epoch!");
                return;
            }
        };

        let system_ms = match now.duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.subsec_millis() as i64,
            Err(_) => 0,
        };

        // Get NMEA2000 time
        let nmea_timestamp = nmea_time.to_unix_timestamp();
        let nmea_ms = nmea_time.milliseconds() as i64;

        // Calculate time skew in milliseconds
        let time_skew_ms = (system_timestamp * 1000 + system_ms) - (nmea_timestamp * 1000 + nmea_ms);
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
                self.print_time_skew_warning(time_skew_ms, system_timestamp, nmea_timestamp);
                self.last_warning_time = Some(now);
            }
        } else {
            self.has_time_skew = false;
        }
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
