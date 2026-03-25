use mysql::Pool;
use std::sync::{Arc, Mutex};
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

#[derive(Debug, serde::Serialize)]
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
    /// Create a new health check manager with the specified interval
    pub fn new(check_interval: Duration) -> Self {
        Self {
            last_check: Instant::now(),
            check_interval,
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
    /// Returns the updated database connection (may be None if reconnection fails)
    pub fn check_and_reconnect(
        &mut self,
        db: &mut Option<VesselDatabase>,
        db_url: &str,
    ) -> bool {
        use tracing::{info, warn};
        
        if !self.should_check() {
            return false;
        }
        
        let mut did_check = false;
        if let Some(database) = db {
            match database.health_check() {
                Ok(_) => {
                    info!("[DB Health] Connection healthy");
                }
                Err(e) => {
                    warn!("[DB Health] Connection check failed: {}", e);
                    warn!("Attempting to reconnect to database...");
                    *db = VesselDatabase::reconnect_with_retry(db_url, 3);
                }
            }
            did_check = true;
        }
        
        self.reset();
        did_check
    }
}
