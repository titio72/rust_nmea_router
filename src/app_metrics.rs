use std::time::{Duration, Instant};
use tracing::info;

use crate::time_monitor::TimeSyncStatus;

/// Application-level metrics for tracking CAN bus and NMEA2000 processing statistics
/// (not to be confused with environmental metrics like wind, temperature, etc.)
pub struct AppMetrics {
    /// Number of CAN frames received
    pub can_frames: u64,
    /// Number of complete NMEA2000 messages assembled
    pub nmea_messages: u64,
    /// Number of vessel status reports written to database
    pub vessel_reports: u64,
    /// Number of environmental data reports written to database
    pub env_reports: u64,
    /// Number of CAN bus errors encountered
    pub can_errors: u64,
    pub gnss_time_skew: i64,
    pub gnss_time_skew_status: TimeSyncStatus
}

impl AppMetrics {
    /// Create a new AppMetrics instance with all counters at zero
    pub fn new() -> Self {
        Self {
            can_frames: 0,
            nmea_messages: 0,
            vessel_reports: 0,
            env_reports: 0,
            can_errors: 0,
            gnss_time_skew: 0,
            gnss_time_skew_status: TimeSyncStatus::NotInitialized,
        }
    }
    
    /// Reset all counters to zero
    pub fn reset(&mut self) {
        self.can_frames = 0;
        self.nmea_messages = 0;
        self.vessel_reports = 0;
        self.env_reports = 0;
        self.can_errors = 0;
        self.gnss_time_skew = 0;
        // Note: Do not reset gnss_time_skew_status
    }
    
    /// Log current metrics to the info log
    pub fn log(&self) {
        info!(
            "[Metrics] CAN frames: {}, NMEA messages: {}, Vessel reports: {}, Env reports: {}, CAN errors: {}, GNSS time sync: {:?}/{} ms",
            self.can_frames,
            self.nmea_messages,
            self.vessel_reports,
            self.env_reports,
            self.can_errors,
            self.gnss_time_skew_status,
            self.gnss_time_skew
        );
    }
}

impl Default for AppMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Manages periodic logging of application metrics
pub struct MetricsLogger {
    last_log: Instant,
    log_interval: Duration,
}

impl MetricsLogger {
    /// Create a new MetricsLogger with the specified logging interval
    pub fn new(log_interval: Duration) -> Self {
        Self {
            last_log: Instant::now(),
            log_interval,
        }
    }
    
    /// Check if it's time to log metrics, and if so, log them and reset
    /// Returns true if metrics were logged
    pub fn check_and_log(&mut self, metrics: &mut AppMetrics) -> bool {
        if self.last_log.elapsed() >= self.log_interval {
            metrics.log();
            metrics.reset();
            self.last_log = Instant::now();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_new_metrics_are_zero() {
        let metrics = AppMetrics::new();
        assert_eq!(metrics.can_frames, 0);
        assert_eq!(metrics.nmea_messages, 0);
        assert_eq!(metrics.vessel_reports, 0);
        assert_eq!(metrics.env_reports, 0);
        assert_eq!(metrics.can_errors, 0);
    }
    
    #[test]
    fn test_reset_clears_all_counters() {
        let mut metrics = AppMetrics::new();
        metrics.can_frames = 100;
        metrics.nmea_messages = 50;
        metrics.vessel_reports = 10;
        metrics.env_reports = 20;
        metrics.can_errors = 5;
        
        metrics.reset();
        
        assert_eq!(metrics.can_frames, 0);
        assert_eq!(metrics.nmea_messages, 0);
        assert_eq!(metrics.vessel_reports, 0);
        assert_eq!(metrics.env_reports, 0);
        assert_eq!(metrics.can_errors, 0);
    }
    
    #[test]
    fn test_metrics_logger_interval() {
        let mut logger = MetricsLogger::new(Duration::from_millis(50));
        let mut metrics = AppMetrics::new();
        
        // Should not log immediately
        assert!(!logger.check_and_log(&mut metrics));
        
        // Wait for interval
        std::thread::sleep(Duration::from_millis(60));
        
        // Should log now
        assert!(logger.check_and_log(&mut metrics));
        
        // Should not log immediately after
        assert!(!logger.check_and_log(&mut metrics));
    }
}
