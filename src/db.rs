use mysql::*;
use mysql::prelude::*;
use std::error::Error;
use crate::vessel_monitor::VesselStatus;
use crate::environmental_monitor::{EnvironmentalReport, MetricId};

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
    pub fn insert_status(&self, status: &VesselStatus) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        
        // Get current system time and convert to UTC
        let now = std::time::SystemTime::now();
        let timestamp = chrono::DateTime::<chrono::Utc>::from(now);
        
        let (latitude, longitude) = if let Some(pos) = &status.current_position {
            (Some(pos.latitude), Some(pos.longitude))
        } else {
            (None, None)
        };
        
        conn.exec_drop(
            r"INSERT INTO vessel_status 
              (timestamp, latitude, longitude, average_speed_ms, max_speed_ms, is_moored, engine_on)
              VALUES (:timestamp, :latitude, :longitude, :avg_speed, :max_speed, :is_moored, :engine_on)",
            params! {
                "timestamp" => timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                "latitude" => latitude,
                "longitude" => longitude,
                "avg_speed" => status.average_speed_30s,
                "max_speed" => status.max_speed_30s,
                "is_moored" => status.is_moored,
                "engine_on" => status.engine_on,
            },
        )?;
        
        Ok(())
    }
        
    /// Insert only specific environmental metrics into the database
    /// This allows for adaptive persistence intervals per metric
    pub fn insert_environmental_metrics(
        &self, 
        report: &EnvironmentalReport, 
        metrics_to_persist: &[MetricId]
    ) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        
        // Get current system time and convert to UTC
        let now = std::time::SystemTime::now();
        let timestamp = chrono::DateTime::<chrono::Utc>::from(now);
        let timestamp_str = timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        
        // Build mapping of all metrics
        let all_metrics = [
            (MetricId::Pressure, &report.pressure),
            (MetricId::CabinTemp, &report.cabin_temp),
            (MetricId::WaterTemp, &report.water_temp),
            (MetricId::Humidity, &report.humidity),
            (MetricId::WindSpeed, &report.wind_speed),
            (MetricId::WindDir, &report.wind_dir),
            (MetricId::Roll, &report.roll),
        ];
        
        // Insert only the specified metrics
        for (metric_id, data) in all_metrics.iter() {
            if metrics_to_persist.contains(metric_id) {
                // Only insert if we have data for this metric
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
            }
        }
        
        Ok(())
    }
}
