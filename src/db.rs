use mysql::*;
use mysql::prelude::*;
use std::{error::Error, time::Instant};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::environmental_monitor::{MetricData, MetricId};

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
    ///     total_distance_m DOUBLE NOT NULL DEFAULT 0,
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
    pub fn insert_status(&self, time: Instant,
        latitude: f64, longitude: f64, average_speed: f64, max_speed: f64, is_moored: bool, engine_on: bool, total_distance_m: f64, total_time_ms: u64) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;

        // Ugly workaround to convert Instant to SystemTime
        let delta = Instant::now().duration_since(time);
        let system_time = SystemTime::now().checked_sub(delta).unwrap_or(UNIX_EPOCH);
        // Get current system time and convert to UTC
        let timestamp = chrono::DateTime::<chrono::Utc>::from(system_time);
               
        conn.exec_drop(
            r"INSERT INTO vessel_status 
              (timestamp, latitude, longitude, average_speed_ms, max_speed_ms, is_moored, engine_on, total_distance_m, total_time_ms)
              VALUES (:timestamp, :latitude, :longitude, :avg_speed, :max_speed, :is_moored, :engine_on, :total_distance, :total_time)",
            params! {
                "timestamp" => timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                "latitude" => latitude,
                "longitude" => longitude,
                "avg_speed" => average_speed,
                "max_speed" => max_speed,
                "is_moored" => is_moored,
                "engine_on" => engine_on,
                "total_distance" => total_distance_m,
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
}
