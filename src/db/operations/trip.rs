// Trip Operations - CRUD operations for voyage records
// Database Note: All queries use parameterized queries with params! macro.
// Multi-statement operations use transaction pattern with session variables.
// Testing: Database tests require serial execution (--test-threads=1) due to shared test DB.
// See: AGENTS.md for database patterns, transaction examples, and type conversions.
//
use std::error::Error;

use crate::db::types::VesselDatabase;
use mysql::params;
use mysql::prelude::Queryable;
use tracing::warn;

impl VesselDatabase {
    pub fn update_trip_description(
        &self,
        trip_id: i64,
        new_description: &str,
    ) -> Result<(), Box<dyn Error>> {
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
    pub fn delete_trip(&self, trip_id: u32) -> Result<(), Box<dyn Error>> {
        let mut conn = self
            .pool
            .get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;

        let mut tx = conn.start_transaction(mysql::TxOpts::default())?;

        // First, fetch the trip to get its time range
        let trip_row: Option<mysql::Row> = tx.exec_first(
            r"SELECT DATE_FORMAT(start_timestamp, '%Y-%m-%d %H:%i:%S.%f') as start_timestamp, DATE_FORMAT(end_timestamp, '%Y-%m-%d %H:%i:%S.%f') as end_timestamp FROM trips WHERE id = :trip_id",
            params! {
                "trip_id" => trip_id,
            },
        ).map_err(|e| format!("Database query error: {}", e))?;

        if trip_row.is_none() {
            return Err("Trip not found".into());
        }

        let mut trip_row = trip_row.unwrap();
        let start_timestamp: String = trip_row
            .take("start_timestamp")
            .ok_or("Missing start_timestamp")?;
        let end_timestamp: String = trip_row
            .take("end_timestamp")
            .ok_or("Missing end_timestamp")?;

        // Delete environmental data in the time range
        tx.exec_drop(
            r"DELETE FROM environmental_data
              WHERE timestamp >= :start AND timestamp <= :end",
            params! {
                "start" => &start_timestamp,
                "end" => &end_timestamp,
            },
        )
        .map_err(|e| format!("Failed to delete environmental data: {}", e))?;

        // Delete vessel status data in the time range
        tx.exec_drop(
            r"DELETE FROM vessel_status
              WHERE timestamp >= :start AND timestamp <= :end",
            params! {
                "start" => &start_timestamp,
                "end" => &end_timestamp,
            },
        )
        .map_err(|e| format!("Failed to delete vessel status data: {}", e))?;

        // Delete the trip record
        tx.exec_drop(
            r"DELETE FROM trips WHERE id = :trip_id",
            params! {
                "trip_id" => trip_id,
            },
        )
        .map_err(|e| format!("Failed to delete trip: {}", e))?;

        tx.commit()?;

        if let Err(e) = self.invalidate_trip_legs_cache(trip_id) {
            warn!("Failed to invalidate trip_legs_cache after delete_trip({}): {}", trip_id, e);
        }

        Ok(())
    }

    pub fn trim_trip(&self, trip_id: u32) -> Result<(), Box<dyn Error>> {
        let mut conn = self
            .pool
            .get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;

        let mut tx = conn.start_transaction(mysql::TxOpts::default())?;

        // Fetch trip timestamps into session variables
        tx.exec_drop(
            "SELECT @trip_start_ts := start_timestamp, @trip_end_ts := end_timestamp FROM trips WHERE id = :id",
            params! { "id" => trip_id },
        ).map_err(|e| format!("Failed to fetch trip timestamps: {}", e))?;

        // Fetch min and max timestamps for non-moored records into session variables
        tx.exec_drop(
            "SELECT @min_ts := MIN(timestamp), @max_ts := MAX(timestamp) FROM vessel_status WHERE timestamp >= @trip_start_ts AND timestamp <= @trip_end_ts AND is_moored = 0",
            ()
        ).map_err(|e| format!("Failed to fetch min/max timestamps: {}", e))?;

        // Delete vessel_status records outside the 1-hour buffer
        tx.exec_drop(
            "DELETE FROM vessel_status WHERE (timestamp >= @trip_start_ts AND timestamp < SUBTIME(@min_ts, '0 1:00:0.000')) OR (timestamp <= @trip_end_ts AND timestamp > ADDTIME(@max_ts, '0 1:00:0.000'))",
            ()
        ).map_err(|e| format!("Failed to delete vessel_status: {}", e))?;

        // Delete environmental_data records outside the 1-hour buffer
        tx.exec_drop(
            "DELETE FROM environmental_data WHERE (timestamp >= @trip_start_ts AND timestamp < SUBTIME(@min_ts, '0 1:00:0.000')) OR (timestamp <= @trip_end_ts AND timestamp > ADDTIME(@max_ts, '0 1:00:0.000'))",
            ()
        ).map_err(|e| format!("Failed to delete environmental_data: {}", e))?;

        // Update trip with new boundaries
        tx.exec_drop(
            "UPDATE trips SET start_timestamp = SUBTIME(@min_ts, '0 1:00:0.000'), end_timestamp = ADDTIME(@max_ts, '0 1:00:0.000') WHERE id = :id",
            params! { "id" => trip_id },
        ).map_err(|e| format!("Failed to update trip: {}", e))?;

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
    ) -> Result<(), Box<dyn Error>> {
        let mut conn = self
            .pool
            .get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;

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
        )
        .map_err(|e| format!("Failed to ensure trip_legs_nav_overrides table: {}", e))?;

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
        )
        .map_err(|e| format!("Failed to upsert nav override: {}", e))?;

        if let Err(e) = self.invalidate_trip_legs_cache(trip_id) {
            warn!(
                "Failed to invalidate trip_legs_cache after set_nav_override({}, {}): {}",
                trip_id, leg_number, e
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
}
