use mysql::*;
use mysql::prelude::*;
use std::{error::Error, time::{Duration, Instant}, sync::{Arc, Mutex}};
use std::time::{SystemTime};
use crate::{environmental_monitor::{MetricData, MetricId}, position_utils::Position, utilities::{dirty_instant_to_systemtime, haversine_distance_nm}};
use crate::trip::Trip;
use chrono::NaiveDateTime;
use tracing::{info, warn};

/// Encapsulates vessel status data for database insertion
#[derive(Debug, Clone)]
pub struct VesselStatusOperation {
    pub timestamp: Instant,
    pub position: Position,
    pub average_speed_kn: f64,
    pub max_speed_kn: f64,
    pub is_moored: bool,
    pub engine_on: bool,
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

/*
pub struct VesselStatus {
    pub timestamp: Instant,
    pub current_position: Position,
    pub median_position: Option<Position>,
    pub number_of_samples: usize,
    pub max_speed_kn: f64,       // Knots
    pub is_moored: bool,
    pub engine_on: bool,
    pub wind_speed_kn: Option<f64>,
    #[allow(dead_code)] // Used in database writes but not in internal logic
    pub max_wind_speed_kn: Option<f64>,
    pub wind_speed_variance: Option<f64>,
    pub wind_angle_deg: Option<f64>,
    pub wind_angle_variance: Option<f64>,
    pub average_heading_deg: Option<f64>,
    pub period: Duration,
}

*/


/// Represents a trip operation to be performed atomically with vessel status insert
pub enum TripOperation {
    CreateTrip(Trip),
    UpdateTrip(Trip),
    None,
}

#[derive(Clone)]
pub struct VesselDatabase {
    pub pool: Pool,
    system_status_cache: Arc<Mutex<std::collections::HashMap<String, bool>>>,
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
        
        let db = VesselDatabase { 
            pool, 
            system_status_cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };
        
        // Load cache from database (non-fatal if table doesn't exist)
        if let Ok(mut conn) = db.pool.get_conn() {
            if let Ok(rows) = conn.query::<(String, String), _>("SELECT status_key, status_value FROM system_status") {
                let mut cache = db.system_status_cache.lock().unwrap();
                for (key, value) in rows {
                    let enabled = value == "1" || value.to_lowercase() == "true";
                    cache.insert(key, enabled);
                }
            }
            // If table doesn't exist, that's okay - cache will start empty and defaults to true
        }

        Ok(db)
    }
    
    /// Check database connection health using a simple query
    /// Returns Ok(()) if the connection is healthy, Err otherwise
    pub fn health_check(&self) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        conn.query_drop("SELECT 1")?;
        Ok(())
    }
    
    /// Export a trip and its associated data to a portable SQL script
    /// 
    /// This function creates a SQL script containing:
    /// 1. The trip record
    /// 2. All vessel status records within the trip's time range
    /// 3. All environmental metrics records within the trip's time range
    /// 
    /// Before exporting, it checks if there's an overlapping trip in the database
    /// and fails if one is found.
    /// 
    /// # Arguments
    /// * `trip_id` - The ID of the trip to export
    /// * `output_path` - Path where the SQL script will be written
    /// 
    /// # Returns
    /// Result indicating success or error
    #[allow(dead_code)]
    pub fn export_trip<P: AsRef<std::path::Path>>(&self, trip_id: i64, output_path: P) -> Result<(), Box<dyn Error>> {
        use serde_json::{json, Value};
        use std::fs::File;
        
        let mut conn = self.pool.get_conn()?;
        
        // Step 1: Fetch the trip to export with formatted timestamps
        let trip_row: Option<mysql::Row> = conn.exec_first(
            "SELECT id, 
                    DATE_FORMAT(start_timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as start_ts,
                    DATE_FORMAT(end_timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as end_ts,
                    description, total_distance_sailed, 
                    total_distance_motoring, total_time_sailing, total_time_motoring, 
                    total_time_moored, 
                    start_timestamp as start_ts_raw, end_timestamp as end_ts_raw
             FROM trips WHERE id = :id",
            params! { "id" => trip_id },
        )?;
        
        let trip_row = trip_row.ok_or("Trip not found")?;
        let trip_id_fetched: i64 = trip_row.get(0).ok_or("Missing id")?;
        let start_ts_str: String = trip_row.get(1).ok_or("Missing start_timestamp")?;
        let end_ts_str: String = trip_row.get(2).ok_or("Missing end_timestamp")?;
        let description: String = trip_row.get(3).ok_or("Missing description")?;
        let total_distance_sailed: f64 = trip_row.get(4).ok_or("Missing total_distance_sailed")?;
        let total_distance_motoring: f64 = trip_row.get(5).ok_or("Missing total_distance_motoring")?;
        let total_time_sailing: u64 = trip_row.get(6).ok_or("Missing total_time_sailing")?;
        let total_time_motoring: u64 = trip_row.get(7).ok_or("Missing total_time_motoring")?;
        let total_time_moored: u64 = trip_row.get(8).ok_or("Missing total_time_moored")?;
        let start_ts: mysql::Value = trip_row.get(9).ok_or("Missing start_timestamp_raw")?;
        let end_ts: mysql::Value = trip_row.get(10).ok_or("Missing end_timestamp_raw")?;

        // Step 2: Fetch vessel status records within time range with formatted timestamps
        let vessel_statuses: Vec<mysql::Row> = conn.exec(
            "SELECT DATE_FORMAT(timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as ts_formatted, 
                    latitude, longitude, average_speed_kn, max_speed_kn, 
                    is_moored, engine_on, total_distance_nm, total_time_ms, 
                    average_wind_speed_kn, average_wind_angle_deg, cog_deg, 
                    average_heading_deg 
             FROM vessel_status 
             WHERE timestamp >= :start_ts AND timestamp <= :end_ts
             ORDER BY timestamp ASC",
            params! {
                "start_ts" => &start_ts,
                "end_ts" => &end_ts,
            },
        )?;
        
        // Step 3: Fetch environmental metrics within time range with formatted timestamps
        let env_metrics: Vec<mysql::Row> = conn.exec(
            "SELECT DATE_FORMAT(timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as ts_formatted, 
                    metric_id, value_avg, value_max, value_min, unit 
             FROM environmental_data 
             WHERE timestamp >= :start_ts AND timestamp <= :end_ts
             ORDER BY timestamp ASC",
            params! {
                "start_ts" => &start_ts,
                "end_ts" => &end_ts,
            },
        )?;
        
        // Step 4: Build JSON structure
        let mut vessel_statuses_json: Vec<Value> = Vec::new();
        for row in vessel_statuses {
            let timestamp: String = row.get(0).ok_or("Missing timestamp")?;
            let latitude: Option<f64> = row.get_opt(1).and_then(|v| v.ok()).flatten();
            let longitude: Option<f64> = row.get_opt(2).and_then(|v| v.ok()).flatten();
            let avg_speed: Option<f64> = row.get_opt(3).and_then(|v| v.ok()).flatten();
            let max_speed: Option<f64> = row.get_opt(4).and_then(|v| v.ok()).flatten();
            let is_moored: bool = row.get(5).ok_or("Missing is_moored")?;
            let engine_on: bool = row.get(6).ok_or("Missing engine_on")?;
            let total_dist: Option<f64> = row.get_opt(7).and_then(|v| v.ok()).flatten();
            let total_time: Option<u64> = row.get_opt(8).and_then(|v| v.ok()).flatten();
            let wind_speed: Option<f64> = row.get_opt(9).and_then(|v| v.ok()).flatten();
            let wind_angle: Option<f64> = row.get_opt(10).and_then(|v| v.ok()).flatten();
            let cog: Option<f64> = row.get_opt(11).and_then(|v| v.ok()).flatten();
            let heading: Option<f64> = row.get_opt(12).and_then(|v| v.ok()).flatten();
            
            vessel_statuses_json.push(json!({
                "timestamp": timestamp,
                "latitude": latitude,
                "longitude": longitude,
                "average_speed_kn": avg_speed,
                "max_speed_kn": max_speed,
                "is_moored": is_moored,
                "engine_on": engine_on,
                "total_distance_nm": total_dist,
                "total_time_ms": total_time,
                "average_wind_speed_kn": wind_speed,
                "average_wind_angle_deg": wind_angle,
                "cog_deg": cog,
                "average_heading_deg": heading,
            }));
        }
        
        let mut env_metrics_json: Vec<Value> = Vec::new();
        for row in env_metrics {
            let timestamp: String = row.get(0).ok_or("Missing timestamp")?;
            let metric_id: u8 = row.get(1).ok_or("Missing metric_id")?;
            let value_avg: Option<f32> = row.get_opt(2).and_then(|v| v.ok()).flatten();
            let value_max: Option<f32> = row.get_opt(3).and_then(|v| v.ok()).flatten();
            let value_min: Option<f32> = row.get_opt(4).and_then(|v| v.ok()).flatten();
            let unit: Option<String> = row.get_opt(5).and_then(|v| v.ok()).flatten();
            
            env_metrics_json.push(json!({
                "timestamp": timestamp,
                "metric_id": metric_id,
                "value_avg": value_avg,
                "value_max": value_max,
                "value_min": value_min,
                "unit": unit,
            }));
        }
        
        let export_data = json!({
            "trip": {
                "id": trip_id_fetched,
                "description": description,
                "start_timestamp": start_ts_str,
                "end_timestamp": end_ts_str,
                "total_distance_sailed": total_distance_sailed,
                "total_distance_motoring": total_distance_motoring,
                "total_time_sailing": total_time_sailing,
                "total_time_motoring": total_time_motoring,
                "total_time_moored": total_time_moored,
            },
            "vessel_statuses": vessel_statuses_json,
            "environmental_metrics": env_metrics_json,
            "export_metadata": {
                "generated_at": chrono::Local::now().to_rfc3339(),
                "trip_id": trip_id,
            }
        });
        
        // Create parent directories if they don't exist
        if let Some(parent) = std::path::Path::new(output_path.as_ref()).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        
        let file = File::create(&output_path)?;
        serde_json::to_writer_pretty(file, &export_data)?;
        
        info!("Trip {} exported successfully to: {}", trip_id, output_path.as_ref().display());
        Ok(())
    }

    pub fn import_trip(&self, json_data: &str) -> Result<i64, Box<dyn Error>> {
        use serde_json::Value;
        
        let json: Value = serde_json::from_str(json_data)?;
        
        // Parse trip data
        let trip = &json["trip"];
        let vessel_statuses = &json["vessel_statuses"];
        let env_metrics = &json["environmental_metrics"];
        
        // Extract trip fields
        let description = trip["description"]
            .as_str()
            .ok_or("Missing or invalid trip.description")?;
        let start_ts_str = trip["start_timestamp"]
            .as_str()
            .ok_or("Missing or invalid trip.start_timestamp")?;
        let end_ts_str = trip["end_timestamp"]
            .as_str()
            .ok_or("Missing or invalid trip.end_timestamp")?;
        let total_distance_sailed = trip["total_distance_sailed"]
            .as_f64()
            .ok_or("Missing or invalid trip.total_distance_sailed")?;
        let total_distance_motoring = trip["total_distance_motoring"]
            .as_f64()
            .ok_or("Missing or invalid trip.total_distance_motoring")?;
        let total_time_sailing = trip["total_time_sailing"]
            .as_u64()
            .ok_or("Missing or invalid trip.total_time_sailing")?;
        let total_time_motoring = trip["total_time_motoring"]
            .as_u64()
            .ok_or("Missing or invalid trip.total_time_motoring")?;
        let total_time_moored = trip["total_time_moored"]
            .as_u64()
            .ok_or("Missing or invalid trip.total_time_moored")?;
        
        let mut conn = self.pool.get_conn()?;
        
        // Check for overlapping trips
        // Parse the start_timestamp to compare with existing trips' end_timestamps
        let new_trip_start = chrono::DateTime::parse_from_rfc3339(start_ts_str)
            .map_err(|e| format!("Invalid start_timestamp format: {}", e))?;
        
        let overlapping_trip: Option<(i64, String)> = conn.exec_first(
            "SELECT id, end_timestamp FROM trips WHERE end_timestamp >= :new_start ORDER BY end_timestamp DESC LIMIT 1",
            mysql::params! {
                "new_start" => new_trip_start.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            },
        )?;
        
        if let Some((existing_id, existing_end_ts)) = overlapping_trip {
            return Err(format!(
                "Trip overlaps with existing trip ID {}. Existing trip ends at {}, new trip starts at {}",
                existing_id, existing_end_ts, start_ts_str
            ).into());
        }
        
        // Start transaction for atomic insert
        let mut tx = conn.start_transaction(TxOpts::default())?;
        
        // Insert trip
        tx.exec_drop(
            "INSERT INTO trips (description, start_timestamp, end_timestamp, total_distance_sailed, total_distance_motoring, total_time_sailing, total_time_motoring, total_time_moored)
             VALUES (:desc, :start_ts, :end_ts, :dist_sailed, :dist_motoring, :time_sailing, :time_motoring, :time_moored)",
            mysql::params! {
                "desc" => description,
                "start_ts" => new_trip_start.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                "end_ts" => chrono::DateTime::parse_from_rfc3339(end_ts_str)?
                    .format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                "dist_sailed" => total_distance_sailed,
                "dist_motoring" => total_distance_motoring,
                "time_sailing" => total_time_sailing,
                "time_motoring" => total_time_motoring,
                "time_moored" => total_time_moored,
            },
        )?;
        
        let new_trip_id = tx.last_insert_id().ok_or("Failed to get inserted trip ID")? as i64;
        
        // Insert vessel statuses
        if let Some(statuses) = vessel_statuses.as_array() {
            for status in statuses {
                let timestamp = status["timestamp"]
                    .as_str()
                    .ok_or("Missing timestamp in vessel_status")?;
                let latitude = status["latitude"].as_f64();
                let longitude = status["longitude"].as_f64();
                let avg_speed = status["average_speed_kn"].as_f64();
                let max_speed = status["max_speed_kn"].as_f64();
                let is_moored = status["is_moored"]
                    .as_bool()
                    .ok_or("Missing is_moored in vessel_status")?;
                let engine_on = status["engine_on"]
                    .as_bool()
                    .ok_or("Missing engine_on in vessel_status")?;
                let total_dist = status["total_distance_nm"].as_f64();
                let total_time = status["total_time_ms"].as_u64();
                let wind_speed = status["average_wind_speed_kn"].as_f64();
                let wind_angle = status["average_wind_angle_deg"].as_f64();
                let cog = status["cog_deg"].as_f64();
                let heading = status["average_heading_deg"].as_f64();
                
                let ts_datetime = chrono::DateTime::parse_from_rfc3339(timestamp)?
                    .format("%Y-%m-%d %H:%M:%S%.3f").to_string();
                
                tx.exec_drop(
                    "INSERT INTO vessel_status (timestamp, latitude, longitude, average_speed_kn, max_speed_kn, is_moored, engine_on, total_distance_nm, total_time_ms, average_wind_speed_kn, average_wind_angle_deg, cog_deg, average_heading_deg)
                     VALUES (:ts, :lat, :lon, :avg_spd, :max_spd, :moored, :engine, :tot_dist, :tot_time, :wind_spd, :wind_ang, :cog, :hdg)",
                    mysql::params! {
                        "ts" => ts_datetime,
                        "lat" => latitude,
                        "lon" => longitude,
                        "avg_spd" => avg_speed,
                        "max_spd" => max_speed,
                        "moored" => is_moored,
                        "engine" => engine_on,
                        "tot_dist" => total_dist,
                        "tot_time" => total_time,
                        "wind_spd" => wind_speed,
                        "wind_ang" => wind_angle,
                        "cog" => cog,
                        "hdg" => heading,
                    },
                )?;
            }
        }
        
        // Insert environmental metrics
        if let Some(metrics) = env_metrics.as_array() {
            for metric in metrics {
                let timestamp = metric["timestamp"]
                    .as_str()
                    .ok_or("Missing timestamp in environmental_data")?;
                let metric_id = metric["metric_id"]
                    .as_u64()
                    .ok_or("Missing metric_id in environmental_data")? as u8;
                let value_avg = metric["value_avg"].as_f64().map(|v| v as f32);
                let value_max = metric["value_max"].as_f64().map(|v| v as f32);
                let value_min = metric["value_min"].as_f64().map(|v| v as f32);
                let unit = metric["unit"].as_str();
                
                let ts_datetime = chrono::DateTime::parse_from_rfc3339(timestamp)?
                    .format("%Y-%m-%d %H:%M:%S%.3f").to_string();
                
                tx.exec_drop(
                    "INSERT INTO environmental_data (timestamp, metric_id, value_avg, value_max, value_min, unit)
                     VALUES (:ts, :metric_id, :val_avg, :val_max, :val_min, :unit)",
                    mysql::params! {
                        "ts" => ts_datetime,
                        "metric_id" => metric_id,
                        "val_avg" => value_avg,
                        "val_max" => value_max,
                        "val_min" => value_min,
                        "unit" => unit,
                    },
                )?;
            }
        }
        
        tx.commit()?;
        
        info!("Trip imported successfully with ID: {}", new_trip_id);
        Ok(new_trip_id)
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

    /// Delete a trip and all associated data
    /// This will delete environmental data, vessel status data, and finally the trip record
    pub fn delete_trip(&self, trip_id: u32) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;

        // First, fetch the trip to get its time range
        let trip_row: Option<mysql::Row> = conn.exec_first(
            r"SELECT start_timestamp, end_timestamp FROM trips WHERE id = :trip_id",
            mysql::params! {
                "trip_id" => trip_id,
            },
        ).map_err(|e| format!("Database query error: {}", e))?;

        if trip_row.is_none() {
            return Err("Trip not found".into());
        }

        let trip_row = trip_row.unwrap();
        let start_timestamp: String = trip_row.get_opt("start_timestamp")
            .and_then(|v| v.ok())
            .ok_or("Missing start_timestamp")?;
        let end_timestamp: String = trip_row.get_opt("end_timestamp")
            .and_then(|v| v.ok())
            .ok_or("Missing end_timestamp")?;

        // Delete environmental data in the time range
        conn.exec_drop(
            r"DELETE FROM environmental_monitoring 
              WHERE timestamp >= :start AND timestamp <= :end",
            mysql::params! {
                "start" => &start_timestamp,
                "end" => &end_timestamp,
            },
        ).map_err(|e| format!("Failed to delete environmental data: {}", e))?;

        // Delete vessel status data in the time range
        conn.exec_drop(
            r"DELETE FROM vessel_status 
              WHERE timestamp >= :start AND timestamp <= :end",
            mysql::params! {
                "start" => &start_timestamp,
                "end" => &end_timestamp,
            },
        ).map_err(|e| format!("Failed to delete vessel status data: {}", e))?;

        // Delete the trip record
        conn.exec_drop(
            r"DELETE FROM trips WHERE id = :trip_id",
            mysql::params! {
                "trip_id" => trip_id,
            },
        ).map_err(|e| format!("Failed to delete trip: {}", e))?;

        Ok(())
    }

    pub fn trim_trip(&self, trip_id: u32) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;

        // Fetch the trip record
        let trip_row: Option<mysql::Row> = conn.exec_first(
            r"SELECT start_timestamp, end_timestamp FROM trips WHERE id = :trip_id",
            mysql::params! {
                "trip_id" => trip_id,
            },
        ).map_err(|e| format!("Database query error: {}", e))?;

        if trip_row.is_none() {
            return Err("Trip not found".into());
        }

        let trip_row = trip_row.unwrap();
        let original_start: String = trip_row.get_opt("start_timestamp")
            .and_then(|v| v.ok())
            .ok_or("Missing start_timestamp")?;
        let original_end: String = trip_row.get_opt("end_timestamp")
            .and_then(|v| v.ok())
            .ok_or("Missing end_timestamp")?;

        // Parse timestamps to work with them
        let start_dt = NaiveDateTime::parse_from_str(&original_start, "%Y-%m-%d %H:%M:%S")
            .map_err(|e| format!("Failed to parse start timestamp: {}", e))?;
        let end_dt = NaiveDateTime::parse_from_str(&original_end, "%Y-%m-%d %H:%M:%S")
            .map_err(|e| format!("Failed to parse end timestamp: {}", e))?;

        // Find when the boat starts moving: first timestamp where is_moored = 0
        let first_moving: Option<mysql::Row> = conn.exec_first(
            r"SELECT timestamp FROM vessel_status 
              WHERE timestamp >= :start AND timestamp <= :end AND is_moored = 0
              ORDER BY timestamp ASC LIMIT 1",
            mysql::params! {
                "start" => &original_start,
                "end" => &original_end,
            },
        ).map_err(|e| format!("Failed to find first moving timestamp: {}", e))?;

        // Find when the boat gets permanently moored: find last is_moored = 0, then first is_moored = 1 after that
        let last_moving: Option<mysql::Row> = conn.exec_first(
            r"SELECT timestamp FROM vessel_status 
              WHERE timestamp >= :start AND timestamp <= :end AND is_moored = 0
              ORDER BY timestamp DESC LIMIT 1",
            mysql::params! {
                "start" => &original_start,
                "end" => &original_end,
            },
        ).map_err(|e| format!("Failed to find last moving timestamp: {}", e))?;

        let last_mooring: Option<mysql::Row> = if let Some(last_mov_row) = last_moving {
            let last_moving_ts: String = last_mov_row.get_opt("timestamp")
                .and_then(|v| v.ok())
                .ok_or("Missing timestamp in last moving row")?;
            conn.exec_first(
                r"SELECT timestamp FROM vessel_status 
                  WHERE timestamp > :last_moving AND timestamp <= :end AND is_moored = 1
                  ORDER BY timestamp ASC LIMIT 1",
                mysql::params! {
                    "last_moving" => &last_moving_ts,
                    "end" => &original_end,
                },
            ).map_err(|e| format!("Failed to find last mooring timestamp: {}", e))?
        } else {
            None
        };

        // Calculate new start and end timestamps
        let new_start = if let Some(first_mov_row) = first_moving {
            let first_moving_ts: String = first_mov_row.get_opt("timestamp")
                .and_then(|v| v.ok())
                .ok_or("Missing timestamp in first moving row")?;
            let first_moving_dt = NaiveDateTime::parse_from_str(&first_moving_ts, "%Y-%m-%d %H:%M:%S")
                .map_err(|e| format!("Failed to parse first moving timestamp: {}", e))?;
            // Subtract 1 hour from first moving time, but not before original start
            let candidate = first_moving_dt - chrono::Duration::hours(1);
            if candidate < start_dt {
                original_start.clone()
            } else {
                candidate.format("%Y-%m-%d %H:%M:%S").to_string()
            }
        } else {
            // If boat never moved, keep original start
            original_start.clone()
        };

        let new_end = if let Some(last_moor_row) = last_mooring {
            let last_mooring_ts: String = last_moor_row.get_opt("timestamp")
                .and_then(|v| v.ok())
                .ok_or("Missing timestamp in last mooring row")?;
            let last_mooring_dt = NaiveDateTime::parse_from_str(&last_mooring_ts, "%Y-%m-%d %H:%M:%S")
                .map_err(|e| format!("Failed to parse last mooring timestamp: {}", e))?;
            // Add 1 hour to last mooring time, but not after original end
            let candidate = last_mooring_dt + chrono::Duration::hours(1);
            if candidate > end_dt {
                original_end.clone()
            } else {
                candidate.format("%Y-%m-%d %H:%M:%S").to_string()
            }
        } else {
            // If boat never got moored, keep original end
            original_end.clone()
        };

        // Delete environmental_monitoring data outside the new range
        conn.exec_drop(
            r"DELETE FROM environmental_monitoring 
              WHERE (timestamp < :new_start OR timestamp > :new_end)
              AND timestamp >= :orig_start AND timestamp <= :orig_end",
            mysql::params! {
                "new_start" => &new_start,
                "new_end" => &new_end,
                "orig_start" => &original_start,
                "orig_end" => &original_end,
            },
        ).map_err(|e| format!("Failed to delete trimmed environmental data: {}", e))?;

        // Delete vessel_status data outside the new range
        conn.exec_drop(
            r"DELETE FROM vessel_status 
              WHERE (timestamp < :new_start OR timestamp > :new_end)
              AND timestamp >= :orig_start AND timestamp <= :orig_end",
            mysql::params! {
                "new_start" => &new_start,
                "new_end" => &new_end,
                "orig_start" => &original_start,
                "orig_end" => &original_end,
            },
        ).map_err(|e| format!("Failed to delete trimmed vessel status data: {}", e))?;

        // Update the trip with new timestamps and recalculate total_time_moored
        conn.exec_drop(
            r"UPDATE trips SET 
                start_timestamp = :new_start, 
                end_timestamp = :new_end,
                total_time_moored = (SELECT COALESCE(SUM(total_time_ms), 0) FROM vessel_status WHERE timestamp >= :new_start AND timestamp <= :new_end AND is_moored = 1)
              WHERE id = :trip_id",
            mysql::params! {
                "new_start" => &new_start,
                "new_end" => &new_end,
                "trip_id" => trip_id,
            },
        ).map_err(|e| format!("Failed to update trip: {}", e))?;

        Ok(())
    }


    /// Get system status (tracking and metrics enabled/disabled state)
    pub fn get_system_status(&self, key: &str) -> Result<bool, Box<dyn Error>> {
        let cache = self.system_status_cache.lock().unwrap();
        if let Some(&cached) = cache.get(key) {
            Ok(cached)
        } else {
            Ok(true) // Default to true if not found in cache for backward compatibility
        }
    }

    /// Set system status (tracking and metrics enabled/disabled state)
    pub fn set_system_status(&self, key: &str, value: bool) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        let value_str = if value { "1" } else { "0" };
        
        // Update database first
        conn.exec_drop(
            "INSERT INTO system_status (status_key, status_value) VALUES (:key, :value) ON DUPLICATE KEY UPDATE status_value = :value",
            mysql::params! {
                "key" => key,
                "value" => value_str,
            },
        )?;
        
        // Update cache to stay in sync
        let mut cache = self.system_status_cache.lock().unwrap();
        cache.insert(key.to_string(), value);
        
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
        let timestamp = chrono::DateTime::<chrono::Utc>::from(dirty_instant_to_systemtime(status_op.timestamp));
               
                tx.exec_drop(
                        r"INSERT INTO vessel_status 
                            (timestamp, latitude, longitude, average_speed_kn, max_speed_kn, is_moored, engine_on, total_distance_nm, total_time_ms, average_wind_speed_kn, average_wind_angle_deg, cog_deg, average_heading_deg)
                            VALUES (:timestamp, :latitude, :longitude, :avg_speed, :max_speed, :is_moored, :engine_on, :total_distance, :total_time, :avg_wind_speed, :avg_wind_angle, :cog_deg, :avg_heading_deg)",
                        params! {
                                "timestamp" => timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                                "latitude" => status_op.position.latitude,
                                "longitude" => status_op.position.longitude,
                                "avg_speed" => status_op.average_speed_kn,
                                "max_speed" => status_op.max_speed_kn,
                                "is_moored" => status_op.is_moored,
                                "engine_on" => status_op.engine_on,
                                "total_distance" => status_op.total_distance_nm,
                                "total_time" => status_op.total_time_ms,
                                "avg_wind_speed" => status_op.wind_speed_kn,
                                "avg_wind_angle" => status_op.wind_angle_deg,
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

    /// Get the last written record from vessel_status table
    /// Returns the most recent vessel status record as a VesselStatusOperation
    pub fn get_last_vessel_status(&self) -> Result<Option<VesselStatusOperation>, Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        
        let row: Option<mysql::Row> = conn.query_first(
            r"SELECT 
                timestamp,
                latitude,
                longitude,
                average_speed_kn,
                max_speed_kn,
                is_moored,
                engine_on,
                total_distance_nm,
                total_time_ms,
                average_wind_speed_kn,
                average_wind_angle_deg,
                cog_deg,
                average_heading_deg
             FROM vessel_status
             ORDER BY timestamp DESC
             LIMIT 1",
        )?;
        
        if let Some(row) = row {
            let timestamp_str: String = row.get(0).ok_or("Failed to get timestamp")?;
            let latitude: Option<f64> = row.get_opt(1).and_then(|v| v.ok()).flatten();
            let longitude: Option<f64> = row.get_opt(2).and_then(|v| v.ok()).flatten();
            let average_speed_kn: Option<f64> = row.get_opt(3).and_then(|v| v.ok()).flatten();
            let max_speed_kn: Option<f64> = row.get_opt(4).and_then(|v| v.ok()).flatten();
            let is_moored: bool = row.get(5).ok_or("Failed to get is_moored")?;
            let engine_on: bool = row.get(6).ok_or("Failed to get engine_on")?;
            let total_distance_nm: Option<f64> = row.get_opt(7).and_then(|v| v.ok()).flatten();
            let total_time_ms: u64 = row.get(8).ok_or("Failed to get total_time_ms")?;
            let wind_speed_kn: Option<f64> = row.get_opt(9).and_then(|v| v.ok()).flatten();
            let wind_angle_deg: Option<f64> = row.get_opt(10).and_then(|v| v.ok()).flatten();
            let cog_deg: Option<f64> = row.get_opt(11).and_then(|v| v.ok()).flatten();
            let average_heading_deg: Option<f64> = row.get_opt(12).and_then(|v| v.ok()).flatten();

            // Convert timestamp string to Instant via SystemTime
            let ts_clean = timestamp_str.trim_end_matches('Z');
            let dt = NaiveDateTime::parse_from_str(ts_clean, "%Y-%m-%d %H:%M:%S%.f")?;
            let datetime = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc);
            let system_time = SystemTime::from(datetime);
            let timestamp = system_time.elapsed().map(|d| std::time::Instant::now() - d).unwrap_or_else(|_| std::time::Instant::now());

            Ok(Some(VesselStatusOperation {
                timestamp,
                position: Position {
                    latitude: latitude.unwrap_or(0.0),
                    longitude: longitude.unwrap_or(0.0),
                },
                average_speed_kn: average_speed_kn.unwrap_or(0.0),
                max_speed_kn: max_speed_kn.unwrap_or(0.0),
                is_moored,
                engine_on,
                total_distance_nm: total_distance_nm.unwrap_or(0.0),
                total_time_ms,
                wind_speed_kn,
                wind_speed_variance: None,
                wind_angle_deg,
                wind_angle_variance: None,
                cog_deg,
                average_heading_deg,
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

#[derive(Debug, serde::Serialize)]
pub struct MonthlyStatistic {
    pub year: i32,
    pub month: u32,
    pub date: String,  // Format: YYYY-MM
    pub sailing_distance_nm: f64,
    pub motoring_distance_nm: f64,
}

#[derive(Debug, serde::Serialize)]
pub struct MonthlyStatistics {
    pub months: Vec<MonthlyStatistic>,
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
                id: row.get_opt("id").and_then(|v| v.ok()).unwrap_or(0),
                description: row.get_opt::<String, _>("description").and_then(|v| v.ok()).unwrap_or_default(),
                start_date: row.get_opt::<String, _>("start_ts").and_then(|v| v.ok()).unwrap_or_default(),
                end_date: row.get_opt::<String, _>("end_ts").and_then(|v| v.ok()).unwrap_or_default(),
                total_distance_nm: row.get_opt::<f64, _>("total_distance").and_then(|v| v.ok()).unwrap_or(0.0),
                total_time_ms: row.get_opt::<i64, _>("total_time").and_then(|v| v.ok()).unwrap_or(0),
                sailing_time_ms: row.get_opt::<i64, _>("total_time_sailing").and_then(|v| v.ok()).unwrap_or(0),
                motoring_time_ms: row.get_opt::<i64, _>("total_time_motoring").and_then(|v| v.ok()).unwrap_or(0),
                moored_time_ms: row.get_opt::<i64, _>("total_time_moored").and_then(|v| v.ok()).unwrap_or(0),
                sailing_distance_nm: row.get_opt::<f64, _>("total_distance_sailed").and_then(|v| v.ok()).unwrap_or(0.0),
                motoring_distance_nm: row.get_opt::<f64, _>("total_distance_motoring").and_then(|v| v.ok()).unwrap_or(0.0),
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
            // If no filters specified, get all trips (to populate year filter with all available years)
            query.push_str(" 1=1");
        }

        query.push_str(" ORDER BY start_timestamp DESC");

        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        let results: Vec<mysql::Row> = conn.query(&query)
            .map_err(|e| format!("Database query error: {}", e))?;

        let trips = results
            .iter()
            .map(|row| TripSummary {
                id: row.get_opt("id").and_then(|v| v.ok()).unwrap_or(0),
                description: row.get_opt::<String, _>("description").and_then(|v| v.ok()).unwrap_or_default(),
                start_date: row.get_opt::<String, _>("start_ts").and_then(|v| v.ok()).unwrap_or_default(),
                end_date: row.get_opt::<String, _>("end_ts").and_then(|v| v.ok()).unwrap_or_default(),
                total_distance_nm: row.get_opt::<f64, _>("total_distance").and_then(|v| v.ok()).unwrap_or(0.0),
                total_time_ms: row.get_opt::<i64, _>("total_time").and_then(|v| v.ok()).unwrap_or(0),
                sailing_time_ms: row.get_opt::<i64, _>("total_time_sailing").and_then(|v| v.ok()).unwrap_or(0),
                motoring_time_ms: row.get_opt::<i64, _>("total_time_motoring").and_then(|v| v.ok()).unwrap_or(0),
                moored_time_ms: row.get_opt::<i64, _>("total_time_moored").and_then(|v| v.ok()).unwrap_or(0),
                sailing_distance_nm: row.get_opt::<f64, _>("total_distance_sailed").and_then(|v| v.ok()).unwrap_or(0.0),
                motoring_distance_nm: row.get_opt::<f64, _>("total_distance_motoring").and_then(|v| v.ok()).unwrap_or(0.0),
            })
            .collect();

        Ok(trips)
    }

    /// Fetch monthly statistics since January 2020
    /// Returns monthly sailed and motored nautical miles, including months with no activity
    pub fn fetch_monthly_statistics(&self) -> Result<MonthlyStatistics, Box<dyn std::error::Error>> {
        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        // Get all trip data grouped by year and month
        let results: Vec<mysql::Row> = conn.query(
            r"SELECT YEAR(start_timestamp) as year,
                     MONTH(start_timestamp) as month,
                     SUM(total_distance_sailed) as sailing_distance,
                     SUM(total_distance_motoring) as motoring_distance
              FROM trips
              WHERE start_timestamp >= '2020-01-01'
              GROUP BY YEAR(start_timestamp), MONTH(start_timestamp)
              ORDER BY year ASC, month ASC"
        )
            .map_err(|e| format!("Database query error: {}", e))?;

        // Build a map of (year, month) -> (sailing_distance, motoring_distance)
        let mut month_data: std::collections::HashMap<(i32, u32), (f64, f64)> = std::collections::HashMap::new();
        
        for row in results {
            let year: i32 = row.get_opt("year")
                .and_then(|v| v.ok())
                .ok_or("Missing year")?;
            let month: u32 = row.get_opt::<u32, _>("month")
                .and_then(|v| v.ok())
                .ok_or("Missing month")?;
            let sailing_distance: f64 = row.get_opt::<f64, _>("sailing_distance")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let motoring_distance: f64 = row.get_opt::<f64, _>("motoring_distance")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            
            month_data.insert((year, month), (sailing_distance, motoring_distance));
        }

        // Generate all months from January 2020 to now
        use chrono::Datelike;
        let now = chrono::Local::now();
        let current_year = now.year();
        let current_month = now.month();
        
        let mut all_months = Vec::new();
        
        for year in 2020..=current_year {
            let start_month = if year == 2020 { 1 } else { 1 };
            let end_month = if year == current_year { current_month } else { 12 };
            
            for month in start_month..=end_month {
                let (sailing_dist, motoring_dist) = month_data
                    .get(&(year, month))
                    .copied()
                    .unwrap_or((0.0, 0.0));
                
                let date = format!("{:04}-{:02}", year, month);
                
                all_months.push(MonthlyStatistic {
                    year,
                    month: month as u32,
                    date,
                    sailing_distance_nm: sailing_dist,
                    motoring_distance_nm: motoring_dist,
                });
            }
        }

        Ok(MonthlyStatistics {
            months: all_months,
        })
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
                timestamp: row.get_opt::<String, _>("timestamp")
                    .and_then(|v| v.ok())
                    .unwrap_or_default(),
                latitude: row.get_opt::<f64, _>("latitude")
                    .and_then(|v| v.ok()),
                longitude: row.get_opt::<f64, _>("longitude")
                    .and_then(|v| v.ok()),
                avg_speed_kn: row.get_opt::<f64, _>("average_speed_kn")
                    .and_then(|v| v.ok()),
                max_speed_kn: row.get_opt::<f64, _>("max_speed_kn")
                    .and_then(|v| v.ok()),
                moored: row.get_opt::<i32, _>("is_moored")
                    .and_then(|v| v.ok())
                    .map(|v| v != 0)
                    .unwrap_or(false),
                engine_on: row.get_opt::<i32, _>("engine_on")
                    .and_then(|v| v.ok())
                    .map(|v| v != 0)
                    .unwrap_or(false),
                total_distance_nm: row.get_opt::<f64, _>("total_distance_nm")
                    .and_then(|v| v.ok()),
                total_time_ms: row.get_opt::<u64, _>("total_time_ms")
                    .and_then(|v| v.ok())
                    .unwrap_or(0),
                average_wind_speed_kn: row.get_opt::<f64, _>("average_wind_speed_kn")
                    .and_then(|v| v.ok()),
                average_wind_angle_deg: row.get_opt::<f64, _>("average_wind_angle_deg")
                    .and_then(|v| v.ok()),
                cog_deg: row.get_opt::<f64, _>("cog_deg")
                    .and_then(|v| v.ok()),
                average_heading_deg: row.get_opt::<f64, _>("average_heading_deg")
                    .and_then(|v| v.ok()),
            })
            .collect();

        Ok(track)
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
                timestamp: row.get_opt::<String, _>("timestamp")
                    .and_then(|v| v.ok())
                    .unwrap_or_default(),
                metric_id: row.get_opt::<String, _>("metric_id")
                    .and_then(|v| v.ok())
                    .unwrap_or_default(),
                avg_value: row.get_opt::<f64, _>("value_avg")
                    .and_then(|v| v.ok()),
                max_value: row.get_opt::<f64, _>("value_max")
                    .and_then(|v| v.ok()),
                min_value: row.get_opt::<f64, _>("value_min")
                    .and_then(|v| v.ok())
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
            let speed: Option<f64> = row.get_opt("average_speed_kn")
                .and_then(|v| v.ok());
            let distance: Option<f64> = row.get_opt("total_distance_nm")
                .and_then(|v| v.ok());
            let engine_on: i32 = row.get_opt("engine_on")
                .and_then(|v| v.ok())
                .unwrap_or(0);
            
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
            let wind_direction: Option<f64> = row.get_opt("average_wind_angle_deg")
                .and_then(|v| v.ok());
            let wind_speed: Option<f64> = row.get_opt("average_wind_speed_kn")
                .and_then(|v| v.ok());
            let timestamp: Option<String> = row.get_opt("timestamp")
                .and_then(|v| v.ok());
            
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
            let timestamp: String = row.get_opt("timestamp")
                .and_then(|v| v.ok())
                .unwrap_or_default();
            let latitude: Option<f64> = row.get_opt("latitude")
                .and_then(|v| v.ok());
            let longitude: Option<f64> = row.get_opt("longitude")
                .and_then(|v| v.ok());
            let speed: Option<f64> = row.get_opt("average_speed_kn")
                .and_then(|v| v.ok());
            let engine_on: bool = row.get_opt("engine_on")
                .and_then(|v| v.ok())
                .map(|v: i32| v != 0)
                .unwrap_or(false);

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
             WHERE DATE(vs.timestamp) BETWEEN '{}' AND '{}' AND vs.is_moored = 0
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
            let date: String = row.get_opt("day")
                .and_then(|v| v.ok())
                .unwrap_or_default();
            let distance: f64 = row.get_opt("total_distance")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            
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
            let segment_dist = haversine_distance_nm(*prev_lat, *prev_lon, *end_lat, *end_lon);
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

    #[test]
    fn test_system_status_set() {
        let db = VesselDatabase::new("mysql://nmea:nmea@localhost:3306/test_nmea_router").unwrap();
        db.set_system_status("tracking_enabled", true).unwrap();
        assert!(db.get_system_status("tracking_enabled").unwrap());
        db.set_system_status("tracking_enabled", false).unwrap();
        assert!(!db.get_system_status("tracking_enabled").unwrap());
    }

    #[test]
    fn test_system_status_default() {
        let db = VesselDatabase::new("mysql://nmea:nmea@localhost:3306/test_nmea_router").unwrap();
        assert!(db.get_system_status("a_key_that_does_not_exist").unwrap());
    }

    #[test]
    fn test_system_status_persistence() {
        let db = VesselDatabase::new("mysql://nmea:nmea@localhost:3306/test_nmea_router").unwrap();
        db.set_system_status("test_key", true).unwrap();
        assert!(db.get_system_status("test_key").unwrap());
        
        // Create a new instance to verify persistence
        let db2 = VesselDatabase::new("mysql://nmea:nmea@localhost:3306/test_nmea_router").unwrap();
        assert!(db2.get_system_status("test_key").unwrap());
    }

    #[test]
    fn test_export_trip() {
        use std::fs;
        use std::path::PathBuf;

        let db = VesselDatabase::new("mysql://nmea:nmea@localhost:3306/test_nmea_router").unwrap();
        
        // Use trip 132 which we know exists from other tests
        let trip_id = 132i64;
        let export_path = PathBuf::from("/tmp/test_trip_export.json");
        
        // Remove file if it exists from previous test run
        let _ = fs::remove_file(&export_path);
        
        // Perform export
        let result = db.export_trip(trip_id, &export_path);
        assert!(result.is_ok(), "Export should succeed: {:?}", result.err());
        
        // Verify file was created and has content
        assert!(export_path.exists(), "Export file should exist");
        let metadata = fs::metadata(&export_path).unwrap();
        assert!(metadata.len() > 0, "Export file should not be empty");
        
        // Verify file contains valid JSON
        let contents = fs::read_to_string(&export_path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&contents)
            .expect("Export file should contain valid JSON");
        
        // Verify JSON structure
        assert!(json["trip"].is_object(), "Should have trip object");
        assert_eq!(json["trip"]["id"], trip_id, "Trip ID should match");
        assert!(json["trip"]["description"].is_string(), "Trip should have description");
        assert!(json["trip"]["start_timestamp"].is_string(), "Trip should have start_timestamp");
        assert!(json["trip"]["end_timestamp"].is_string(), "Trip should have end_timestamp");
        
        // Verify arrays exist
        assert!(json["vessel_statuses"].is_array(), "Should have vessel_statuses array");
        assert!(json["environmental_metrics"].is_array(), "Should have environmental_metrics array");
        assert!(json["export_metadata"].is_object(), "Should have export_metadata object");
        
        // Vessel statuses should have records if they exist in database
        let vessel_statuses = json["vessel_statuses"].as_array().unwrap();
        if !vessel_statuses.is_empty() {
            let first_status = &vessel_statuses[0];
            assert!(first_status["timestamp"].is_string(), "Status should have timestamp");
            assert!(first_status["is_moored"].is_boolean(), "Status should have is_moored");
            assert!(first_status["engine_on"].is_boolean(), "Status should have engine_on");
        }
        
        // Clean up
        fs::remove_file(&export_path).expect("Should be able to delete test file");
    }


}