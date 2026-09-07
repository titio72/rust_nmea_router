// Single write path for in-place edits to `trips`. Every caller supplies just the
// SET fragment for the fields it's changing; this always appends
// `version = version + 1`, so no call site can update a trip's row without
// bumping its version. See
// docs/superpowers/specs/2026-09-06-trip-version-sync-design.md.
use crate::db::types::VesselDatabase;
use crate::error::AppError;
use mysql::params;
use mysql::prelude::Queryable;

pub(crate) fn exec_trip_update(
    conn: &mut impl Queryable,
    set_fragment: &str,
    params: impl Into<mysql::Params>,
) -> Result<(), mysql::Error> {
    let sql = format!("UPDATE trips SET {set_fragment}, version = version + 1 WHERE id = :trip_id");
    conn.exec_drop(sql, params)
}

impl VesselDatabase {
    /// Bump a trip's version with no other field changes — for direct SQL
    /// corrections against vessel_status/environmental_data that don't
    /// otherwise touch the trips row (see DB_ANALYST.md).
    pub fn bump_trip_version(&self, trip_id: i64) -> Result<(), AppError> {
        let mut conn = self.pool.get_conn()?;
        exec_trip_update(&mut conn, "id = id", params! { "trip_id" => trip_id })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::exec_trip_update;
    use crate::db::test_helpers::{add_test_trip, setup_db};
    use mysql::params;
    use mysql::prelude::Queryable;
    use std::time::{Duration, SystemTime};

    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_exec_trip_update_bumps_version_by_one() {
        let db = setup_db();
        let t = SystemTime::now();
        let trip_id = add_test_trip(
            &db,
            "Version Test".to_string(),
            t,
            t + Duration::from_secs(3600),
            0.0,
            0.0,
            0,
            0,
            0,
        )
        .expect("add_test_trip failed");

        let mut conn = db.pool.get_conn().unwrap();
        let version_before: u64 = conn
            .exec_first(
                "SELECT version FROM trips WHERE id = :id",
                params! { "id" => trip_id },
            )
            .unwrap()
            .unwrap();
        assert_eq!(version_before, 1, "a freshly inserted trip starts at version 1");

        exec_trip_update(
            &mut conn,
            "description = :description",
            params! { "description" => "Updated", "trip_id" => trip_id },
        )
        .expect("exec_trip_update failed");

        let version_after: u64 = conn
            .exec_first(
                "SELECT version FROM trips WHERE id = :id",
                params! { "id" => trip_id },
            )
            .unwrap()
            .unwrap();
        assert_eq!(version_after, 2, "exec_trip_update must bump version by exactly 1");

        let description: String = conn
            .exec_first(
                "SELECT description FROM trips WHERE id = :id",
                params! { "id" => trip_id },
            )
            .unwrap()
            .unwrap();
        assert_eq!(description, "Updated", "the caller's SET fragment must still apply");
    }

    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_bump_trip_version_only_changes_version() {
        let db = setup_db();
        let t = SystemTime::now();
        let trip_id = add_test_trip(
            &db,
            "Bump Test".to_string(),
            t,
            t + Duration::from_secs(3600),
            1.5,
            0.5,
            100,
            50,
            10,
        )
        .expect("add_test_trip failed");

        db.bump_trip_version(trip_id as i64)
            .expect("bump_trip_version failed");

        let mut conn = db.pool.get_conn().unwrap();
        let (version, dist_sailed): (u64, f64) = conn
            .exec_first(
                "SELECT version, total_distance_sailed FROM trips WHERE id = :id",
                params! { "id" => trip_id },
            )
            .unwrap()
            .unwrap();
        assert_eq!(version, 2, "bump_trip_version must increment by exactly 1");
        assert_eq!(dist_sailed, 1.5, "bump_trip_version must not touch other fields");
    }
}
