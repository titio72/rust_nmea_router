use mysql::*;
use mysql::prelude::*;
use std::{error::Error, time::{Duration, Instant}};
use std::time::{SystemTime};
use crate::{environmental_monitor::{MetricData, MetricId}, utilities::dirty_instant_to_systemtime};
use crate::trip::Trip;
use chrono::NaiveDateTime;
use tracing::{info, warn};

/// Encapsulates vessel status data for database insertion
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
        let pool = Pool::new(opts)?;
        
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
        status_op: VesselStatusOperation,
        trip_operation: TripOperation,
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
                     DATE_FORMAT(start_timestamp, '%Y-%m-%d %H:%i:%S.%f') as start_ts,
                     DATE_FORMAT(end_timestamp, '%Y-%m-%d %H:%i:%S.%f') as end_ts,
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
            
            // Parse timestamps
            let start_dt = NaiveDateTime::parse_from_str(&start_ts, "%Y-%m-%d %H:%M:%S%.6f")?;
            let end_dt = NaiveDateTime::parse_from_str(&end_ts, "%Y-%m-%d %H:%M:%S%.6f")?;
            
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
    pub latitude: f64,
    pub longitude: f64,
    pub avg_speed_kn: f64,
    pub max_speed_kn: f64,
    pub moored: bool,
    pub engine_on: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct WebMetricData {
    pub timestamp: String,
    pub metric_id: String,
    pub avg_value: Option<f64>,
    pub max_value: Option<f64>,
    pub min_value: Option<f64>,
    pub count: Option<u32>,
}

impl VesselDatabase {

    pub fn fetch_trip(&self, trip_id: u32) -> Result<Option<TripSummary>, Box<dyn std::error::Error>> {
        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        let row: Option<mysql::Row> = conn.exec_first(
            r"SELECT id, description, 
                     DATE_FORMAT(start_timestamp, '%Y-%m-%d %H:%i:%S.%f') as start_ts,
                     DATE_FORMAT(end_timestamp, '%Y-%m-%d %H:%i:%S.%f') as end_ts,
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
                    DATE_FORMAT(start_timestamp, '%Y-%m-%d %H:%i:%S') as start_ts,
                    DATE_FORMAT(end_timestamp, '%Y-%m-%d %H:%i:%S') as end_ts,
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
                "SELECT DATE_FORMAT(vs.timestamp, '%Y-%m-%d %H:%i:%S') as timestamp,
                        vs.latitude, vs.longitude, vs.average_speed_kn, vs.max_speed_kn, 
                        vs.is_moored, vs.engine_on 
                 FROM vessel_status vs
                 JOIN trips t ON vs.timestamp BETWEEN t.start_timestamp AND COALESCE(t.end_timestamp, NOW())
                 WHERE t.id = {}
                 ORDER BY vs.timestamp",
                trip_id
            )
        } else if let (Some(start), Some(end)) = (start, end) {
            format!(
                "SELECT DATE_FORMAT(timestamp, '%Y-%m-%d %H:%i:%S') as timestamp,
                        latitude, longitude, average_speed_kn, max_speed_kn, is_moored, engine_on 
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
                latitude: row.get::<f64, _>("latitude").unwrap_or(0.0),
                longitude: row.get::<f64, _>("longitude").unwrap_or(0.0),
                avg_speed_kn: row.get::<f64, _>("average_speed_kn").unwrap_or(0.0),
                max_speed_kn: row.get::<f64, _>("max_speed_kn").unwrap_or(0.0),
                moored: row.get::<i32, _>("is_moored").unwrap_or(0) != 0,
                engine_on: row.get::<i32, _>("engine_on").unwrap_or(0) != 0,
            })
            .collect();

        Ok(track)
    }

    /// Fetch environmental metrics by metric_id with optional trip_id or date range
    pub fn fetch_metrics(&self, metric: &str, trip_id: Option<u32>, start: Option<&str>, end: Option<&str>) -> Result<Vec<WebMetricData>, Box<dyn std::error::Error>> {
        let query = if let Some(trip_id) = trip_id {
            format!(
                "SELECT DATE_FORMAT(e.timestamp, '%Y-%m-%d %H:%i:%S') as timestamp,
                        e.metric_id, e.avg_value, e.max_value, e.min_value, e.count 
                 FROM environmental_data e 
                 JOIN vessel_status v ON DATE(e.timestamp) = DATE(v.timestamp) 
                 WHERE v.trip_id = {} AND e.metric_id = '{}' 
                 ORDER BY e.timestamp",
                trip_id, metric
            )
        } else if let (Some(start), Some(end)) = (start, end) {
            format!(
                "SELECT DATE_FORMAT(timestamp, '%Y-%m-%d %H:%i:%S') as timestamp,
                        metric_id, avg_value, max_value, min_value, count 
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
                avg_value: row.get("avg_value"),
                max_value: row.get("max_value"),
                min_value: row.get("min_value"),
                count: row.get("count"),
            })
            .collect();

        Ok(metrics)
    }
}
