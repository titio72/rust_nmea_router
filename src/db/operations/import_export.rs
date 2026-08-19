use crate::db::types::VesselDatabase;
use crate::error::AppError;
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
    #[serde(rename = "desc")]
    description: String,
    #[serde(rename = "start")]
    start_timestamp: String,
    #[serde(rename = "end")]
    end_timestamp: String,
    #[serde(rename = "dist_sail")]
    total_distance_sailed: f64,
    #[serde(rename = "dist_motor")]
    total_distance_motoring: f64,
    #[serde(rename = "t_sail")]
    total_time_sailing: u64,
    #[serde(rename = "t_motor")]
    total_time_motoring: u64,
    #[serde(rename = "t_moor")]
    total_time_moored: u64,
}

#[derive(serde::Serialize)]
struct ExportVesselStatus {
    #[serde(rename = "ts")]
    timestamp: String,
    #[serde(rename = "lat")]
    latitude: Option<f64>,
    #[serde(rename = "lon")]
    longitude: Option<f64>,
    #[serde(rename = "sog")]
    average_speed_kn: Option<f64>,
    #[serde(rename = "sog_max")]
    max_speed_kn: Option<f64>,
    #[serde(rename = "moor")]
    is_moored: bool,
    #[serde(rename = "eng")]
    engine_on: u8,
    #[serde(rename = "dist")]
    total_distance_nm: Option<f64>,
    #[serde(rename = "dur")]
    total_time_ms: Option<u64>,
    #[serde(rename = "tws")]
    average_wind_speed_kn: Option<f64>,
    #[serde(rename = "twa")]
    average_wind_angle_deg: Option<f64>,
    #[serde(rename = "cog")]
    cog_deg: Option<f64>,
    #[serde(rename = "hdg")]
    average_heading_deg: Option<f64>,
}

#[derive(serde::Serialize)]
struct ExportEnvMetric {
    #[serde(rename = "ts")]
    timestamp: String,
    #[serde(rename = "mid")]
    metric_id: u8,
    #[serde(rename = "avg")]
    value_avg: Option<f32>,
    #[serde(rename = "max")]
    value_max: Option<f32>,
    #[serde(rename = "min")]
    value_min: Option<f32>,
    unit: Option<String>,
}

#[derive(serde::Serialize)]
struct ExportMetadata {
    #[serde(rename = "at")]
    generated_at: String,
    #[serde(rename = "tid")]
    trip_id: i64,
}

#[derive(serde::Serialize)]
struct ExportData {
    trip: ExportTrip,
    #[serde(rename = "vs")]
    vessel_statuses: Vec<ExportVesselStatus>,
    #[serde(rename = "em")]
    environmental_metrics: Vec<ExportEnvMetric>,
    #[serde(rename = "meta")]
    export_metadata: ExportMetadata,
}

// Converts a mysql::Value::Date (returned for DATETIME(3) columns via prepared
// statements) to an ISO-8601 UTC string without calling DATE_FORMAT in SQL.
// Also handles Value::Bytes for the text-protocol fallback path.
fn mysql_datetime_to_iso(val: &mysql::Value) -> Result<String, AppError> {
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
        other => Err(AppError::Database(format!(
            "Unexpected MySQL value type for timestamp: {:?}",
            other
        ))),
    }
}

impl VesselDatabase {
    /// Serialize a trip and all its data to a compact JSON string (same format as export_trip).
    pub fn export_trip_to_string(&self, trip_id: i64) -> Result<String, AppError> {
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

        let trip_row = trip_row.ok_or(AppError::Database("Trip not found".to_string()))?;

        let trip_id_fetched: i64    = trip_row.get(0).ok_or(AppError::Database("Missing id".to_string()))?;
        let start_ts: mysql::Value  = trip_row.get(1).ok_or(AppError::Database("Missing start_timestamp".to_string()))?;
        let end_ts: mysql::Value    = trip_row.get(2).ok_or(AppError::Database("Missing end_timestamp".to_string()))?;
        let description: String     = trip_row.get(3).ok_or(AppError::Database("Missing description".to_string()))?;
        let total_distance_sailed:   f64 = trip_row.get(4).ok_or(AppError::Database("Missing total_distance_sailed".to_string()))?;
        let total_distance_motoring: f64 = trip_row.get(5).ok_or(AppError::Database("Missing total_distance_motoring".to_string()))?;
        let total_time_sailing:  u64 = trip_row.get(6).ok_or(AppError::Database("Missing total_time_sailing".to_string()))?;
        let total_time_motoring: u64 = trip_row.get(7).ok_or(AppError::Database("Missing total_time_motoring".to_string()))?;
        let total_time_moored:   u64 = trip_row.get(8).ok_or(AppError::Database("Missing total_time_moored".to_string()))?;
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
            let vs = s.spawn(move || -> Result<Vec<mysql::Row>, AppError> {
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

            let em = s.spawn(move || -> Result<Vec<mysql::Row>, AppError> {
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
            .map_err(|_| AppError::Database("vessel_status query thread panicked".to_string()))??;
        let env_rows = em_result
            .map_err(|_| AppError::Database("environmental_data query thread panicked".to_string()))??;

        // Step 4: Map raw rows into typed structs.
        // Struct serialization via serde avoids allocating a serde_json::Value
        // node for every field of every row.
        let mut vessel_statuses: Vec<ExportVesselStatus> = Vec::with_capacity(vessel_rows.len());
        for row in vessel_rows {
            let ts_val: mysql::Value = row.get(0).ok_or(AppError::Database("Missing timestamp in vessel_status".to_string()))?;
            vessel_statuses.push(ExportVesselStatus {
                timestamp:              mysql_datetime_to_iso(&ts_val)?,
                latitude:               row.get_opt(1).and_then(|v| v.ok()).flatten(),
                longitude:              row.get_opt(2).and_then(|v| v.ok()).flatten(),
                average_speed_kn:       row.get_opt(3).and_then(|v| v.ok()).flatten(),
                max_speed_kn:           row.get_opt(4).and_then(|v| v.ok()).flatten(),
                is_moored:              row.get(5).ok_or(AppError::Database("Missing is_moored".to_string()))?,
                engine_on:              row.get(6).ok_or(AppError::Database("Missing engine_on".to_string()))?,
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
            let ts_val: mysql::Value = row.get(0).ok_or(AppError::Database("Missing timestamp in environmental_data".to_string()))?;
            env_metrics.push(ExportEnvMetric {
                timestamp: mysql_datetime_to_iso(&ts_val)?,
                metric_id: row.get(1).ok_or(AppError::Database("Missing metric_id".to_string()))?,
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

        Ok(serde_json::to_string(&export_data)?)
    }

    pub fn export_trip<P: AsRef<std::path::Path>>(&self, trip_id: i64, output_path: P) -> Result<(), AppError> {
        use std::fs::File;
        use std::io::BufWriter;

        let json = self.export_trip_to_string(trip_id)?;

        if let Some(parent) = std::path::Path::new(output_path.as_ref()).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        // BufWriter batches small writes into fewer syscalls.
        let file = File::create(&output_path)?;
        file.metadata()?; // ensure file is created
        let mut writer = BufWriter::new(file);
        std::io::Write::write_all(&mut writer, json.as_bytes())?;

        info!("Trip {} exported successfully to: {}", trip_id, output_path.as_ref().display());
        Ok(())
    }

    pub fn import_trip(&self, json_data: &str) -> Result<i64, AppError> {
        use serde_json::Value;
        
        let json: Value = serde_json::from_str(json_data)?;
        
        let trip = &json["trip"];
        let vessel_statuses = &json["vs"];
        let env_metrics = &json["em"];
        
        let description = trip["desc"].as_str()
            .ok_or(AppError::Database("Missing or invalid trip.desc".to_string()))?;
        let start_ts_str = trip["start"].as_str()
            .ok_or(AppError::Database("Missing or invalid trip.start".to_string()))?;
        let end_ts_str = trip["end"].as_str()
            .ok_or(AppError::Database("Missing or invalid trip.end".to_string()))?;
        let total_distance_sailed = trip["dist_sail"].as_f64()
            .ok_or(AppError::Database("Missing or invalid trip.dist_sail".to_string()))?;
        let total_distance_motoring = trip["dist_motor"].as_f64()
            .ok_or(AppError::Database("Missing or invalid trip.dist_motor".to_string()))?;
        let total_time_sailing = trip["t_sail"].as_u64()
            .ok_or(AppError::Database("Missing or invalid trip.t_sail".to_string()))?;
        let total_time_motoring = trip["t_motor"].as_u64()
            .ok_or(AppError::Database("Missing or invalid trip.t_motor".to_string()))?;
        let total_time_moored = trip["t_moor"].as_u64()
            .ok_or(AppError::Database("Missing or invalid trip.t_moor".to_string()))?;
        let import_uuid: Option<&str> = trip["uuid"].as_str();
        
        let mut conn = self.pool.get_conn()?;
        
        let new_trip_start = chrono::DateTime::parse_from_rfc3339(start_ts_str)
            .map_err(|e| AppError::Database(format!("Invalid start_timestamp format: {}", e)))?;

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
                return Err(AppError::Database(format!(
                    "Trip overlaps with existing trip ID {}. Existing trip ends at {}, new trip starts at {}",
                    existing_id, existing_end_ts, start_ts_str
                )));
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
        
        let new_trip_id = tx.last_insert_id().ok_or(AppError::Database("Failed to get inserted trip ID".to_string()))? as i64;
        
        // Insert vessel statuses
        let mut imported_status_count = 0usize;
        if let Some(statuses) = vessel_statuses.as_array() {
            imported_status_count = statuses.len();
            for status in statuses {
                let timestamp = status["ts"].as_str()
                    .ok_or(AppError::Database("Missing ts in vessel_status".to_string()))?;
                let latitude = status["lat"].as_f64();
                let longitude = status["lon"].as_f64();
                let avg_speed = status["sog"].as_f64();
                let max_speed = status["sog_max"].as_f64();
                let is_moored = status["moor"].as_bool()
                    .ok_or(AppError::Database("Missing moor in vessel_status".to_string()))?;
                let engine_on: u8 = match &status["eng"] {
                    v if v.is_boolean() => if v.as_bool().unwrap_or(false) { 1 } else { 0 },
                    v if v.is_u64() => v.as_u64().unwrap_or(2) as u8,
                    _ => 2,
                };
                let total_dist = status["dist"].as_f64();
                let total_time = status["dur"].as_u64();
                let wind_speed = status["tws"].as_f64();
                let wind_angle = status["twa"].as_f64();
                let cog = status["cog"].as_f64();
                let heading = status["hdg"].as_f64();
                
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
                let timestamp = metric["ts"].as_str()
                    .ok_or(AppError::Database("Missing ts in environmental_data".to_string()))?;
                let metric_id = metric["mid"].as_u64()
                    .ok_or(AppError::Database("Missing mid in environmental_data".to_string()))? as u8;
                let value_avg = metric["avg"].as_f64().map(|v| v as f32);
                let value_max = metric["max"].as_f64().map(|v| v as f32);
                let value_min = metric["min"].as_f64().map(|v| v as f32);
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

        let trip_end = chrono::DateTime::parse_from_rfc3339(end_ts_str)
            .map_err(|e| AppError::Database(format!("Invalid end_timestamp format: {}", e)))?;

        // The export format carries only the sailing/motoring trip-level summary, not the
        // point-of-sail breakdown, so recompute every trip aggregate (including the six
        // upwind/reaching/running columns) from the vessel_status rows just imported.
        // Deriving from the raw rows keeps the JSON format unchanged in both directions.
        // Skipped when the file carried no vessel_status rows: there would be nothing to
        // recompute from, and the SUM() aggregates over an empty set come back NULL.
        if imported_status_count > 0 {
            self.recalculate_and_update_trip(
                new_trip_id,
                std::time::SystemTime::from(new_trip_start.with_timezone(&chrono::Utc)),
                std::time::SystemTime::from(trip_end.with_timezone(&chrono::Utc)),
            )?;
        }

        // Invalidate the heatmap cache for every day the imported trip spans so that
        // fetch_heatmap() recomputes those days from the newly inserted vessel_status rows.
        let trip_start_date = new_trip_start.date_naive();
        let trip_end_date = trip_end.date_naive();
        self.invalidate_heatmap_cache(trip_start_date, trip_end_date)?;

        info!("Trip imported successfully with ID: {}", new_trip_id);
        Ok(new_trip_id)
    }
}

#[cfg(test)]
mod tests {
    use crate::db::test_helpers::{assert_approx_equal, setup_db};
    use mysql::params;
    use mysql::prelude::Queryable;

    /// The export format carries no point-of-sail breakdown, so import must derive it
    /// from the imported vessel_status rows' wind angles.
    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_import_trip_computes_point_of_sail() {
        let db = setup_db();

        // Three sailing rows spanning all three buckets (folded TWA 30 / 90 / 160),
        // plus one motoring row with an upwind angle that must not be counted.
        let json = r#"{
            "trip": {
                "desc": "POS Import",
                "start": "2020-05-01T10:00:00.000Z",
                "end": "2020-05-01T10:05:00.000Z",
                "dist_sail": 0.0,
                "dist_motor": 0.0,
                "t_sail": 0,
                "t_motor": 0,
                "t_moor": 0,
                "uuid": "11111111-2222-3333-4444-555555555555"
            },
            "vs": [
                {"ts":"2020-05-01T10:00:00.000Z","lat":43.0,"lon":11.0,"sog":6.0,"sog_max":6.5,
                 "moor":false,"eng":0,"dist":2.0,"dur":60000,"tws":12.0,"twa":30.0},
                {"ts":"2020-05-01T10:01:00.000Z","lat":43.1,"lon":11.0,"sog":6.0,"sog_max":6.5,
                 "moor":false,"eng":0,"dist":3.0,"dur":60000,"tws":12.0,"twa":90.0},
                {"ts":"2020-05-01T10:02:00.000Z","lat":43.2,"lon":11.0,"sog":6.0,"sog_max":6.5,
                 "moor":false,"eng":0,"dist":1.0,"dur":30000,"tws":12.0,"twa":200.0},
                {"ts":"2020-05-01T10:03:00.000Z","lat":43.3,"lon":11.0,"sog":6.0,"sog_max":6.5,
                 "moor":false,"eng":1,"dist":4.0,"dur":60000,"tws":12.0,"twa":30.0}
            ],
            "em": []
        }"#;

        let trip_id = db.import_trip(json).expect("import_trip should succeed");

        let mut conn = db.pool.get_conn().unwrap();
        let row: (f64, f64, f64, f64, f64, u64, u64, u64) = conn
            .exec_first(
                "SELECT total_distance_sailed, total_distance_motoring,
                        total_distance_upwind, total_distance_reaching, total_distance_running,
                        total_time_upwind, total_time_reaching, total_time_running
                 FROM trips WHERE id = :id",
                params! { "id" => trip_id },
            )
            .unwrap()
            .expect("imported trip should exist");

        assert_approx_equal(row.0, 6.0, 0.001, "total_distance_sailed");
        assert_approx_equal(row.1, 4.0, 0.001, "total_distance_motoring");
        assert_approx_equal(row.2, 2.0, 0.001, "total_distance_upwind");
        assert_approx_equal(row.3, 3.0, 0.001, "total_distance_reaching");
        assert_approx_equal(row.4, 1.0, 0.001, "total_distance_running");
        assert_eq!(row.5, 60_000, "total_time_upwind");
        assert_eq!(row.6, 60_000, "total_time_reaching");
        assert_eq!(row.7, 30_000, "total_time_running");
    }
}
