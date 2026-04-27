use crate::db::types::VesselDatabase;
use mysql::params;
use mysql::prelude::Queryable;
use serde::{Deserialize, Serialize};
use std::error::Error;
use tracing::{info, warn};

/// Payload sent from boat to viewer's `/api/sync/manifest` endpoint.
/// Contains all UUID the boat has (for orphan deletion) and the sync timestamp.
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncManifestPayload {
    pub all_uuids: Vec<String>,
    pub synced_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncManifestResult {
    pub deleted_count: usize,
    /// UUIDs from the manifest payload that the viewer does not yet have.
    /// The boat should send exactly these trips.
    pub missing_uuids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncResult {
    pub deleted_count: usize,
    pub upserted_count: usize,
    pub synced_at: String,
}

#[derive(Debug, Serialize)]
pub struct SyncStatus {
    pub last_synced_at: Option<String>,
}

impl VesselDatabase {
    /// Returns all trip UUIDs. Used by the push side to populate `all_uuids`.
    pub fn get_all_trip_uuids(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        let uuids: Vec<String> = conn.exec(
            "SELECT uuid FROM trips WHERE uuid IS NOT NULL ORDER BY end_timestamp ASC",
            (),
        )?;
        Ok(uuids)
    }

    /// Returns export-format JSON values for the trips whose UUID is in the given list.
    pub fn get_trips_by_uuids(
        &self,
        uuids: &[String],
    ) -> Result<Vec<serde_json::Value>, Box<dyn Error>> {
        if uuids.is_empty() {
            return Ok(vec![]);
        }

        let valid_uuids: Vec<&String> = uuids.iter().filter(|u| is_valid_uuid(u)).collect();
        if valid_uuids.is_empty() {
            return Ok(vec![]);
        }

        let uuid_list = valid_uuids
            .iter()
            .map(|u| format!("'{}'", u))
            .collect::<Vec<_>>()
            .join(",");

        let mut conn = self.pool.get_conn()?;
        let ids: Vec<i64> = conn.exec(
            format!(
                "SELECT id FROM trips WHERE uuid IN ({}) ORDER BY end_timestamp ASC",
                uuid_list
            ),
            (),
        )?;
        drop(conn);

        let mut trips = Vec::with_capacity(ids.len());
        for id in ids {
            let json_str = self.export_trip_to_string(id)?;
            let value: serde_json::Value = serde_json::from_str(&json_str)?;
            trips.push(value);
        }
        Ok(trips)
    }

    /// Delete all trips whose UUID is not in keep_uuids, cascading to vessel_status
    /// and environmental_data. All deletions run inside a single transaction.
    /// Returns the number of trips deleted.
    pub fn delete_trips_not_in_uuids(&self, keep_uuids: &[String]) -> Result<usize, Box<dyn Error>> {
        // Validate UUIDs to prevent any SQL injection risk before embedding in literal.
        let valid_uuids: Vec<&String> = keep_uuids
            .iter()
            .filter(|u| is_valid_uuid(u))
            .collect();

        if valid_uuids.len() != keep_uuids.len() {
            warn!(
                "Sync: {} of {} UUIDs failed validation and were ignored",
                keep_uuids.len() - valid_uuids.len(),
                keep_uuids.len()
            );
        }

        let mut conn = self.pool.get_conn()?;

        // Find trips to delete and their time ranges.
        let orphan_rows: Vec<mysql::Row> = if valid_uuids.is_empty() {
            conn.exec(
                "SELECT id, DATE_FORMAT(start_timestamp, '%Y-%m-%d %H:%i:%S.%f'), \
                        DATE_FORMAT(end_timestamp, '%Y-%m-%d %H:%i:%S.%f') FROM trips",
                (),
            )?
        } else {
            let uuid_list = valid_uuids
                .iter()
                .map(|u| format!("'{}'", u))
                .collect::<Vec<_>>()
                .join(",");
            conn.exec(
                format!(
                    "SELECT id, DATE_FORMAT(start_timestamp, '%Y-%m-%d %H:%i:%S.%f'), \
                            DATE_FORMAT(end_timestamp, '%Y-%m-%d %H:%i:%S.%f') \
                     FROM trips WHERE uuid IS NULL OR uuid NOT IN ({})",
                    uuid_list
                ),
                (),
            )?
        };

        if orphan_rows.is_empty() {
            return Ok(0);
        }

        let orphans: Vec<(u64, String, String)> = orphan_rows
            .into_iter()
            .map(|row| {
                let id: u64 = row.get(0).unwrap_or(0);
                let start: String = row.get(1).unwrap_or_default();
                let end: String = row.get(2).unwrap_or_default();
                (id, start, end)
            })
            .collect();

        let count = orphans.len();
        let mut tx = conn.start_transaction(mysql::TxOpts::default())?;

        for (id, start_ts, end_ts) in &orphans {
            tx.exec_drop(
                "DELETE FROM environmental_data WHERE timestamp >= :start AND timestamp <= :end",
                params! { "start" => start_ts, "end" => end_ts },
            )?;
            tx.exec_drop(
                "DELETE FROM vessel_status WHERE timestamp >= :start AND timestamp <= :end",
                params! { "start" => start_ts, "end" => end_ts },
            )?;
            tx.exec_drop(
                "DELETE FROM trips WHERE id = :id",
                params! { "id" => id },
            )?;
        }

        tx.commit()?;
        info!("Sync: deleted {} orphan trip(s)", count);
        Ok(count)
    }

    /// Read last sync timestamp from system_status.
    pub fn get_sync_status(&self) -> Result<SyncStatus, Box<dyn Error>> {
        let last_synced_at = self.get_system_status_string("last_synced_at")?;
        Ok(SyncStatus { last_synced_at })
    }
}

fn is_valid_uuid(s: &str) -> bool {
    // UUID v4: xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx (36 chars, hex + hyphens)
    if s.len() != 36 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_hexdigit() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::{
        add_test_env, add_test_trip, add_test_vessel_status, reset_test_db, setup_db,
    };
    use crate::utilities::EngineStatus;
    use mysql::prelude::Queryable;
    use std::ops::Add;
    use std::time::{Duration, SystemTime};

    const ONE_HOUR_S: u64 = 3600;

    // Insert a trip and return (trip_id, uuid).
    fn make_trip(db: &VesselDatabase, desc: &str, start: SystemTime, hours: u64) -> (u32, String) {
        let end = start.add(Duration::from_secs(hours * ONE_HOUR_S));
        let id = add_test_trip(
            db,
            desc.to_string(),
            start,
            end,
            10.0,
            2.0,
            hours * ONE_HOUR_S * 1000,
            0,
            0,
        )
        .expect("add_test_trip failed");
        let uuid = db
            .fetch_trip(id)
            .expect("fetch_trip failed")
            .expect("trip not found")
            .uuid
            .expect("trip has no uuid");
        (id, uuid)
    }

    fn count_rows(db: &VesselDatabase, table: &str) -> u64 {
        let mut conn = db.pool.get_conn().unwrap();
        let sql = format!("SELECT COUNT(*) FROM {}", table);
        conn.exec_first::<u64, _, _>(&sql, ()).unwrap().unwrap_or(0)
    }

    // -------------------------------------------------------------------------
    // Unit tests — no database required
    // -------------------------------------------------------------------------

    #[test]
    fn test_is_valid_uuid_accepts_valid() {
        assert!(is_valid_uuid("550e8400-e29b-41d4-a716-446655440000"));
        assert!(is_valid_uuid("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11"));
        assert!(is_valid_uuid("00000000-0000-0000-0000-000000000000"));
    }

    #[test]
    fn test_is_valid_uuid_rejects_invalid() {
        assert!(!is_valid_uuid(""));
        assert!(!is_valid_uuid("not-a-uuid"));
        assert!(!is_valid_uuid("550e8400-e29b-41d4-a716"));
        assert!(!is_valid_uuid("550e8400-e29b-41d4-a716-4466554400001"));
        assert!(!is_valid_uuid("550e8400-e29b-41d4-a716-44665544000g"));
        assert!(!is_valid_uuid("'; DROP TABLE trips; --00000000000"));
    }

    // -------------------------------------------------------------------------
    // Database tests — require running MySQL and test_config.json
    // -------------------------------------------------------------------------

    #[test]
    #[ignore]
    fn test_get_all_trip_uuids_empty() {
        let db = setup_db();
        let uuids = db.get_all_trip_uuids().expect("should succeed");
        assert!(uuids.is_empty(), "fresh DB has no UUIDs");
    }

    #[test]
    #[ignore]
    fn test_get_all_trip_uuids_returns_all() {
        let db = setup_db();
        let t = SystemTime::now();
        let (_, uuid1) = make_trip(&db, "Trip 1", t, 2);
        let (_, uuid2) = make_trip(&db, "Trip 2", t.add(Duration::from_secs(3 * ONE_HOUR_S)), 2);

        let uuids = db.get_all_trip_uuids().expect("should succeed");
        assert_eq!(uuids.len(), 2);
        assert!(uuids.contains(&uuid1));
        assert!(uuids.contains(&uuid2));
    }

    #[test]
    #[ignore]
    fn test_get_trips_by_uuids_returns_matching() {
        let db = setup_db();
        let t = SystemTime::now();
        let (_, uuid1) = make_trip(&db, "Trip A", t, 2);
        let (_, uuid2) = make_trip(&db, "Trip B", t.add(Duration::from_secs(3 * ONE_HOUR_S)), 2);

        let trips = db.get_trips_by_uuids(&[uuid1.clone(), uuid2.clone()]).expect("should succeed");
        assert_eq!(trips.len(), 2);
        for trip in &trips {
            assert!(trip["trip"].is_object());
            assert!(trip["vs"].is_array());
            assert!(trip["em"].is_array());
            assert!(trip["meta"].is_object());
        }
    }

    #[test]
    #[ignore]
    fn test_get_trips_by_uuids_partial_match() {
        let db = setup_db();
        let t = SystemTime::now();
        let (_, uuid1) = make_trip(&db, "Trip A", t, 2);
        make_trip(&db, "Trip B", t.add(Duration::from_secs(3 * ONE_HOUR_S)), 2);

        let trips = db.get_trips_by_uuids(&[uuid1.clone()]).expect("should succeed");
        assert_eq!(trips.len(), 1);
        assert_eq!(trips[0]["trip"]["desc"].as_str(), Some("Trip A"));
    }

    #[test]
    #[ignore]
    fn test_get_trips_by_uuids_empty_input() {
        let db = setup_db();
        let t = SystemTime::now();
        make_trip(&db, "Trip A", t, 2);

        let trips = db.get_trips_by_uuids(&[]).expect("should succeed");
        assert_eq!(trips.len(), 0, "empty input → empty output");
    }

    #[test]
    #[ignore]
    fn test_delete_trips_not_in_uuids_empty_keep_deletes_all() {
        let db = setup_db();
        let t = SystemTime::now();
        make_trip(&db, "Trip 1", t, 2);
        make_trip(&db, "Trip 2", t.add(Duration::from_secs(3 * ONE_HOUR_S)), 2);

        let deleted = db.delete_trips_not_in_uuids(&[]).expect("should succeed");
        assert_eq!(deleted, 2);
        assert_eq!(count_rows(&db, "trips"), 0);
    }

    #[test]
    #[ignore]
    fn test_delete_trips_not_in_uuids_keeps_specified() {
        let db = setup_db();
        let t = SystemTime::now();
        let (_, uuid1) = make_trip(&db, "Keep 1", t, 2);
        let (_, uuid2) = make_trip(&db, "Keep 2", t.add(Duration::from_secs(3 * ONE_HOUR_S)), 2);
        make_trip(&db, "Delete", t.add(Duration::from_secs(6 * ONE_HOUR_S)), 2);

        let deleted = db
            .delete_trips_not_in_uuids(&[uuid1.clone(), uuid2.clone()])
            .expect("should succeed");
        assert_eq!(deleted, 1, "one orphan deleted");
        assert_eq!(count_rows(&db, "trips"), 2);

        let remaining = db.get_all_trip_uuids().expect("should succeed");
        assert!(remaining.contains(&uuid1));
        assert!(remaining.contains(&uuid2));
    }

    #[test]
    #[ignore]
    fn test_delete_trips_not_in_uuids_all_kept() {
        let db = setup_db();
        let t = SystemTime::now();
        let (_, uuid1) = make_trip(&db, "Trip 1", t, 2);

        let deleted = db
            .delete_trips_not_in_uuids(&[uuid1])
            .expect("should succeed");
        assert_eq!(deleted, 0, "no orphans when all UUIDs are kept");
        assert_eq!(count_rows(&db, "trips"), 1);
    }

    #[test]
    #[ignore]
    fn test_delete_trips_not_in_uuids_cascades_to_related_data() {
        let db = setup_db();
        let t = SystemTime::now();
        let end = t.add(Duration::from_secs(2 * ONE_HOUR_S));

        // Trip we keep — add some vessel_status inside its time window
        let (_, uuid_keep) = make_trip(&db, "Keep", t, 2);
        add_test_vessel_status(
            &db, t.add(Duration::from_secs(ONE_HOUR_S)),
            43.0, 10.0, 5.0, 6.0, None, None, false, EngineStatus::Off, 0.1, 30000, None, None,
        ).expect("add vessel_status failed");

        // Trip we delete — in a later time window; add vessel_status + env data
        let t2 = end.add(Duration::from_secs(ONE_HOUR_S));
        make_trip(&db, "Delete", t2, 2);
        add_test_vessel_status(
            &db, t2.add(Duration::from_secs(ONE_HOUR_S)),
            44.0, 11.0, 3.0, 4.0, None, None, false, EngineStatus::Off, 0.2, 30000, None, None,
        ).expect("add vessel_status failed");
        add_test_env(
            &db, t2.add(Duration::from_secs(ONE_HOUR_S)),
            1, Some(101325.0), Some(101500.0), Some(101100.0), "Pa",
        ).expect("add env failed");

        let vs_before = count_rows(&db, "vessel_status");
        let em_before = count_rows(&db, "environmental_data");
        assert_eq!(vs_before, 2);
        assert_eq!(em_before, 1);

        db.delete_trips_not_in_uuids(&[uuid_keep]).expect("should succeed");

        // vessel_status and env_data for the deleted trip should be gone;
        // the row belonging to the kept trip must remain.
        assert_eq!(count_rows(&db, "trips"), 1);
        assert_eq!(count_rows(&db, "vessel_status"), 1, "cascade should remove deleted-trip VS row");
        assert_eq!(count_rows(&db, "environmental_data"), 0, "cascade should remove deleted-trip env row");
    }

    #[test]
    #[ignore]
    fn test_sync_status_never_synced() {
        let db = setup_db();
        let status = db.get_sync_status().expect("should succeed");
        assert!(status.last_synced_at.is_none(), "fresh DB has no sync timestamp");
    }

    #[test]
    #[ignore]
    fn test_sync_status_reflects_synced_at() {
        let db = setup_db();
        let ts = "2026-04-25T10:00:00+00:00";
        db.set_system_status_string("last_synced_at", ts)
            .expect("should succeed");
        let status = db.get_sync_status().expect("should succeed");
        assert_eq!(status.last_synced_at.as_deref(), Some(ts));
    }

    #[test]
    #[ignore]
    fn test_sync_round_trip_per_trip() {
        let db = setup_db();
        let t = SystemTime::now();

        let (_, uuid1) = make_trip(&db, "Trip Alpha", t, 2);
        let (_, uuid2) = make_trip(&db, "Trip Beta", t.add(Duration::from_secs(3 * ONE_HOUR_S)), 2);
        add_test_vessel_status(
            &db, t.add(Duration::from_secs(ONE_HOUR_S)),
            43.5, 10.5, 5.5, 6.5, Some(12.0), Some(45.0), false, EngineStatus::On,
            0.5, 30000, Some(90.0), None,
        ).expect("add vessel_status failed");

        let all_uuids = db.get_all_trip_uuids().expect("get UUIDs");
        let updated_trips = db.get_trips_by_uuids(&all_uuids).expect("get trips");
        assert_eq!(updated_trips.len(), 2);

        // Simulate what trips_viewer does: process manifest then each trip
        reset_test_db(&db).expect("reset failed");
        assert_eq!(count_rows(&db, "trips"), 0);

        // Manifest step: no orphans to delete on empty DB
        let deleted = db.delete_trips_not_in_uuids(&all_uuids).expect("delete orphans");
        assert_eq!(deleted, 0);
        db.set_system_status_string("last_synced_at", "2026-04-25T10:00:00+00:00").expect("set ts");

        // Per-trip step
        for trip_value in &updated_trips {
            let json_str = serde_json::to_string(trip_value).unwrap();
            db.import_trip(&json_str).expect("import_trip failed");
        }

        assert_eq!(count_rows(&db, "trips"), 2);
        assert_eq!(count_rows(&db, "vessel_status"), 1, "vessel_status row restored");

        let uuids_after = db.get_all_trip_uuids().expect("get UUIDs after");
        assert!(uuids_after.contains(&uuid1));
        assert!(uuids_after.contains(&uuid2));

        let status = db.get_sync_status().expect("get sync status");
        assert_eq!(status.last_synced_at.as_deref(), Some("2026-04-25T10:00:00+00:00"));
    }

    #[test]
    #[ignore]
    fn test_sync_manifest_deletes_orphans() {
        let db = setup_db();
        let t = SystemTime::now();

        let (_, uuid1) = make_trip(&db, "Trip 1", t, 2);
        let (_, uuid2) = make_trip(&db, "Trip 2", t.add(Duration::from_secs(3 * ONE_HOUR_S)), 2);
        make_trip(&db, "Trip 3 deleted", t.add(Duration::from_secs(6 * ONE_HOUR_S)), 2);

        // Manifest only lists trips 1 & 2 — trip 3 is orphan
        let deleted = db
            .delete_trips_not_in_uuids(&[uuid1.clone(), uuid2.clone()])
            .expect("delete orphans");
        assert_eq!(deleted, 1, "trip 3 should be deleted");
        assert_eq!(count_rows(&db, "trips"), 2, "only 2 trips remain");

        let remaining = db.get_all_trip_uuids().expect("get UUIDs");
        assert!(remaining.contains(&uuid1));
        assert!(remaining.contains(&uuid2));
    }

    #[test]
    #[ignore]
    fn test_sync_trip_idempotent() {
        let db = setup_db();
        let t = SystemTime::now();
        let (trip_id, uuid) = make_trip(&db, "Idempotent Trip", t, 2);

        let trip_json: serde_json::Value =
            serde_json::from_str(&db.export_trip_to_string(trip_id as i64).unwrap()).unwrap();
        let json_str = serde_json::to_string(&trip_json).unwrap();

        // Import twice — second call must produce the same DB state
        db.import_trip(&json_str).expect("first import failed");
        db.import_trip(&json_str).expect("second import failed");
        assert_eq!(count_rows(&db, "trips"), 1, "still exactly one trip");

        let uuids = db.get_all_trip_uuids().expect("get UUIDs");
        assert_eq!(uuids, vec![uuid]);
    }
}
