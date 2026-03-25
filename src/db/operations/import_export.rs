use crate::db::types::VesselDatabase;
use std::error::Error;
use mysql::params;
use tracing::info;
use mysql::prelude::Queryable;

impl VesselDatabase {
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
                    total_time_moored, uuid,
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
        let trip_uuid: Option<String> = trip_row.get(9).unwrap_or(None);
        let start_ts: mysql::Value = trip_row.get(10).ok_or("Missing start_timestamp_raw")?;
        let end_ts: mysql::Value = trip_row.get(11).ok_or("Missing end_timestamp_raw")?;

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
            let engine_on: u8 = row.get(6).ok_or("Missing engine_on")?;  // 0=off, 1=on, 2=unknown
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
                "uuid": trip_uuid,
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
        let import_uuid: Option<&str> = trip["uuid"].as_str();
        
        let mut conn = self.pool.get_conn()?;
        
        let new_trip_start = chrono::DateTime::parse_from_rfc3339(start_ts_str)
            .map_err(|e| format!("Invalid start_timestamp format: {}", e))?;

        if let Some(uuid) = import_uuid {
            // UUID present: if a trip with this UUID already exists, delete it first (replace semantics)
            let existing_id: Option<u64> = conn.exec_first(
                "SELECT id FROM trips WHERE uuid = :uuid LIMIT 1",
                params! { "uuid" => uuid },
            )?;
            if let Some(id) = existing_id {
                info!("Import: deleting existing trip {} with UUID {} before re-import", id, uuid);
                self.delete_trip(id as u32)?;
            }
        } else {
            // No UUID in file: use the old overlap check
            let overlapping_trip: Option<(i64, String)> = conn.exec_first(
                "SELECT id, CAST(end_timestamp AS CHAR) FROM trips WHERE end_timestamp >= :new_start ORDER BY end_timestamp DESC LIMIT 1",
                params! {
                    "new_start" => new_trip_start.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                },
            )?;
            if let Some((existing_id, existing_end_ts)) = overlapping_trip {
                return Err(format!(
                    "Trip overlaps with existing trip ID {}. Existing trip ends at {}, new trip starts at {}",
                    existing_id, existing_end_ts, start_ts_str
                ).into());
            }
        }

        // Re-acquire connection after possible delete_trip (which borrows &self)
        let mut conn = self.pool.get_conn()?;

        // Start transaction for atomic insert
        let mut tx = conn.start_transaction(mysql::TxOpts::default())?;
        
        // The UUID to store: use the one from the file, or generate a new one for legacy files
        let effective_uuid = import_uuid
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        // Insert trip
        tx.exec_drop(
            "INSERT INTO trips (description, start_timestamp, end_timestamp, total_distance_sailed, total_distance_motoring, total_time_sailing, total_time_motoring, total_time_moored, uuid)
             VALUES (:desc, :start_ts, :end_ts, :dist_sailed, :dist_motoring, :time_sailing, :time_motoring, :time_moored, :uuid)",
            params! {
                "desc" => description,
                "start_ts" => new_trip_start.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                "end_ts" => chrono::DateTime::parse_from_rfc3339(end_ts_str)?
                    .format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                "dist_sailed" => total_distance_sailed,
                "dist_motoring" => total_distance_motoring,
                "time_sailing" => total_time_sailing,
                "time_motoring" => total_time_motoring,
                "time_moored" => total_time_moored,
                "uuid" => &effective_uuid,
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
                // Handle both old boolean format and new u8 format for engine_on
                let engine_on: u8 = match &status["engine_on"] {
                    v if v.is_boolean() => if v.as_bool().unwrap_or(false) { 1 } else { 0 },
                    v if v.is_u64() => v.as_u64().unwrap_or(2) as u8,
                    _ => 2,  // Default to unknown
                };
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
                    params! {
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
                    params! {
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
}
