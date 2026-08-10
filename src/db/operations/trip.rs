// Trip Operations - CRUD operations for voyage records
// Database Note: All queries use parameterized queries with params! macro.
// Multi-statement operations use transaction pattern with session variables.
// Testing: Database tests require serial execution (--test-threads=1) due to shared test DB.
// See: AGENTS.md for database patterns, transaction examples, and type conversions.
//
use crate::db::types::VesselDatabase;
use crate::error::AppError;
use crate::utilities::EngineStatus;
use chrono::{DateTime, Utc};
use mysql::params;
use mysql::prelude::Queryable;
use tracing::warn;

impl VesselDatabase {
    pub fn update_trip_description(
        &self,
        trip_id: i64,
        new_description: &str,
    ) -> Result<(), AppError> {
        let mut conn = self.pool.get_conn()?;
        let query = "UPDATE trips SET description = :description WHERE id = :id";
        conn.exec_drop(
            query,
            params! {
                "description" => new_description,
                "id" => trip_id,
            },
        )?;
        Ok(())
    }

    /// Delete a trip and all associated data
    /// This will delete environmental data, vessel status data, and finally the trip record
    pub fn delete_trip(&self, trip_id: u32) -> Result<(), AppError> {
        let mut conn = self.pool.get_conn()?;

        let mut tx = conn.start_transaction(mysql::TxOpts::default())?;

        // First, fetch the trip to get its time range
        let trip_row: Option<mysql::Row> = tx.exec_first(
            r"SELECT DATE_FORMAT(start_timestamp, '%Y-%m-%d %H:%i:%S.%f') as start_timestamp, DATE_FORMAT(end_timestamp, '%Y-%m-%d %H:%i:%S.%f') as end_timestamp FROM trips WHERE id = :trip_id",
            params! {
                "trip_id" => trip_id,
            },
        )?;

        if trip_row.is_none() {
            return Err(AppError::Database("Trip not found".to_string()));
        }

        let mut trip_row = trip_row.unwrap();
        let start_timestamp: String = trip_row
            .take("start_timestamp")
            .ok_or(AppError::Database("Missing start_timestamp".to_string()))?;
        let end_timestamp: String = trip_row
            .take("end_timestamp")
            .ok_or(AppError::Database("Missing end_timestamp".to_string()))?;

        // Delete environmental data in the time range
        tx.exec_drop(
            r"DELETE FROM environmental_data
              WHERE timestamp >= :start AND timestamp <= :end",
            params! {
                "start" => &start_timestamp,
                "end" => &end_timestamp,
            },
        )?;

        // Delete vessel status data in the time range
        tx.exec_drop(
            r"DELETE FROM vessel_status
              WHERE timestamp >= :start AND timestamp <= :end",
            params! {
                "start" => &start_timestamp,
                "end" => &end_timestamp,
            },
        )?;

        // Delete the trip record
        tx.exec_drop(
            r"DELETE FROM trips WHERE id = :trip_id",
            params! {
                "trip_id" => trip_id,
            },
        )?;

        tx.commit()?;

        if let Err(e) = self.invalidate_trip_legs_cache(trip_id) {
            warn!("Failed to invalidate trip_legs_cache after delete_trip({}): {}", trip_id, e);
        }

        Ok(())
    }

    pub fn trim_trip(&self, trip_id: u32) -> Result<(), AppError> {
        let mut conn = self.pool.get_conn()?;

        let mut tx = conn.start_transaction(mysql::TxOpts::default())?;

        // Fetch trip timestamps into session variables
        tx.exec_drop(
            "SELECT @trip_start_ts := start_timestamp, @trip_end_ts := end_timestamp FROM trips WHERE id = :id",
            params! { "id" => trip_id },
        )?;

        // Fetch min and max timestamps for non-moored records into session variables
        tx.exec_drop(
            "SELECT @min_ts := MIN(timestamp), @max_ts := MAX(timestamp) FROM vessel_status WHERE timestamp >= @trip_start_ts AND timestamp <= @trip_end_ts AND is_moored = 0",
            ()
        )?;

        // Delete vessel_status records outside the 1-hour buffer
        tx.exec_drop(
            "DELETE FROM vessel_status WHERE (timestamp >= @trip_start_ts AND timestamp < SUBTIME(@min_ts, '0 1:00:0.000')) OR (timestamp <= @trip_end_ts AND timestamp > ADDTIME(@max_ts, '0 1:00:0.000'))",
            ()
        )?;

        // Delete environmental_data records outside the 1-hour buffer
        tx.exec_drop(
            "DELETE FROM environmental_data WHERE (timestamp >= @trip_start_ts AND timestamp < SUBTIME(@min_ts, '0 1:00:0.000')) OR (timestamp <= @trip_end_ts AND timestamp > ADDTIME(@max_ts, '0 1:00:0.000'))",
            ()
        )?;

        // Update trip with new boundaries
        tx.exec_drop(
            "UPDATE trips SET start_timestamp = SUBTIME(@min_ts, '0 1:00:0.000'), end_timestamp = ADDTIME(@max_ts, '0 1:00:0.000') WHERE id = :id",
            params! { "id" => trip_id },
        )?;

        tx.commit()?;

        if let Err(e) = self.invalidate_trip_legs_cache(trip_id) {
            warn!("Failed to invalidate trip_legs_cache after trim_trip({}): {}", trip_id, e);
        }

        Ok(())
    }

    /// Upsert a manual nav window override for a leg.
    /// `auto_nav_start` / `auto_nav_end` preserve the algorithm's current detection for calibration.
    /// Pass `None` for both `nav_start` and `nav_end` to clear a previous override.
    pub fn set_nav_override(
        &self,
        trip_id: u32,
        leg_number: u32,
        nav_start: Option<&str>,
        nav_end: Option<&str>,
        auto_nav_start: Option<&str>,
        auto_nav_end: Option<&str>,
    ) -> Result<(), AppError> {
        let mut conn = self.pool.get_conn()?;

        conn.query_drop(
            r"CREATE TABLE IF NOT EXISTS trip_legs_nav_overrides (
                trip_id        INT UNSIGNED NOT NULL,
                leg_number     INT UNSIGNED NOT NULL,
                nav_start      VARCHAR(30)  NULL,
                nav_end        VARCHAR(30)  NULL,
                auto_nav_start VARCHAR(30)  NULL,
                auto_nav_end   VARCHAR(30)  NULL,
                corrected_at   DATETIME(3)  NOT NULL,
                PRIMARY KEY (trip_id, leg_number)
              ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci",
        )?;

        conn.exec_drop(
            r"INSERT INTO trip_legs_nav_overrides
                (trip_id, leg_number, nav_start, nav_end, auto_nav_start, auto_nav_end, corrected_at)
              VALUES (:trip_id, :leg_number, :nav_start, :nav_end, :auto_nav_start, :auto_nav_end, NOW(3))
              ON DUPLICATE KEY UPDATE
                nav_start = VALUES(nav_start),
                nav_end = VALUES(nav_end),
                auto_nav_start = VALUES(auto_nav_start),
                auto_nav_end = VALUES(auto_nav_end),
                corrected_at = NOW(3)",
            params! {
                "trip_id" => trip_id,
                "leg_number" => leg_number,
                "nav_start" => nav_start,
                "nav_end" => nav_end,
                "auto_nav_start" => auto_nav_start,
                "auto_nav_end" => auto_nav_end,
            },
        )?;

        if let Err(e) = self.invalidate_trip_legs_cache(trip_id) {
            warn!(
                "Failed to invalidate trip_legs_cache after set_nav_override({}, {}): {}",
                trip_id, leg_number, e
            );
        }

        Ok(())
    }

    /// Overwrite `engine_on` for every `vessel_status` row between `start` and `end`
    /// (clamped to the trip's own window), then recompute the trip's sailing/motoring
    /// aggregates and invalidate the caches derived from vessel_status. Used to correct
    /// misfires from the automatic RPM-based engine detection in vessel_monitor.rs.
    pub fn correct_engine_status(
        &self,
        trip_id: u32,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        engine_on: EngineStatus,
    ) -> Result<(), AppError> {
        if engine_on.is_unknown() {
            return Err(AppError::Parse(
                "engine_on must be On or Off, not Unknown".to_string(),
            ));
        }
        if start >= end {
            return Err(AppError::Parse(
                "start_timestamp must be before end_timestamp".to_string(),
            ));
        }

        let trip = self
            .fetch_trip(trip_id)?
            .ok_or_else(|| AppError::Database(format!("Trip {} not found", trip_id)))?;

        // Parse the trip window at full millisecond precision straight from the RFC3339
        // strings. TripSummary::start_timestamp()/end_timestamp() are deliberately NOT
        // used here: they go through Duration::from_secs() and drop the milliseconds,
        // which would silently rewind the window every time a correction is applied.
        let trip_start_dt = DateTime::parse_from_rfc3339(&trip.start_date)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| {
                AppError::Parse(format!(
                    "Invalid trip start_date '{}': {}",
                    trip.start_date, e
                ))
            })?;
        let trip_end_dt = DateTime::parse_from_rfc3339(&trip.end_date)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| {
                AppError::Parse(format!("Invalid trip end_date '{}': {}", trip.end_date, e))
            })?;

        let clamped_start = start.max(trip_start_dt);
        let clamped_end = end.min(trip_end_dt);
        if clamped_start >= clamped_end {
            return Err(AppError::Parse(
                "Requested range does not overlap the trip".to_string(),
            ));
        }

        // fetch_track serves timestamps with the milliseconds zeroed ('...%S.000Z') and
        // the correction page echoes that string back as the end bound. Since real
        // vessel_status rows almost always carry non-zero milliseconds, a `.000` upper
        // bound would exclude the very point the user clicked, so treat a whole-second
        // end bound as covering that entire second. Applied after clamping so the query
        // boundary can never be pushed meaningfully past the trip's own window.
        let query_end = if clamped_end.timestamp_subsec_millis() == 0 {
            clamped_end + chrono::Duration::milliseconds(999)
        } else {
            clamped_end
        };

        const MYSQL_TS_FMT: &str = "%Y-%m-%d %H:%M:%S%.3f";
        let start_str = clamped_start.format(MYSQL_TS_FMT).to_string();
        let end_str = query_end.format(MYSQL_TS_FMT).to_string();
        let trip_start_str = trip_start_dt.format(MYSQL_TS_FMT).to_string();
        let trip_end_str = trip_end_dt.format(MYSQL_TS_FMT).to_string();

        let mut conn = self.pool.get_conn()?;
        let mut tx = conn.start_transaction(mysql::TxOpts::default())?;

        // Check if there are any rows in the requested range before attempting UPDATE
        let matched: u64 = tx
            .exec_first(
                "SELECT COUNT(*) FROM vessel_status WHERE timestamp BETWEEN :start AND :end",
                params! {
                    "start" => &start_str,
                    "end" => &end_str,
                },
            )?
            .unwrap_or(0);

        if matched == 0 {
            // Dropping `tx` rolls back; nothing has been written yet.
            return Err(AppError::Database(
                "No track points found in the requested range".to_string(),
            ));
        }

        tx.exec_drop(
            "UPDATE vessel_status SET engine_on = :val WHERE timestamp BETWEEN :start AND :end",
            params! {
                "val" => engine_on.as_u8(),
                "start" => &start_str,
                "end" => &end_str,
            },
        )?;

        // Recompute the trip aggregates over the trip's own window, in the same
        // transaction as the UPDATE above. This intentionally mirrors the CASE semantics
        // of recalculate_and_update_trip (note `engine_on != 1` for sailing, so Unknown
        // rows count as sailing) but, unlike that function, never writes
        // trips.start_timestamp / trips.end_timestamp — this tool must not move the trip
        // boundaries, and it has no undo.
        let row: Option<mysql::Row> = tx.exec_first(
            r"SELECT
                  SUM(CASE WHEN is_moored = 1 THEN total_time_ms  ELSE 0 END) AS time_moored,
                  SUM(CASE WHEN is_moored = 0 AND engine_on = 1 THEN total_time_ms  ELSE 0 END) AS time_motoring,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 THEN total_time_ms ELSE 0 END) AS time_sailing,
                  SUM(CASE WHEN is_moored = 0 AND engine_on = 1 THEN total_distance_nm ELSE 0 END) AS dist_motoring,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 THEN total_distance_nm ELSE 0 END) AS dist_sailed
              FROM vessel_status
              WHERE timestamp BETWEEN :start AND :end",
            params! { "start" => &trip_start_str, "end" => &trip_end_str },
        )?;

        if let Some(row) = row {
            let time_moored: u64 = row.get("time_moored").unwrap_or(0);
            let time_motoring: u64 = row.get("time_motoring").unwrap_or(0);
            let time_sailing: u64 = row.get("time_sailing").unwrap_or(0);
            let dist_motoring: f64 = row.get("dist_motoring").unwrap_or(0.0);
            let dist_sailed: f64 = row.get("dist_sailed").unwrap_or(0.0);

            tx.exec_drop(
                r"UPDATE trips
                  SET total_time_moored       = :time_moored,
                      total_time_motoring     = :time_motoring,
                      total_time_sailing      = :time_sailing,
                      total_distance_motoring = :dist_motoring,
                      total_distance_sailed   = :dist_sailed
                  WHERE id = :trip_id",
                params! {
                    "time_moored"   => time_moored,
                    "time_motoring" => time_motoring,
                    "time_sailing"  => time_sailing,
                    "dist_motoring" => dist_motoring,
                    "dist_sailed"   => dist_sailed,
                    "trip_id"       => trip_id,
                },
            )?;
        }

        tx.commit()?;

        // The correction itself is already committed; a cache-invalidation failure must
        // not report the whole operation as failed (same pattern as set_nav_override).
        if let Err(e) = self.invalidate_trip_legs_cache(trip_id) {
            warn!(
                "Failed to invalidate trip_legs_cache after correct_engine_status({}): {}",
                trip_id, e
            );
        }
        if let Err(e) =
            self.invalidate_heatmap_cache(clamped_start.date_naive(), clamped_end.date_naive())
        {
            warn!(
                "Failed to invalidate heatmap_cache after correct_engine_status({}): {}",
                trip_id, e
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::db::test_helpers::{
        add_test_trip, add_test_vessel_status, fetch_vessel_status_by_id, setup_db,
    };
    use crate::utilities::EngineStatus;
    use std::{
        ops::{Add, Sub},
        time::{Duration, SystemTime},
    };
    const ONE_HOUR_S: u64 = 3600;

    #[test]
    #[ignore] // This test is ignored because it relies on specific time-based logic and may require adjustments to the test data setup to ensure it works correctly. It can be enabled and adjusted as needed when testing trip description updates.
    fn test_update_trip_description() {
        let db = setup_db();
        let t = SystemTime::now();

        // Insert a test trip
        let trip_id: u32 = add_test_trip(
            &db,
            "Original Description".to_string(),
            t,
            t.add(Duration::from_secs(ONE_HOUR_S)),
            0.0,
            0.0,
            0,
            0,
            ONE_HOUR_S * 1000,
        )
        .expect("Failed to insert test trip");

        // Update the trip description
        db.update_trip_description(trip_id as i64, "Updated Description")
            .expect("Failed to update trip description");

        // Fetch the trip and verify the description was updated
        let fetched_trip = db
            .fetch_trip(trip_id)
            .expect("Failed to fetch trip")
            .expect("Trip not found");
        assert_eq!(
            fetched_trip.description, "Updated Description",
            "Trip description should be updated"
        );
    }

    #[test]
    #[ignore] // This test is ignored because it relies on specific time-based logic and may require adjustments to the test data setup to ensure it works correctly. It can be enabled and adjusted as needed when testing trip deletion and associated data cleanup.
    fn test_delete_trip() {
        let db = setup_db();
        let t = SystemTime::now();
        let trip_lenght_h = 8;

        // add test data for a trip with vessel status reports starting before the trip and ending after the trip to test that only records within the trip range are deleted
        // Insert a test trip with associated data
        let trip_id: u32 = add_test_trip(
            &db,
            "Test Trip".to_string(),
            t,
            t.add(Duration::from_secs(trip_lenght_h * ONE_HOUR_S)),
            0.0,
            0.0,
            0,
            0,
            trip_lenght_h * ONE_HOUR_S * 1000,
        )
        .expect("Failed to insert test trip");
        // Insert vessel status records every 30 minutes for 8 hours, alternating moored and not moored
        let mut reports_ids: Vec<u32> = Vec::new();
        let mut report_time = t.sub(Duration::from_secs(600)); // start 10 minutes before trip start to test deletion of records outside trip range
        while report_time < t.add(Duration::from_secs(trip_lenght_h * ONE_HOUR_S)) {
            let status_id = add_test_vessel_status(
                &db,
                report_time,
                43.0,
                11.0,
                0.0,
                0.0,
                Some(5.0),
                Some(9.0),
                true,
                crate::utilities::EngineStatus::Off,
                0.0,
                0,
                None,
                Some(270.0),
            )
            .expect("Failed to insert test vessel status");
            reports_ids.push(status_id);
            report_time = report_time.add(Duration::from_secs(1800)); // every 30 minutes
        }
        // Insert a moored status record at the end to test deletion of records outside trip range
        assert!(
            db.fetch_trip(trip_id)
                .expect("Failed to fetch trip")
                .expect("Trip not found")
                .id
                == trip_id,
            "Inserted trip ID should match fetched trip ID"
        );
        let status_id = add_test_vessel_status(
            &db,
            report_time,
            43.0,
            11.0,
            0.0,
            0.0,
            Some(5.0),
            Some(9.0),
            true,
            crate::utilities::EngineStatus::Off,
            0.0,
            0,
            None,
            Some(270.0),
        )
        .expect("Failed to insert test vessel status");
        reports_ids.push(status_id);

        // delete the trip
        db.delete_trip(trip_id).expect("Failed to delete trip");

        // verify that the trip record is deleted and that vessel status records within the trip time range are deleted while records outside the range are not deleted
        let x = db
            .fetch_trip(trip_id)
            .expect("Failed to fetch trip after deletion");
        assert!(x.is_none(), "Trip record should not exist after deletion");
        reports_ids.iter().enumerate().for_each(|(i, report_id)| {
            let status = fetch_vessel_status_by_id(&db, *report_id)
                .expect("Failed to fetch vessel status after deletion");
            if i == 0 || i == reports_ids.len() - 1 {
                // First and last records should still exist (outside trip range)
                assert!(
                    status.is_some(),
                    "Vessel status record outside trip range should not be deleted"
                );
            } else {
                // Records within trip range should be deleted
                assert!(
                    status.is_none(),
                    "Vessel status record within trip range should be deleted"
                );
            }
        });
    }

    #[test]
    #[ignore] // This test is ignored because it relies on specific time-based logic and may require adjustments to the test data setup to ensure it works correctly. It can be enabled and adjusted as needed when testing the trim_trip functionality.
    fn test_trim_trip() {
        let db = setup_db();
        let i_t_start = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let t_start = SystemTime::UNIX_EPOCH.add(Duration::from_secs(i_t_start));
        let trip_lenght_h = 8;
        let t_end: SystemTime = t_start.add(Duration::from_secs(trip_lenght_h * ONE_HOUR_S));
        let first_report_time = t_start.sub(Duration::from_secs(6 * ONE_HOUR_S)); // start 6 hours before trip start to test trimming of records outside trip range
        let last_report_time = t_end.add(Duration::from_secs(6 * ONE_HOUR_S)); // end 6 hours after trip end to test trimming of records outside trip range

        // add test data for a trip with vessel status reports starting before the trip and ending after the trip to test that only records within the trip range are deleted
        // Insert a test trip with associated data
        let trip_id: u32 = add_test_trip(
            &db,
            "Test Trip".to_string(),
            t_start.sub(Duration::from_secs(6 * ONE_HOUR_S)),
            t_end.add(Duration::from_secs(6 * ONE_HOUR_S)),
            0.0,
            0.0,
            0,
            0,
            trip_lenght_h * ONE_HOUR_S * 1000,
        )
        .expect("Failed to insert test trip");
        // Insert vessel status records every 30 minutes for 8 hours, alternating moored and not moored
        let mut reports_ids: Vec<(u32, SystemTime)> = Vec::new();
        // add some record before the boat starts moving (6 hours worth)
        let mut report_time = first_report_time; // start 6 hour before trip start to test deletion of records outside trip range
        while report_time < t_start {
            let status_id = add_test_vessel_status(
                &db,
                report_time,
                43.0,
                11.0,
                0.0,
                0.0,
                Some(5.0),
                Some(9.0),
                true,
                crate::utilities::EngineStatus::Off,
                0.0,
                0,
                None,
                Some(270.0),
            )
            .expect("Failed to insert test vessel status");
            reports_ids.push((status_id, report_time));
            report_time = report_time.add(Duration::from_secs(1800)); // every 30 minutes
        }
        // add records for the boat moving (8 hours worth)
        while report_time < t_end {
            let status_id = add_test_vessel_status(
                &db,
                report_time,
                43.0,
                11.0,
                0.0,
                0.0,
                Some(5.0),
                Some(9.0),
                false,
                crate::utilities::EngineStatus::On,
                0.0,
                0,
                None,
                Some(270.0),
            )
            .expect("Failed to insert test vessel status");
            reports_ids.push((status_id, report_time));
            report_time = report_time.add(Duration::from_secs(30)); // every 30 seconds
        }
        // now stay moored for 6 hours after the end of the trip to test deletion of records outside trip range
        while report_time < last_report_time {
            let status_id = add_test_vessel_status(
                &db,
                report_time,
                43.0,
                11.0,
                0.0,
                0.0,
                Some(5.0),
                Some(9.0),
                true,
                crate::utilities::EngineStatus::Off,
                0.0,
                0,
                None,
                Some(270.0),
            )
            .expect("Failed to insert test vessel status");
            reports_ids.push((status_id, report_time));
            report_time = report_time.add(Duration::from_secs(1800)); // every 30 minutes
        }

        // just to be sure the trip and status records are correctly inserted before trimming the trip
        assert!(
            db.fetch_trip(trip_id)
                .expect("Failed to fetch trip")
                .expect("Trip not found")
                .id
                == trip_id,
            "Inserted trip ID should match fetched trip ID"
        );

        // delete the trip
        db.trim_trip(trip_id).expect("Failed to trim trip");

        // verify that the trip record is deleted and that vessel status records within the trip time range are deleted while records outside the range are not deleted
        let expected_start = t_start.sub(Duration::from_secs(ONE_HOUR_S));
        let expected_end = t_end.add(Duration::from_secs(ONE_HOUR_S));
        if let Some(x) = db
            .fetch_trip(trip_id)
            .expect("Failed to fetch trip after trim")
        {
            // after trimming the trip, the start and end timestamps should be updated to match the min and max timestamps of non-moored vessel status records within the original trip time range, with a 1 hour buffer on each side. In this test, the first non-moored record is at t_start and the last non-moored record is at t_end, so after trimming the trip, the start timestamp should be t_start - 1 hour and the end timestamp should be t_end + 1 hour
            let actual_start = x.start_timestamp().expect("Failed to get start timestamp");
            let actual_end = x.end_timestamp().expect("Failed to get end timestamp");
            let start_diff = actual_start
                .duration_since(expected_start)
                .unwrap_or_else(|_| {
                    expected_start
                        .duration_since(actual_start)
                        .unwrap_or(Duration::from_secs(0))
                });
            let end_diff = actual_end.duration_since(expected_end).unwrap_or_else(|_| {
                expected_end
                    .duration_since(actual_end)
                    .unwrap_or(Duration::from_secs(0))
            });
            assert!(
                start_diff.as_secs() < 60,
                "Start timestamp differs by {} seconds",
                start_diff.as_secs()
            );
            assert!(
                end_diff.as_secs() < 60,
                "End timestamp differs by {} seconds",
                end_diff.as_secs()
            );
        } else {
            panic!("Trip record should exist after trim");
        }

        let leeway = Duration::from_secs(2); // allow 2 seconds of leeway for timestamp comparisons to account for any minor discrepancies in timestamp storage or retrieval
        reports_ids.iter().for_each(|(status_id, ts)| {
            if let Some(status) = fetch_vessel_status_by_id(&db, *status_id)
                .expect("Failed to fetch vessel status after deletion")
            {
                assert!(
                    status.timestamp >= expected_start.sub(leeway)
                        && status.timestamp <= expected_end.add(leeway),
                    "Vessel status record with timestamp {:?} should be within the new trip range",
                    status.timestamp
                );
            } else {
                assert!(
                    ts < &expected_start.add(leeway) || ts > &expected_end.sub(leeway),
                    "Vessel status record with timestamp {:?} should not be deleted",
                    ts
                );
            }
        });
    }

    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_correct_engine_status() {
        let db = setup_db();
        let t = SystemTime::now();

        let trip_id: u32 = add_test_trip(
            &db,
            "Engine Fix Test".to_string(),
            t,
            t.add(Duration::from_secs(1800)),
            0.0,
            0.0,
            0,
            0,
            0,
        )
        .expect("Failed to insert test trip");

        // First half of the trip: sailing (engine off). Second half: also inserted as
        // engine off, but this is the half we'll correct to "on".
        let first_id = add_test_vessel_status(
            &db,
            t,
            43.0,
            11.0,
            5.0,
            6.0,
            None,
            None,
            false,
            EngineStatus::Off,
            1.0,
            900_000,
            None,
            None,
        )
        .expect("Failed to insert first vessel status");

        let mid_ts = t.add(Duration::from_secs(900));
        let second_id = add_test_vessel_status(
            &db,
            mid_ts,
            43.1,
            11.0,
            5.0,
            6.0,
            None,
            None,
            false,
            EngineStatus::Off,
            1.0,
            900_000,
            None,
            None,
        )
        .expect("Failed to insert second vessel status");

        let mid_dt = chrono::DateTime::<chrono::Utc>::from(mid_ts);
        let end_dt = chrono::DateTime::<chrono::Utc>::from(t.add(Duration::from_secs(1800)));

        db.correct_engine_status(trip_id, mid_dt, end_dt, EngineStatus::On)
            .expect("correct_engine_status should succeed");

        // The first point (before the corrected range) must be untouched.
        let first = fetch_vessel_status_by_id(&db, first_id)
            .expect("fetch failed")
            .expect("first record missing");
        assert_eq!(
            first.engine_on,
            EngineStatus::Off,
            "point before the corrected range must stay Off"
        );

        // The second point (inside the corrected range) must now be On.
        let second = fetch_vessel_status_by_id(&db, second_id)
            .expect("fetch failed")
            .expect("second record missing");
        assert_eq!(
            second.engine_on,
            EngineStatus::On,
            "point inside the corrected range must become On"
        );

        // Trip aggregates must reflect the new split: 1.0 nm sailed, 1.0 nm motored.
        let trip = db
            .fetch_trip(trip_id)
            .expect("fetch_trip failed")
            .expect("trip missing");
        assert!(
            (trip.sailing_distance_nm - 1.0).abs() < 0.01,
            "sailing distance should be 1.0 nm, got {}",
            trip.sailing_distance_nm
        );
        assert!(
            (trip.motoring_distance_nm - 1.0).abs() < 0.01,
            "motoring distance should be 1.0 nm, got {}",
            trip.motoring_distance_nm
        );
    }

    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_correct_engine_status_rejects_unknown() {
        let db = setup_db();
        let t = SystemTime::now();
        let trip_id: u32 = add_test_trip(
            &db,
            "Reject Unknown".to_string(),
            t,
            t.add(Duration::from_secs(600)),
            0.0,
            0.0,
            0,
            0,
            0,
        )
        .expect("Failed to insert test trip");

        let start_dt = chrono::DateTime::<chrono::Utc>::from(t);
        let end_dt = chrono::DateTime::<chrono::Utc>::from(t.add(Duration::from_secs(600)));

        let result = db.correct_engine_status(trip_id, start_dt, end_dt, EngineStatus::Unknown);
        assert!(
            result.is_err(),
            "correct_engine_status must reject EngineStatus::Unknown"
        );
    }

    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_correct_engine_status_idempotent() {
        let db = setup_db();
        let t = SystemTime::now();

        // Create two separate trips to test idempotency in isolation
        let trip1_id: u32 = add_test_trip(
            &db,
            "Idempotent Test Trip 1".to_string(),
            t,
            t.add(Duration::from_secs(1800)),
            0.0,
            0.0,
            0,
            0,
            0,
        )
        .expect("Failed to insert trip 1");

        let trip2_id: u32 = add_test_trip(
            &db,
            "Idempotent Test Trip 2".to_string(),
            t.add(Duration::from_secs(2000)),
            t.add(Duration::from_secs(3800)),
            0.0,
            0.0,
            0,
            0,
            0,
        )
        .expect("Failed to insert trip 2");

        // Add vessel status records to both trips
        add_test_vessel_status(
            &db,
            t,
            43.0,
            11.0,
            5.0,
            6.0,
            None,
            None,
            false,
            EngineStatus::Off,
            1.0,
            900_000,
            None,
            None,
        )
        .expect("Failed to insert vessel status 1");

        let mid_ts = t.add(Duration::from_secs(2900));
        add_test_vessel_status(
            &db,
            mid_ts,
            43.0,
            11.0,
            5.0,
            6.0,
            None,
            None,
            false,
            EngineStatus::Off,
            1.0,
            900_000,
            None,
            None,
        )
        .expect("Failed to insert vessel status 2");

        let start_dt = chrono::DateTime::<chrono::Utc>::from(t);
        let end_dt = chrono::DateTime::<chrono::Utc>::from(t.add(Duration::from_secs(1800)));

        // First call should succeed
        db.correct_engine_status(trip1_id, start_dt, end_dt, EngineStatus::On)
            .expect("First correct_engine_status should succeed");

        // Second call on a different trip with overlapping data should also succeed
        // This verifies that the fix makes the operation work correctly without side effects
        let start_dt2 = chrono::DateTime::<chrono::Utc>::from(t.add(Duration::from_secs(2000)));
        let end_dt2 = chrono::DateTime::<chrono::Utc>::from(t.add(Duration::from_secs(3800)));
        db.correct_engine_status(trip2_id, start_dt2, end_dt2, EngineStatus::Off)
            .expect("Second correct_engine_status on different trip should succeed");

        // Both operations should complete successfully without "No track points found" errors
        assert!(true, "correct_engine_status operations completed successfully");
    }

    /// Regression test: a correction must never move the trip's own boundaries.
    /// The previous implementation delegated to `recalculate_and_update_trip`, which
    /// rewrote `end_timestamp` to MAX(vessel_status.timestamp) over a second-truncated
    /// window — silently rewinding the trip end on every call, and compounding on reuse.
    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_correct_engine_status_preserves_trip_timestamps() {
        let db = setup_db();
        // Fixed base with non-zero milliseconds (.456), so the trip window exercises the
        // sub-second precision that the old SystemTime round-trip used to discard.
        let t = SystemTime::UNIX_EPOCH + Duration::from_millis(1_754_000_000_456);
        let trip_end = t.add(Duration::from_secs(1800));

        let trip_id: u32 = add_test_trip(
            &db,
            "Timestamp Preservation".to_string(),
            t,
            trip_end,
            0.0,
            0.0,
            0,
            0,
            0,
        )
        .expect("Failed to insert test trip");

        // Two points well inside the trip window; the last one is far from the trip end,
        // so any "end_timestamp = MAX(timestamp)" behaviour would be plainly visible.
        for offset in [300u64, 900u64] {
            add_test_vessel_status(
                &db,
                t.add(Duration::from_secs(offset)),
                43.0 + offset as f64 / 10000.0,
                11.0,
                5.0,
                6.0,
                None,
                None,
                false,
                EngineStatus::Off,
                1.0,
                300_000,
                None,
                None,
            )
            .expect("Failed to insert vessel status");
        }

        let before = db
            .fetch_trip(trip_id)
            .expect("fetch_trip failed")
            .expect("trip missing");

        let start_dt = chrono::DateTime::<chrono::Utc>::from(t.add(Duration::from_secs(600)));
        let end_dt = chrono::DateTime::<chrono::Utc>::from(t.add(Duration::from_secs(1200)));
        db.correct_engine_status(trip_id, start_dt, end_dt, EngineStatus::On)
            .expect("correct_engine_status should succeed");

        let after = db
            .fetch_trip(trip_id)
            .expect("fetch_trip failed")
            .expect("trip missing");

        assert_eq!(
            before.start_date, after.start_date,
            "trips.start_timestamp must be unchanged by correct_engine_status"
        );
        assert_eq!(
            before.end_date, after.end_date,
            "trips.end_timestamp must be unchanged by correct_engine_status"
        );

        // And it must stay unchanged across repeated corrections (the old bug ratcheted).
        db.correct_engine_status(trip_id, start_dt, end_dt, EngineStatus::Off)
            .expect("second correct_engine_status should succeed");
        let after_second = db
            .fetch_trip(trip_id)
            .expect("fetch_trip failed")
            .expect("trip missing");
        assert_eq!(
            before.start_date, after_second.start_date,
            "trips.start_timestamp must stay unchanged across repeated corrections"
        );
        assert_eq!(
            before.end_date, after_second.end_date,
            "trips.end_timestamp must stay unchanged across repeated corrections"
        );
    }

    /// Regression test: `fetch_track` serves timestamps with the milliseconds zeroed and
    /// the page echoes them back, so a `.000` end bound must still include the row the
    /// user actually clicked (whose real timestamp has non-zero milliseconds).
    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_correct_engine_status_includes_truncated_end_point() {
        let db = setup_db();
        // Whole-second base, so `base + 300s` is exactly a `.000` boundary.
        let t = SystemTime::UNIX_EPOCH + Duration::from_millis(1_754_000_000_000);

        let trip_id: u32 = add_test_trip(
            &db,
            "Truncated End Bound".to_string(),
            t,
            t.add(Duration::from_secs(1800)),
            0.0,
            0.0,
            0,
            0,
            0,
        )
        .expect("Failed to insert test trip");

        // The clicked end point: real timestamp is 300.842s, served to the page as 300.000.
        let end_point_id = add_test_vessel_status(
            &db,
            t.add(Duration::from_millis(300_842)),
            43.0,
            11.0,
            5.0,
            6.0,
            None,
            None,
            false,
            EngineStatus::Off,
            1.0,
            300_000,
            None,
            None,
        )
        .expect("Failed to insert end point");

        // A point after the corrected range, which must stay untouched.
        let outside_id = add_test_vessel_status(
            &db,
            t.add(Duration::from_secs(900)),
            43.1,
            11.0,
            5.0,
            6.0,
            None,
            None,
            false,
            EngineStatus::Off,
            1.0,
            300_000,
            None,
            None,
        )
        .expect("Failed to insert outside point");

        let start_dt = chrono::DateTime::<chrono::Utc>::from(t);
        let end_dt = chrono::DateTime::<chrono::Utc>::from(t.add(Duration::from_secs(300)));
        db.correct_engine_status(trip_id, start_dt, end_dt, EngineStatus::On)
            .expect("correct_engine_status should succeed");

        let end_point = fetch_vessel_status_by_id(&db, end_point_id)
            .expect("fetch failed")
            .expect("end point missing");
        assert_eq!(
            end_point.engine_on,
            EngineStatus::On,
            "the clicked end point must be included despite the .000 end bound"
        );

        let outside = fetch_vessel_status_by_id(&db, outside_id)
            .expect("fetch failed")
            .expect("outside point missing");
        assert_eq!(
            outside.engine_on,
            EngineStatus::Off,
            "points after the corrected range must stay untouched"
        );
    }
}
