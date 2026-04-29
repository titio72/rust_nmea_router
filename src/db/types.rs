use mysql::Pool;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime};
use crate::trip::Trip;
use crate::position_utils::Position;
use crate::utilities::EngineStatus;

/// Encapsulates vessel status data for database insertion
#[derive(Debug, Clone)]
pub struct VesselStatusOperation {
    pub timestamp: Instant,
    pub position: Position,
    pub average_speed_kn: f64,
    pub max_speed_kn: f64,
    pub is_moored: bool,
    pub engine_on: EngineStatus,
    pub total_distance_nm: f64,
    pub total_time_ms: u64,
    pub wind_speed_kn: Option<f64>,
    #[allow(dead_code)]
    pub wind_speed_variance: Option<f64>,
    pub wind_angle_deg: Option<f64>,
    #[allow(dead_code)]
    pub wind_angle_variance: Option<f64>,
    pub cog_deg: Option<f64>,
    pub average_heading_deg: Option<f64>,
}

/// Represents a trip operation to be performed atomically with vessel status insert
pub enum TripOperation {
    CreateTrip(Trip),
    UpdateTrip(Trip),
    None,
}

/// Main database connection wrapper
#[derive(Clone)]
pub struct VesselDatabase {
    pub pool: Pool,
    pub(crate) system_status_cache: Arc<Mutex<std::collections::HashMap<String, bool>>>,
}

/// Manages database health check timing and execution
pub struct HealthCheckManager {
    pub(crate) last_check: Instant,
    pub(crate) check_interval: Duration,
    pub(crate) pool_min: usize,
    pub(crate) pool_max: usize,
}

// ============== Web API Query Response Types ==============

#[derive(Debug, serde::Serialize)]
pub struct TripSummary {
    pub id: u32,
    pub uuid: Option<String>,
    pub description: String,
    pub start_date: String,
    pub end_date: String,
    pub total_distance_nm: f64,
    pub total_time_ms: i64,
    pub sailing_time_ms: i64,
    pub motoring_time_ms: i64,
    pub moored_time_ms: i64,
    pub sailing_distance_nm: f64,
    pub motoring_distance_nm: f64,
}

impl TripSummary {
    #[allow(dead_code)]
    pub fn start_timestamp(&self) -> Result<std::time::SystemTime, Box<dyn std::error::Error>> {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&self.start_date) {
            return Ok(SystemTime::UNIX_EPOCH + Duration::from_secs(dt.timestamp() as u64));
        }
        Err("Invalid start_date format".into())
    }

    #[allow(dead_code)]
    pub fn end_timestamp(&self) -> Result<std::time::SystemTime, Box<dyn std::error::Error>> {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&self.end_date) {
            return Ok(SystemTime::UNIX_EPOCH + Duration::from_secs(dt.timestamp() as u64));
        }
        Err("Invalid end_date format".into())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TrackPoint {
    pub timestamp: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub avg_speed_kn: Option<f64>,
    pub max_speed_kn: Option<f64>,
    pub moored: bool,
    pub engine_on: u8,  // 0=off, 1=on, 2=unknown
    pub total_distance_nm: Option<f64>,
    pub total_time_ms: u64,
    pub average_wind_speed_kn: Option<f64>,
    pub average_wind_angle_deg: Option<f64>,
    pub cog_deg: Option<f64>,
    pub average_heading_deg: Option<f64>,
}

#[derive(Debug, serde::Serialize)]
pub struct WebMetricData {
    pub timestamp: String,
    pub metric_id: String,
    pub avg_value: Option<f64>,
    pub max_value: Option<f64>,
    pub min_value: Option<f64>,
}

#[derive(Debug, serde::Serialize)]
pub struct MultiMetricData {
    pub metrics: std::collections::HashMap<String, Vec<WebMetricData>>,
}

#[derive(Debug, serde::Serialize)]
pub struct SpeedDistributionData {
    pub labels: Vec<String>,
    pub sailing: Vec<f64>,
    pub motoring: Vec<f64>,
}

#[derive(Debug, serde::Serialize)]
pub struct WindStatisticsData {
    pub directions: Vec<f64>,
    pub wind_distances: Vec<f64>,
    pub max_wind_speeds: Vec<f64>,
}

#[derive(Debug, serde::Serialize)]
pub struct TripLeg {
    pub leg_number: u32,
    pub start_timestamp: String,
    pub end_timestamp: String,
    pub total_distance_nm: f64,
    pub sailing_distance_nm: f64,
    pub motoring_distance_nm: f64,
    pub sailing_time_ms: u64,
    pub motoring_time_ms: u64,
    pub sailing_time_formatted: String,
    pub motoring_time_formatted: String,
    pub start_lat: Option<f64>,
    pub start_lon: Option<f64>,
    pub end_lat: Option<f64>,
    pub end_lon: Option<f64>,
}

#[derive(Debug, serde::Serialize)]
pub struct TripLegsData {
    pub legs: Vec<TripLeg>,
}

#[derive(Debug, serde::Serialize)]
pub struct HeatmapDay {
    pub date: String,
    pub distance_nm: f64,
}

#[derive(Debug, serde::Serialize)]
pub struct HeatmapData {
    pub days: Vec<HeatmapDay>,
    pub min_distance: f64,
    pub max_distance: f64,
    pub total_distance: f64,
}

#[derive(Debug, serde::Serialize)]
pub struct FastestSegment {
    pub distance_nm: f64,
    pub average_speed_kn: f64,
    pub duration_ms: u64,
    pub start_timestamp: String,
    pub end_timestamp: String,
}

#[derive(Debug, serde::Serialize)]
pub struct TrackAnalytics {
    pub max_speed_kn: Option<f64>,
    pub max_speed_timestamp: Option<String>,
    pub average_speed_kn: Option<f64>,
    pub average_speed_sailing_kn: Option<f64>,
    pub average_speed_motoring_kn: Option<f64>,
    pub fastest_1nm: Option<FastestSegment>,
    pub fastest_5nm: Option<FastestSegment>,
    pub fastest_10nm: Option<FastestSegment>,
    pub fastest_25nm: Option<FastestSegment>,
}

#[derive(Debug, serde::Serialize)]
pub struct MonthlyStatistic {
    pub year: i32,
    pub month: u32,
    pub date: String,
    pub sailing_distance_nm: f64,
    pub motoring_distance_nm: f64,
}

#[derive(Debug, serde::Serialize)]
pub struct MonthlyStatistics {
    pub months: Vec<MonthlyStatistic>,
}

/// Format milliseconds as human-readable duration (e.g., "1h 30m" or "45m")
pub fn format_duration_ms(milliseconds: u64) -> String {
    let total_seconds = milliseconds / 1000;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    
    if hours > 0 {
        if minutes > 0 {
            format!("{}h {}m", hours, minutes)
        } else {
            format!("{}h", hours)
        }
    } else if minutes > 0 {
        format!("{}m", minutes)
    } else {
        format!("{}s", seconds)
    }
}

impl HealthCheckManager {
    /// Create a new health check manager with the specified interval and pool sizing
    pub fn new(check_interval: Duration, pool_min: usize, pool_max: usize) -> Self {
        Self {
            last_check: Instant::now(),
            check_interval,
            pool_min,
            pool_max,
        }
    }
    
    /// Check if it's time to perform a health check
    pub fn should_check(&self) -> bool {
        self.last_check.elapsed() >= self.check_interval
    }
    
    /// Reset the health check timer
    pub fn reset(&mut self) {
        self.last_check = Instant::now();
    }
    
    /// Perform health check and handle reconnection if needed
    /// Returns Ok(true) if a check was performed, Ok(false) if not time yet,
    /// or Err if reconnection failed after all retries
    pub fn check_and_reconnect(
        &mut self,
        db: &Arc<RwLock<VesselDatabase>>,
        db_url: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        use tracing::{info, warn};

        if !self.should_check() {
            return Ok(false);
        }

        // Acquire read lock for health check
        let health_ok = {
            let db_read = db.read().unwrap_or_else(|e| e.into_inner());
            db_read.health_check().is_ok()
        };

        if health_ok {
            info!("[DB Health] Connection healthy");
        } else {
            warn!("[DB Health] Connection check failed");
            self.do_reconnect(db, db_url)?;
        }

        self.reset();
        Ok(true)
    }

    /// Force immediate reconnection attempt (called reactively on write failures)
    /// Returns Ok(true) if reconnection succeeded, Err if it failed
    pub fn force_reconnect(
        &mut self,
        db: &Arc<RwLock<VesselDatabase>>,
        db_url: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        use tracing::warn;
        warn!("[DB Health] Reactive reconnection triggered");
        self.do_reconnect(db, db_url)?;
        self.reset();
        Ok(true)
    }

    /// Internal reconnection logic shared by periodic and reactive paths
    fn do_reconnect(
        &self,
        db: &Arc<RwLock<VesselDatabase>>,
        db_url: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use tracing::warn;
        warn!("Attempting to reconnect to database...");
        match VesselDatabase::reconnect_with_retry(db_url, 3, self.pool_min, self.pool_max) {
            Some(new_db) => {
                // Acquire write lock to replace the database
                let mut db_write = db.write().unwrap_or_else(|e| e.into_inner());
                *db_write = new_db;
                Ok(())
            }
            None => {
                Err("Database reconnection failed after all retries".into())
            }
        }
    }
}

/// Check if an error is likely a database connection error that warrants reconnection
pub fn is_connection_error(err: &dyn std::error::Error) -> bool {
    let err_str = err.to_string().to_lowercase();
    err_str.contains("connection")
        || err_str.contains("timeout")
        || err_str.contains("broken pipe")
        || err_str.contains("reset by peer")
        || err_str.contains("refused")
        || err_str.contains("network")
        || err_str.contains("mysql server has gone away")
        || err_str.contains("lost connection")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_connection_error_detects_connection_errors() {
        let err: Box<dyn std::error::Error> = "Connection refused".into();
        assert!(is_connection_error(err.as_ref()));

        let err: Box<dyn std::error::Error> = "Connection timeout".into();
        assert!(is_connection_error(err.as_ref()));

        let err: Box<dyn std::error::Error> = "MySQL server has gone away".into();
        assert!(is_connection_error(err.as_ref()));

        let err: Box<dyn std::error::Error> = "Lost connection to MySQL server".into();
        assert!(is_connection_error(err.as_ref()));

        let err: Box<dyn std::error::Error> = "Broken pipe".into();
        assert!(is_connection_error(err.as_ref()));

        let err: Box<dyn std::error::Error> = "Network unreachable".into();
        assert!(is_connection_error(err.as_ref()));
    }

    #[test]
    fn test_is_connection_error_ignores_other_errors() {
        let err: Box<dyn std::error::Error> = "Duplicate entry for key".into();
        assert!(!is_connection_error(err.as_ref()));

        let err: Box<dyn std::error::Error> = "Column not found".into();
        assert!(!is_connection_error(err.as_ref()));

        let err: Box<dyn std::error::Error> = "Syntax error in SQL".into();
        assert!(!is_connection_error(err.as_ref()));
    }
}
