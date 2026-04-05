use crate::db::types::VesselDatabase;
use std::error::Error;
use mysql::params;
use tracing::info;
use mysql::prelude::Queryable;

// ---------------------------------------------------------------------------
// Typed structs for export serialization.
// Using #[derive(serde::Serialize)] avoids building a serde_json::Value tree
// (~100 K heap-allocated nodes for a long trip), which was the primary
// performance bottleneck on low-power hardware.
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
struct ExportTrip {
    id: i64,
    uuid: Option<String>,
    description: String,
    start_timestamp: String,
    end_timestamp: String,
    total_distance_sailed: f64,
    total_distance_motoring: f64,
    total_time_sailing: u64,
    total_time_motoring: u64,
    total_time_moored: u64,
}

#[derive(serde::Serialize)]
struct ExportVesselStatus {
    timestamp: String,
    latitude: Option<f64>,
    longitude: Option<f64>,
    average_speed_kn: Option<f64>,
    max_speed_kn: Option<f64>,
    is_moored: bool,
    engine_on: u8,
    total_distance_nm: Option<f64>,
    total_time_ms: Option<u64>,
    average_wind_speed_kn: Option<f64>,
    average_wind_angle_deg: Option<f64>,
    cog_deg: Option<f64>,
    average_heading_deg: Option<f64>,
}

#[derive(serde::Serialize)]
struct ExportEnvMetric {
    timestamp: String,
    metric_id: u8,
    value_avg: Option<f32>,
    value_max: Option<f32>,
    value_min: Option<f32>,
    unit: Option<String>,
}

#[derive(serde::Serialize)]
struct ExportMetadata {
    generated_at: String,
    trip_id: i64,
}

#[derive(serde::Serialize)]
struct ExportData {
    trip: ExportTrip,
    vessel_statuses: Vec<ExportVesselStatus>,
    environmental_metrics: Vec<ExportEnvMetric>,
    export_metadata: ExportMetadata,
}

// Converts a mysql::Value::Date (returned for DATETIME(3) columns via prepared
// statements) to an ISO-8601 UTC string without calling DATE_FORMAT in SQL.
// Also handles Value::Bytes for the text-protocol fallback path.
fn mysql_datetime_to_iso(val: &mysql::Value) -> Result<String, Box<dyn Error>> {
    match val {
        mysql::Value::Date(year, month, day, hour, minute, second, micros) => {
            let millis = *micros / 1000;
            Ok(format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
                year, month, day, hour, minute, second, millis
            ))
        }
        mysql::Value::Bytes(b) => {
            // Text-protocol path: "YYYY-MM-DD HH:MM:SS.mmm" → ISO 8601
            let s = std::str::from_utf8(b)?;
            Ok(s.replacen(' ', "T", 1) + "Z")
        }
        other => Err(format!("Unexpected MySQL value type for timestamp: {:?}", other).into()),
    }
}

impl VesselDatabase {
    pub fn export_trip<P: AsRef<std::path::Path>>(&self, trip_id: i64, output_path: P) -> Result<(), Box<dyn Error>> {
        use std::fs::File;
        use std::io::BufWriter;

        let mut conn = self.pool.get_conn()?;

        // Step 1: Fetch the trip record.
        // Raw DATETIME(3) columns are read directly; mysql_datetime_to_iso formats
        // them in Rust, avoiding DATE_FORMAT() calls on the server side.
        let trip_row: Option<mysql::Row> = conn.exec_first(
            "SELECT id, start_timestamp, end_timestamp,
                    description, total_distance_sailed,
                    total_distance_motoring, total_time_sailing, total_time_motoring,
                    total_time_moored, uuid
             FROM trips WHERE id = :id",
            params! { "id" => trip_id },
        )?;

        let trip_row = trip_row.ok_or("Trip not found")?;

        let trip_id_fetched: i64    = trip_row.get(0).ok_or("Missing id")?;
        let start_ts: mysql::Value  = trip_row.get(1).ok_or("Missing start_timestamp")?;
        let end_ts: mysql::Value    = trip_row.get(2).ok_or("Missing end_timestamp")?;
        let description: String     = trip_row.get(3).ok_or("Missing description")?;
        let total_distance_sailed:   f64 = trip_row.get(4).ok_or("Missing total_distance_sailed")?;
        let total_distance_motoring: f64 = trip_row.get(5).ok_or("Missing total_distance_motoring")?;
        let total_time_sailing:  u64 = trip_row.get(6).ok_or("Missing total_time_sailing")?;
        let total_time_motoring: u64 = trip_row.get(7).ok_or("Missing total_time_motoring")?;
        let total_time_moored:   u64 = trip_row.get(8).ok_or("Missing total_time_moored")?;
        let trip_uuid: Option<String> = trip_row.get(9).unwrap_or(None);

        let start_ts_str = mysql_datetime_to_iso(&start_ts)?;
        let end_ts_str   = mysql_datetime_to_iso(&end_ts)?;

        // Release the first connection before acquiring two more for parallel queries.
        drop(conn);

        // Steps 2 & 3: Fetch vessel_status and environmental_data concurrently.
        // Each thread gets its own connection from the pool so both queries run
        // simultaneously instead of sequentially.
        let pool_vs  = self.pool.clone();
        let pool_em  = self.pool.clone();
        let start_vs = start_ts.clone();
        let end_vs   = end_ts.clone();
        let start_em = start_ts.clone();
        let end_em   = end_ts.clone();

        let (vs_result, em_result) = std::thread::scope(|s| {
            let vs = s.spawn(move || -> Result<Vec<mysql::Row>, Box<dyn std::error::Error + Send + Sync>> {
                let mut conn = pool_vs.get_conn()?;
                Ok(conn.exec(
                    "SELECT timestamp, latitude, longitude, average_speed_kn, max_speed_kn,
                             is_moored, engine_on, total_distance_nm, total_time_ms,
                             average_wind_speed_kn, average_wind_angle_deg, cog_deg,
                             average_heading_deg
                     FROM vessel_status
                     WHERE timestamp >= :start_ts AND timestamp <= :end_ts
                     ORDER BY timestamp ASC",
                    params! { "start_ts" => start_vs, "end_ts" => end_vs },
                )?)
            });

            let em = s.spawn(move || -> Result<Vec<mysql::Row>, Box<dyn std::error::Error + Send + Sync>> {
                let mut conn = pool_em.get_conn()?;
                Ok(conn.exec(
                    "SELECT timestamp, metric_id, value_avg, value_max, value_min, unit
                     FROM environmental_data
                     WHERE timestamp >= :start_ts AND timestamp <= :end_ts
                     ORDER BY timestamp ASC",
                    params! { "start_ts" => start_em, "end_ts" => end_em },
                )?)
            });

            (vs.join(), em.join())
        });

        let vessel_rows = vs_result
            .map_err(|_| -> Box<dyn Error> { "vessel_status query thread panicked".into() })?
            .map_err(|e| -> Box<dyn Error> { e.to_string().into() })?;
        let env_rows = em_result
            .map_err(|_| -> Box<dyn Error> { "environmental_data query thread panicked".into() })?
            .map_err(|e| -> Box<dyn Error> { e.to_string().into() })?;

        // Step 4: Map raw rows into typed structs.
        // Struct serialization via serde avoids allocating a serde_json::Value
        // node for every field of every row.
        let mut vessel_statuses: Vec<ExportVesselStatus> = Vec::with_capacity(vessel_rows.len());
        for row in vessel_rows {
            let ts_val: mysql::Value = row.get(0).ok_or("Missing timestamp in vessel_status")?;
            vessel_statuses.push(ExportVesselStatus {
                timestamp:              mysql_datetime_to_iso(&ts_val)?,
                latitude:               row.get_opt(1).and_then(|v| v.ok()).flatten(),
                longitude:              row.get_opt(2).and_then(|v| v.ok()).flatten(),
                average_speed_kn:       row.get_opt(3).and_then(|v| v.ok()).flatten(),
                max_speed_kn:           row.get_opt(4).and_then(|v| v.ok()).flatten(),
                is_moored:              row.get(5).ok_or("Missing is_moored")?,
                engine_on:              row.get(6).ok_or("Missing engine_on")?,
                total_distance_nm:      row.get_opt(7).and_then(|v| v.ok()).flatten(),
                total_time_ms:          row.get_opt(8).and_then(|v| v.ok()).flatten(),
                average_wind_speed_kn:  row.get_opt(9).and_then(|v| v.ok()).flatten(),
                average_wind_angle_deg: row.get_opt(10).and_then(|v| v.ok()).flatten(),
                cog_deg:                row.get_opt(11).and_then(|v| v.ok()).flatten(),
                average_heading_deg:    row.get_opt(12).and_then(|v| v.ok()).flatten(),
            });
        }

        let mut env_metrics: Vec<ExportEnvMetric> = Vec::with_capacity(env_rows.len());
        for row in env_rows {
            let ts_val: mysql::Value = row.get(0).ok_or("Missing timestamp in environmental_data")?;
            env_metrics.push(ExportEnvMetric {
                timestamp: mysql_datetime_to_iso(&ts_val)?,
                metric_id: row.get(1).ok_or("Missing metric_id")?,
                value_avg: row.get_opt(2).and_then(|v| v.ok()).flatten(),
                value_max: row.get_opt(3).and_then(|v| v.ok()).flatten(),
                value_min: row.get_opt(4).and_then(|v| v.ok()).flatten(),
                unit:      row.get_opt(5).and_then(|v| v.ok()).flatten(),
            });
        }

        let export_data = ExportData {
            trip: ExportTrip {
                id: trip_id_fetched,
                uuid: trip_uuid,
                description,
                start_timestamp: start_ts_str,
                end_timestamp: end_ts_str,
                total_distance_sailed,
                total_distance_motoring,
                total_time_sailing,
                total_time_motoring,
                total_time_moored,
            },
            vessel_statuses,
            environmental_metrics: env_metrics,
            export_metadata: ExportMetadata {
                generated_at: chrono::Utc::now().to_rfc3339(),
                trip_id,
            },
        };

        if let Some(parent) = std::path::Path::new(output_path.as_ref()).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        // BufWriter batches small writes into fewer syscalls; compact JSON
        // (to_writer vs to_writer_pretty) skips indentation computation entirely.
        let file = File::create(&output_path)?;
        serde_json::to_writer(BufWriter::new(file), &export_data)?;

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

        // Invalidate the heatmap cache for every day the imported trip spans so that
        // fetch_heatmap() recomputes those days from the newly inserted vessel_status rows.
        let trip_start_date = new_trip_start.date_naive();
        let trip_end_date = chrono::DateTime::parse_from_rfc3339(end_ts_str)
            .map_err(|e| format!("Invalid end_timestamp format: {}", e))?
            .date_naive();
        self.invalidate_heatmap_cache(trip_start_date, trip_end_date)?;

        info!("Trip imported successfully with ID: {}", new_trip_id);
        Ok(new_trip_id)
    }
}
