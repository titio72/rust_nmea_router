use mysql::*;
use mysql::prelude::*;
use std::{error::Error, time::Instant};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::environmental_monitor::{MetricData, MetricId};
use crate::trip::Trip;
use chrono::NaiveDateTime;

pub struct VesselDatabase {
    pool: Pool,
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
    ///     average_speed_ms DOUBLE NOT NULL,
    ///     max_speed_ms DOUBLE NOT NULL,
    ///     is_moored BOOLEAN NOT NULL,
    ///     engine_on BOOLEAN NOT NULL DEFAULT 0,
    ///     total_distance_nm DOUBLE NOT NULL DEFAULT 0,
    ///     total_time_ms BIGINT NOT NULL DEFAULT 0,
    ///     INDEX idx_timestamp (timestamp)
    /// );
    /// ```
    pub fn new(connection_url: &str) -> Result<Self, Box<dyn Error>> {
        let opts = Opts::from_url(connection_url)?;
        let pool = Pool::new(opts)?;
        
        Ok(VesselDatabase { pool })
    }
    
    /// Insert a vessel status report into the database
    /// All timestamps are stored in UTC timezone
    /// Distances are in nautical miles
    pub fn insert_status(&self, time: Instant,
        latitude: f64, longitude: f64, average_speed: f64, max_speed: f64, is_moored: bool, engine_on: bool, total_distance_nm: f64, total_time_ms: u64) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;

        // Ugly workaround to convert Instant to SystemTime
        let delta = Instant::now().duration_since(time);
        let system_time = SystemTime::now().checked_sub(delta).unwrap_or(UNIX_EPOCH);
        // Get current system time and convert to UTC
        let timestamp = chrono::DateTime::<chrono::Utc>::from(system_time);
               
        conn.exec_drop(
            r"INSERT INTO vessel_status 
              (timestamp, latitude, longitude, average_speed_ms, max_speed_ms, is_moored, engine_on, total_distance_nm, total_time_ms)
              VALUES (:timestamp, :latitude, :longitude, :avg_speed, :max_speed, :is_moored, :engine_on, :total_distance, :total_time)",
            params! {
                "timestamp" => timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                "latitude" => latitude,
                "longitude" => longitude,
                "avg_speed" => average_speed,
                "max_speed" => max_speed,
                "is_moored" => is_moored,
                "engine_on" => engine_on,
                "total_distance" => total_distance_nm,
                "total_time" => total_time_ms,
            },
        )?;
        
        Ok(())
    }
        
    /// Insert only specific environmental metrics into the database
    /// This allows for adaptive persistence intervals per metric
    pub fn insert_environmental_metrics(
        &self, 
        data: &MetricData, 
        metric_id: MetricId
    ) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        
        // Get current system time and convert to UTC
        let now = std::time::SystemTime::now();
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
            let start_system = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(start_dt, chrono::Utc);
            let end_system = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(end_dt, chrono::Utc);
            
            let now_system = SystemTime::now();
            let now_instant = Instant::now();
            
            // Calculate duration from end_timestamp to now
            let duration_since_end = now_system.duration_since(SystemTime::UNIX_EPOCH)?
                .saturating_sub(std::time::Duration::from_secs(end_system.timestamp() as u64));
            
            let duration_since_start = now_system.duration_since(SystemTime::UNIX_EPOCH)?
                .saturating_sub(std::time::Duration::from_secs(start_system.timestamp() as u64));
            
            // Reconstruct Instant by subtracting from now
            let end_instant = now_instant.checked_sub(duration_since_end).unwrap_or(now_instant);
            let start_instant = now_instant.checked_sub(duration_since_start).unwrap_or(now_instant);
            
            Ok(Some(Trip {
                id: Some(id),
                description,
                start_timestamp: start_instant,
                end_timestamp: end_instant,
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
    
    /// Insert a new trip into the database
    pub fn insert_trip(&self, trip: &Trip) -> Result<i64, Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        
        // Convert Instant to SystemTime
        let delta_start = Instant::now().duration_since(trip.start_timestamp);
        let delta_end = Instant::now().duration_since(trip.end_timestamp);
        
        let start_system = SystemTime::now().checked_sub(delta_start).unwrap_or(UNIX_EPOCH);
        let end_system = SystemTime::now().checked_sub(delta_end).unwrap_or(UNIX_EPOCH);
        
        let start_timestamp = chrono::DateTime::<chrono::Utc>::from(start_system);
        let end_timestamp = chrono::DateTime::<chrono::Utc>::from(end_system);
        
        conn.exec_drop(
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
        
        Ok(conn.last_insert_id() as i64)
    }
    
    /// Update an existing trip in the database
    pub fn update_trip(&self, trip: &Trip) -> Result<(), Box<dyn Error>> {
        if trip.id.is_none() {
            return Err("Cannot update trip without id".into());
        }
        
        let mut conn = self.pool.get_conn()?;
        
        // Convert Instant to SystemTime
        let delta_end = Instant::now().duration_since(trip.end_timestamp);
        let end_system = SystemTime::now().checked_sub(delta_end).unwrap_or(UNIX_EPOCH);
        let end_timestamp = chrono::DateTime::<chrono::Utc>::from(end_system);
        
        conn.exec_drop(
            r"UPDATE trips 
              SET end_timestamp = :end_ts,
                  total_distance_sailed = :distance_sailed,
                  total_distance_motoring = :distance_motoring,
                  total_time_sailing = :time_sailing,
                  total_time_motoring = :time_motoring,
                  total_time_moored = :time_moored
              WHERE id = :id",
            params! {
                "id" => trip.id.unwrap(),
                "end_ts" => end_timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                "distance_sailed" => trip.total_distance_sailed,
                "distance_motoring" => trip.total_distance_motoring,
                "time_sailing" => trip.total_time_sailing,
                "time_motoring" => trip.total_time_motoring,
                "time_moored" => trip.total_time_moored,
            },
        )?;
        
        Ok(())
    }
}
