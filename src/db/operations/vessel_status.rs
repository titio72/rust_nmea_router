use crate::db::types::{VesselDatabase, VesselStatusOperation, TripOperation};
use crate::trip::Trip;
use std::error::Error;
use std::time::SystemTime;
use mysql::params;
use chrono::NaiveDateTime;
use crate::utilities::dirty_instant_to_systemtime;
use mysql::prelude::Queryable;

impl VesselDatabase {
    /// Insert vessel status and create/update trip in a single transaction
    /// This ensures atomicity - either both operations succeed or both fail
    pub fn insert_status_and_trip(
        &self,
        status_op: &VesselStatusOperation,
        trip_operation: &TripOperation,
    ) -> Result<Option<i64>, Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        let mut tx = conn.start_transaction(mysql::TxOpts::default())?;
        
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
                "engine_on" => status_op.engine_on.as_u8(),
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
        use crate::position_utils::Position;
        
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
            let engine_on_u8: u8 = row.get(6).ok_or("Failed to get engine_on")?;
            let engine_on = crate::utilities::EngineStatus::from_u8(engine_on_u8);
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
}
