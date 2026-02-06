use mysql::*;
use mysql::prelude::*;
use std::{error::Error, time::{Duration, Instant}};
use std::time::{SystemTime};
use crate::{environmental_monitor::{MetricData, MetricId}, utilities::dirty_instant_to_systemtime};
use crate::trip::Trip;
use chrono::NaiveDateTime;
use tracing::{info, warn};

/// Encapsulates vessel status data for database insertion
#[derive(Debug, Clone)]
pub struct VesselStatusOperation {
    pub time: Instant,
    pub latitude: f64,
    pub longitude: f64,
    pub average_speed_kn: f64,
    pub max_speed_kn: f64,
    pub is_moored: bool,
    pub engine_on: bool,
    pub total_distance_nm: f64,
    pub total_time_ms: u64,
    pub average_wind_speed_kn: Option<f64>,
    #[allow(dead_code)]
    pub wind_speed_variance: Option<f64>,
    pub average_wind_angle_deg: Option<f64>,
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

#[derive(Clone)]
pub struct VesselDatabase {
    pub pool: Pool,
}

impl VesselDatabase {
    /// Create a new database connection
    /// 
    /// Example connection string: "mysql://user:password@localhost:3306/nmea_router"
    /// 
    /// Required table schema:
    /// ```sql
    /// CREATE TABLE vessel_status (
    ///     id BIGINT AUTO_INCREMENT PRIMARY KEY,
    ///     timestamp DATETIME(3) NOT NULL COMMENT 'UTC timezone',
    ///     latitude DOUBLE,
    ///     longitude DOUBLE,
    ///     average_speed_kn DECIMAL(6,3) NOT NULL,
    ///     max_speed_kn DECIMAL(6,3) NOT NULL,
    ///     is_moored BOOLEAN NOT NULL,
    ///     engine_on BOOLEAN NOT NULL DEFAULT 0,
    ///     total_distance_nm DOUBLE NOT NULL DEFAULT 0,
    ///     total_time_ms BIGINT NOT NULL DEFAULT 0,
    ///     average_wind_speed_kn DECIMAL(6,3),
    ///     average_wind_angle_deg DECIMAL(6,3),
    ///     cog_deg DECIMAL(6,3),
    ///     average_heading_deg DECIMAL(6,3),
    ///     INDEX idx_timestamp (timestamp)
    /// );
    /// ```
    pub fn new(connection_url: &str) -> Result<Self, Box<dyn Error>> {
        let opts = Opts::from_url(connection_url)?;
        // Set session timezone to UTC to ensure all timestamps are handled consistently
        let opts_builder = mysql::OptsBuilder::from_opts(opts)
            .init(vec!["SET time_zone = '+00:00'"]);
        let pool = Pool::new(opts_builder)?;
        
        Ok(VesselDatabase { pool })
    }
    
    /// Check database connection health using a simple query
    /// Returns Ok(()) if the connection is healthy, Err otherwise
    pub fn health_check(&self) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        conn.query_drop("SELECT 1")?;
        Ok(())
    }
    

    pub fn update_trip_description(&self, trip_id: i64, new_description: &str) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        let query = "UPDATE trips SET description = :description WHERE id = :id";
        conn.exec_drop(query, mysql::params! {
            "description" => new_description,
            "id" => trip_id,
        })?;
        Ok(())
    }

    /// Insert vessel status and create/update trip in a single transaction
    /// This ensures atomicity - either both operations succeed or both fail
    pub fn insert_status_and_trip(
        &self,
        status_op: &VesselStatusOperation,
        trip_operation: &TripOperation,
    ) -> Result<Option<i64>, Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        let mut tx = conn.start_transaction(TxOpts::default())?;
        
        // Insert vessel status
        let timestamp = chrono::DateTime::<chrono::Utc>::from(dirty_instant_to_systemtime(status_op.time));
               
                tx.exec_drop(
                        r"INSERT INTO vessel_status 
                            (timestamp, latitude, longitude, average_speed_kn, max_speed_kn, is_moored, engine_on, total_distance_nm, total_time_ms, average_wind_speed_kn, average_wind_angle_deg, cog_deg, average_heading_deg)
                            VALUES (:timestamp, :latitude, :longitude, :avg_speed, :max_speed, :is_moored, :engine_on, :total_distance, :total_time, :avg_wind_speed, :avg_wind_angle, :cog_deg, :avg_heading_deg)",
                        params! {
                                "timestamp" => timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                                "latitude" => status_op.latitude,
                                "longitude" => status_op.longitude,
                                "avg_speed" => status_op.average_speed_kn,
                                "max_speed" => status_op.max_speed_kn,
                                "is_moored" => status_op.is_moored,
                                "engine_on" => status_op.engine_on,
                                "total_distance" => status_op.total_distance_nm,
                                "total_time" => status_op.total_time_ms,
                                "avg_wind_speed" => status_op.average_wind_speed_kn,
                                "avg_wind_angle" => status_op.average_wind_angle_deg,
                                "cog_deg" => status_op.cog_deg,
                                "avg_heading_deg" => status_op.average_heading_deg,
                        },
                )?;
        
        // Handle trip operation
        let trip_id = match trip_operation {
            TripOperation::CreateTrip(trip) => {
               
                let start_timestamp = chrono::DateTime::<chrono::Utc>::from(trip.start_timestamp);
                let end_timestamp = chrono::DateTime::<chrono::Utc>::from(trip.end_timestamp);
                
                tx.exec_drop(
                    r"INSERT INTO trips 
                      (description, start_timestamp, end_timestamp, 
                       total_distance_sailed, total_distance_motoring,
                       total_time_sailing, total_time_motoring, total_time_moored)
                      VALUES (:description, :start_ts, :end_ts, 
                              :distance_sailed, :distance_motoring,
                              :time_sailing, :time_motoring, :time_moored)",
                    params! {
                        "description" => &trip.description,
                        "start_ts" => start_timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                        "end_ts" => end_timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                        "distance_sailed" => trip.total_distance_sailed,
                        "distance_motoring" => trip.total_distance_motoring,
                        "time_sailing" => trip.total_time_sailing,
                        "time_motoring" => trip.total_time_motoring,
                        "time_moored" => trip.total_time_moored,
                    },
                )?;
                
                tx.last_insert_id().map(|id| id as i64)
            }
            TripOperation::UpdateTrip(trip) => {
                if let Some(trip_id) = trip.id {
                    let end_timestamp = chrono::DateTime::<chrono::Utc>::from(trip.end_timestamp);
                    
                    tx.exec_drop(
                        r"UPDATE trips 
                          SET end_timestamp = :end_ts,
                              total_distance_sailed = :distance_sailed,
                              total_distance_motoring = :distance_motoring,
                              total_time_sailing = :time_sailing,
                              total_time_motoring = :time_motoring,
                              total_time_moored = :time_moored
                          WHERE id = :trip_id",
                        params! {
                            "trip_id" => trip_id,
                            "end_ts" => end_timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                            "distance_sailed" => trip.total_distance_sailed,
                            "distance_motoring" => trip.total_distance_motoring,
                            "time_sailing" => trip.total_time_sailing,
                            "time_motoring" => trip.total_time_motoring,
                            "time_moored" => trip.total_time_moored,
                        },
                    )?;
                }
                None
            }
            TripOperation::None => None,
        };
        
        tx.commit()?;
        Ok(trip_id)
    }
        
    /// Insert only specific environmental metrics into the database
    /// This allows for adaptive persistence intervals per metric
    pub fn insert_environmental_metrics(
        &self, 
        data: &MetricData, 
        metric_id: MetricId,
        now: std::time::SystemTime,
    ) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        
        // Get current system time and convert to UTC
        let timestamp = chrono::DateTime::<chrono::Utc>::from(now);
        let timestamp_str = timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        
        if data.avg.is_some() || data.max.is_some() || data.min.is_some() {
            conn.exec_drop(
                r"INSERT INTO environmental_data 
                    (timestamp, metric_id, value_avg, value_max, value_min, unit)
                    VALUES (:timestamp, :metric_id, :value_avg, :value_max, :value_min, :unit)
                    ON DUPLICATE KEY UPDATE
                        value_avg = VALUES(value_avg),
                        value_max = VALUES(value_max),
                        value_min = VALUES(value_min),
                        unit = VALUES(unit)",
                params! {
                    "timestamp" => &timestamp_str,
                    "metric_id" => metric_id.as_u8(),
                    "value_avg" => data.avg,
                    "value_max" => data.max,
                    "value_min" => data.min,
                    "unit" => metric_id.unit(),
                },
            )?;
        }

        
        Ok(())
    }

    /// Get the most recent trip from the database
    /// Required table schema:
    /// ```sql
    /// CREATE TABLE trips (
    ///     id BIGINT AUTO_INCREMENT PRIMARY KEY,
    ///     description VARCHAR(255) NOT NULL,
    ///     start_timestamp DATETIME(3) NOT NULL COMMENT 'UTC timezone',
    ///     end_timestamp DATETIME(3) NOT NULL COMMENT 'UTC timezone',
    ///     total_distance_sailed DOUBLE NOT NULL DEFAULT 0 COMMENT 'nautical miles',
    ///     total_distance_motoring DOUBLE NOT NULL DEFAULT 0 COMMENT 'nautical miles',
    ///     total_time_sailing BIGINT NOT NULL DEFAULT 0,
    ///     total_time_motoring BIGINT NOT NULL DEFAULT 0,
    ///     total_time_moored BIGINT NOT NULL DEFAULT 0,
    ///     INDEX idx_end_timestamp (end_timestamp)
    /// );
    /// ```
    pub fn get_last_trip(&self) -> Result<Option<Trip>, Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        
        let row: Option<mysql::Row> = conn.exec_first(
            r"SELECT id, description, 
                     DATE_FORMAT(start_timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as start_ts,
                     DATE_FORMAT(end_timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as end_ts,
                     total_distance_sailed, total_distance_motoring,
                     total_time_sailing, total_time_motoring, total_time_moored
              FROM trips
              ORDER BY end_timestamp DESC
              LIMIT 1",
            (),
        )?;
        
        if let Some(mut row) = row {
            let id: i64 = row.take("id").ok_or("Missing id")?;
            let description: String = row.take("description").ok_or("Missing description")?;
            let start_ts: String = row.take("start_ts").ok_or("Missing start_ts")?;
            let end_ts: String = row.take("end_ts").ok_or("Missing end_ts")?;
            let total_distance_sailed: f64 = row.take("total_distance_sailed").ok_or("Missing total_distance_sailed")?;
            let total_distance_motoring: f64 = row.take("total_distance_motoring").ok_or("Missing total_distance_motoring")?;
            let total_time_sailing: u64 = row.take("total_time_sailing").ok_or("Missing total_time_sailing")?;
            let total_time_motoring: u64 = row.take("total_time_motoring").ok_or("Missing total_time_motoring")?;
            let total_time_moored: u64 = row.take("total_time_moored").ok_or("Missing total_time_moored")?;
            
            // Parse timestamps - remove 'Z' suffix and parse ISO 8601 format
            let start_ts_clean = start_ts.trim_end_matches('Z');
            let end_ts_clean = end_ts.trim_end_matches('Z');
            let start_dt = NaiveDateTime::parse_from_str(start_ts_clean, "%Y-%m-%dT%H:%M:%S%.f")?;
            let end_dt = NaiveDateTime::parse_from_str(end_ts_clean, "%Y-%m-%dT%H:%M:%S%.f")?;
            
            // Convert to SystemTime then to Instant (approximate)
            let start_datetime = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(start_dt, chrono::Utc);
            let end_datetime = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(end_dt, chrono::Utc);
            let start_timestamp = SystemTime::from(start_datetime);
            let end_timestamp = SystemTime::from(end_datetime);

            Ok(Some(Trip {
                id: Some(id),
                description,
                start_timestamp,
                end_timestamp,
                total_distance_sailed,
                total_distance_motoring,
                total_time_sailing,
                total_time_motoring,
                total_time_moored,
            }))
        } else {
            Ok(None)
        }
    }
    
    /// Attempt to reconnect to the database with exponential backoff
    /// Returns Some(VesselDatabase) if successful, None if all retries fail
    pub fn reconnect_with_retry(db_url: &str, max_retries: u32) -> Option<Self> {
        for attempt in 1..=max_retries {
            warn!("Attempting to reconnect to database (attempt {}/{})...", attempt, max_retries);
            match Self::new(db_url) {
                Ok(db) => {
                    info!("Database reconnection successful");
                    return Some(db);
                }
                Err(e) => {
                    warn!("Database reconnection attempt {} failed: {}", attempt, e);
                    if attempt < max_retries {
                        let wait_time = std::cmp::min(2_u64.pow(attempt - 1), 30); // Exponential backoff, max 30s
                        warn!("Waiting {} seconds before retry...", wait_time);
                        std::thread::sleep(Duration::from_secs(wait_time));
                    }
                }
            }
        }
        warn!("Failed to reconnect to database after {} attempts", max_retries);
        None
    }
}

/// Manages database health check timing and execution
pub struct HealthCheckManager {
    last_check: Instant,
    check_interval: Duration,
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

// Web API query structures
#[derive(Debug, serde::Serialize)]
pub struct TripSummary {
    pub id: u32,
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

#[derive(Debug, serde::Serialize)]
pub struct TrackPoint {
    pub timestamp: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub avg_speed_kn: Option<f64>,
    pub max_speed_kn: Option<f64>,
    pub moored: bool,
    pub engine_on: bool,
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
    pub directions: Vec<f64>,  // Wind direction angles (0, 5, 10, ..., 355)
    pub wind_distances: Vec<f64>,  // Sum of (wind_speed * time) for each direction bucket
    pub max_wind_speeds: Vec<f64>,  // Maximum wind speed for each direction bucket
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

/// Format milliseconds as human-readable duration (e.g., "1h 30m" or "45m")
fn format_duration_ms(milliseconds: u64) -> String {
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

#[derive(Debug, serde::Serialize)]
pub struct TripLegsData {
    pub legs: Vec<TripLeg>,
}

#[derive(Debug, serde::Serialize)]
pub struct HeatmapDay {
    pub date: String,  // ISO 8601 date format: YYYY-MM-DD
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
    pub fastest_1nm: Option<FastestSegment>,
    pub fastest_5nm: Option<FastestSegment>,
    pub fastest_10nm: Option<FastestSegment>,
}

impl VesselDatabase {

    pub fn fetch_trip(&self, trip_id: u32) -> Result<Option<TripSummary>, Box<dyn std::error::Error>> {
        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        let row: Option<mysql::Row> = conn.exec_first(
            r"SELECT id, description, 
                     DATE_FORMAT(start_timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as start_ts,
                     DATE_FORMAT(end_timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as end_ts,
                     total_distance_sailed, total_distance_motoring,
                     (total_distance_sailed + total_distance_motoring) as total_distance,
                     total_time_sailing, total_time_motoring, total_time_moored
              FROM trips
              WHERE id = :trip_id",
            params! {
                "trip_id" => trip_id,
            },
        ).map_err(|e| format!("Database query error: {}", e))?;
        
        if let Some(row) = row {
            let trip = TripSummary {
                id: row.get("id").unwrap_or(0),
                description: row.get::<String, _>("description").unwrap_or_default(),
                start_date: row.get::<String, _>("start_ts").unwrap_or_default(),
                end_date: row.get::<String, _>("end_ts").unwrap_or_default(),
                total_distance_nm: row.get::<f64, _>("total_distance").unwrap_or(0.0),
                total_time_ms: row.get::<i64, _>("total_time").unwrap_or(0),
                sailing_time_ms: row.get::<i64, _>("total_time_sailing").unwrap_or(0),
                motoring_time_ms: row.get::<i64, _>("total_time_motoring").unwrap_or(0),
                moored_time_ms: row.get::<i64, _>("total_time_moored").unwrap_or(0),
                sailing_distance_nm: row.get::<f64, _>("total_distance_sailed").unwrap_or(0.0),
                motoring_distance_nm: row.get::<f64, _>("total_distance_motoring").unwrap_or(0.0),
            };
            Ok(Some(trip))
        } else {
            Ok(None)
        }
    }

    /// Fetch trips with optional filtering
    pub fn fetch_trips(&self, year: Option<i32>, last_months: Option<u32>) -> Result<Vec<TripSummary>, Box<dyn std::error::Error>> {
        let mut query = String::from(
            "SELECT id, 
                    description,
                    DATE_FORMAT(start_timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as start_ts,
                    DATE_FORMAT(end_timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as end_ts,
                    (total_distance_sailed + total_distance_motoring) as total_distance,
                    (total_time_sailing + total_time_motoring + total_time_moored) as total_time,
                    total_time_sailing as total_time_sailing,
                    total_time_motoring as total_time_motoring,
                    total_time_moored as total_time_moored,
                    total_distance_sailed as total_distance_sailed,
                    total_distance_motoring as total_distance_motoring
             FROM trips WHERE "
        );

        if let Some(year) = year {
            query.push_str(&format!(" YEAR(start_timestamp) = {}", year));
        } else if let Some(months) = last_months {
            query.push_str(&format!(" start_timestamp >= DATE_SUB(NOW(), INTERVAL {} MONTH)", months));
        } else {
            query.push_str(&format!(" start_timestamp >= DATE_SUB(NOW(), INTERVAL {} MONTH)", 12)); // default last 12 months
        }

        query.push_str(" ORDER BY start_timestamp DESC");

        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        let results: Vec<mysql::Row> = conn.query(&query)
            .map_err(|e| format!("Database query error: {}", e))?;

        let trips = results
            .iter()
            .map(|row| TripSummary {
                id: row.get("id").unwrap_or(0),
                description: row.get::<String, _>("description").unwrap_or_default(),
                start_date: row.get::<String, _>("start_ts").unwrap_or_default(),
                end_date: row.get::<String, _>("end_ts").unwrap_or_default(),
                total_distance_nm: row.get::<f64, _>("total_distance").unwrap_or(0.0),
                total_time_ms: row.get::<i64, _>("total_time").unwrap_or(0),
                sailing_time_ms: row.get::<i64, _>("total_time_sailing").unwrap_or(0),
                motoring_time_ms: row.get::<i64, _>("total_time_motoring").unwrap_or(0),
                moored_time_ms: row.get::<i64, _>("total_time_moored").unwrap_or(0),
                sailing_distance_nm: row.get::<f64, _>("total_distance_sailed").unwrap_or(0.0),
                motoring_distance_nm: row.get::<f64, _>("total_distance_motoring").unwrap_or(0.0),
            })
            .collect();

        Ok(trips)
    }

    /// Fetch vessel track data by trip_id or date range
    pub fn fetch_track(&self, trip_id: Option<u32>, start: Option<&str>, end: Option<&str>) -> Result<Vec<TrackPoint>, Box<dyn std::error::Error>> {
        let query = if let Some(trip_id) = trip_id {
            // Get trip date range and fetch vessel_status data for that period
            format!(
                "SELECT DATE_FORMAT(vs.timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as timestamp,
                        vs.latitude, vs.longitude, vs.average_speed_kn, vs.max_speed_kn, 
                        vs.is_moored, vs.engine_on, vs.total_distance_nm, vs.total_time_ms,
                        vs.average_wind_speed_kn, vs.average_wind_angle_deg,
                        vs.cog_deg, vs.average_heading_deg
                 FROM vessel_status vs
                 JOIN trips t ON vs.timestamp BETWEEN t.start_timestamp AND COALESCE(t.end_timestamp, NOW())
                 WHERE t.id = {}
                 ORDER BY vs.timestamp",
                trip_id
            )
        } else if let (Some(start), Some(end)) = (start, end) {
            format!(
                "SELECT DATE_FORMAT(timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as timestamp,
                        latitude, longitude, average_speed_kn, max_speed_kn, is_moored, engine_on,
                        total_distance_nm, total_time_ms,
                        average_wind_speed_kn, average_wind_angle_deg,
                        cog_deg, average_heading_deg
                 FROM vessel_status WHERE timestamp BETWEEN '{}' AND '{}' ORDER BY timestamp",
                start, end
            )
        } else {
            return Err("Either trip_id or both start and end timestamps are required".into());
        };

        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        let results: Vec<mysql::Row> = conn.query(&query)
            .map_err(|e| format!("Database query error: {}", e))?;

        let track = results
            .iter()
            .map(|row| TrackPoint {
                timestamp: row.get::<String, _>("timestamp").unwrap_or_default(),
                latitude: self.fetch_value(row, "latitude"),
                longitude: self.fetch_value(row, "longitude"),
                avg_speed_kn: self.fetch_value(row, "average_speed_kn"),
                max_speed_kn: self.fetch_value(row, "max_speed_kn"),
                moored: row.get::<i32, _>("is_moored").unwrap_or(0) != 0,
                engine_on: row.get::<i32, _>("engine_on").unwrap_or(0) != 0,
                total_distance_nm: self.fetch_value(row, "total_distance_nm"),
                total_time_ms: row.get::<u64, _>("total_time_ms").unwrap_or(0),
                average_wind_speed_kn: self.fetch_value(row, "average_wind_speed_kn"),
                average_wind_angle_deg: self.fetch_value(row, "average_wind_angle_deg"),
                cog_deg: self.fetch_value(row, "cog_deg"),
                average_heading_deg: self.fetch_value(row, "average_heading_deg"),
            })
            .collect();

        Ok(track)
    }

    fn fetch_value(&self, row: &Row, column: &str) -> Option<f64> {
        match std::panic::catch_unwind(|| row.get::<f64, _>(column)) {
            Ok(v) => v,
            Err(_panic) => None,
        }
    }

    /// Fetch environmental metrics by metric_id with optional trip_id or date range
    pub fn fetch_metrics(&self, metric: &str, trip_id: Option<u32>, start: Option<&str>, end: Option<&str>) -> Result<Vec<WebMetricData>, Box<dyn std::error::Error>> {
        let query = if let Some(trip_id) = trip_id {
            format!(
                "SELECT DATE_FORMAT(e.timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as timestamp,
                        e.metric_id, e.value_avg, e.value_max, e.value_min 
                 FROM environmental_data e 
                 WHERE e.timestamp >= (SELECT COALESCE(start_timestamp, NOW()) FROM trips WHERE id = {}) AND e.timestamp <= (SELECT COALESCE(end_timestamp, NOW()) FROM trips WHERE id = {})
                 AND e.metric_id = '{}' 
                 ORDER BY e.timestamp",
                trip_id, trip_id, metric
            )
        } else if let (Some(start), Some(end)) = (start, end) {
            format!(
                "SELECT DATE_FORMAT(timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as timestamp,
                        metric_id, value_avg, value_max, value_min
                 FROM environmental_data 
                 WHERE metric_id = '{}' AND timestamp BETWEEN '{}' AND '{}' 
                 ORDER BY timestamp",
                metric, start, end
            )
        } else {
            return Err("Either trip_id or both start and end timestamps are required".into());
        };

        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        let results: Vec<mysql::Row> = conn.query(&query)
            .map_err(|e| format!("Database query error: {}", e))?;

        let metrics = results
            .iter()
            .map(|row| WebMetricData {
                timestamp: row.get::<String, _>("timestamp").unwrap_or_default(),
                metric_id: row.get::<String, _>("metric_id").unwrap_or_default(),
                avg_value: row.get("value_avg"),
                max_value: row.get("value_max"),
                min_value: row.get("value_min")
            })
            .collect();

        Ok(metrics)
    }

    /// Fetch speed distribution data for a trip
    pub fn fetch_speed_distribution(&self, trip_id: Option<u32>, start: Option<&str>, end: Option<&str>) -> Result<SpeedDistributionData, Box<dyn std::error::Error>> {
        // Create buckets for speeds from 0 to 10 knots in 0.5 knot increments
        let max_speed = 10.0;
        let bucket_size = 0.5;
        let num_buckets = ((max_speed / bucket_size) as f64).ceil() as usize;
        
        let mut sailing_buckets = vec![0.0; num_buckets];
        let mut motoring_buckets = vec![0.0; num_buckets];
        let mut labels = Vec::with_capacity(num_buckets);
        
        // Initialize labels
        for i in 0..num_buckets {
            let min_speed = i as f64 * bucket_size;
            let max_speed = (i + 1) as f64 * bucket_size;
            labels.push(format!("{:.1}-{:.1}", min_speed, max_speed));
        }
        
        // Build query based on parameters
        let query = if let Some(trip_id) = trip_id {
            format!(
                "SELECT vs.average_speed_kn, vs.total_distance_nm, vs.engine_on
                 FROM vessel_status vs
                 JOIN trips t ON vs.timestamp BETWEEN t.start_timestamp AND COALESCE(t.end_timestamp, NOW())
                 WHERE t.id = {}
                 ORDER BY vs.timestamp",
                trip_id
            )
        } else if let (Some(start), Some(end)) = (start, end) {
            format!(
                "SELECT vs.average_speed_kn, vs.total_distance_nm, vs.engine_on
                 FROM vessel_status vs
                 WHERE vs.timestamp BETWEEN '{}' AND '{}'
                 ORDER BY vs.timestamp",
                start, end
            )
        } else {
            return Err("Either trip_id or both start and end timestamps are required".into());
        };

        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        let results: Vec<mysql::Row> = conn.query(&query)
            .map_err(|e| format!("Database query error: {}", e))?;

        // Process each row and accumulate distances in buckets
        for row in results {
            let speed: Option<f64> = row.get("average_speed_kn");
            let distance: Option<f64> = row.get("total_distance_nm");
            let engine_on: i32 = row.get("engine_on").unwrap_or(0);
            
            if let (Some(speed), Some(distance)) = (speed, distance) {
                let bucket_index = ((speed / bucket_size).floor() as usize).min(num_buckets - 1);
                
                if engine_on != 0 {
                    motoring_buckets[bucket_index] += distance;
                } else {
                    sailing_buckets[bucket_index] += distance;
                }
            }
        }

        Ok(SpeedDistributionData {
            labels,
            sailing: sailing_buckets,
            motoring: motoring_buckets,
        })
    }

    /// Fetch wind statistics data for a trip or time range
    pub fn fetch_wind_statistics(&self, trip_id: Option<u32>, start: Option<&str>, end: Option<&str>) -> Result<WindStatisticsData, Box<dyn std::error::Error>> {
        // Create 72 buckets for wind directions (360 degrees / 5 degrees = 72 buckets)
        let bucket_size = 5.0;
        let num_buckets = 72;
        
        let mut wind_distances = vec![0.0; num_buckets];
        let mut max_wind_speeds = vec![0.0; num_buckets];
        let mut directions = Vec::with_capacity(num_buckets);
        
        // Initialize directions (0, 5, 10, ..., 355)
        for i in 0..num_buckets {
            directions.push(i as f64 * bucket_size);
        }
        
        // Build query based on parameters
        let query = if let Some(trip_id) = trip_id {
            format!(
                r"SELECT 
                    vs.average_wind_angle_deg, 
                    vs.average_wind_speed_kn,
                    vs.timestamp
                 FROM vessel_status vs
                 JOIN trips t ON vs.timestamp BETWEEN t.start_timestamp AND COALESCE(t.end_timestamp, NOW())
                 WHERE t.id = {}
                 AND vs.average_wind_angle_deg IS NOT NULL 
                 AND vs.average_wind_speed_kn IS NOT NULL
                 AND vs.is_moored = false
                 ORDER BY vs.timestamp",
                trip_id
            )
        } else if let (Some(start), Some(end)) = (start, end) {
            format!(
                r"SELECT 
                    vs.average_wind_angle_deg, 
                    vs.average_wind_speed_kn,
                    vs.timestamp
                 FROM vessel_status vs
                 WHERE vs.timestamp BETWEEN '{}' AND '{}'
                 AND vs.average_wind_angle_deg IS NOT NULL 
                 AND vs.average_wind_speed_kn IS NOT NULL
                 AND vs.is_moored = false
                 ORDER BY vs.timestamp",
                start, end
            )
        } else {
            return Err("Either trip_id or both start and end timestamps are required".into());
        };

        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        let results: Vec<mysql::Row> = conn.query(&query)
            .map_err(|e| format!("Database query error: {}", e))?;

        // Collect all data points first
        let mut data_points = Vec::new();
        for row in results {
            let wind_direction: Option<f64> = row.get("average_wind_angle_deg");
            let wind_speed: Option<f64> = row.get("average_wind_speed_kn");
            let timestamp: Option<String> = row.get("timestamp");
            
            if let (Some(direction), Some(speed), Some(ts)) = (wind_direction, wind_speed, timestamp) {
                let dt = chrono::NaiveDateTime::parse_from_str(&ts, "%Y-%m-%d %H:%M:%S%.f")
                    .or_else(|_| chrono::NaiveDateTime::parse_from_str(&ts, "%Y-%m-%d %H:%M:%S"))
                    .map_err(|e| format!("Timestamp parse error for '{}': {}", ts, e))?;
                data_points.push((direction, speed, dt));
            }
        }

        // Process consecutive data points to calculate time intervals
        for i in 0..data_points.len().saturating_sub(1) {
            let (direction, speed, curr_dt) = data_points[i];
            let (_, _, next_dt) = data_points[i + 1];
            
            let time_hours = (next_dt - curr_dt).num_seconds() as f64 / 3600.0;
            
            if time_hours > 0.0 {
                // Calculate bucket index (normalize direction to 0-359, then divide by 5)
                let normalized_direction = direction % 360.0;
                let bucket_index = ((normalized_direction / bucket_size).floor() as usize).min(num_buckets - 1);
                
                // Add wind distance (speed * time)
                let wind_distance = speed * time_hours;
                wind_distances[bucket_index] += wind_distance;
                
                // Update max wind speed for this bucket
                max_wind_speeds[bucket_index] = f64::max(max_wind_speeds[bucket_index], speed);
            }
        }

        Ok(WindStatisticsData {
            directions,
            wind_distances,
            max_wind_speeds,
        })
    }

    /// Fetch trip legs data - divides trip into legs between mooring periods
    pub fn fetch_trip_legs(&self, trip_id: u32) -> Result<TripLegsData, Box<dyn std::error::Error>> {
        let query = format!(
            r"SELECT 
                DATE_FORMAT(vs.timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as timestamp,
                vs.is_moored,
                vs.engine_on,
                vs.total_distance_nm,
                vs.total_time_ms
             FROM vessel_status vs
             JOIN trips t ON vs.timestamp BETWEEN t.start_timestamp AND COALESCE(t.end_timestamp, NOW())
             WHERE t.id = {}
             ORDER BY vs.timestamp",
            trip_id
        );

        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        let results: Vec<mysql::Row> = conn.query(&query)
            .map_err(|e| format!("Database query error: {}", e))?;

        let mut legs = Vec::new();
        let mut in_leg = false;
        let mut leg_start_timestamp = String::new();
        let mut leg_total_distance = 0.0;
        let mut leg_sailing_distance = 0.0;
        let mut leg_motoring_distance = 0.0;
        let mut leg_sailing_time = 0_u64;
        let mut leg_motoring_time = 0_u64;
        let mut leg_number = 0;

        for row in &results {
            let timestamp: String = row.get("timestamp").unwrap_or_default();
            let is_moored: bool = row.get("is_moored").unwrap_or(false);
            let engine_on: bool = row.get("engine_on").unwrap_or(false);
            let interval_distance: f64 = row.get("total_distance_nm").unwrap_or(0.0);
            let interval_time: u64 = row.get("total_time_ms").unwrap_or(0);

            if is_moored {
                // End current leg if we have one
                if in_leg && leg_total_distance >= 0.5 {
                    leg_number += 1;
                    legs.push(TripLeg {
                        leg_number,
                        start_timestamp: leg_start_timestamp.clone(),
                        end_timestamp: timestamp.clone(),
                        total_distance_nm: leg_total_distance,
                        sailing_distance_nm: leg_sailing_distance,
                        motoring_distance_nm: leg_motoring_distance,
                        sailing_time_ms: leg_sailing_time,
                        motoring_time_ms: leg_motoring_time,
                        sailing_time_formatted: format_duration_ms(leg_sailing_time),
                        motoring_time_formatted: format_duration_ms(leg_motoring_time),
                    });
                }
                
                // Reset for next leg
                in_leg = false;
                leg_total_distance = 0.0;
                leg_sailing_distance = 0.0;
                leg_motoring_distance = 0.0;
                leg_sailing_time = 0;
                leg_motoring_time = 0;
            } else {
                // Not moored - either starting or continuing a leg
                if !in_leg {
                    // Start a new leg
                    in_leg = true;
                    leg_start_timestamp = timestamp.clone();
                }
                
                // Accumulate distance and time for this interval
                leg_total_distance += interval_distance;
                
                if engine_on {
                    leg_motoring_distance += interval_distance;
                    leg_motoring_time += interval_time;
                } else {
                    leg_sailing_distance += interval_distance;
                    leg_sailing_time += interval_time;
                }
            }
        }

        // Handle last leg if trip ended while underway
        if in_leg && leg_total_distance >= 0.5 {
            leg_number += 1;
            let last_timestamp = results.last()
                .and_then(|r| r.get::<String, _>("timestamp"))
                .unwrap_or_default();
                
            legs.push(TripLeg {
                leg_number,
                start_timestamp: leg_start_timestamp,
                end_timestamp: last_timestamp,
                total_distance_nm: leg_total_distance,
                sailing_distance_nm: leg_sailing_distance,
                motoring_distance_nm: leg_motoring_distance,
                sailing_time_ms: leg_sailing_time,
                motoring_time_ms: leg_motoring_time,
                sailing_time_formatted: format_duration_ms(leg_sailing_time),
                motoring_time_formatted: format_duration_ms(leg_motoring_time),
            });
        }

        Ok(TripLegsData { legs })
    }

    /// Fetch track analytics for a time range - calculates max speed and fastest segments
    pub fn fetch_track_analytics(&self, start: &str, end: &str) -> Result<TrackAnalytics, Box<dyn std::error::Error>> {
        let query = format!(
            r"SELECT 
                DATE_FORMAT(vs.timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as timestamp,
                vs.latitude,
                vs.longitude,
                vs.average_speed_kn,
                vs.engine_on
             FROM vessel_status vs
             WHERE vs.timestamp BETWEEN '{}' AND '{}'
             AND vs.average_speed_kn IS NOT NULL
             ORDER BY vs.timestamp",
            start, end
        );

        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        let results: Vec<mysql::Row> = conn.query(&query)
            .map_err(|e| format!("Database query error: {}", e))?;

        if results.is_empty() {
            return Ok(TrackAnalytics {
                max_speed_kn: None,
                max_speed_timestamp: None,
                fastest_1nm: None,
                fastest_5nm: None,
                fastest_10nm: None,
            });
        }

        // Collect track points
        let mut track_points = Vec::new();
        for row in &results {
            let timestamp: String = row.get("timestamp").unwrap_or_default();
            let latitude: Option<f64> = row.get("latitude");
            let longitude: Option<f64> = row.get("longitude");
            let speed: Option<f64> = row.get("average_speed_kn");
            let engine_on: bool = row.get("engine_on").unwrap_or(false);

            if let (Some(lat), Some(lon), Some(spd)) = (latitude, longitude, speed) {
                track_points.push((timestamp, lat, lon, spd, engine_on));
            }
        }

        // Find max speed when sailing
        let mut max_speed = None;
        let mut max_speed_timestamp = None;
        for (timestamp, _, _, speed, engine_on) in &track_points {
            if !engine_on && (max_speed.is_none() || *speed > max_speed.unwrap()) {
                max_speed = Some(*speed);
                max_speed_timestamp = Some(timestamp.clone());
            }
        }

        // Calculate fastest segments for 1NM, 5NM, and 10NM
        let fastest_1nm = find_fastest_segment(&track_points, 1.0);
        let fastest_5nm = find_fastest_segment(&track_points, 5.0);
        let fastest_10nm = find_fastest_segment(&track_points, 10.0);

        Ok(TrackAnalytics {
            max_speed_kn: max_speed,
            max_speed_timestamp,
            fastest_1nm,
            fastest_5nm,
            fastest_10nm,
        })
    }

    /// Fetch heatmap data - distance traveled grouped by day for 365 days before the given date
    pub fn fetch_heatmap(&self, end_date: &str) -> Result<HeatmapData, Box<dyn std::error::Error>> {
        // Parse the end date and calculate start date (365 days before)
        let end_dt = chrono::NaiveDate::parse_from_str(end_date, "%Y-%m-%d")?;
        let start_dt = end_dt - chrono::Duration::days(365);
        
        let query = format!(
            r"SELECT DATE(vs.timestamp) as day, COALESCE(SUM(COALESCE(vs.total_distance_nm, 0)), 0) as total_distance
             FROM vessel_status vs
             WHERE DATE(vs.timestamp) BETWEEN '{}' AND '{}'
             GROUP BY DATE(vs.timestamp)
             ORDER BY vs.timestamp",
            start_dt, end_dt
        );

        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        let results: Vec<mysql::Row> = conn.query(&query)
            .map_err(|e| format!("Database query error: {}", e))?;

        let mut days = Vec::new();
        let mut min_distance: f64 = f64::MAX;
        let mut max_distance: f64 = 0.0;
        let mut total_distance: f64 = 0.0;

        for row in results {
            let date: String = row.get("day").unwrap_or_default();
            let distance: f64 = row.get("total_distance").unwrap_or(0.0);
            
            days.push(HeatmapDay {
                date,
                distance_nm: distance,
            });
            
            total_distance += distance;
            if distance > 0.0 {
                min_distance = min_distance.min(distance);
                max_distance = max_distance.max(distance);
            }
        }

        // If no days with distance data, set min_distance to 0
        if min_distance == f64::MAX {
            min_distance = 0.0;
        }

        Ok(HeatmapData {
            days,
            min_distance,
            max_distance,
            total_distance,
        })
    }
}

/// Helper function to find fastest segment for a given target distance
fn find_fastest_segment(
    track_points: &[(String, f64, f64, f64, bool)],
    target_distance_nm: f64,
) -> Option<FastestSegment> {
    if track_points.len() < 2 {
        return None;
    }

    let mut fastest: Option<FastestSegment> = None;

    // Use sliding window approach
    for start_idx in 0..track_points.len() {
        let (start_ts, _start_lat, _start_lon, _, start_engine) = &track_points[start_idx];
        
        // Skip if motoring
        if *start_engine {
            continue;
        }

        let mut cumulative_distance = 0.0;

        for end_idx in (start_idx + 1)..track_points.len() {
            let (end_ts, end_lat, end_lon, _, end_engine) = &track_points[end_idx];
            
            // Check if entire segment is sailing
            if *end_engine {
                break;
            }

            // Calculate distance between consecutive points
            let prev_idx = end_idx - 1;
            let (_, prev_lat, prev_lon, _, _) = &track_points[prev_idx];
            let segment_dist = haversine_distance(*prev_lat, *prev_lon, *end_lat, *end_lon);
            cumulative_distance += segment_dist;

            // Check if we've reached or exceeded target distance
            if cumulative_distance >= target_distance_nm {
                // Calculate duration
                let start_time = match chrono::NaiveDateTime::parse_from_str(
                    &start_ts.replace('Z', ""),
                    "%Y-%m-%dT%H:%M:%S%.f"
                ) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                let end_time = match chrono::NaiveDateTime::parse_from_str(
                    &end_ts.replace('Z', ""),
                    "%Y-%m-%dT%H:%M:%S%.f"
                ) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                let duration_ms = (end_time - start_time).num_milliseconds() as u64;

                if duration_ms > 0 {
                    let avg_speed = cumulative_distance / (duration_ms as f64 / 1000.0 / 3600.0);
                    
                    // Check if this is the fastest so far
                    if fastest.is_none() || avg_speed > fastest.as_ref().unwrap().average_speed_kn {
                        fastest = Some(FastestSegment {
                            distance_nm: cumulative_distance,
                            average_speed_kn: avg_speed,
                            duration_ms,
                            start_timestamp: start_ts.clone(),
                            end_timestamp: end_ts.clone(),
                        });
                    }
                }
                break;
            }
        }
    }

    fastest
}

/// Calculate haversine distance between two points in nautical miles
fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 3440.065; // Earth radius in nautical miles
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();

    let a = (d_lat / 2.0).sin().powi(2)
        + lat1_rad.cos() * lat2_rad.cos() * (d_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

    r * c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_track() {
        let db = VesselDatabase::new("mysql://nmea:nmea@localhost:3306/test_nmea_router").unwrap();
        let points = db.fetch_track(Some(132), None, None).unwrap();
        assert!(!points.is_empty());
        // Debug output removed - test verifies data retrieval works
    }
}