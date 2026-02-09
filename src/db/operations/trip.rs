use crate::db::types::VesselDatabase;
use std::error::Error;
use chrono::NaiveDateTime;
use mysql::params;
use mysql::prelude::Queryable;

impl VesselDatabase {
    pub fn update_trip_description(&self, trip_id: i64, new_description: &str) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        let query = "UPDATE trips SET description = :description WHERE id = :id";
        conn.exec_drop(query, params! {
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
            params! {
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
            params! {
                "start" => &start_timestamp,
                "end" => &end_timestamp,
            },
        ).map_err(|e| format!("Failed to delete environmental data: {}", e))?;

        // Delete vessel status data in the time range
        conn.exec_drop(
            r"DELETE FROM vessel_status 
              WHERE timestamp >= :start AND timestamp <= :end",
            params! {
                "start" => &start_timestamp,
                "end" => &end_timestamp,
            },
        ).map_err(|e| format!("Failed to delete vessel status data: {}", e))?;

        // Delete the trip record
        conn.exec_drop(
            r"DELETE FROM trips WHERE id = :trip_id",
            params! {
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
            params! {
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
            params! {
                "start" => &original_start,
                "end" => &original_end,
            },
        ).map_err(|e| format!("Failed to find first moving timestamp: {}", e))?;

        // Find when the boat gets permanently moored: find last is_moored = 0, then first is_moored = 1 after that
        let last_moving: Option<mysql::Row> = conn.exec_first(
            r"SELECT timestamp FROM vessel_status 
              WHERE timestamp >= :start AND timestamp <= :end AND is_moored = 0
              ORDER BY timestamp DESC LIMIT 1",
            params! {
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
                params! {
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
            params! {
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
            params! {
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
            params! {
                "new_start" => &new_start,
                "new_end" => &new_end,
                "trip_id" => trip_id,
            },
        ).map_err(|e| format!("Failed to update trip: {}", e))?;

        Ok(())
    }
}
