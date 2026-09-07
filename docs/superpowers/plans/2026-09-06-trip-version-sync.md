# Trip Version-Based Sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the fragile `updated_at`/`last_synced_at` change-detection in the boat↔Railway trip sync with a per-trip `version` counter that both sides compare directly, so a trip that changed locally is never silently skipped.

**Architecture:** Add a `version BIGINT UNSIGNED` column to `trips`. Every in-place edit goes through one primitive (`exec_trip_update`) that always appends `version = version + 1`, so no write path can forget to bump it. `import_trip` (used both for manual re-imports and as the remote's sync-receive handler) gains an `is_sync: bool` parameter controlling whether it adopts an incoming version verbatim (sync) or bumps from its own stored value (manual). The manifest protocol sends a `{uuid: version}` map instead of a UUID list plus timestamp cursor; the receiving side diffs it directly and returns exactly the UUIDs that need pushing — no cursor state anywhere.

**Tech Stack:** Rust, `mysql` crate 25.0, MariaDB, Axum, `rmcp` (MCP server in `src/bin/mcp_server.rs`).

**Spec:** `docs/superpowers/specs/2026-09-06-trip-version-sync-design.md`

## Global Constraints

- Column: `version BIGINT UNSIGNED NOT NULL DEFAULT 1` on `trips`, added via the existing best-effort self-migration list in `src/db/connection.rs` (not a manual script) so it can never again go missing on one side only.
- `import_trip(&self, json_data: &str, is_sync: bool) -> Result<i64, AppError>` — `is_sync = true` reads `trip["ver"]` from the payload and uses it verbatim (hard error if absent); `is_sync = false` computes `existing_version_for_uuid.unwrap_or(0) + 1`.
- Export/import JSON field name for version: `"ver"` (matches the existing short-key convention: `desc`, `start`, `end`, `dist_sail`, ...).
- `exec_trip_update` is the only function allowed to build an `UPDATE trips SET ...` statement anywhere in the codebase from here on.
- `last_synced_at` (`system_status`) stays, but only for the `GET /api/sync/status` display — no sync-decision code may read it after this plan.
- DB tests in this codebase are `#[ignore]`d and require `cargo test -- --test-threads=1 --include-ignored` against a live MariaDB test DB (`test_config.json`). Follow existing test style: `setup_db()`, `add_test_trip`, `#[ignore]` with the standard comment `// Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).`

---

### Task 1: Schema — self-migrating `version` column

**Files:**
- Modify: `src/db/connection.rs:86-95`
- Modify: `schema.sql` (the `CREATE TABLE trips` block, near line 98-99, and the commented migration block near line 109-114)

**Interfaces:**
- Produces: every `trips` row has a `version` column, defaulted to 1, on any DB the app connects to (existing or fresh), with no manual step.

- [ ] **Step 1: Add the migration to the self-migrating list**

In `src/db/connection.rs`, add one more entry to the `for sql in &[...]` list (around line 92, right after `total_time_running`):

```rust
            for sql in &[
                "ALTER TABLE trips ADD COLUMN total_distance_upwind DOUBLE NOT NULL DEFAULT 0",
                "ALTER TABLE trips ADD COLUMN total_distance_reaching DOUBLE NOT NULL DEFAULT 0",
                "ALTER TABLE trips ADD COLUMN total_distance_running DOUBLE NOT NULL DEFAULT 0",
                "ALTER TABLE trips ADD COLUMN total_time_upwind BIGINT NOT NULL DEFAULT 0",
                "ALTER TABLE trips ADD COLUMN total_time_reaching BIGINT NOT NULL DEFAULT 0",
                "ALTER TABLE trips ADD COLUMN total_time_running BIGINT NOT NULL DEFAULT 0",
                "ALTER TABLE trips ADD COLUMN version BIGINT UNSIGNED NOT NULL DEFAULT 1 COMMENT 'Bumped on every change to this row; drives remote sync change-detection'",
            ] {
                let _ = conn.query_drop(sql);
            }
```

**Step 2: Update `schema.sql` for fresh installs**

Add to the `CREATE TABLE trips (...)` block (near the existing `uuid` column, around line 98):

```sql
    uuid CHAR(36) NULL COMMENT 'UUID v4 for portable trip identification (used for import deduplication)',
    version BIGINT UNSIGNED NOT NULL DEFAULT 1 COMMENT 'Bumped on every change to this row; drives remote sync change-detection',
```

Delete the old commented-out manual migration block for `updated_at` (the two `-- ALTER TABLE trips ADD COLUMN updated_at ...` / `-- UPDATE trips SET updated_at = end_timestamp;` lines) — `updated_at` is being replaced by `version`, not kept alongside it. If `updated_at` already exists on a deployed DB (Railway does, boat doesn't, per the investigation that produced this plan), leave it in place; nothing in code will read it after Task 12, so a stray unused column is harmless and not worth a DROP migration.

- [ ] **Step 3: Verify by running any existing DB test**

Run: `cargo test -- --test-threads=1 --include-ignored trip::tests::test_correct_engine_status_idempotent`
Expected: PASS, and check via the `mariadb`/`mariadb_railway` MCP tool or `mysql` CLI that `SHOW COLUMNS FROM trips` now includes `version` with `Default: 1`.

- [ ] **Step 4: Commit**

```bash
git add src/db/connection.rs schema.sql
git commit -m "feat: self-migrate a version column onto trips

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>"
```

---

### Task 2: The `exec_trip_update` primitive

**Files:**
- Create: `src/db/operations/trip_update.rs`
- Modify: `src/db/operations/mod.rs` (register the module)

**Interfaces:**
- Produces: `pub(crate) fn exec_trip_update(conn: &mut impl Queryable, set_fragment: &str, params: impl Into<mysql::Params>) -> Result<(), mysql::Error>` and `pub fn VesselDatabase::bump_trip_version(&self, trip_id: i64) -> Result<(), AppError>`. Every later task in this plan consumes `exec_trip_update`.
- Consumes: nothing new (uses `mysql::prelude::Queryable`, already a project dependency).

- [ ] **Step 1: Write the failing test**

Create `src/db/operations/trip_update.rs`:

```rust
// Single write path for in-place edits to `trips`. Every caller supplies just the
// SET fragment for the fields it's changing; this always appends
// `version = version + 1`, so no call site can update a trip's row without
// bumping its version. See
// docs/superpowers/specs/2026-09-06-trip-version-sync-design.md.
use mysql::prelude::Queryable;

pub(crate) fn exec_trip_update(
    conn: &mut impl Queryable,
    set_fragment: &str,
    params: impl Into<mysql::Params>,
) -> Result<(), mysql::Error> {
    todo!()
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
```

Register the module in `src/db/operations/mod.rs` (add alongside the existing `pub mod` lines):

```rust
pub mod trip_update;
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -- --include-ignored trip_update::tests::test_exec_trip_update_bumps_version_by_one`
Expected: FAIL (compile error — `todo!()` panics, or `bump_trip_version` not defined yet)

- [ ] **Step 3: Write minimal implementation**

Replace the `todo!()` body and add `bump_trip_version` to `src/db/operations/trip_update.rs`:

```rust
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -- --test-threads=1 --include-ignored trip_update::tests`
Expected: PASS (2 tests)

- [ ] **Step 5: Commit**

```bash
git add src/db/operations/trip_update.rs src/db/operations/mod.rs
git commit -m "feat: add exec_trip_update primitive and manual version bump

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>"
```

---

### Task 3: Refactor `update_trip_description` onto the primitive

**Files:**
- Modify: `src/db/operations/trip.rs:16-31`

**Interfaces:**
- Consumes: `exec_trip_update` from Task 2.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/db/operations/trip.rs` (the existing `test_update_trip_description` at line 417 is `#[ignore]`d for unrelated reasons — leave it, and add a new one):

```rust
    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_update_trip_description_bumps_version() {
        let db = setup_db();
        let t = SystemTime::now();
        let trip_id: u32 = add_test_trip(
            &db,
            "Before".to_string(),
            t,
            t.add(Duration::from_secs(ONE_HOUR_S)),
            0.0,
            0.0,
            0,
            0,
            0,
        )
        .expect("Failed to insert test trip");

        db.update_trip_description(trip_id as i64, "After")
            .expect("update_trip_description failed");

        let mut conn = db.pool.get_conn().unwrap();
        let version: u64 = conn
            .exec_first(
                "SELECT version FROM trips WHERE id = :id",
                mysql::params! { "id" => trip_id },
            )
            .unwrap()
            .unwrap();
        assert_eq!(version, 2, "update_trip_description must bump version");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -- --include-ignored trip::tests::test_update_trip_description_bumps_version`
Expected: FAIL (`version` stays 1 — no bump yet)

- [ ] **Step 3: Refactor `update_trip_description`**

In `src/db/operations/trip.rs`, add the import at the top:

```rust
use crate::db::operations::trip_update::exec_trip_update;
```

Replace the function body (lines 16-31):

```rust
    pub fn update_trip_description(
        &self,
        trip_id: i64,
        new_description: &str,
    ) -> Result<(), AppError> {
        let mut conn = self.pool.get_conn()?;
        exec_trip_update(
            &mut conn,
            "description = :description",
            params! {
                "description" => new_description,
                "trip_id" => trip_id,
            },
        )?;
        Ok(())
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -- --test-threads=1 --include-ignored trip::tests::test_update_trip_description_bumps_version`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/db/operations/trip.rs
git commit -m "refactor: route update_trip_description through exec_trip_update

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>"
```

---

### Task 4: Refactor `trim_trip` onto the primitive

**Files:**
- Modify: `src/db/operations/trip.rs:97-139`

**Interfaces:**
- Consumes: `exec_trip_update` from Task 2.

- [ ] **Step 1: Write the failing test**

Add to `src/db/operations/trip.rs` tests:

```rust
    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_trim_trip_bumps_version() {
        let db = setup_db();
        let t = SystemTime::now();
        let trip_lenght_h = 2;
        let trip_id: u32 = add_test_trip(
            &db,
            "Trim Version Test".to_string(),
            t,
            t.add(Duration::from_secs(trip_lenght_h * ONE_HOUR_S)),
            0.0,
            0.0,
            0,
            0,
            trip_lenght_h * ONE_HOUR_S * 1000,
        )
        .expect("Failed to insert test trip");

        add_test_vessel_status(
            &db, t, 43.0, 11.0, 0.0, 0.0, None, None, false,
            crate::utilities::EngineStatus::On, 0.1, 30000, None, None,
        )
        .expect("Failed to insert vessel status");

        db.trim_trip(trip_id).expect("trim_trip failed");

        let mut conn = db.pool.get_conn().unwrap();
        let version: u64 = conn
            .exec_first(
                "SELECT version FROM trips WHERE id = :id",
                mysql::params! { "id" => trip_id },
            )
            .unwrap()
            .unwrap();
        assert_eq!(version, 2, "trim_trip must bump version");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -- --include-ignored trip::tests::test_trim_trip_bumps_version`
Expected: FAIL (`version` stays 1)

- [ ] **Step 3: Refactor `trim_trip`**

In `src/db/operations/trip.rs`, replace the `tx.exec_drop` call at lines 126-130:

```rust
        // Update trip with new boundaries
        exec_trip_update(
            &mut tx,
            "start_timestamp = SUBTIME(@min_ts, '0 1:00:0.000'), end_timestamp = ADDTIME(@max_ts, '0 1:00:0.000')",
            params! { "trip_id" => trip_id },
        )?;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -- --test-threads=1 --include-ignored trip::tests::test_trim_trip_bumps_version`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/db/operations/trip.rs
git commit -m "refactor: route trim_trip through exec_trip_update

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>"
```

---

### Task 5: Refactor `correct_engine_status` onto the primitive

**Files:**
- Modify: `src/db/operations/trip.rs:349-377`

**Interfaces:**
- Consumes: `exec_trip_update` from Task 2.

- [ ] **Step 1: Write the failing test**

Add to `src/db/operations/trip.rs` tests (reuses the existing `test_correct_engine_status` setup pattern):

```rust
    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_correct_engine_status_bumps_version() {
        let db = setup_db();
        let t = SystemTime::now();

        let trip_id: u32 = add_test_trip(
            &db,
            "Engine Fix Version Test".to_string(),
            t,
            t.add(Duration::from_secs(1800)),
            0.0,
            0.0,
            0,
            0,
            0,
        )
        .expect("Failed to insert test trip");

        add_test_vessel_status(
            &db, t, 43.0, 11.0, 5.0, 6.0, None, None, false,
            EngineStatus::Off, 1.0, 900_000, None, None,
        )
        .expect("Failed to insert vessel status");

        let start_dt = chrono::DateTime::<chrono::Utc>::from(t);
        let end_dt = chrono::DateTime::<chrono::Utc>::from(t.add(Duration::from_secs(1800)));
        db.correct_engine_status(trip_id, start_dt, end_dt, EngineStatus::On)
            .expect("correct_engine_status should succeed");

        let mut conn = db.pool.get_conn().unwrap();
        let version: u64 = conn
            .exec_first(
                "SELECT version FROM trips WHERE id = :id",
                mysql::params! { "id" => trip_id },
            )
            .unwrap()
            .unwrap();
        assert_eq!(version, 2, "correct_engine_status must bump version");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -- --include-ignored trip::tests::test_correct_engine_status_bumps_version`
Expected: FAIL

- [ ] **Step 3: Refactor `correct_engine_status`**

In `src/db/operations/trip.rs`, replace the `tx.exec_drop` call at lines 349-377:

```rust
            exec_trip_update(
                &mut tx,
                r"total_time_moored       = :time_moored,
                  total_time_motoring     = :time_motoring,
                  total_time_sailing      = :time_sailing,
                  total_distance_motoring = :dist_motoring,
                  total_distance_sailed   = :dist_sailed,
                  total_distance_upwind   = :dist_upwind,
                  total_distance_reaching = :dist_reaching,
                  total_distance_running  = :dist_running,
                  total_time_upwind       = :time_upwind,
                  total_time_reaching     = :time_reaching,
                  total_time_running      = :time_running",
                params! {
                    "time_moored"   => time_moored,
                    "time_motoring" => time_motoring,
                    "time_sailing"  => time_sailing,
                    "dist_motoring" => dist_motoring,
                    "dist_sailed"   => dist_sailed,
                    "dist_upwind"   => dist_upwind,
                    "dist_reaching" => dist_reaching,
                    "dist_running"  => dist_running,
                    "time_upwind"   => time_upwind,
                    "time_reaching" => time_reaching,
                    "time_running"  => time_running,
                    "trip_id"       => trip_id,
                },
            )?;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -- --test-threads=1 --include-ignored trip::tests::test_correct_engine_status_bumps_version`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/db/operations/trip.rs
git commit -m "refactor: route correct_engine_status through exec_trip_update

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>"
```

---

### Task 6: Refactor the live-trip `UpdateTrip` branch onto the primitive

**Files:**
- Modify: `src/db/operations/vessel_status.rs:1-128`

**Interfaces:**
- Consumes: `exec_trip_update` from Task 2.

This is the hottest path (runs on every status report for an open trip, ~every 30s underway), so its own test matters most for the whole feature: it's the exact path the original bug report's "trip that's longer locally than remote" comes from.

- [ ] **Step 1: Write the failing test**

Add a `#[cfg(test)] mod tests` block at the end of `src/db/operations/vessel_status.rs` (there isn't one yet):

```rust
#[cfg(test)]
mod tests {
    use crate::db::test_helpers::{add_test_trip, setup_db};
    use crate::db::types::{TripOperation, VesselStatusOperation};
    use crate::trip::Trip;
    use crate::utilities::EngineStatus;
    use mysql::params;
    use mysql::prelude::Queryable;
    use std::ops::Add;
    use std::time::{Duration, SystemTime};

    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_insert_status_and_trip_update_bumps_version() {
        let db = setup_db();
        let t = SystemTime::now();
        let trip_id: u32 = add_test_trip(
            &db,
            "Live Trip".to_string(),
            t,
            t.add(Duration::from_secs(1800)),
            0.0,
            0.0,
            0,
            0,
            0,
        )
        .expect("add_test_trip failed");

        let status_op = VesselStatusOperation {
            timestamp: std::time::Instant::now(),
            position: crate::position_utils::Position { latitude: 43.0, longitude: 11.0 },
            average_speed_kn: 5.0,
            max_speed_kn: 6.0,
            is_moored: false,
            engine_on: EngineStatus::Off,
            total_distance_nm: 1.0,
            total_time_ms: 1800,
            wind_speed_kn: None,
            wind_speed_variance: None,
            wind_angle_deg: None,
            wind_angle_variance: None,
            cog_deg: None,
            average_heading_deg: None,
        };
        let trip = Trip {
            id: Some(trip_id as i64),
            uuid: uuid::Uuid::new_v4().to_string(),
            description: "Live Trip".to_string(),
            start_timestamp: t,
            end_timestamp: t.add(Duration::from_secs(2400)),
            total_distance_sailed: 1.0,
            total_distance_motoring: 0.0,
            total_time_sailing: 2400,
            total_time_motoring: 0,
            total_time_moored: 0,
            total_distance_upwind: 0.0,
            total_distance_reaching: 0.0,
            total_distance_running: 0.0,
            total_time_upwind: 0,
            total_time_reaching: 0,
            total_time_running: 0,
        };
        let trip_operation = TripOperation::UpdateTrip(trip);

        db.insert_status_and_trip(&status_op, &trip_operation)
            .expect("insert_status_and_trip failed");

        let mut conn = db.pool.get_conn().unwrap();
        let version: u64 = conn
            .exec_first(
                "SELECT version FROM trips WHERE id = :id",
                params! { "id" => trip_id },
            )
            .unwrap()
            .unwrap();
        assert_eq!(version, 2, "advancing the live trip must bump version");
    }
}
```

`Trip::uuid` is a plain `String` (not `Option`) and `VesselStatusOperation` carries two extra `#[allow(dead_code)]` fields (`wind_speed_variance`, `wind_angle_variance`) alongside the ones read by `insert_status_and_trip` — both included above (verified against `src/trip.rs:5-22` and `src/db/types.rs:10-27`).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -- --include-ignored vessel_status::tests::test_insert_status_and_trip_update_bumps_version`
Expected: FAIL (compile error until the branch is refactored, or `version` stays 1)

- [ ] **Step 3: Refactor the `UpdateTrip` branch**

In `src/db/operations/vessel_status.rs`, add the import:

```rust
use crate::db::operations::trip_update::exec_trip_update;
```

Replace the `tx.exec_drop` call at lines 89-119 (inside `TripOperation::UpdateTrip(trip) => { ... }`):

```rust
            TripOperation::UpdateTrip(trip) => {
                if let Some(trip_id) = trip.id {
                    let end_timestamp = chrono::DateTime::<chrono::Utc>::from(trip.end_timestamp);

                    exec_trip_update(
                        &mut tx,
                        r"end_timestamp = :end_ts,
                          total_distance_sailed = :distance_sailed,
                          total_distance_motoring = :distance_motoring,
                          total_time_sailing = :time_sailing,
                          total_time_motoring = :time_motoring,
                          total_time_moored = :time_moored,
                          total_distance_upwind = :distance_upwind,
                          total_distance_reaching = :distance_reaching,
                          total_distance_running = :distance_running,
                          total_time_upwind = :time_upwind,
                          total_time_reaching = :time_reaching,
                          total_time_running = :time_running",
                        params! {
                            "trip_id" => trip_id,
                            "end_ts" => end_timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                            "distance_sailed" => trip.total_distance_sailed,
                            "distance_motoring" => trip.total_distance_motoring,
                            "time_sailing" => trip.total_time_sailing,
                            "time_motoring" => trip.total_time_motoring,
                            "time_moored" => trip.total_time_moored,
                            "distance_upwind" => trip.total_distance_upwind,
                            "distance_reaching" => trip.total_distance_reaching,
                            "distance_running" => trip.total_distance_running,
                            "time_upwind" => trip.total_time_upwind,
                            "time_reaching" => trip.total_time_reaching,
                            "time_running" => trip.total_time_running,
                        },
                    )?;
                }
                None
            }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -- --test-threads=1 --include-ignored vessel_status::tests::test_insert_status_and_trip_update_bumps_version`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/db/operations/vessel_status.rs
git commit -m "refactor: route live-trip UpdateTrip through exec_trip_update

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>"
```

---

### Task 7: Refactor `fix_mooring_status` onto the primitive

**Files:**
- Modify: `src/db/operations/mooring_fix.rs` (the `UPDATE trips` block ending around line 476-478; find it by searching `total_time_moored      = :time_moored` in this file — it's the second occurrence in the codebase, right before the `tx.commit()?;` that precedes the cache-invalidation calls)

**Interfaces:**
- Consumes: `exec_trip_update` from Task 2.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/db/operations/mooring_fix.rs` (`fix_mooring_status`'s signature is `(&self, start: DateTime<Utc>, end: DateTime<Utc>, is_moored: bool, moored_interval_secs: u64) -> Result<FixMooringReport, AppError>`, confirmed at `src/db/operations/mooring_fix.rs:213-219`):

```rust
    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_fix_mooring_status_bumps_version() {
        let db = setup_db();
        let t = SystemTime::now();
        let trip_id: u32 = add_test_trip(
            &db,
            "Mooring Fix Version Test".to_string(),
            t,
            t.add(Duration::from_secs(3600)),
            0.0,
            0.0,
            0,
            0,
            0,
        )
        .expect("add_test_trip failed");

        add_test_vessel_status(
            &db, t.add(Duration::from_secs(600)), 43.0, 11.0, 0.1, 0.2, None, None,
            false, crate::utilities::EngineStatus::Off, 0.05, 30_000, None, None,
        )
        .expect("add_test_vessel_status failed");

        let start = chrono::DateTime::<chrono::Utc>::from(t.add(Duration::from_secs(300)));
        let end = chrono::DateTime::<chrono::Utc>::from(t.add(Duration::from_secs(900)));
        db.fix_mooring_status(start, end, true, 1800)
            .expect("fix_mooring_status failed");

        let mut conn = db.pool.get_conn().unwrap();
        let version: u64 = conn
            .exec_first(
                "SELECT version FROM trips WHERE id = :id",
                mysql::params! { "id" => trip_id },
            )
            .unwrap()
            .unwrap();
        assert_eq!(version, 2, "fix_mooring_status must bump version");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -- --include-ignored mooring_fix::tests::test_fix_mooring_status_bumps_version`
Expected: FAIL

- [ ] **Step 3: Refactor**

Add `use crate::db::operations::trip_update::exec_trip_update;` at the top of `src/db/operations/mooring_fix.rs`. Replace the `tx.exec_drop` UPDATE block (the one with the 11-field SET clause, no `end_timestamp`, matching Task 5's fragment exactly):

```rust
            exec_trip_update(
                &mut tx,
                r"total_time_moored       = :time_moored,
                  total_time_motoring     = :time_motoring,
                  total_time_sailing      = :time_sailing,
                  total_distance_motoring = :dist_motoring,
                  total_distance_sailed   = :dist_sailed,
                  total_distance_upwind   = :dist_upwind,
                  total_distance_reaching = :dist_reaching,
                  total_distance_running  = :dist_running,
                  total_time_upwind       = :time_upwind,
                  total_time_reaching     = :time_reaching,
                  total_time_running      = :time_running",
                params! {
                    "time_moored"   => time_moored,
                    "time_motoring" => time_motoring,
                    "time_sailing"  => time_sailing,
                    "dist_motoring" => dist_motoring,
                    "dist_sailed"   => dist_sailed,
                    "dist_upwind"   => dist_upwind,
                    "dist_reaching" => dist_reaching,
                    "dist_running"  => dist_running,
                    "time_upwind"   => time_upwind,
                    "time_reaching" => time_reaching,
                    "time_running"  => time_running,
                    "trip_id"       => trip_id,
                },
            )?;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -- --test-threads=1 --include-ignored mooring_fix::tests::test_fix_mooring_status_bumps_version`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/db/operations/mooring_fix.rs
git commit -m "refactor: route fix_mooring_status through exec_trip_update

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>"
```

---

### Task 8: Refactor `recalculate_and_update_trip` onto the primitive

**Files:**
- Modify: `src/db/operations/gap_fill.rs:312-424`

**Interfaces:**
- Consumes: `exec_trip_update` from Task 2. Produces: (unchanged signature) `recalculate_and_update_trip(&self, trip_id: i64, trip_start: SystemTime, trip_end: SystemTime) -> Result<(), AppError>`, consumed by `import_trip` (Task 10) and `src/bin/gap_filler.rs`.

- [ ] **Step 1: Write the failing test**

Find or add a `#[cfg(test)] mod tests` block in `src/db/operations/gap_fill.rs`:

```rust
    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_recalculate_and_update_trip_bumps_version() {
        use crate::db::test_helpers::{add_test_trip, add_test_vessel_status, setup_db};
        use std::time::{Duration, SystemTime};

        let db = setup_db();
        let t = SystemTime::now();
        let trip_id: u32 = add_test_trip(
            &db,
            "Recalc Version Test".to_string(),
            t,
            t + Duration::from_secs(1800),
            0.0,
            0.0,
            0,
            0,
            0,
        )
        .expect("add_test_trip failed");

        add_test_vessel_status(
            &db, t, 43.0, 11.0, 5.0, 6.0, None, None, false,
            crate::utilities::EngineStatus::Off, 1.0, 900_000, None, None,
        )
        .expect("add_test_vessel_status failed");

        db.recalculate_and_update_trip(trip_id as i64, t, t + Duration::from_secs(1800))
            .expect("recalculate_and_update_trip failed");

        let mut conn = db.pool.get_conn().unwrap();
        let version: u64 = conn
            .exec_first(
                "SELECT version FROM trips WHERE id = :id",
                mysql::params! { "id" => trip_id },
            )
            .unwrap()
            .unwrap();
        assert_eq!(version, 2, "recalculate_and_update_trip must bump version");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -- --include-ignored gap_fill::tests::test_recalculate_and_update_trip_bumps_version`
Expected: FAIL

- [ ] **Step 3: Refactor**

Add `use crate::db::operations::trip_update::exec_trip_update;` at the top of `src/db/operations/gap_fill.rs`. Replace the `tx.exec_drop` block at lines 389-419:

```rust
            exec_trip_update(
                &mut tx,
                r"total_time_moored      = :time_moored,
                  total_time_motoring    = :time_motoring,
                  total_time_sailing     = :time_sailing,
                  total_distance_motoring = :dist_motoring,
                  total_distance_sailed   = :dist_sailed,
                  total_distance_upwind   = :dist_upwind,
                  total_distance_reaching = :dist_reaching,
                  total_distance_running  = :dist_running,
                  total_time_upwind       = :time_upwind,
                  total_time_reaching     = :time_reaching,
                  total_time_running      = :time_running,
                  end_timestamp          = :end_ts",
                params! {
                    "time_moored"    => time_moored,
                    "time_motoring"  => time_motoring,
                    "time_sailing"   => time_sailing,
                    "dist_motoring"  => dist_motoring,
                    "dist_sailed"    => dist_sailed,
                    "dist_upwind"    => dist_upwind,
                    "dist_reaching"  => dist_reaching,
                    "dist_running"   => dist_running,
                    "time_upwind"    => time_upwind,
                    "time_reaching"  => time_reaching,
                    "time_running"   => time_running,
                    "end_ts"         => &end_ts_str,
                    "trip_id"        => trip_id,
                },
            )?;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -- --test-threads=1 --include-ignored gap_fill::tests::test_recalculate_and_update_trip_bumps_version`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/db/operations/gap_fill.rs
git commit -m "refactor: route recalculate_and_update_trip through exec_trip_update

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>"
```

---

### Task 9: `bump_trip_version` MCP tool + DB_ANALYST.md

**Files:**
- Modify: `src/bin/mcp_server.rs` (add a params struct near line 100, and a tool method near line 447)
- Modify: `DB_ANALYST.md` (the "Fix Anomalous Sensor Readings" section and the MCP tool table)

**Interfaces:**
- Consumes: `VesselDatabase::bump_trip_version` from Task 2.

- [ ] **Step 1: Add the params struct**

In `src/bin/mcp_server.rs`, after `FixMooringParams` (around line 100):

```rust
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct BumpTripVersionParams {
    trip_id: u32,
}
```

- [ ] **Step 2: Add the tool method**

Inside the same `impl` block as `fix_mooring_status`, right after it (before the closing `}` around line 448):

```rust
    #[tool(description = "Bump a trip's version counter with no other field changes. Use after a direct SQL correction to vessel_status/environmental_data that doesn't itself touch the trips row, so the change is picked up by the next remote sync (see DB_ANALYST.md).")]
    async fn bump_trip_version(
        &self,
        Parameters(BumpTripVersionParams { trip_id }): Parameters<BumpTripVersionParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            db.bump_trip_version(trip_id as i64).map_err(|e| e.to_string())
        })
        .await
        .map_err(db_err)?
        .map_err(db_err)?;
        Ok(CallToolResult::success(vec![Content::text("ok")]))
    }
```

- [ ] **Step 3: Build to verify it compiles**

Run: `cargo build --bin mcp_server`
Expected: builds cleanly

- [ ] **Step 4: Update DB_ANALYST.md**

In the "Fix Anomalous Sensor Readings" section, after the three UPDATE/DELETE strategies (around the current end of that section), add:

```markdown
After any of the above, call the `bump_trip_version` MCP tool with the trip's
`id` so the correction is picked up by the next remote sync — edits to
`vessel_status`/`environmental_data` alone never touch the `trips` row, so
nothing else will flag the trip as changed.
```

Add a row to the MCP tool table:

```markdown
| `bump_trip_version` | Bump a trip's version with no other field changes (for direct SQL corrections that don't touch `trips` itself) |
```

- [ ] **Step 5: Commit**

```bash
git add src/bin/mcp_server.rs DB_ANALYST.md
git commit -m "feat: expose bump_trip_version as an MCP tool

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>"
```

---

### Task 10: `import_trip` gains `is_sync`; version flows through export/import JSON

**Files:**
- Modify: `src/db/operations/import_export.rs` (the `ExportTrip` struct at lines 14-34, `export_trip_to_string` at lines 126-155 and 240-252, `import_trip` at lines 286-373, and the test at line 525)

**Interfaces:**
- Produces: `pub fn import_trip(&self, json_data: &str, is_sync: bool) -> Result<i64, AppError>` (was `import_trip(&self, json_data: &str)`). Every other call site in the codebase is updated in Task 11.
- Consumes: nothing new.

- [ ] **Step 1: Write the failing tests**

Replace the existing test at the bottom of `src/db/operations/import_export.rs` (line 525) — change the call and add version assertions:

```rust
        let trip_id = db.import_trip(json, false).expect("import_trip should succeed");
```

Add two new tests in the same `mod tests` block:

```rust
    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_import_trip_manual_new_uuid_starts_at_version_one() {
        let db = setup_db();
        let json = r#"{
            "trip": {
                "desc": "Manual Import", "start": "2020-05-01T10:00:00.000Z",
                "end": "2020-05-01T10:05:00.000Z", "dist_sail": 0.0, "dist_motor": 0.0,
                "t_sail": 0, "t_motor": 0, "t_moor": 0,
                "uuid": "aaaaaaaa-1111-2222-3333-444444444444"
            }, "vs": [], "em": []
        }"#;

        let trip_id = db.import_trip(json, false).expect("import_trip should succeed");

        let mut conn = db.pool.get_conn().unwrap();
        let version: u64 = conn
            .exec_first(
                "SELECT version FROM trips WHERE id = :id",
                mysql::params! { "id" => trip_id },
            )
            .unwrap()
            .unwrap();
        assert_eq!(version, 1, "a brand-new manually-imported trip starts at version 1");
    }

    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_import_trip_sync_adopts_payload_version_verbatim() {
        let db = setup_db();
        let json = r#"{
            "trip": {
                "desc": "Synced Trip", "start": "2020-05-01T10:00:00.000Z",
                "end": "2020-05-01T10:05:00.000Z", "dist_sail": 0.0, "dist_motor": 0.0,
                "t_sail": 0, "t_motor": 0, "t_moor": 0,
                "uuid": "bbbbbbbb-1111-2222-3333-444444444444", "ver": 7
            }, "vs": [], "em": []
        }"#;

        let trip_id = db.import_trip(json, true).expect("import_trip should succeed");

        let mut conn = db.pool.get_conn().unwrap();
        let version: u64 = conn
            .exec_first(
                "SELECT version FROM trips WHERE id = :id",
                mysql::params! { "id" => trip_id },
            )
            .unwrap()
            .unwrap();
        assert_eq!(version, 7, "is_sync=true must adopt the payload's version verbatim");
    }

    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_import_trip_sync_missing_version_is_an_error() {
        let db = setup_db();
        let json = r#"{
            "trip": {
                "desc": "No Version", "start": "2020-05-01T10:00:00.000Z",
                "end": "2020-05-01T10:05:00.000Z", "dist_sail": 0.0, "dist_motor": 0.0,
                "t_sail": 0, "t_motor": 0, "t_moor": 0,
                "uuid": "cccccccc-1111-2222-3333-444444444444"
            }, "vs": [], "em": []
        }"#;

        let result = db.import_trip(json, true);
        assert!(result.is_err(), "is_sync=true with no 'ver' field must fail, not guess");
    }

    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_import_trip_manual_reimport_bumps_from_existing_version() {
        let db = setup_db();
        let fixed_uuid = "dddddddd-1111-2222-3333-444444444444";
        let make_payload = |desc: &str| {
            format!(
                r#"{{"trip": {{"desc": "{desc}", "start": "2020-05-01T10:00:00.000Z",
                "end": "2020-05-01T10:05:00.000Z", "dist_sail": 0.0, "dist_motor": 0.0,
                "t_sail": 0, "t_motor": 0, "t_moor": 0, "uuid": "{fixed_uuid}"}},
                "vs": [], "em": []}}"#
            )
        };

        db.import_trip(&make_payload("Original"), false)
            .expect("first import should succeed");
        let second_id = db
            .import_trip(&make_payload("Re-imported"), false)
            .expect("second import should succeed");

        let mut conn = db.pool.get_conn().unwrap();
        let version: u64 = conn
            .exec_first(
                "SELECT version FROM trips WHERE id = :id",
                mysql::params! { "id" => second_id },
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            version, 2,
            "manual re-import of the same UUID must bump from the prior stored version, \
             not reset to 1 — this is the exact scenario that silently failed to \
             re-sync before this feature"
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -- --include-ignored import_export::tests`
Expected: FAIL (compile error — `import_trip` still takes one argument; `"ver"` field unread)

- [ ] **Step 3: Add `version` to `ExportTrip` and `export_trip_to_string`**

In `src/db/operations/import_export.rs`, add a field to `ExportTrip` (after `total_time_moored`, line 33):

```rust
    #[serde(rename = "ver")]
    version: u64,
```

In `export_trip_to_string`, extend the SELECT (lines 132-139) and the row reads (after line 152):

```rust
        let trip_row: Option<mysql::Row> = conn.exec_first(
            "SELECT id, start_timestamp, end_timestamp,
                    description, total_distance_sailed,
                    total_distance_motoring, total_time_sailing, total_time_motoring,
                    total_time_moored, uuid, version
             FROM trips WHERE id = :id",
            params! { "id" => trip_id },
        )?;
```

```rust
        let trip_uuid: Option<String> = trip_row.get(9).unwrap_or(None);
        let version: u64 = trip_row.get(10).ok_or(AppError::Database("Missing version".to_string()))?;
```

And in the `ExportTrip` literal (lines 241-252):

```rust
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
                version,
            },
```

- [ ] **Step 4: Change `import_trip`'s signature and version handling**

In `src/db/operations/import_export.rs`, change the signature (line 286):

```rust
    pub fn import_trip(&self, json_data: &str, is_sync: bool) -> Result<i64, AppError> {
```

Replace the UUID-dedup block (lines 318-327) to also capture the existing version:

```rust
        let mut existing_version: Option<u64> = None;
        if let Some(uuid) = import_uuid {
            // UUID present: if a trip with this UUID already exists, delete it first (replace semantics)
            let existing: Option<(u64, u64)> = conn.exec_first(
                "SELECT id, version FROM trips WHERE uuid = :uuid LIMIT 1",
                params! { "uuid" => uuid },
            )?;
            if let Some((id, version)) = existing {
                existing_version = Some(version);
                info!("Import: deleting existing trip {} with UUID {} before re-import", id, uuid);
                self.delete_trip(id as u32)?;
            }
        } else {
```

(the `else` branch with the overlap check is unchanged).

After the `// Re-acquire connection after possible delete_trip` line (line 345), before the `INSERT` (line 356), compute the new version:

```rust
        // Sync receives are authoritative from the boat: adopt its version number
        // verbatim so both sides converge on the exact same value. Manual imports
        // have no authoritative external version to trust — bump from whatever is
        // already stored locally (or start at 1 for a brand-new UUID).
        let new_version: u64 = if is_sync {
            trip["ver"]
                .as_u64()
                .ok_or(AppError::Database("Missing or invalid trip.ver in sync payload".to_string()))?
        } else {
            existing_version.unwrap_or(0) + 1
        };
```

Update the `INSERT` (lines 356-371) to include `version`:

```rust
        tx.exec_drop(
            "INSERT INTO trips (description, start_timestamp, end_timestamp, total_distance_sailed, total_distance_motoring, total_time_sailing, total_time_motoring, total_time_moored, uuid, version)
             VALUES (:desc, :start_ts, :end_ts, :dist_sailed, :dist_motoring, :time_sailing, :time_motoring, :time_moored, :uuid, :version)",
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
                "version" => new_version,
            },
        )?;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -- --test-threads=1 --include-ignored import_export::tests`
Expected: PASS (4 tests: the original point-of-sail test plus the 4 new version tests — 5 total)

- [ ] **Step 6: Commit**

```bash
git add src/db/operations/import_export.rs
git commit -m "feat: import_trip adopts or bumps version depending on is_sync

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>"
```

---

### Task 11: Update every other `import_trip` call site

**Files:**
- Modify: `src/web/api.rs:822` (manual file-upload endpoint — `is_sync = false`)
- Modify: `src/web/api.rs:1621` (`post_sync_trip`, the remote's receive-handler — `is_sync = true`)
- Modify: `src/db/test_examples.rs:584,681,701` (`is_sync = false` — these test the manual-import path)
- Modify: `src/db/operations/sync.rs:656,716,717` (`is_sync = true` — these simulate the remote receiving a boat push)

**Interfaces:**
- Consumes: `import_trip`'s new signature from Task 10.

- [ ] **Step 1: Update `src/web/api.rs:822`** (manual upload endpoint)

```rust
                        match state.db().import_trip(json_content, false) {
```

- [ ] **Step 2: Update `src/web/api.rs:1621`** (`post_sync_trip`)

```rust
    match state.db().import_trip(&json_str, true) {
```

- [ ] **Step 3: Update `src/db/test_examples.rs`**

Line 584:
```rust
            .import_trip(&json_data, false)
```
Line 681:
```rust
            .import_trip(&make_payload("Original"), false)
```
Line 701:
```rust
            .import_trip(&make_payload("Re-imported"), false)
```

- [ ] **Step 4: Update `src/db/operations/sync.rs`**

Line 656 (inside `test_sync_round_trip_per_trip`, which explicitly simulates "what trips_viewer does"):
```rust
            db.import_trip(&json_str, true).expect("import_trip failed");
```
Lines 716-717 (inside `test_sync_trip_idempotent`, same remote-receive simulation):
```rust
        db.import_trip(&json_str, true).expect("first import failed");
        db.import_trip(&json_str, true).expect("second import failed");
```

- [ ] **Step 5: Build and run the full non-DB test suite**

Run: `cargo build && cargo test`
Expected: builds cleanly, all non-`#[ignore]`d tests pass (this task only touches call sites, no new test coverage of its own — Task 10's tests already cover the behavior)

- [ ] **Step 6: Commit**

```bash
git add src/web/api.rs src/db/test_examples.rs src/db/operations/sync.rs
git commit -m "chore: pass is_sync at every import_trip call site

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>"
```

---

### Task 12: Version-map diff replaces `updated_at`-based change detection

**Files:**
- Modify: `src/db/operations/sync.rs` (remove `get_trip_uuids_modified_since` at lines 170-189 and its 4 tests at lines 486-604; add `get_trip_versions` and `compute_uuids_to_push`)

**Interfaces:**
- Produces: `pub fn get_trip_versions(&self) -> Result<HashMap<String, u64>, Box<dyn Error>>` (uuid → version for every trip with a non-null uuid) and `pub(crate) fn compute_uuids_to_push(local_versions: &HashMap<String, u64>, payload_versions: &HashMap<String, u64>) -> Vec<String>` (pure function: UUIDs in `payload_versions` that are either absent from `local_versions` or whose `payload_versions` value exceeds the stored `local_versions` value).
- Consumed by: `src/web/api.rs` in Task 13 (the manifest handlers).

Naming note: `local_versions` here means "the receiving side's own stored versions" and `payload_versions` means "what the sender (boat) just sent" — the function runs on the receiving side.

- [ ] **Step 1: Write the failing tests**

Delete the 4 tests that exercise `get_trip_uuids_modified_since` in `src/db/operations/sync.rs`: `test_modified_since_excludes_trip_not_touched_after_cutoff`, `test_modified_since_includes_trip_touched_after_cutoff`, `test_modified_since_catches_live_trip_end_timestamp_update`, `test_modified_since_detects_totals_only_edit_with_unchanged_end_timestamp` (lines 491-604), and the comment block right above them (lines 486-490).

Add new tests in the same `mod tests` block:

```rust
    #[test]
    fn test_compute_uuids_to_push_flags_missing_uuid() {
        let local: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        let mut payload = std::collections::HashMap::new();
        payload.insert("uuid-1".to_string(), 1u64);

        let result = compute_uuids_to_push(&local, &payload);
        assert_eq!(result, vec!["uuid-1".to_string()]);
    }

    #[test]
    fn test_compute_uuids_to_push_flags_stale_local_version() {
        let mut local = std::collections::HashMap::new();
        local.insert("uuid-1".to_string(), 3u64);
        let mut payload = std::collections::HashMap::new();
        payload.insert("uuid-1".to_string(), 5u64);

        let result = compute_uuids_to_push(&local, &payload);
        assert_eq!(result, vec!["uuid-1".to_string()]);
    }

    #[test]
    fn test_compute_uuids_to_push_skips_up_to_date_trip() {
        let mut local = std::collections::HashMap::new();
        local.insert("uuid-1".to_string(), 5u64);
        let mut payload = std::collections::HashMap::new();
        payload.insert("uuid-1".to_string(), 5u64);

        let result = compute_uuids_to_push(&local, &payload);
        assert!(result.is_empty(), "equal versions must not be re-pushed");
    }

    #[test]
    fn test_compute_uuids_to_push_skips_when_local_is_ahead() {
        // Should not happen in the boat-authoritative one-way flow, but the
        // function must not treat "local newer than payload" as needing a push.
        let mut local = std::collections::HashMap::new();
        local.insert("uuid-1".to_string(), 9u64);
        let mut payload = std::collections::HashMap::new();
        payload.insert("uuid-1".to_string(), 5u64);

        let result = compute_uuids_to_push(&local, &payload);
        assert!(result.is_empty());
    }

    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_get_trip_versions_returns_all() {
        let db = setup_db();
        let t = SystemTime::now();
        let (_, uuid1) = make_trip(&db, "Trip 1", t, 2);
        let (_, uuid2) = make_trip(&db, "Trip 2", t.add(Duration::from_secs(3 * ONE_HOUR_S)), 2);

        let versions = db.get_trip_versions().expect("get_trip_versions failed");
        assert_eq!(versions.len(), 2);
        assert_eq!(versions.get(&uuid1), Some(&1));
        assert_eq!(versions.get(&uuid2), Some(&1));
    }

    /// Regression test for the original bug: a trip edited locally after its
    /// first successful sync must be flagged for re-push on the next manifest
    /// diff, with no cursor/timestamp bookkeeping involved at all.
    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_edit_after_initial_sync_is_flagged_by_version_diff() {
        let boat = setup_db();
        let remote = setup_db();
        let t = SystemTime::now();
        let (trip_id, uuid) = make_trip(&boat, "Round Trip", t, 2);

        // Initial sync: remote adopts the boat's export verbatim (is_sync = true).
        let json = boat.export_trip_to_string(trip_id as i64).unwrap();
        remote.import_trip(&json, true).expect("initial sync import failed");

        let boat_versions = boat.get_trip_versions().expect("boat versions failed");
        let remote_versions = remote.get_trip_versions().expect("remote versions failed");
        assert!(
            compute_uuids_to_push(&remote_versions, &boat_versions).is_empty(),
            "freshly synced trip must not need re-push"
        );

        // Boat edits the trip locally (any exec_trip_update-backed write bumps version).
        boat.update_trip_description(trip_id as i64, "Round Trip (edited)")
            .expect("update_trip_description failed");

        let boat_versions = boat.get_trip_versions().expect("boat versions failed");
        let to_push = compute_uuids_to_push(&remote_versions, &boat_versions);
        assert_eq!(
            to_push,
            vec![uuid],
            "an edit after the initial sync must be flagged for push, with no \
             timestamp cursor involved"
        );
    }
```

Add these `use` statements to the top of the `mod tests` block if not already present: `use std::collections::HashMap;` (for the pure-function tests, which don't need `#[ignore]` or a live DB).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -- sync::tests::test_compute_uuids_to_push`
Expected: FAIL (compile error — `compute_uuids_to_push` not defined)

- [ ] **Step 3: Remove `get_trip_uuids_modified_since`, add the new functions**

In `src/db/operations/sync.rs`, delete `get_trip_uuids_modified_since` (lines 170-189) and its doc comment.

Add, in its place:

```rust
    /// Returns `{uuid: version}` for every trip with a non-null UUID. Sent as
    /// the manifest payload; the receiving side diffs it directly against its
    /// own stored versions — no cursor or timestamp involved.
    pub fn get_trip_versions(&self) -> Result<std::collections::HashMap<String, u64>, Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        let rows: Vec<(String, u64)> = conn.exec(
            "SELECT uuid, version FROM trips WHERE uuid IS NOT NULL",
            (),
        )?;
        Ok(rows.into_iter().collect())
    }
```

Add this free function at the bottom of the file (outside `impl VesselDatabase`, before `fn is_valid_uuid`):

```rust
/// Diff two version maps and return the UUIDs from `payload_versions` that
/// the receiving side (whose own state is `local_versions`) needs pushed:
/// UUIDs it doesn't have at all, or has at a lower version than the payload.
/// Pure and DB-free so it's fully unit-testable without a live database.
pub(crate) fn compute_uuids_to_push(
    local_versions: &std::collections::HashMap<String, u64>,
    payload_versions: &std::collections::HashMap<String, u64>,
) -> Vec<String> {
    payload_versions
        .iter()
        .filter(|(uuid, &payload_version)| match local_versions.get(*uuid) {
            None => true,
            Some(&local_version) => local_version < payload_version,
        })
        .map(|(uuid, _)| uuid.clone())
        .collect()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -- sync::tests::test_compute_uuids_to_push` (no DB needed for these 4)
Expected: PASS

Run: `cargo test -- --test-threads=1 --include-ignored sync::tests`
Expected: PASS (all remaining sync.rs tests, including the two new DB-backed ones and the existing orphan-deletion/idempotency tests from Task 11)

- [ ] **Step 5: Commit**

```bash
git add src/db/operations/sync.rs
git commit -m "feat: replace updated_at-based change detection with a version diff

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>"
```

---

### Task 13: Rewrite the manifest protocol in `api.rs`

**Files:**
- Modify: `src/db/operations/sync.rs:10-22` (`SyncManifestPayload`/`SyncManifestResult` structs)
- Modify: `src/web/api.rs:1345-1529` (`post_sync_push`) and `1557-1605` (`post_sync_manifest`)
- Modify: `DB_ANALYST.md` (the "Remote sync scope" section, lines 112-123)

**Interfaces:**
- Consumes: `get_trip_versions` and `compute_uuids_to_push` from Task 12.
- Produces: the new wire format for `/api/sync/manifest` (a breaking change to both endpoints — both sides must deploy together, as noted in the spec's Rollout section).

This task has no new automated test (it's the HTTP glue layer; the logic it calls was tested in Task 12). Verify manually per Step 4 below.

- [ ] **Step 1: Update the manifest structs**

In `src/db/operations/sync.rs`, replace the struct definitions (lines 10-22):

```rust
/// Payload sent from boat to viewer's `/api/sync/manifest` endpoint: every
/// local trip's UUID and current version. Replaces the old UUID-list-plus-
/// timestamp-cursor exchange — the receiving side diffs this directly
/// against its own stored versions.
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncManifestPayload {
    pub trip_versions: std::collections::HashMap<String, u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SyncManifestResult {
    pub deleted_count: usize,
    /// UUIDs the boat should push: unknown to the remote, or known at a
    /// lower version than the boat just reported.
    pub uuids_to_push: Vec<String>,
}
```

- [ ] **Step 2: Rewrite `post_sync_push`**

In `src/web/api.rs`, replace the body of `post_sync_push` (lines 1345-1529):

```rust
pub async fn post_sync_push(State(state): State<AppState>) -> Json<ApiResponse<SyncResult>> {
    let sync_cfg = &state.config.sync;

    if !sync_cfg.enabled {
        return Json(ApiResponse::error(
            "Sync push is not enabled in config".to_string(),
        ));
    }
    if sync_cfg.target_url.is_empty() {
        return Json(ApiResponse::error(
            "sync.target_url is not configured".to_string(),
        ));
    }
    let api_key = match &sync_cfg.api_key {
        Some(k) => k.clone(),
        None => {
            return Json(ApiResponse::error(
                "sync.api_key is not configured".to_string(),
            ))
        }
    };

    let trip_versions = match state.db().get_trip_versions() {
        Ok(v) => v,
        Err(e) => {
            error!(error = %e, "Sync: failed to get trip versions");
            return Json(ApiResponse::error(format!("DB error: {}", e)));
        }
    };

    let synced_at = chrono::Utc::now().to_rfc3339();

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(sync_cfg.timeout_secs))
        .danger_accept_invalid_certs(sync_cfg.accept_invalid_certs)
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "Sync: failed to build HTTP client");
            return Json(ApiResponse::error(format!("HTTP client error: {}", e)));
        }
    };

    let base_url = sync_cfg.target_url.trim_end_matches('/').to_string();

    // Step 1: Send the version manifest. The remote deletes any of its own
    // trips whose UUID isn't in this map (orphans) and returns exactly the
    // UUIDs it needs — new to it, or at a lower version than reported here.
    let manifest_url = format!("{}/api/sync/manifest", base_url);
    let manifest = SyncManifestPayload { trip_versions };
    let manifest_resp = match client
        .post(&manifest_url)
        .bearer_auth(&api_key)
        .json(&manifest)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let detail = err_chain(&e);
            error!(url = %manifest_url, error = %detail, "Sync: manifest HTTP request failed");
            return Json(ApiResponse::error(format!(
                "Manifest push failed: {}",
                detail
            )));
        }
    };
    if !manifest_resp.status().is_success() {
        let http_status = manifest_resp.status().as_u16();
        let body = manifest_resp.text().await.unwrap_or_default();
        error!(http_status, body = %body, "Sync: manifest rejected by remote");
        return Json(ApiResponse::error(format!(
            "Remote returned HTTP {}: {}",
            http_status, body
        )));
    }
    let manifest_result: ApiResponse<SyncManifestResult> = match manifest_resp.json().await {
        Ok(r) => r,
        Err(e) => {
            let detail = err_chain(&e);
            error!(error = %detail, "Sync: failed to parse manifest response");
            return Json(ApiResponse::error(format!(
                "Bad manifest response: {}",
                detail
            )));
        }
    };
    if manifest_result.status != "ok" {
        return Json(ApiResponse::error(
            manifest_result
                .error
                .unwrap_or_else(|| "Manifest step failed".to_string()),
        ));
    }
    let (deleted_count, uuids_to_push) = match manifest_result.data {
        Some(r) => (r.deleted_count, r.uuids_to_push),
        None => (0, vec![]),
    };

    // Step 2: Fetch and send exactly the trips the remote asked for.
    let updated_trips = match state.db().get_trips_by_uuids(&uuids_to_push) {
        Ok(v) => v,
        Err(e) => {
            error!(error = %e, "Sync: failed to fetch trips by UUID");
            return Json(ApiResponse::error(format!("DB error: {}", e)));
        }
    };

    let trip_url = format!("{}/api/sync/trip", base_url);
    let mut upserted_count = 0usize;
    for trip_value in &updated_trips {
        let trip_resp = match client
            .post(&trip_url)
            .bearer_auth(&api_key)
            .json(trip_value)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let uuid = trip_value["trip"]["uuid"].as_str().unwrap_or("unknown");
                error!(uuid, error = %err_chain(&e), "Sync: trip send failed");
                continue;
            }
        };
        if !trip_resp.status().is_success() {
            let uuid = trip_value["trip"]["uuid"].as_str().unwrap_or("unknown");
            let http_status = trip_resp.status().as_u16();
            error!(uuid, http_status, "Sync: remote rejected trip");
            continue;
        }
        let trip_result: ApiResponse<serde_json::Value> = match trip_resp.json().await {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %err_chain(&e), "Sync: failed to parse trip response");
                continue;
            }
        };
        if trip_result.status == "ok" {
            upserted_count += 1;
        } else {
            let uuid = trip_value["trip"]["uuid"].as_str().unwrap_or("unknown");
            warn!(uuid, error = ?trip_result.error, "Sync: remote failed to upsert trip");
        }
    }

    // Purely cosmetic now — no sync-decision code reads this. A push that failed
    // for some UUIDs simply leaves the remote's version behind for that UUID, so
    // the next manifest diff catches it again automatically.
    if let Err(e) = state
        .db()
        .set_system_status_string("last_synced_at", &synced_at)
    {
        error!(error = %e, "Sync: failed to persist last_synced_at locally");
    }

    info!(deleted_count, upserted_count, "Sync push complete");
    Json(ApiResponse::ok(SyncResult {
        deleted_count,
        upserted_count,
        synced_at,
    }))
}
```

- [ ] **Step 3: Rewrite `post_sync_manifest`**

Replace the body (lines 1557-1605):

```rust
pub async fn post_sync_manifest(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SyncManifestPayload>,
) -> Response {
    if let Some(err) = verify_sync_token(&state, &headers) {
        return err;
    }

    let keep_uuids: Vec<String> = payload.trip_versions.keys().cloned().collect();
    let deleted_count = match state.db().delete_trips_not_in_uuids(&keep_uuids) {
        Ok(n) => n,
        Err(e) => {
            error!(error = %e, "Sync manifest: delete orphans failed");
            return Json(ApiResponse::<SyncManifestResult>::error(e.to_string())).into_response();
        }
    };

    let local_versions = match state.db().get_trip_versions() {
        Ok(v) => v,
        Err(e) => {
            error!(error = %e, "Sync manifest: failed to get local trip versions");
            return Json(ApiResponse::<SyncManifestResult>::error(e.to_string())).into_response();
        }
    };
    let uuids_to_push = compute_uuids_to_push(&local_versions, &payload.trip_versions);

    // Cosmetic only, matching post_sync_push — last_synced_at is display-only now.
    let synced_at = chrono::Utc::now().to_rfc3339();
    if let Err(e) = state
        .db()
        .set_system_status_string("last_synced_at", &synced_at)
    {
        error!(error = %e, "Sync manifest: failed to persist synced_at");
    }

    let push_count = uuids_to_push.len();
    info!(deleted_count, push_count, "Sync manifest applied");
    Json(ApiResponse::ok(SyncManifestResult {
        deleted_count,
        uuids_to_push,
    }))
    .into_response()
}
```

`compute_uuids_to_push` is `pub(crate)` (Task 12), and `api.rs` is in the same crate, so update the import line near the top of `api.rs`:

```rust
use crate::db::operations::sync::{compute_uuids_to_push, SyncManifestPayload, SyncManifestResult, SyncResult};
```

- [ ] **Step 4: Verify manually**

Run: `cargo build --release`
Expected: builds cleanly.

Since both `post_sync_push` and `post_sync_manifest` live in the same binary and this is a breaking wire-format change, a real end-to-end check requires the new binary on **both** the boat and Railway (see the spec's Rollout section — this is a coordinated two-deploy change, not something to test against the currently-deployed old-format Railway instance). Defer the live push/pull check until both sides are redeployed; `cargo test -- --test-threads=1 --include-ignored` covers the DB-level logic already.

- [ ] **Step 5: Update DB_ANALYST.md's "Remote sync scope" section**

Replace the existing section (lines 112-123) with:

```markdown
### Remote sync scope

Every trip carries a `version` counter (`BIGINT UNSIGNED`, starts at 1).
`exec_trip_update` (`src/db/operations/trip_update.rs`) is the only function
allowed to write to `trips` in place, and it always bumps `version` — so no
write path can forget to. The boat's push sync sends `{uuid: version}` for
every trip; the remote diffs it directly against its own stored versions and
reports back exactly which UUIDs need pushing. There is no timestamp cursor
involved in this decision anymore.

A direct SQL edit to `vessel_status`/`environmental_data` that never touches
the `trips` row itself will not be picked up automatically — call the
`bump_trip_version` MCP tool (or `UPDATE trips SET version = version + 1
WHERE id = <id>`) afterward so the change reaches the remote viewer.
```

- [ ] **Step 6: Commit**

```bash
git add src/db/operations/sync.rs src/web/api.rs DB_ANALYST.md
git commit -m "feat: rewrite the manifest protocol around version diffing

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>"
```

---

## Post-plan note

This plan intentionally does not touch `last_synced_at`'s *storage* (kept for
the `GET /api/sync/status` display) — only its use as a change-detection
cursor, which is removed in Task 13. Both sides (boat and Railway) must be
redeployed together once all tasks are done; per the spec, a mismatched
single-side deploy fails closed (JSON deserialization error on the new
`trip_versions` field) rather than corrupting data.
