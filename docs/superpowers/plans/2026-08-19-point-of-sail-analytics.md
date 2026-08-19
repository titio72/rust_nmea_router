# Point-of-Sail Analytics (Upwind / Reaching / Running) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Break sailing time and distance down into upwind / reaching / running at the trip, leg, and yearly analytics levels, using the true wind angle already recorded on every `vessel_status` row.

**Architecture:** Extend the three existing sailing/motoring aggregation pipelines (`trips` table live-updated per report, `trip_legs_cache` computed per leg, `heatmap_cache` computed per day and rolled up into months/years) with three parallel sub-buckets, computed by folding `average_wind_angle_deg` to 0–180° and classifying against fixed thresholds. No new tables, no new API endpoints — existing response structs and MCP tools gain fields and serialize them automatically.

**Tech Stack:** Rust (mysql crate, `params!` macro), MariaDB, vanilla JS/HTML frontend.

**Spec:** `docs/superpowers/specs/2026-08-19-point-of-sail-analytics-design.md`

## Global Constraints

- Classification: fold true wind angle to 0–180° via `wind_angle_deg.min(360.0 - wind_angle_deg)`, then **upwind** `folded <= 60.0`, **reaching** `60.0 < folded < 120.0`, **running** `folded >= 120.0`. Fixed, not configurable.
- Scope: only rows where `is_moored = 0 AND engine_on != 1` (sailing) are ever classified. Motoring rows and moored rows contribute to none of the three buckets. Rows with `average_wind_angle_deg IS NULL` count toward sailing totals but none of the three buckets (uncategorized remainder).
- All new DB columns are `NOT NULL DEFAULT 0` (DOUBLE for distance/nm, BIGINT UNSIGNED for time/ms), added via each table's existing self-migration pattern where one exists.
- Naming: DB columns on `trips` follow that table's own `total_distance_sailed` / `total_time_sailing` convention → `total_distance_upwind/reaching/running`, `total_time_upwind/reaching/running`. DB columns on `trip_legs_cache`/`heatmap_cache` and all API-facing struct fields (`TripSummary`, `TripLeg`, `MonthlyStatistic`) follow the `sailing_distance_nm` / `sailing_time_ms` convention → `upwind_distance_nm`, `reaching_distance_nm`, `running_distance_nm`, `upwind_time_ms`, `reaching_time_ms`, `running_time_ms`.
- **Do not modify `trim_trip`** (`src/db/operations/trip.rs`). It only deletes moored padding beyond a 1-hour buffer around the first/last non-moored point and adjusts `start_timestamp`/`end_timestamp` — it never recomputes `total_distance_sailed`/`total_time_sailing` today, because moored padding contributes ~0 to those sums. The same reasoning holds for the new point-of-sail sums: they're a subdivision of sailing time only, which trimming never touches. Adding a recompute here would be an unrelated, unrequested change to existing behavior.
- **Do not modify `src/bin/mcp_server.rs`.** Its `get_trip`, `get_trip_legs`, and `get_monthly_statistics` tools call `to_json(data)` directly on the same `TripSummary`/`TripLegsData`/`MonthlyStatistics` structs this plan extends — new fields serialize automatically once the structs gain them. No MCP-specific code exists to touch.
- Per this project's CLAUDE.md: do not run `git commit` or `git push` unless the user (not this plan) explicitly asks in that session. Each task below ends with a `git add` + `git commit` step per the writing-plans template — **executors must skip the actual commit command and instead stop for the user to review and commit**, exactly as CLAUDE.md requires. Do run `cargo build`/`cargo test` as written; only the commit step is skipped.

---

### Task 1: Point-of-sail classification helper

**Files:**
- Modify: `src/utilities.rs`

**Interfaces:**
- Produces: `pub enum PointOfSail { Upwind, Reaching, Running }` and `pub fn point_of_sail_from_twa(wind_angle_deg: f64) -> PointOfSail` — used by Task 2 (live path), Task 4 (`recalculate_and_update_trip` uses the SQL-equivalent `LEAST()` form directly, not this function), Task 7 (`finalize_leg`).

- [ ] **Step 1: Write the failing tests**

Add to the existing `#[cfg(test)] mod tests` block at the bottom of `src/utilities.rs` (alongside `test_true_wind_headwind` etc.):

```rust
#[test]
fn test_point_of_sail_upwind_boundaries() {
    assert_eq!(point_of_sail_from_twa(0.0), PointOfSail::Upwind);
    assert_eq!(point_of_sail_from_twa(60.0), PointOfSail::Upwind);
    assert_eq!(point_of_sail_from_twa(300.0), PointOfSail::Upwind); // folds to 60
}

#[test]
fn test_point_of_sail_reaching_boundaries() {
    assert_eq!(point_of_sail_from_twa(60.1), PointOfSail::Reaching);
    assert_eq!(point_of_sail_from_twa(90.0), PointOfSail::Reaching);
    assert_eq!(point_of_sail_from_twa(119.9), PointOfSail::Reaching);
    assert_eq!(point_of_sail_from_twa(270.0), PointOfSail::Reaching); // folds to 90
}

#[test]
fn test_point_of_sail_running_boundaries() {
    assert_eq!(point_of_sail_from_twa(120.0), PointOfSail::Running);
    assert_eq!(point_of_sail_from_twa(180.0), PointOfSail::Running);
    assert_eq!(point_of_sail_from_twa(185.0), PointOfSail::Running); // folds to 175
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test point_of_sail -- --nocapture`
Expected: FAIL with "cannot find function `point_of_sail_from_twa`" / "cannot find type `PointOfSail`" (compile error).

- [ ] **Step 3: Implement**

Add near the top of `src/utilities.rs`, alongside the other angle-related functions:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointOfSail {
    Upwind,
    Reaching,
    Running,
}

/// Classify a point of sail from a true wind angle (0-360 deg, relative to the bow).
/// Folds to 0-180 deg (symmetric port/starboard), then buckets on fixed thresholds:
/// upwind <=60 deg, reaching 60-120 deg, running >=120 deg.
pub fn point_of_sail_from_twa(wind_angle_deg: f64) -> PointOfSail {
    let folded = wind_angle_deg.min(360.0 - wind_angle_deg);
    if folded <= 60.0 {
        PointOfSail::Upwind
    } else if folded < 120.0 {
        PointOfSail::Reaching
    } else {
        PointOfSail::Running
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test point_of_sail -- --nocapture`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/utilities.rs
git commit -m "feat: add point-of-sail classification helper"
```

---

### Task 2: Live incremental trip accumulation (`Trip` struct)

**Files:**
- Modify: `src/trip.rs`
- Modify: `src/vessel_status_handler.rs:200-224`

**Interfaces:**
- Consumes: `point_of_sail_from_twa(f64) -> PointOfSail` (Task 1), `PointOfSail` enum (Task 1).
- Produces: `Trip` struct gains `total_distance_upwind: f64`, `total_distance_reaching: f64`, `total_distance_running: f64`, `total_time_upwind: u64`, `total_time_reaching: u64`, `total_time_running: u64`. `Trip::update()` signature becomes `update(&mut self, end_timestamp: SystemTime, distance: f64, time_ms: u64, engine_on: EngineStatus, is_moored: bool, wind_angle_deg: Option<f64>)`. Consumed by Task 3 (`insert_status_and_trip` persists these fields).

- [ ] **Step 1: Write the failing tests**

Add to `src/trip.rs`'s existing `#[cfg(test)] mod tests` block, and update the 4 existing calls to `trip.update(...)` to pass a 6th argument:

```rust
    #[test]
    fn test_update_sailing() {
        let now = SystemTime::now();
        let mut trip = Trip::new(now, "Test Trip".to_string());

        let later = now + Duration::from_secs(100);
        trip.update(later, 1000.0, 100000, EngineStatus::Off, false, None);

        assert_eq!(trip.total_distance_sailed, 1000.0);
        assert_eq!(trip.total_time_sailing, 100000);
        assert_eq!(trip.total_distance_motoring, 0.0);
        assert_eq!(trip.total_time_motoring, 0);
        assert_eq!(trip.total_time_moored, 0);
    }

    #[test]
    fn test_update_motoring() {
        let now = SystemTime::now();
        let mut trip = Trip::new(now, "Test Trip".to_string());

        let later = now + Duration::from_secs(100);
        trip.update(later, 2000.0, 100000, EngineStatus::On, false, Some(30.0));

        assert_eq!(trip.total_distance_motoring, 2000.0);
        assert_eq!(trip.total_time_motoring, 100000);
        assert_eq!(trip.total_distance_sailed, 0.0);
        assert_eq!(trip.total_time_sailing, 0);
        assert_eq!(trip.total_time_moored, 0);
        // Motoring never contributes to point-of-sail buckets, even with a wind angle present
        assert_eq!(trip.total_distance_upwind, 0.0);
        assert_eq!(trip.total_time_upwind, 0);
    }

    #[test]
    fn test_update_moored() {
        let now = SystemTime::now();
        let mut trip = Trip::new(now, "Test Trip".to_string());

        let later = now + Duration::from_secs(100);
        trip.update(later, 0.0, 100000, EngineStatus::Off, true, Some(30.0));

        assert_eq!(trip.total_time_moored, 100000);
        assert_eq!(trip.total_distance_sailed, 0.0);
        assert_eq!(trip.total_distance_motoring, 0.0);
        assert_eq!(trip.total_time_sailing, 0);
        assert_eq!(trip.total_time_motoring, 0);
        assert_eq!(trip.total_distance_upwind, 0.0);
    }

    #[test]
    fn test_update_sailing_upwind() {
        let now = SystemTime::now();
        let mut trip = Trip::new(now, "Test Trip".to_string());

        let later = now + Duration::from_secs(100);
        trip.update(later, 5.0, 100000, EngineStatus::Off, false, Some(30.0));

        assert_eq!(trip.total_distance_sailed, 5.0);
        assert_eq!(trip.total_distance_upwind, 5.0);
        assert_eq!(trip.total_time_upwind, 100000);
        assert_eq!(trip.total_distance_reaching, 0.0);
        assert_eq!(trip.total_distance_running, 0.0);
    }

    #[test]
    fn test_update_sailing_reaching() {
        let now = SystemTime::now();
        let mut trip = Trip::new(now, "Test Trip".to_string());

        let later = now + Duration::from_secs(100);
        trip.update(later, 5.0, 100000, EngineStatus::Off, false, Some(90.0));

        assert_eq!(trip.total_distance_reaching, 5.0);
        assert_eq!(trip.total_time_reaching, 100000);
        assert_eq!(trip.total_distance_upwind, 0.0);
        assert_eq!(trip.total_distance_running, 0.0);
    }

    #[test]
    fn test_update_sailing_running() {
        let now = SystemTime::now();
        let mut trip = Trip::new(now, "Test Trip".to_string());

        let later = now + Duration::from_secs(100);
        trip.update(later, 5.0, 100000, EngineStatus::Off, false, Some(160.0));

        assert_eq!(trip.total_distance_running, 5.0);
        assert_eq!(trip.total_time_running, 100000);
        assert_eq!(trip.total_distance_upwind, 0.0);
        assert_eq!(trip.total_distance_reaching, 0.0);
    }

    #[test]
    fn test_update_sailing_no_wind_data() {
        let now = SystemTime::now();
        let mut trip = Trip::new(now, "Test Trip".to_string());

        let later = now + Duration::from_secs(100);
        trip.update(later, 5.0, 100000, EngineStatus::Off, false, None);

        // Counted in sailing totals, but none of the three buckets
        assert_eq!(trip.total_distance_sailed, 5.0);
        assert_eq!(trip.total_distance_upwind, 0.0);
        assert_eq!(trip.total_distance_reaching, 0.0);
        assert_eq!(trip.total_distance_running, 0.0);
    }
```

Also update `test_new_trip` to assert the 6 new fields start at zero:

```rust
        assert_eq!(trip.total_distance_upwind, 0.0);
        assert_eq!(trip.total_distance_reaching, 0.0);
        assert_eq!(trip.total_distance_running, 0.0);
        assert_eq!(trip.total_time_upwind, 0);
        assert_eq!(trip.total_time_reaching, 0);
        assert_eq!(trip.total_time_running, 0);
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib trip::tests`
Expected: FAIL to compile — `update()` called with 6 args but takes 5; `total_distance_upwind` field doesn't exist.

- [ ] **Step 3: Implement**

In `src/trip.rs`, add the 6 fields to the `Trip` struct and initialize them in `new()`:

```rust
#[derive(Debug, Clone)]
pub struct Trip {
    pub id: Option<i64>,
    pub uuid: String,
    pub description: String,
    pub start_timestamp: SystemTime,
    pub end_timestamp: SystemTime,
    pub total_distance_sailed: f64,   // nautical miles
    pub total_distance_motoring: f64, // nautical miles
    pub total_time_sailing: u64,      // milliseconds
    pub total_time_motoring: u64,     // milliseconds
    pub total_time_moored: u64,       // milliseconds
    pub total_distance_upwind: f64,   // nautical miles, subset of total_distance_sailed
    pub total_distance_reaching: f64, // nautical miles, subset of total_distance_sailed
    pub total_distance_running: f64,  // nautical miles, subset of total_distance_sailed
    pub total_time_upwind: u64,       // milliseconds, subset of total_time_sailing
    pub total_time_reaching: u64,     // milliseconds, subset of total_time_sailing
    pub total_time_running: u64,      // milliseconds, subset of total_time_sailing
}

impl Trip {
    pub fn new(start_timestamp: SystemTime, description: String) -> Self {
        Self {
            id: None,
            uuid: uuid::Uuid::new_v4().to_string(),
            description,
            start_timestamp,
            end_timestamp: start_timestamp,
            total_distance_sailed: 0.0,
            total_distance_motoring: 0.0,
            total_time_sailing: 0,
            total_time_motoring: 0,
            total_time_moored: 0,
            total_distance_upwind: 0.0,
            total_distance_reaching: 0.0,
            total_distance_running: 0.0,
            total_time_upwind: 0,
            total_time_reaching: 0,
            total_time_running: 0,
        }
    }
```

Update `update()`:

```rust
    /// Update the trip with new vessel status data
    /// Unknown engine status is treated as sailing (conservative approach)
    pub fn update(
        &mut self,
        end_timestamp: SystemTime,
        distance: f64,
        time_ms: u64,
        engine_on: EngineStatus,
        is_moored: bool,
        wind_angle_deg: Option<f64>,
    ) {
        self.end_timestamp = end_timestamp;

        if is_moored {
            self.total_time_moored += time_ms;
        } else if engine_on.is_on() {
            self.total_distance_motoring += distance;
            self.total_time_motoring += time_ms;
        } else {
            // Both Off and Unknown are treated as sailing
            self.total_distance_sailed += distance;
            self.total_time_sailing += time_ms;

            if let Some(angle) = wind_angle_deg {
                use crate::utilities::{point_of_sail_from_twa, PointOfSail};
                match point_of_sail_from_twa(angle) {
                    PointOfSail::Upwind => {
                        self.total_distance_upwind += distance;
                        self.total_time_upwind += time_ms;
                    }
                    PointOfSail::Reaching => {
                        self.total_distance_reaching += distance;
                        self.total_time_reaching += time_ms;
                    }
                    PointOfSail::Running => {
                        self.total_distance_running += distance;
                        self.total_time_running += time_ms;
                    }
                }
            }
        }
    }
```

In `src/vessel_status_handler.rs`, update both call sites (around line 200 and 213) to pass the wind angle from `status`:

```rust
            let mut new_trip = Trip::new(start_time, description);
            new_trip.update(
                report_systemtime,
                effective_distance,
                delta_time_ms,
                status.engine_on,
                status.is_moored,
                status.wind_angle_deg,
            );
```

```rust
            if let Some(ref mut trip) = *current_trip {
                trip.update(
                    report_systemtime,
                    effective_distance,
                    delta_time_ms,
                    status.engine_on,
                    status.is_moored,
                    status.wind_angle_deg,
                );
                TripOperation::UpdateTrip(trip.clone())
            } else {
                TripOperation::None
            }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib trip::tests && cargo build`
Expected: PASS; crate builds clean (confirms `vessel_status_handler.rs` call sites compile).

- [ ] **Step 5: Commit**

```bash
git add src/trip.rs src/vessel_status_handler.rs
git commit -m "feat: accumulate point-of-sail distance/time on live Trip updates"
```

---

### Task 3: Persist point-of-sail totals on the `trips` table

**Files:**
- Modify: `schema.sql`
- Modify: `src/db/operations/vessel_status.rs`

**Interfaces:**
- Consumes: `Trip.total_distance_upwind/reaching/running`, `Trip.total_time_upwind/reaching/running` (Task 2).
- Produces: `trips` table columns `total_distance_upwind`, `total_distance_reaching`, `total_distance_running` (DOUBLE), `total_time_upwind`, `total_time_reaching`, `total_time_running` (BIGINT). Consumed by Task 4, Task 5, Task 6.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/db/operations/vessel_status.rs` (alongside `test_insert_status_creates_trip`):

```rust
    #[test]
    #[ignore]
    fn test_insert_status_creates_trip_persists_point_of_sail() {
        let db = setup_db();
        let op = make_status_op(); // wind_angle_deg: Some(45.0) -> upwind (folded 45 <= 60)
        let ts_sys = dirty_instant_to_systemtime(op.timestamp);
        let mut trip = Trip::new(ts_sys, "POS Test".to_string());
        trip.update(
            ts_sys,
            op.total_distance_nm,
            op.total_time_ms,
            op.engine_on,
            op.is_moored,
            op.wind_angle_deg,
        );
        let result = db
            .insert_status_and_trip(&op, &TripOperation::CreateTrip(trip))
            .unwrap();
        let trip_id = result.expect("CreateTrip should return Some(id)");

        let mut conn = db.pool.get_conn().unwrap();
        let row: (f64, f64, f64, u64, u64, u64) = conn
            .exec_first(
                "SELECT total_distance_upwind, total_distance_reaching, total_distance_running,
                        total_time_upwind, total_time_reaching, total_time_running
                 FROM trips WHERE id = :id",
                mysql::params! { "id" => trip_id },
            )
            .unwrap()
            .unwrap();
        assert_approx_equal(row.0, 1.2, 0.001, "total_distance_upwind");
        assert_approx_equal(row.1, 0.0, 0.001, "total_distance_reaching");
        assert_approx_equal(row.2, 0.0, 0.001, "total_distance_running");
        assert_eq!(row.3, 600_000, "total_time_upwind");
        assert_eq!(row.4, 0, "total_time_reaching");
        assert_eq!(row.5, 0, "total_time_running");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --ignored test_insert_status_creates_trip_persists_point_of_sail -- --test-threads=1`
Expected: FAIL — `Unknown column 'total_distance_upwind' in 'field list'`.

- [ ] **Step 3: Implement**

In `schema.sql`, extend the `trips` table definition (after `total_time_moored`, before `uuid`):

```sql
CREATE TABLE IF NOT EXISTS trips (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    description VARCHAR(255) NOT NULL COMMENT 'Trip name, auto-generated as "Trip YYYY-MM-DD"',
    start_timestamp DATETIME(3) NOT NULL COMMENT 'Trip start time in UTC',
    end_timestamp DATETIME(3) NOT NULL COMMENT 'Trip end time in UTC (updated with each status report)',
    total_distance_sailed DOUBLE NOT NULL DEFAULT 0 COMMENT 'Distance traveled under sail in nautical miles',
    total_distance_motoring DOUBLE NOT NULL DEFAULT 0 COMMENT 'Distance traveled with engine in nautical miles',
    total_time_sailing BIGINT NOT NULL DEFAULT 0 COMMENT 'Time spent sailing in milliseconds',
    total_time_motoring BIGINT NOT NULL DEFAULT 0 COMMENT 'Time spent motoring in milliseconds',
    total_time_moored BIGINT NOT NULL DEFAULT 0 COMMENT 'Time spent moored in milliseconds',
    total_distance_upwind DOUBLE NOT NULL DEFAULT 0 COMMENT 'Sailing distance with folded TWA <= 60 deg, in nautical miles',
    total_distance_reaching DOUBLE NOT NULL DEFAULT 0 COMMENT 'Sailing distance with folded TWA 60-120 deg, in nautical miles',
    total_distance_running DOUBLE NOT NULL DEFAULT 0 COMMENT 'Sailing distance with folded TWA >= 120 deg, in nautical miles',
    total_time_upwind BIGINT NOT NULL DEFAULT 0 COMMENT 'Sailing time with folded TWA <= 60 deg, in milliseconds',
    total_time_reaching BIGINT NOT NULL DEFAULT 0 COMMENT 'Sailing time with folded TWA 60-120 deg, in milliseconds',
    total_time_running BIGINT NOT NULL DEFAULT 0 COMMENT 'Sailing time with folded TWA >= 120 deg, in milliseconds',
    uuid CHAR(36) NULL COMMENT 'UUID v4 for portable trip identification (used for import deduplication)',
    updated_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3) COMMENT 'Bumped by MariaDB on any UPDATE to this row; drives remote sync change-detection',
    INDEX idx_end_timestamp (end_timestamp),
    INDEX idx_start_timestamp (start_timestamp),
    INDEX idx_trips_time_range (start_timestamp, end_timestamp),
    UNIQUE INDEX idx_trips_uuid (uuid)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='Stores vessel trips with sailing vs motoring breakdown';

-- For existing databases, run:
-- ALTER TABLE trips ADD COLUMN uuid CHAR(36) NULL COMMENT 'UUID v4 for portable trip identification';
-- ALTER TABLE trips
--     ADD COLUMN total_distance_upwind DOUBLE NOT NULL DEFAULT 0,
--     ADD COLUMN total_distance_reaching DOUBLE NOT NULL DEFAULT 0,
--     ADD COLUMN total_distance_running DOUBLE NOT NULL DEFAULT 0,
--     ADD COLUMN total_time_upwind BIGINT NOT NULL DEFAULT 0,
--     ADD COLUMN total_time_reaching BIGINT NOT NULL DEFAULT 0,
--     ADD COLUMN total_time_running BIGINT NOT NULL DEFAULT 0;
```

In `src/db/operations/vessel_status.rs`, extend both the `CreateTrip` INSERT and `UpdateTrip` UPDATE statements in `insert_status_and_trip`:

```rust
            TripOperation::CreateTrip(trip) => {
                let start_timestamp = chrono::DateTime::<chrono::Utc>::from(trip.start_timestamp);
                let end_timestamp = chrono::DateTime::<chrono::Utc>::from(trip.end_timestamp);

                tx.exec_drop(
                    r"INSERT INTO trips
                      (description, start_timestamp, end_timestamp,
                       total_distance_sailed, total_distance_motoring,
                       total_time_sailing, total_time_motoring, total_time_moored,
                       total_distance_upwind, total_distance_reaching, total_distance_running,
                       total_time_upwind, total_time_reaching, total_time_running, uuid)
                      VALUES (:description, :start_ts, :end_ts,
                              :distance_sailed, :distance_motoring,
                              :time_sailing, :time_motoring, :time_moored,
                              :distance_upwind, :distance_reaching, :distance_running,
                              :time_upwind, :time_reaching, :time_running, :uuid)",
                    params! {
                        "description" => &trip.description,
                        "start_ts" => start_timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
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
                        "uuid" => &trip.uuid,
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
                              total_time_moored = :time_moored,
                              total_distance_upwind = :distance_upwind,
                              total_distance_reaching = :distance_reaching,
                              total_distance_running = :distance_running,
                              total_time_upwind = :time_upwind,
                              total_time_reaching = :time_reaching,
                              total_time_running = :time_running
                          WHERE id = :trip_id",
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

Since `trips` has no in-app auto-migration (unlike `heatmap_cache`/`trip_legs_cache`, it's created once from `schema.sql`), the test database needs the new columns manually before running the test. Run this once against the database `test_config.json` points at:

```sql
ALTER TABLE trips
    ADD COLUMN total_distance_upwind DOUBLE NOT NULL DEFAULT 0,
    ADD COLUMN total_distance_reaching DOUBLE NOT NULL DEFAULT 0,
    ADD COLUMN total_distance_running DOUBLE NOT NULL DEFAULT 0,
    ADD COLUMN total_time_upwind BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN total_time_reaching BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN total_time_running BIGINT NOT NULL DEFAULT 0;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --ignored test_insert_status_creates_trip_persists_point_of_sail -- --test-threads=1`
Expected: PASS. Also run the full existing suite to confirm no regression: `cargo test --ignored -- --test-threads=1`.

- [ ] **Step 5: Commit**

```bash
git add schema.sql src/db/operations/vessel_status.rs
git commit -m "feat: persist point-of-sail totals on the trips table"
```

---

### Task 4: Extend `recalculate_and_update_trip` correction path

**Files:**
- Modify: `src/db/operations/gap_fill.rs:306-380`

**Interfaces:**
- Consumes: `trips.total_distance_upwind/reaching/running`, `total_time_upwind/reaching/running` columns (Task 3).
- Produces: nothing new consumed elsewhere — this is a leaf correction path used by the gap filler.

- [ ] **Step 1: Write the failing test**

Add to the existing test module in `src/db/operations/gap_fill.rs` (find the module's existing `recalculate_and_update_trip` test for exact helper usage/imports to mirror; use `add_test_vessel_status` with varying `average_wind_angle_deg` values):

```rust
    #[test]
    #[ignore]
    fn test_recalculate_and_update_trip_computes_point_of_sail() {
        let db = setup_db();
        let start = SystemTime::now();
        let trip_id = add_test_trip(
            &db, "POS Recalc".to_string(), start,
            start + Duration::from_secs(300), 0.0, 0.0, 0, 0, 0,
        ).unwrap();

        // Sailing, upwind (folded 30 <= 60): 2.0 nm / 60_000 ms
        add_test_vessel_status(
            &db, start, 50.0, -1.0, 6.0, 6.0,
            Some(12.0), Some(30.0), false, EngineStatus::Off, 2.0, 60_000, None, None,
        ).unwrap();
        // Sailing, reaching (folded 90): 3.0 nm / 60_000 ms
        add_test_vessel_status(
            &db, start + Duration::from_secs(60), 50.01, -1.0, 6.0, 6.0,
            Some(12.0), Some(90.0), false, EngineStatus::Off, 3.0, 60_000, None, None,
        ).unwrap();
        // Sailing, running (folded 160): 1.0 nm / 30_000 ms
        add_test_vessel_status(
            &db, start + Duration::from_secs(120), 50.02, -1.0, 6.0, 6.0,
            Some(12.0), Some(160.0), false, EngineStatus::Off, 1.0, 30_000, None, None,
        ).unwrap();
        // Motoring with a wind angle present: must NOT count toward any bucket
        add_test_vessel_status(
            &db, start + Duration::from_secs(180), 50.03, -1.0, 6.0, 6.0,
            Some(12.0), Some(30.0), false, EngineStatus::On, 4.0, 60_000, None, None,
        ).unwrap();

        db.recalculate_and_update_trip(
            trip_id as i64, start, start + Duration::from_secs(300),
        ).unwrap();

        let mut conn = db.pool.get_conn().unwrap();
        let row: (f64, f64, f64, u64, u64, u64) = conn
            .exec_first(
                "SELECT total_distance_upwind, total_distance_reaching, total_distance_running,
                        total_time_upwind, total_time_reaching, total_time_running
                 FROM trips WHERE id = :id",
                mysql::params! { "id" => trip_id },
            )
            .unwrap()
            .unwrap();
        assert_approx_equal(row.0, 2.0, 0.001, "total_distance_upwind");
        assert_approx_equal(row.1, 3.0, 0.001, "total_distance_reaching");
        assert_approx_equal(row.2, 1.0, 0.001, "total_distance_running");
        assert_eq!(row.3, 60_000, "total_time_upwind");
        assert_eq!(row.4, 60_000, "total_time_reaching");
        assert_eq!(row.5, 30_000, "total_time_running");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --ignored test_recalculate_and_update_trip_computes_point_of_sail -- --test-threads=1`
Expected: FAIL — columns exist (Task 3 already added them) but remain `0` because `recalculate_and_update_trip` doesn't populate them yet, so the `assert_approx_equal(row.0, 2.0, ...)` assertion fails.

- [ ] **Step 3: Implement**

In `src/db/operations/gap_fill.rs`, extend the aggregate `SELECT` and the `UPDATE trips` in `recalculate_and_update_trip`:

```rust
        let row: Option<mysql::Row> = tx.exec_first(
            r"SELECT
                  SUM(CASE WHEN is_moored = 1 THEN total_time_ms  ELSE 0 END) AS time_moored,
                  SUM(CASE WHEN is_moored = 0 AND engine_on = 1 THEN total_time_ms  ELSE 0 END) AS time_motoring,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 THEN total_time_ms ELSE 0 END) AS time_sailing,
                  SUM(CASE WHEN is_moored = 0 AND engine_on = 1 THEN total_distance_nm ELSE 0 END) AS dist_motoring,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 THEN total_distance_nm ELSE 0 END) AS dist_sailed,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 AND average_wind_angle_deg IS NOT NULL
                           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) <= 60
                           THEN total_distance_nm ELSE 0 END) AS dist_upwind,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 AND average_wind_angle_deg IS NOT NULL
                           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) > 60
                           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) < 120
                           THEN total_distance_nm ELSE 0 END) AS dist_reaching,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 AND average_wind_angle_deg IS NOT NULL
                           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) >= 120
                           THEN total_distance_nm ELSE 0 END) AS dist_running,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 AND average_wind_angle_deg IS NOT NULL
                           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) <= 60
                           THEN total_time_ms ELSE 0 END) AS time_upwind,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 AND average_wind_angle_deg IS NOT NULL
                           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) > 60
                           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) < 120
                           THEN total_time_ms ELSE 0 END) AS time_reaching,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 AND average_wind_angle_deg IS NOT NULL
                           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) >= 120
                           THEN total_time_ms ELSE 0 END) AS time_running,
                  MAX(timestamp) AS last_ts
              FROM vessel_status
              WHERE timestamp BETWEEN :start AND :end",
            params! { "start" => &start_str, "end" => &end_str },
        )?;

        if let Some(row) = row {
            let time_moored: u64 = row.get("time_moored").unwrap_or(0);
            let time_motoring: u64 = row.get("time_motoring").unwrap_or(0);
            let time_sailing: u64 = row.get("time_sailing").unwrap_or(0);
            let dist_motoring: f64 = row.get("dist_motoring").unwrap_or(0.0);
            let dist_sailed: f64 = row.get("dist_sailed").unwrap_or(0.0);
            let dist_upwind: f64 = row.get("dist_upwind").unwrap_or(0.0);
            let dist_reaching: f64 = row.get("dist_reaching").unwrap_or(0.0);
            let dist_running: f64 = row.get("dist_running").unwrap_or(0.0);
            let time_upwind: u64 = row.get("time_upwind").unwrap_or(0);
            let time_reaching: u64 = row.get("time_reaching").unwrap_or(0);
            let time_running: u64 = row.get("time_running").unwrap_or(0);
```

(leave the `last_ts_val`/`last_ts_str`/`end_ts_str` block exactly as-is), then extend the `UPDATE`:

```rust
            tx.exec_drop(
                r"UPDATE trips
                  SET total_time_moored      = :time_moored,
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
                      end_timestamp          = :end_ts
                  WHERE id = :trip_id",
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

Run: `cargo test --ignored test_recalculate_and_update_trip_computes_point_of_sail -- --test-threads=1`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/db/operations/gap_fill.rs
git commit -m "feat: compute point-of-sail totals in recalculate_and_update_trip"
```

---

### Task 5: Extend `fix_mooring_status` correction path

**Files:**
- Modify: `src/db/operations/mooring_fix.rs` (the aggregate `SELECT`/`UPDATE trips` block within `fix_mooring_status`, immediately before `tx.commit()`)

**Interfaces:**
- Consumes: same columns as Task 4.
- Produces: nothing new consumed elsewhere.

- [ ] **Step 1: Write the failing test**

Add to `mooring_fix.rs`'s existing test module, following the same setup pattern as its existing `fix_mooring_status` tests (a trip, several `vessel_status` rows, a call to `db.fix_mooring_status(...)`, then assertions on the resulting `trips` row):

```rust
    #[test]
    #[ignore]
    fn test_fix_mooring_status_recomputes_point_of_sail() {
        let db = setup_db();
        let start = SystemTime::now();
        let trip_id = add_test_trip(
            &db, "POS Fix".to_string(), start,
            start + Duration::from_secs(300), 0.0, 0.0, 0, 0, 0,
        ).unwrap();

        // Sailing, upwind: 2.0 nm / 60_000 ms
        add_test_vessel_status(
            &db, start, 50.0, -1.0, 6.0, 6.0,
            Some(12.0), Some(20.0), false, EngineStatus::Off, 2.0, 60_000, None, None,
        ).unwrap();
        // Sailing, running: 1.0 nm / 30_000 ms
        add_test_vessel_status(
            &db, start + Duration::from_secs(60), 50.01, -1.0, 6.0, 6.0,
            Some(12.0), Some(150.0), false, EngineStatus::Off, 1.0, 30_000, None, None,
        ).unwrap();
        // Mislabeled as underway but actually stationary — will be flipped to moored
        add_test_vessel_status(
            &db, start + Duration::from_secs(120), 50.011, -1.0, 0.1, 0.2,
            Some(12.0), Some(20.0), false, EngineStatus::Off, 0.01, 30_000, None, None,
        ).unwrap();

        db.fix_mooring_status(
            start + Duration::from_secs(120),
            start + Duration::from_secs(150),
            true,
        ).unwrap();

        let mut conn = db.pool.get_conn().unwrap();
        let row: (f64, f64, u64, u64) = conn
            .exec_first(
                "SELECT total_distance_upwind, total_distance_running,
                        total_time_upwind, total_time_running
                 FROM trips WHERE id = :id",
                mysql::params! { "id" => trip_id },
            )
            .unwrap()
            .unwrap();
        assert_approx_equal(row.0, 2.0, 0.001, "total_distance_upwind unaffected by the fix");
        assert_approx_equal(row.1, 1.0, 0.001, "total_distance_running unaffected by the fix");
        assert_eq!(row.2, 60_000, "total_time_upwind unaffected by the fix");
        assert_eq!(row.3, 30_000, "total_time_running unaffected by the fix");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --ignored test_fix_mooring_status_recomputes_point_of_sail -- --test-threads=1`
Expected: FAIL — `total_distance_upwind`/`total_distance_running` are `0.0` after the fix because the recompute block doesn't populate them yet.

- [ ] **Step 3: Implement**

In `src/db/operations/mooring_fix.rs`, extend the aggregate `SELECT` and `UPDATE trips` block exactly as in Task 4 (same `LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg)` CASE branches, same 6 new column names):

```rust
        let agg_row: Option<mysql::Row> = tx.exec_first(
            r"SELECT
                  SUM(CASE WHEN is_moored = 1 THEN total_time_ms  ELSE 0 END) AS time_moored,
                  SUM(CASE WHEN is_moored = 0 AND engine_on = 1 THEN total_time_ms  ELSE 0 END) AS time_motoring,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 THEN total_time_ms ELSE 0 END) AS time_sailing,
                  SUM(CASE WHEN is_moored = 0 AND engine_on = 1 THEN total_distance_nm ELSE 0 END) AS dist_motoring,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 THEN total_distance_nm ELSE 0 END) AS dist_sailed,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 AND average_wind_angle_deg IS NOT NULL
                           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) <= 60
                           THEN total_distance_nm ELSE 0 END) AS dist_upwind,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 AND average_wind_angle_deg IS NOT NULL
                           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) > 60
                           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) < 120
                           THEN total_distance_nm ELSE 0 END) AS dist_reaching,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 AND average_wind_angle_deg IS NOT NULL
                           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) >= 120
                           THEN total_distance_nm ELSE 0 END) AS dist_running,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 AND average_wind_angle_deg IS NOT NULL
                           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) <= 60
                           THEN total_time_ms ELSE 0 END) AS time_upwind,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 AND average_wind_angle_deg IS NOT NULL
                           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) > 60
                           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) < 120
                           THEN total_time_ms ELSE 0 END) AS time_reaching,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 AND average_wind_angle_deg IS NOT NULL
                           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) >= 120
                           THEN total_time_ms ELSE 0 END) AS time_running
              FROM vessel_status
              WHERE timestamp BETWEEN :start AND :end",
            params! { "start" => &trip_start_str, "end" => &trip_end_str },
        )?;

        if let Some(row) = agg_row {
            let time_moored: u64 = row.get("time_moored").unwrap_or(0);
            let time_motoring: u64 = row.get("time_motoring").unwrap_or(0);
            let time_sailing: u64 = row.get("time_sailing").unwrap_or(0);
            let dist_motoring: f64 = row.get("dist_motoring").unwrap_or(0.0);
            let dist_sailed: f64 = row.get("dist_sailed").unwrap_or(0.0);
            let dist_upwind: f64 = row.get("dist_upwind").unwrap_or(0.0);
            let dist_reaching: f64 = row.get("dist_reaching").unwrap_or(0.0);
            let dist_running: f64 = row.get("dist_running").unwrap_or(0.0);
            let time_upwind: u64 = row.get("time_upwind").unwrap_or(0);
            let time_reaching: u64 = row.get("time_reaching").unwrap_or(0);
            let time_running: u64 = row.get("time_running").unwrap_or(0);

            tx.exec_drop(
                r"UPDATE trips
                  SET total_time_moored       = :time_moored,
                      total_time_motoring     = :time_motoring,
                      total_time_sailing      = :time_sailing,
                      total_distance_motoring = :dist_motoring,
                      total_distance_sailed   = :dist_sailed,
                      total_distance_upwind   = :dist_upwind,
                      total_distance_reaching = :dist_reaching,
                      total_distance_running  = :dist_running,
                      total_time_upwind       = :time_upwind,
                      total_time_reaching     = :time_reaching,
                      total_time_running      = :time_running
                  WHERE id = :trip_id",
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
        }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --ignored test_fix_mooring_status_recomputes_point_of_sail -- --test-threads=1`
Expected: PASS. Also run `cargo test --ignored -- --test-threads=1` to confirm no regression in the rest of `mooring_fix.rs`'s suite.

- [ ] **Step 5: Commit**

```bash
git add src/db/operations/mooring_fix.rs
git commit -m "feat: recompute point-of-sail totals in fix_mooring_status"
```

---

### Task 6: Expose point-of-sail on `TripSummary` (trip-level API)

**Files:**
- Modify: `src/db/types.rs:53-67` (`TripSummary` struct)
- Modify: `src/db/operations/query.rs` (`fetch_trip`, `fetch_trips`, `fetch_trip_by_uuid`)

**Interfaces:**
- Consumes: `trips.total_distance_upwind/reaching/running`, `total_time_upwind/reaching/running` (Task 3/4/5 populate them).
- Produces: `TripSummary` gains `upwind_distance_nm: f64`, `reaching_distance_nm: f64`, `running_distance_nm: f64`, `upwind_time_ms: i64`, `reaching_time_ms: i64`, `running_time_ms: i64`. Serialized automatically to `GET /api/trip`, `/api/trips`, `/api/trip_by_uuid`, and the MCP `get_trip`/`get_trip_by_uuid` tools (no code change needed there — see Global Constraints).

- [ ] **Step 1: Write the failing test**

Add to `src/db/operations/query.rs`'s test module (find its existing `fetch_trip`-adjacent tests to match import/setup style):

```rust
    #[test]
    #[ignore]
    fn test_fetch_trip_includes_point_of_sail() {
        let db = setup_db();
        let start = SystemTime::now();
        let trip_id = add_test_trip(
            &db, "POS API".to_string(), start,
            start + Duration::from_secs(300), 5.0, 0.0, 300_000, 0, 0,
        ).unwrap();
        let mut conn = db.pool.get_conn().unwrap();
        conn.exec_drop(
            "UPDATE trips SET total_distance_upwind = 2.0, total_distance_reaching = 2.0,
                    total_distance_running = 1.0, total_time_upwind = 120000,
                    total_time_reaching = 120000, total_time_running = 60000
             WHERE id = :id",
            mysql::params! { "id" => trip_id },
        ).unwrap();

        let trip = db.fetch_trip(trip_id).unwrap().expect("trip should exist");
        assert_approx_equal(trip.upwind_distance_nm, 2.0, 0.001, "upwind_distance_nm");
        assert_approx_equal(trip.reaching_distance_nm, 2.0, 0.001, "reaching_distance_nm");
        assert_approx_equal(trip.running_distance_nm, 1.0, 0.001, "running_distance_nm");
        assert_eq!(trip.upwind_time_ms, 120_000);
        assert_eq!(trip.reaching_time_ms, 120_000);
        assert_eq!(trip.running_time_ms, 60_000);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --ignored test_fetch_trip_includes_point_of_sail -- --test-threads=1`
Expected: FAIL to compile — `TripSummary` has no field `upwind_distance_nm`.

- [ ] **Step 3: Implement**

In `src/db/types.rs`, extend `TripSummary`:

```rust
#[derive(Debug, serde::Serialize)]
pub struct TripSummary {
    pub id: u32,
    pub uuid: Option<String>,
    pub description: String,
    pub start_date: String,
    pub end_date: String,
    pub total_distance_nm: f64,
    pub total_time_ms: i64,
    pub sailing_time_ms: i64,
    pub motoring_time_ms: i64,
    pub moored_time_ms: i64,
    pub sailing_distance_nm: f64,
    pub motoring_distance_nm: f64,
    pub upwind_distance_nm: f64,
    pub reaching_distance_nm: f64,
    pub running_distance_nm: f64,
    pub upwind_time_ms: i64,
    pub reaching_time_ms: i64,
    pub running_time_ms: i64,
}
```

In `src/db/operations/query.rs`, add the 6 columns to each of the three `SELECT`s and to each `TripSummary { ... }` construction. `fetch_trip`:

```rust
        let row: Option<mysql::Row> = conn.exec_first(
            r"SELECT id, description,
                     DATE_FORMAT(start_timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as start_ts,
                     DATE_FORMAT(end_timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as end_ts,
                     total_distance_sailed, total_distance_motoring,
                     (total_distance_sailed + total_distance_motoring) as total_distance,
                     (total_time_sailing + total_time_motoring + total_time_moored) as total_time,
                     total_time_sailing, total_time_motoring, total_time_moored, uuid,
                     total_distance_upwind, total_distance_reaching, total_distance_running,
                     total_time_upwind, total_time_reaching, total_time_running
              FROM trips
              WHERE id = :trip_id",
            mysql::params! {
                "trip_id" => trip_id,
            },
        )?;

        if let Some(row) = row {
            let trip = TripSummary {
                id: get_or_log(&row, "id", 0u32, "fetch_trip"),
                uuid: row
                    .get_opt::<Option<String>, _>("uuid")
                    .and_then(|v| v.ok())
                    .flatten(),
                description: get_or_log(&row, "description", String::new(), "fetch_trip"),
                start_date: get_or_log(&row, "start_ts", String::new(), "fetch_trip"),
                end_date: get_or_log(&row, "end_ts", String::new(), "fetch_trip"),
                total_distance_nm: get_or_log(&row, "total_distance", 0.0f64, "fetch_trip"),
                total_time_ms: get_or_log(&row, "total_time", 0i64, "fetch_trip"),
                sailing_time_ms: get_or_log(&row, "total_time_sailing", 0i64, "fetch_trip"),
                motoring_time_ms: get_or_log(&row, "total_time_motoring", 0i64, "fetch_trip"),
                moored_time_ms: get_or_log(&row, "total_time_moored", 0i64, "fetch_trip"),
                sailing_distance_nm: get_or_log(
                    &row,
                    "total_distance_sailed",
                    0.0f64,
                    "fetch_trip",
                ),
                motoring_distance_nm: get_or_log(
                    &row,
                    "total_distance_motoring",
                    0.0f64,
                    "fetch_trip",
                ),
                upwind_distance_nm: get_or_log(&row, "total_distance_upwind", 0.0f64, "fetch_trip"),
                reaching_distance_nm: get_or_log(&row, "total_distance_reaching", 0.0f64, "fetch_trip"),
                running_distance_nm: get_or_log(&row, "total_distance_running", 0.0f64, "fetch_trip"),
                upwind_time_ms: get_or_log(&row, "total_time_upwind", 0i64, "fetch_trip"),
                reaching_time_ms: get_or_log(&row, "total_time_reaching", 0i64, "fetch_trip"),
                running_time_ms: get_or_log(&row, "total_time_running", 0i64, "fetch_trip"),
            };
            log_timing("fetch_trip", "total", t0, Some(1));
            Ok(Some(trip))
        } else {
            log_timing("fetch_trip", "total", t0, Some(0));
            Ok(None)
        }
    }
```

`fetch_trips`'s `SELECT_TRIPS` constant currently reads:

```rust
        const SELECT_TRIPS: &str = "SELECT id,
                    description,
                    DATE_FORMAT(start_timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as start_ts,
                    DATE_FORMAT(end_timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as end_ts,
                    (total_distance_sailed + total_distance_motoring) as total_distance,
                    (total_time_sailing + total_time_motoring + total_time_moored) as total_time,
                    total_time_sailing as total_time_sailing,
                    total_time_motoring as total_time_motoring,
                    total_time_moored as total_time_moored,
                    total_distance_sailed as total_distance_sailed,
                    total_distance_motoring as total_distance_motoring,
                    uuid
             FROM trips WHERE ";
```

Change to:

```rust
        const SELECT_TRIPS: &str = "SELECT id,
                    description,
                    DATE_FORMAT(start_timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as start_ts,
                    DATE_FORMAT(end_timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as end_ts,
                    (total_distance_sailed + total_distance_motoring) as total_distance,
                    (total_time_sailing + total_time_motoring + total_time_moored) as total_time,
                    total_time_sailing as total_time_sailing,
                    total_time_motoring as total_time_motoring,
                    total_time_moored as total_time_moored,
                    total_distance_sailed as total_distance_sailed,
                    total_distance_motoring as total_distance_motoring,
                    total_distance_upwind, total_distance_reaching, total_distance_running,
                    total_time_upwind, total_time_reaching, total_time_running,
                    uuid
             FROM trips WHERE ";
```

and its `TripSummary { ... }` map closure gains the same 6 fields, same as `fetch_trip`'s construction but with context string `"fetch_trips"` and reading from `row` (not `&row`, matching this closure's existing `|row| TripSummary { ... }` signature):

```rust
                upwind_distance_nm: get_or_log(row, "total_distance_upwind", 0.0f64, "fetch_trips"),
                reaching_distance_nm: get_or_log(row, "total_distance_reaching", 0.0f64, "fetch_trips"),
                running_distance_nm: get_or_log(row, "total_distance_running", 0.0f64, "fetch_trips"),
                upwind_time_ms: get_or_log(row, "total_time_upwind", 0i64, "fetch_trips"),
                reaching_time_ms: get_or_log(row, "total_time_reaching", 0i64, "fetch_trips"),
                running_time_ms: get_or_log(row, "total_time_running", 0i64, "fetch_trips"),
```

`fetch_trip_by_uuid` is byte-for-byte the same shape as `fetch_trip` (same `SELECT` column list, same construction), differing only in its `WHERE uuid = :uuid` clause and its `"fetch_trip_by_uuid"` context strings. Apply the exact same `SELECT` addition and the exact same 6 struct-field additions shown for `fetch_trip` above, substituting the context string `"fetch_trip_by_uuid"` for `"fetch_trip"` in every `get_or_log(...)` call.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --ignored test_fetch_trip_includes_point_of_sail -- --test-threads=1`
Expected: PASS. Also run: `cargo build && cargo test --ignored -- --test-threads=1` to confirm `fetch_trips`/`fetch_trip_by_uuid` and every other `TripSummary` construction site still compiles (this struct has `#[derive(Debug, serde::Serialize)]` with no `Default`, so every construction site must be updated or the build fails — this is the mechanism that guarantees the three functions are fully in sync).

- [ ] **Step 5: Commit**

```bash
git add src/db/types.rs src/db/operations/query.rs
git commit -m "feat: expose point-of-sail totals on TripSummary"
```

---

### Task 7: Leg-level point-of-sail (`trip_legs_cache`, `TripLeg`)

**Files:**
- Modify: `schema.sql` (`trip_legs_cache` table)
- Modify: `src/db/types.rs` (`TripLeg` struct)
- Modify: `src/db/operations/query.rs` (`LegRecord`, `compute_trip_legs`, `finalize_leg`, `get_cached_trip_legs`, `save_trip_legs_to_cache`)

**Interfaces:**
- Consumes: `PointOfSail`/`point_of_sail_from_twa` (Task 1).
- Produces: `TripLeg` gains `upwind_distance_nm: f64`, `reaching_distance_nm: f64`, `running_distance_nm: f64`, `upwind_time_ms: u64`, `reaching_time_ms: u64`, `running_time_ms: u64`. Serialized automatically via `GET /api/trip_legs` and the MCP `get_trip_legs` tool. Consumed by Task 10 (frontend).

- [ ] **Step 1: Write the failing test**

Add to `query.rs`'s test module, alongside `finalize_leg_populates_speed_records` (reuse its `synthetic_leg_constant_speed`-style helper if present, or build `LegRecord`s directly — `LegRecord` is a private struct in this same module, so the test can construct it directly):

```rust
    #[test]
    fn finalize_leg_buckets_point_of_sail() {
        let records = vec![
            LegRecord {
                timestamp: "2026-01-01T00:00:00.000Z".to_string(),
                speed_kn: 6.0,
                distance_nm: 1.0,
                time_ms: 60_000,
                engine_on: false,
                lat: Some(50.0),
                lon: Some(-1.0),
                wind_angle_deg: Some(30.0), // upwind
            },
            LegRecord {
                timestamp: "2026-01-01T00:01:00.000Z".to_string(),
                speed_kn: 6.0,
                distance_nm: 1.0,
                time_ms: 60_000,
                engine_on: false,
                lat: Some(50.01),
                lon: Some(-1.0),
                wind_angle_deg: Some(90.0), // reaching
            },
            LegRecord {
                timestamp: "2026-01-01T00:02:00.000Z".to_string(),
                speed_kn: 6.0,
                distance_nm: 1.0,
                time_ms: 60_000,
                engine_on: false,
                lat: Some(50.02),
                lon: Some(-1.0),
                wind_angle_deg: Some(150.0), // running
            },
            LegRecord {
                timestamp: "2026-01-01T00:03:00.000Z".to_string(),
                speed_kn: 6.0,
                distance_nm: 1.0,
                time_ms: 60_000,
                engine_on: true, // motoring — must not count toward any bucket
                lat: Some(50.03),
                lon: Some(-1.0),
                wind_angle_deg: Some(30.0),
            },
        ];

        let leg = finalize_leg(&records, 1, records[0].lat, records[0].lon)
            .expect("leg should finalize — total distance exceeds the 0.5nm minimum");

        assert_approx_equal_f64(leg.upwind_distance_nm, 1.0);
        assert_approx_equal_f64(leg.reaching_distance_nm, 1.0);
        assert_approx_equal_f64(leg.running_distance_nm, 1.0);
        assert_eq!(leg.upwind_time_ms, 60_000);
        assert_eq!(leg.reaching_time_ms, 60_000);
        assert_eq!(leg.running_time_ms, 60_000);
    }
```

(If the file's existing tests use a different float-assertion helper than `assert_approx_equal_f64`, use whichever helper the surrounding tests in this exact module already import — check the top of the `#[cfg(test)] mod tests` block in `query.rs` and match it; do not introduce a second helper.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib finalize_leg_buckets_point_of_sail`
Expected: FAIL to compile — `LegRecord` has no field `wind_angle_deg`; `TripLeg` has no field `upwind_distance_nm`.

- [ ] **Step 3: Implement**

In `src/db/operations/query.rs`, add the field to `LegRecord`:

```rust
struct LegRecord {
    timestamp: String,
    speed_kn: f64,
    distance_nm: f64,
    time_ms: u64,
    engine_on: bool,
    lat: Option<f64>,
    lon: Option<f64>,
    wind_angle_deg: Option<f64>,
}
```

In `compute_trip_legs`, select the wind angle and populate it when pushing a record:

```rust
        let results: Vec<mysql::Row> = conn.exec(
            r"SELECT
                DATE_FORMAT(timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as timestamp,
                latitude,
                longitude,
                is_moored,
                engine_on,
                total_distance_nm,
                total_time_ms,
                average_speed_kn,
                average_wind_angle_deg
             FROM vessel_status
             WHERE timestamp BETWEEN
                 (SELECT start_timestamp FROM trips WHERE id = :trip_id)
                 AND COALESCE((SELECT end_timestamp FROM trips WHERE id = :trip_id), NOW())
             ORDER BY timestamp",
            mysql::params! { "trip_id" => trip_id },
        )?;
```

```rust
            let wind_angle_deg: Option<f64> = row
                .get_opt::<f64, _>("average_wind_angle_deg")
                .and_then(|v| v.ok());
```

(add this alongside the existing `speed_kn` extraction, before the `if is_moored { ... }` branch), then add it to the `current_leg.push(LegRecord { ... })` call:

```rust
                current_leg.push(LegRecord {
                    timestamp,
                    speed_kn,
                    distance_nm: interval_distance,
                    time_ms: interval_time,
                    engine_on,
                    lat: last_lat,
                    lon: last_lon,
                    wind_angle_deg,
                });
```

In `finalize_leg`, extend the sailing/motoring accumulation loop:

```rust
    let mut sailing_distance = 0.0_f64;
    let mut motoring_distance = 0.0_f64;
    let mut sailing_time = 0_u64;
    let mut motoring_time = 0_u64;
    let mut upwind_distance = 0.0_f64;
    let mut reaching_distance = 0.0_f64;
    let mut running_distance = 0.0_f64;
    let mut upwind_time = 0_u64;
    let mut reaching_time = 0_u64;
    let mut running_time = 0_u64;
    for r in records {
        if r.engine_on {
            motoring_distance += r.distance_nm;
            motoring_time += r.time_ms;
        } else {
            sailing_distance += r.distance_nm;
            sailing_time += r.time_ms;

            if let Some(angle) = r.wind_angle_deg {
                use crate::utilities::{point_of_sail_from_twa, PointOfSail};
                match point_of_sail_from_twa(angle) {
                    PointOfSail::Upwind => {
                        upwind_distance += r.distance_nm;
                        upwind_time += r.time_ms;
                    }
                    PointOfSail::Reaching => {
                        reaching_distance += r.distance_nm;
                        reaching_time += r.time_ms;
                    }
                    PointOfSail::Running => {
                        running_distance += r.distance_nm;
                        running_time += r.time_ms;
                    }
                }
            }
        }
    }
```

and the `Some(TripLeg { ... })` construction at the end of `finalize_leg`:

```rust
    Some(TripLeg {
        leg_number,
        start_timestamp,
        end_timestamp,
        total_distance_nm: total_distance,
        sailing_distance_nm: sailing_distance,
        motoring_distance_nm: motoring_distance,
        sailing_time_ms: sailing_time,
        motoring_time_ms: motoring_time,
        sailing_time_formatted: format_duration_ms(sailing_time),
        motoring_time_formatted: format_duration_ms(motoring_time),
        upwind_distance_nm: upwind_distance,
        reaching_distance_nm: reaching_distance,
        running_distance_nm: running_distance,
        upwind_time_ms: upwind_time,
        reaching_time_ms: reaching_time,
        running_time_ms: running_time,
        start_lat,
        start_lon,
        end_lat,
        end_lon,
        nav_start_timestamp,
        nav_end_timestamp,
        nav_distance_nm,
        nav_time_ms,
        nav_detection_method,
        max_speed_kn,
        max_speed_timestamp,
        fastest_1nm,
        fastest_5nm,
        fastest_10nm,
        fastest_25nm,
    })
```

In `src/db/types.rs`, add the 6 fields to `TripLeg` (placed after `motoring_time_formatted`, matching the construction order above):

```rust
    pub sailing_time_formatted: String,
    pub motoring_time_formatted: String,
    pub upwind_distance_nm: f64,
    pub reaching_distance_nm: f64,
    pub running_distance_nm: f64,
    pub upwind_time_ms: u64,
    pub reaching_time_ms: u64,
    pub running_time_ms: u64,
    pub start_lat: Option<f64>,
```

In `schema.sql`, add the 6 columns to the `trip_legs_cache` `CREATE TABLE` (after `motoring_time_ms`):

```sql
    sailing_time_ms      BIGINT UNSIGNED NOT NULL DEFAULT 0,
    motoring_time_ms     BIGINT UNSIGNED NOT NULL DEFAULT 0,
    upwind_distance_nm   DOUBLE          NOT NULL DEFAULT 0,
    reaching_distance_nm DOUBLE          NOT NULL DEFAULT 0,
    running_distance_nm  DOUBLE          NOT NULL DEFAULT 0,
    upwind_time_ms       BIGINT UNSIGNED NOT NULL DEFAULT 0,
    reaching_time_ms     BIGINT UNSIGNED NOT NULL DEFAULT 0,
    running_time_ms      BIGINT UNSIGNED NOT NULL DEFAULT 0,
```

In `src/db/operations/query.rs`, `get_cached_trip_legs`: add 6 entries to the best-effort `ALTER TABLE trip_legs_cache ADD COLUMN ...` list:

```rust
            "ALTER TABLE trip_legs_cache ADD COLUMN upwind_distance_nm DOUBLE NOT NULL DEFAULT 0",
            "ALTER TABLE trip_legs_cache ADD COLUMN reaching_distance_nm DOUBLE NOT NULL DEFAULT 0",
            "ALTER TABLE trip_legs_cache ADD COLUMN running_distance_nm DOUBLE NOT NULL DEFAULT 0",
            "ALTER TABLE trip_legs_cache ADD COLUMN upwind_time_ms BIGINT UNSIGNED NOT NULL DEFAULT 0",
            "ALTER TABLE trip_legs_cache ADD COLUMN reaching_time_ms BIGINT UNSIGNED NOT NULL DEFAULT 0",
            "ALTER TABLE trip_legs_cache ADD COLUMN running_time_ms BIGINT UNSIGNED NOT NULL DEFAULT 0",
```

add the 6 columns to its `SELECT`:

```rust
                         sailing_time_ms, motoring_time_ms,
                         upwind_distance_nm, reaching_distance_nm, running_distance_nm,
                         upwind_time_ms, reaching_time_ms, running_time_ms,
                         start_lat, start_lon, end_lat, end_lon,
```

and to the `TripLeg { ... }` construction in its row-mapping closure (right after the existing `sailing_time_formatted`/`motoring_time_formatted` lines):

```rust
                    sailing_time_formatted: format_duration_ms(sailing_time_ms),
                    motoring_time_formatted: format_duration_ms(motoring_time_ms),
                    upwind_distance_nm: get_or_log(row, "upwind_distance_nm", 0.0f64, "get_cached_trip_legs"),
                    reaching_distance_nm: get_or_log(row, "reaching_distance_nm", 0.0f64, "get_cached_trip_legs"),
                    running_distance_nm: get_or_log(row, "running_distance_nm", 0.0f64, "get_cached_trip_legs"),
                    upwind_time_ms: get_or_log(row, "upwind_time_ms", 0u64, "get_cached_trip_legs"),
                    reaching_time_ms: get_or_log(row, "reaching_time_ms", 0u64, "get_cached_trip_legs"),
                    running_time_ms: get_or_log(row, "running_time_ms", 0u64, "get_cached_trip_legs"),
```

In `save_trip_legs_to_cache`, add the 6 columns (in the same position as the schema: right after `motoring_time_ms`, before `start_lat`) to the `INSERT IGNORE` column list:

```rust
                 total_distance_nm, sailing_distance_nm, motoring_distance_nm,
                 sailing_time_ms, motoring_time_ms,
                 upwind_distance_nm, reaching_distance_nm, running_distance_nm,
                 upwind_time_ms, reaching_time_ms, running_time_ms,
                 start_lat, start_lon, end_lat, end_lon,
```

add 6 more `?` placeholders to the `VALUES (...)` list (it currently reads `(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)` — 40 placeholders — becomes 46), and add the 6 corresponding values to the `Vec<mysql::Value>` builder in the same position as the column list, right after `leg.motoring_time_ms.into()`:

```rust
            legs.iter().map(|leg| -> Vec<mysql::Value> {
                let mut values: Vec<mysql::Value> = vec![
                    trip_id.into(),
                    leg.leg_number.into(),
                    leg.start_timestamp.as_str().into(),
                    leg.end_timestamp.as_str().into(),
                    leg.total_distance_nm.into(),
                    leg.sailing_distance_nm.into(),
                    leg.motoring_distance_nm.into(),
                    leg.sailing_time_ms.into(),
                    leg.motoring_time_ms.into(),
                    leg.upwind_distance_nm.into(),
                    leg.reaching_distance_nm.into(),
                    leg.running_distance_nm.into(),
                    leg.upwind_time_ms.into(),
                    leg.reaching_time_ms.into(),
                    leg.running_time_ms.into(),
                    leg.start_lat.into(),
                    leg.start_lon.into(),
                    leg.end_lat.into(),
                    leg.end_lon.into(),
                    leg.nav_start_timestamp.as_deref().into(),
                    leg.nav_end_timestamp.as_deref().into(),
                    leg.nav_distance_nm.into(),
                    leg.nav_time_ms.into(),
                    leg.nav_detection_method.as_deref().into(),
                    leg.max_speed_kn.into(),
                    leg.max_speed_timestamp.as_deref().into(),
                ];
```

(leave the rest of the closure — the `for segment in [&leg.fastest_1nm, ...]` loop and everything after — exactly as it is today; only the `values` initializer above gains the 6 new entries).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib finalize_leg_buckets_point_of_sail`
Expected: PASS. Then run: `cargo build` to confirm `TripLeg`'s other construction sites (only `finalize_leg` and `get_cached_trip_legs`'s row-mapping construct it — both updated above) compile.

- [ ] **Step 5: DB-backed cache round-trip test**

Add to `query.rs`'s `#[ignore]` test group, mirroring `test_trip_legs_cache_round_trips_speed_records`:

```rust
    #[test]
    #[ignore]
    fn test_trip_legs_cache_round_trips_point_of_sail() {
        let db = setup_db();
        let leg = TripLeg {
            leg_number: 1,
            start_timestamp: "2026-01-01T00:00:00.000Z".to_string(),
            end_timestamp: "2026-01-01T01:00:00.000Z".to_string(),
            total_distance_nm: 3.0,
            sailing_distance_nm: 3.0,
            motoring_distance_nm: 0.0,
            sailing_time_ms: 180_000,
            motoring_time_ms: 0,
            sailing_time_formatted: "3m".to_string(),
            motoring_time_formatted: "0s".to_string(),
            upwind_distance_nm: 1.0,
            reaching_distance_nm: 1.0,
            running_distance_nm: 1.0,
            upwind_time_ms: 60_000,
            reaching_time_ms: 60_000,
            running_time_ms: 60_000,
            start_lat: Some(50.0),
            start_lon: Some(-1.0),
            end_lat: Some(50.1),
            end_lon: Some(-1.1),
            nav_start_timestamp: None,
            nav_end_timestamp: None,
            nav_distance_nm: 0.0,
            nav_time_ms: 0,
            nav_detection_method: None,
            max_speed_kn: None,
            max_speed_timestamp: None,
            fastest_1nm: None,
            fastest_5nm: None,
            fastest_10nm: None,
            fastest_25nm: None,
        };
        db.save_trip_legs_to_cache_for_test(1, &[leg]).unwrap();
        let cached = db.get_cached_trip_legs_for_test(1).unwrap().expect("cache row should exist");
        let cached_leg = &cached.legs[0];
        assert_approx_equal(cached_leg.upwind_distance_nm, 1.0, 0.001, "upwind_distance_nm");
        assert_approx_equal(cached_leg.reaching_distance_nm, 1.0, 0.001, "reaching_distance_nm");
        assert_approx_equal(cached_leg.running_distance_nm, 1.0, 0.001, "running_distance_nm");
        assert_eq!(cached_leg.upwind_time_ms, 60_000);
        assert_eq!(cached_leg.reaching_time_ms, 60_000);
        assert_eq!(cached_leg.running_time_ms, 60_000);
    }
```

Run: `cargo test --ignored test_trip_legs_cache_round_trips_point_of_sail -- --test-threads=1`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add schema.sql src/db/types.rs src/db/operations/query.rs
git commit -m "feat: add leg-level point-of-sail breakdown to trip_legs_cache"
```

---

### Task 8: Day-level point-of-sail in `heatmap_cache`

**Files:**
- Modify: `schema.sql` (`heatmap_cache` table)
- Modify: `src/db/operations/query.rs` (`fetch_heatmap`)

**Interfaces:**
- Consumes: `point_of_sail_from_twa` is not used here — SQL-level `LEAST()` folding is used directly (same reasoning as Task 4/5: SQL aggregation, not row-by-row Rust logic).
- Produces: `heatmap_cache` columns `upwind_distance_nm`, `reaching_distance_nm`, `running_distance_nm`, `upwind_time_ms`, `reaching_time_ms`, `running_time_ms`. **Not** added to the public `HeatmapDay`/`HeatmapData` structs (out of scope — the heatmap widget itself shows daily totals only; these columns exist purely to feed Task 9's monthly rollup). Consumed by Task 9.

- [ ] **Step 1: Write the failing test**

Add to `query.rs`'s `#[ignore]` test group:

```rust
    #[test]
    #[ignore]
    fn test_fetch_heatmap_populates_point_of_sail_cache() {
        let db = setup_db();
        let day = chrono::Utc::now().date_naive() - chrono::Duration::days(5);
        let day_start = day.and_hms_opt(10, 0, 0).unwrap();
        let ts = SystemTime::UNIX_EPOCH
            + Duration::from_secs(day_start.and_utc().timestamp() as u64);

        // Sailing, upwind: 2.0 nm / 60_000 ms
        add_test_vessel_status(
            &db, ts, 50.0, -1.0, 6.0, 6.0,
            Some(12.0), Some(20.0), false, EngineStatus::Off, 2.0, 60_000, None, None,
        ).unwrap();
        // Sailing, reaching: 1.0 nm / 30_000 ms
        add_test_vessel_status(
            &db, ts + Duration::from_secs(60), 50.01, -1.0, 6.0, 6.0,
            Some(12.0), Some(90.0), false, EngineStatus::Off, 1.0, 30_000, None, None,
        ).unwrap();

        db.fetch_heatmap(chrono::Utc::now().date_naive()).unwrap();

        let mut conn = db.pool.get_conn().unwrap();
        let row: (f64, f64, u64, u64) = conn
            .exec_first(
                "SELECT upwind_distance_nm, reaching_distance_nm, upwind_time_ms, reaching_time_ms
                 FROM heatmap_cache WHERE date = :d",
                mysql::params! { "d" => day.format("%Y-%m-%d").to_string() },
            )
            .unwrap()
            .unwrap();
        assert_approx_equal(row.0, 2.0, 0.001, "upwind_distance_nm");
        assert_approx_equal(row.1, 1.0, 0.001, "reaching_distance_nm");
        assert_eq!(row.2, 60_000, "upwind_time_ms");
        assert_eq!(row.3, 30_000, "reaching_time_ms");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --ignored test_fetch_heatmap_populates_point_of_sail_cache -- --test-threads=1`
Expected: FAIL — `Unknown column 'upwind_distance_nm' in 'field list'`.

- [ ] **Step 3: Implement**

In `schema.sql`, add the 6 columns to `heatmap_cache`:

```sql
CREATE TABLE IF NOT EXISTS heatmap_cache (
    date DATE NOT NULL COMMENT 'UTC date of the aggregated sailing distance',
    distance_nm DOUBLE NOT NULL DEFAULT 0 COMMENT 'Total distance (sailing + motoring) in nautical miles',
    sailing_distance_nm DOUBLE NOT NULL DEFAULT 0 COMMENT 'Distance with engine off (engine_on=0) in nautical miles',
    motoring_distance_nm DOUBLE NOT NULL DEFAULT 0 COMMENT 'Distance with engine on (engine_on=1) in nautical miles',
    upwind_distance_nm DOUBLE NOT NULL DEFAULT 0 COMMENT 'Sailing distance with folded TWA <= 60 deg, in nautical miles',
    reaching_distance_nm DOUBLE NOT NULL DEFAULT 0 COMMENT 'Sailing distance with folded TWA 60-120 deg, in nautical miles',
    running_distance_nm DOUBLE NOT NULL DEFAULT 0 COMMENT 'Sailing distance with folded TWA >= 120 deg, in nautical miles',
    upwind_time_ms BIGINT UNSIGNED NOT NULL DEFAULT 0 COMMENT 'Sailing time with folded TWA <= 60 deg, in milliseconds',
    reaching_time_ms BIGINT UNSIGNED NOT NULL DEFAULT 0 COMMENT 'Sailing time with folded TWA 60-120 deg, in milliseconds',
    running_time_ms BIGINT UNSIGNED NOT NULL DEFAULT 0 COMMENT 'Sailing time with folded TWA >= 120 deg, in milliseconds',
    PRIMARY KEY (date)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='Per-day heatmap distance cache; recomputed only for missing past days and today';

-- For existing databases, run:
-- ALTER TABLE heatmap_cache
--     ADD COLUMN IF NOT EXISTS sailing_distance_nm DOUBLE NOT NULL DEFAULT 0,
--     ADD COLUMN IF NOT EXISTS motoring_distance_nm DOUBLE NOT NULL DEFAULT 0,
--     ADD COLUMN IF NOT EXISTS upwind_distance_nm DOUBLE NOT NULL DEFAULT 0,
--     ADD COLUMN IF NOT EXISTS reaching_distance_nm DOUBLE NOT NULL DEFAULT 0,
--     ADD COLUMN IF NOT EXISTS running_distance_nm DOUBLE NOT NULL DEFAULT 0,
--     ADD COLUMN IF NOT EXISTS upwind_time_ms BIGINT UNSIGNED NOT NULL DEFAULT 0,
--     ADD COLUMN IF NOT EXISTS reaching_time_ms BIGINT UNSIGNED NOT NULL DEFAULT 0,
--     ADD COLUMN IF NOT EXISTS running_time_ms BIGINT UNSIGNED NOT NULL DEFAULT 0;
```

In `src/db/operations/query.rs`, `fetch_heatmap`: extend the runtime `ALTER TABLE heatmap_cache` best-effort migration:

```rust
        let _ = conn.query_drop(
            "ALTER TABLE heatmap_cache \
             ADD COLUMN sailing_distance_nm DOUBLE NOT NULL DEFAULT 0, \
             ADD COLUMN motoring_distance_nm DOUBLE NOT NULL DEFAULT 0, \
             ADD COLUMN upwind_distance_nm DOUBLE NOT NULL DEFAULT 0, \
             ADD COLUMN reaching_distance_nm DOUBLE NOT NULL DEFAULT 0, \
             ADD COLUMN running_distance_nm DOUBLE NOT NULL DEFAULT 0, \
             ADD COLUMN upwind_time_ms BIGINT UNSIGNED NOT NULL DEFAULT 0, \
             ADD COLUMN reaching_time_ms BIGINT UNSIGNED NOT NULL DEFAULT 0, \
             ADD COLUMN running_time_ms BIGINT UNSIGNED NOT NULL DEFAULT 0",
        );
```

Widen `DayEntry` from a 3-tuple to a 9-tuple representing `(total, sail, motor, upwind_dist, reaching_dist, running_dist, upwind_time, reaching_time, running_time)`, and thread the 6 new values through every place `DayEntry` is built or read. Today (Step 1, cached-rows load):

```rust
        // Tuple layout: (total_nm, sailing_nm, motoring_nm)
        type DayEntry = (f64, f64, f64);

        // Step 1: Load already-cached days for [start_dt, cache_end]
        let cached_rows: Vec<mysql::Row> = conn.exec(
            "SELECT DATE_FORMAT(date, '%Y-%m-%d') as day, distance_nm, \
                    sailing_distance_nm, motoring_distance_nm \
             FROM heatmap_cache WHERE date BETWEEN :start AND :end",
            mysql::params! {
                "start" => start_dt.to_string(),
                "end" => cache_end.to_string(),
            },
        )?;

        let mut day_map: std::collections::HashMap<String, DayEntry> =
            std::collections::HashMap::new();
        for row in cached_rows {
            let date: String = row.get_opt("day").and_then(|v| v.ok()).unwrap_or_default();
            let total: f64 = row
                .get_opt("distance_nm")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let sail: f64 = row
                .get_opt("sailing_distance_nm")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let motor: f64 = row
                .get_opt("motoring_distance_nm")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            day_map.insert(date, (total, sail, motor));
        }
```

Change to:

```rust
        // Tuple layout: (total_nm, sailing_nm, motoring_nm, upwind_nm, reaching_nm, running_nm,
        //                upwind_ms, reaching_ms, running_ms)
        type DayEntry = (f64, f64, f64, f64, f64, f64, u64, u64, u64);

        // Step 1: Load already-cached days for [start_dt, cache_end]
        let cached_rows: Vec<mysql::Row> = conn.exec(
            "SELECT DATE_FORMAT(date, '%Y-%m-%d') as day, distance_nm, \
                    sailing_distance_nm, motoring_distance_nm, \
                    upwind_distance_nm, reaching_distance_nm, running_distance_nm, \
                    upwind_time_ms, reaching_time_ms, running_time_ms \
             FROM heatmap_cache WHERE date BETWEEN :start AND :end",
            mysql::params! {
                "start" => start_dt.to_string(),
                "end" => cache_end.to_string(),
            },
        )?;

        let mut day_map: std::collections::HashMap<String, DayEntry> =
            std::collections::HashMap::new();
        for row in cached_rows {
            let date: String = row.get_opt("day").and_then(|v| v.ok()).unwrap_or_default();
            let total: f64 = row
                .get_opt("distance_nm")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let sail: f64 = row
                .get_opt("sailing_distance_nm")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let motor: f64 = row
                .get_opt("motoring_distance_nm")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let upwind: f64 = row
                .get_opt("upwind_distance_nm")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let reaching: f64 = row
                .get_opt("reaching_distance_nm")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let running: f64 = row
                .get_opt("running_distance_nm")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let upwind_ms: u64 = row
                .get_opt("upwind_time_ms")
                .and_then(|v| v.ok())
                .unwrap_or(0);
            let reaching_ms: u64 = row
                .get_opt("reaching_time_ms")
                .and_then(|v| v.ok())
                .unwrap_or(0);
            let running_ms: u64 = row
                .get_opt("running_time_ms")
                .and_then(|v| v.ok())
                .unwrap_or(0);
            day_map.insert(date, (total, sail, motor, upwind, reaching, running, upwind_ms, reaching_ms, running_ms));
        }
```

Step 3 (recompute from first missing date) today reads:

```rust
        if let Some(from_dt) = recompute_from {
            let results: Vec<mysql::Row> = conn.exec(
                "SELECT DATE_FORMAT(timestamp, '%Y-%m-%d') as day, \
                        COALESCE(SUM(COALESCE(total_distance_nm, 0)), 0) as total_distance, \
                        COALESCE(SUM(CASE WHEN engine_on = 0 THEN COALESCE(total_distance_nm, 0) ELSE 0 END), 0) as sailing_distance, \
                        COALESCE(SUM(CASE WHEN engine_on = 1 THEN COALESCE(total_distance_nm, 0) ELSE 0 END), 0) as motoring_distance \
                 FROM vessel_status \
                 WHERE timestamp >= :from_dt AND DATE(timestamp) <= :cache_end AND is_moored = 0 \
                 GROUP BY DATE_FORMAT(timestamp, '%Y-%m-%d')",
                mysql::params! {
                    "from_dt" => from_dt.to_string(),
                    "cache_end" => cache_end.to_string(),
                },
            )?;

            let mut computed: std::collections::HashMap<String, DayEntry> =
                std::collections::HashMap::new();
            for row in results {
                let date: String = row.get_opt("day").and_then(|v| v.ok()).unwrap_or_default();
                let total: f64 = row
                    .get_opt("total_distance")
                    .and_then(|v| v.ok())
                    .unwrap_or(0.0);
                let sail: f64 = row
                    .get_opt("sailing_distance")
                    .and_then(|v| v.ok())
                    .unwrap_or(0.0);
                let motor: f64 = row
                    .get_opt("motoring_distance")
                    .and_then(|v| v.ok())
                    .unwrap_or(0.0);
                computed.insert(date, (total, sail, motor));
            }

            // Batch INSERT IGNORE all dates in [from_dt, cache_end] — including 0-distance days
            // so they won't be considered missing on the next call.
            let mut rows: Vec<(String, f64, f64, f64)> = Vec::new();
            let mut d = from_dt;
            while d <= cache_end {
                let s = d.format("%Y-%m-%d").to_string();
                let (total, sail, motor) = computed.get(&s).copied().unwrap_or((0.0, 0.0, 0.0));
                let total = if total.is_finite() { total } else { 0.0 };
                let sail = if sail.is_finite() { sail } else { 0.0 };
                let motor = if motor.is_finite() { motor } else { 0.0 };
                rows.push((s.clone(), total, sail, motor));
                day_map.entry(s).or_insert((total, sail, motor));
                d += chrono::Duration::days(1);
            }

            if !rows.is_empty() {
                conn.exec_batch(
                    "INSERT IGNORE INTO heatmap_cache \
                     (date, distance_nm, sailing_distance_nm, motoring_distance_nm) \
                     VALUES (?, ?, ?, ?)",
                    rows.iter()
                        .map(|(date, total, sail, motor)| (date.as_str(), *total, *sail, *motor)),
                )?;
            }
        }
```

Change to:

```rust
        if let Some(from_dt) = recompute_from {
            let results: Vec<mysql::Row> = conn.exec(
                "SELECT DATE_FORMAT(timestamp, '%Y-%m-%d') as day, \
                        COALESCE(SUM(COALESCE(total_distance_nm, 0)), 0) as total_distance, \
                        COALESCE(SUM(CASE WHEN engine_on = 0 THEN COALESCE(total_distance_nm, 0) ELSE 0 END), 0) as sailing_distance, \
                        COALESCE(SUM(CASE WHEN engine_on = 1 THEN COALESCE(total_distance_nm, 0) ELSE 0 END), 0) as motoring_distance, \
                        COALESCE(SUM(CASE WHEN engine_on = 0 AND average_wind_angle_deg IS NOT NULL \
                                          AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) <= 60 \
                                     THEN COALESCE(total_distance_nm, 0) ELSE 0 END), 0) as upwind_distance, \
                        COALESCE(SUM(CASE WHEN engine_on = 0 AND average_wind_angle_deg IS NOT NULL \
                                          AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) > 60 \
                                          AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) < 120 \
                                     THEN COALESCE(total_distance_nm, 0) ELSE 0 END), 0) as reaching_distance, \
                        COALESCE(SUM(CASE WHEN engine_on = 0 AND average_wind_angle_deg IS NOT NULL \
                                          AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) >= 120 \
                                     THEN COALESCE(total_distance_nm, 0) ELSE 0 END), 0) as running_distance, \
                        COALESCE(SUM(CASE WHEN engine_on = 0 AND average_wind_angle_deg IS NOT NULL \
                                          AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) <= 60 \
                                     THEN COALESCE(total_time_ms, 0) ELSE 0 END), 0) as upwind_time, \
                        COALESCE(SUM(CASE WHEN engine_on = 0 AND average_wind_angle_deg IS NOT NULL \
                                          AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) > 60 \
                                          AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) < 120 \
                                     THEN COALESCE(total_time_ms, 0) ELSE 0 END), 0) as reaching_time, \
                        COALESCE(SUM(CASE WHEN engine_on = 0 AND average_wind_angle_deg IS NOT NULL \
                                          AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) >= 120 \
                                     THEN COALESCE(total_time_ms, 0) ELSE 0 END), 0) as running_time \
                 FROM vessel_status \
                 WHERE timestamp >= :from_dt AND DATE(timestamp) <= :cache_end AND is_moored = 0 \
                 GROUP BY DATE_FORMAT(timestamp, '%Y-%m-%d')",
                mysql::params! {
                    "from_dt" => from_dt.to_string(),
                    "cache_end" => cache_end.to_string(),
                },
            )?;

            let mut computed: std::collections::HashMap<String, DayEntry> =
                std::collections::HashMap::new();
            for row in results {
                let date: String = row.get_opt("day").and_then(|v| v.ok()).unwrap_or_default();
                let total: f64 = row
                    .get_opt("total_distance")
                    .and_then(|v| v.ok())
                    .unwrap_or(0.0);
                let sail: f64 = row
                    .get_opt("sailing_distance")
                    .and_then(|v| v.ok())
                    .unwrap_or(0.0);
                let motor: f64 = row
                    .get_opt("motoring_distance")
                    .and_then(|v| v.ok())
                    .unwrap_or(0.0);
                let upwind: f64 = row
                    .get_opt("upwind_distance")
                    .and_then(|v| v.ok())
                    .unwrap_or(0.0);
                let reaching: f64 = row
                    .get_opt("reaching_distance")
                    .and_then(|v| v.ok())
                    .unwrap_or(0.0);
                let running: f64 = row
                    .get_opt("running_distance")
                    .and_then(|v| v.ok())
                    .unwrap_or(0.0);
                let upwind_ms: u64 = row.get_opt("upwind_time").and_then(|v| v.ok()).unwrap_or(0);
                let reaching_ms: u64 = row.get_opt("reaching_time").and_then(|v| v.ok()).unwrap_or(0);
                let running_ms: u64 = row.get_opt("running_time").and_then(|v| v.ok()).unwrap_or(0);
                computed.insert(date, (total, sail, motor, upwind, reaching, running, upwind_ms, reaching_ms, running_ms));
            }

            // Batch INSERT IGNORE all dates in [from_dt, cache_end] — including 0-distance days
            // so they won't be considered missing on the next call.
            let mut rows: Vec<(String, f64, f64, f64, f64, f64, f64, u64, u64, u64)> = Vec::new();
            let mut d = from_dt;
            while d <= cache_end {
                let s = d.format("%Y-%m-%d").to_string();
                let (total, sail, motor, upwind, reaching, running, upwind_ms, reaching_ms, running_ms) =
                    computed.get(&s).copied().unwrap_or((0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0, 0, 0));
                let total = if total.is_finite() { total } else { 0.0 };
                let sail = if sail.is_finite() { sail } else { 0.0 };
                let motor = if motor.is_finite() { motor } else { 0.0 };
                let upwind = if upwind.is_finite() { upwind } else { 0.0 };
                let reaching = if reaching.is_finite() { reaching } else { 0.0 };
                let running = if running.is_finite() { running } else { 0.0 };
                rows.push((s.clone(), total, sail, motor, upwind, reaching, running, upwind_ms, reaching_ms, running_ms));
                day_map.entry(s).or_insert((total, sail, motor, upwind, reaching, running, upwind_ms, reaching_ms, running_ms));
                d += chrono::Duration::days(1);
            }

            if !rows.is_empty() {
                conn.exec_batch(
                    "INSERT IGNORE INTO heatmap_cache \
                     (date, distance_nm, sailing_distance_nm, motoring_distance_nm, \
                      upwind_distance_nm, reaching_distance_nm, running_distance_nm, \
                      upwind_time_ms, reaching_time_ms, running_time_ms) \
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    rows.iter().map(|(date, total, sail, motor, upwind, reaching, running, upwind_ms, reaching_ms, running_ms)| {
                        (date.as_str(), *total, *sail, *motor, *upwind, *reaching, *running, *upwind_ms, *reaching_ms, *running_ms)
                    }),
                )?;
            }
        }
```

Step 4 ("always recompute today") today reads:

```rust
        if end_dt >= today {
            let today_str = today.format("%Y-%m-%d").to_string();
            let row: Option<mysql::Row> = conn.exec_first(
                "SELECT \
                    COALESCE(SUM(COALESCE(total_distance_nm, 0)), 0) as total_distance, \
                    COALESCE(SUM(CASE WHEN engine_on = 0 THEN COALESCE(total_distance_nm, 0) ELSE 0 END), 0) as sailing_distance, \
                    COALESCE(SUM(CASE WHEN engine_on = 1 THEN COALESCE(total_distance_nm, 0) ELSE 0 END), 0) as motoring_distance \
                 FROM vessel_status \
                 WHERE DATE(timestamp) = :today AND is_moored = 0",
                mysql::params! { "today" => &today_str },
            )?;
            let (total, sail, motor) = row
                .map(|r| {
                    let t: f64 = r
                        .get_opt("total_distance")
                        .and_then(|v| v.ok())
                        .unwrap_or(0.0);
                    let s: f64 = r
                        .get_opt("sailing_distance")
                        .and_then(|v| v.ok())
                        .unwrap_or(0.0);
                    let m: f64 = r
                        .get_opt("motoring_distance")
                        .and_then(|v| v.ok())
                        .unwrap_or(0.0);
                    (t, s, m)
                })
                .unwrap_or((0.0, 0.0, 0.0));
            day_map.insert(today_str, (total, sail, motor));
        }
```

Change to (same 6 `SUM(CASE ...)` branches as Step 3's query, applied to a single-day range):

```rust
        if end_dt >= today {
            let today_str = today.format("%Y-%m-%d").to_string();
            let row: Option<mysql::Row> = conn.exec_first(
                "SELECT \
                    COALESCE(SUM(COALESCE(total_distance_nm, 0)), 0) as total_distance, \
                    COALESCE(SUM(CASE WHEN engine_on = 0 THEN COALESCE(total_distance_nm, 0) ELSE 0 END), 0) as sailing_distance, \
                    COALESCE(SUM(CASE WHEN engine_on = 1 THEN COALESCE(total_distance_nm, 0) ELSE 0 END), 0) as motoring_distance, \
                    COALESCE(SUM(CASE WHEN engine_on = 0 AND average_wind_angle_deg IS NOT NULL \
                                      AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) <= 60 \
                                 THEN COALESCE(total_distance_nm, 0) ELSE 0 END), 0) as upwind_distance, \
                    COALESCE(SUM(CASE WHEN engine_on = 0 AND average_wind_angle_deg IS NOT NULL \
                                      AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) > 60 \
                                      AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) < 120 \
                                 THEN COALESCE(total_distance_nm, 0) ELSE 0 END), 0) as reaching_distance, \
                    COALESCE(SUM(CASE WHEN engine_on = 0 AND average_wind_angle_deg IS NOT NULL \
                                      AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) >= 120 \
                                 THEN COALESCE(total_distance_nm, 0) ELSE 0 END), 0) as running_distance, \
                    COALESCE(SUM(CASE WHEN engine_on = 0 AND average_wind_angle_deg IS NOT NULL \
                                      AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) <= 60 \
                                 THEN COALESCE(total_time_ms, 0) ELSE 0 END), 0) as upwind_time, \
                    COALESCE(SUM(CASE WHEN engine_on = 0 AND average_wind_angle_deg IS NOT NULL \
                                      AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) > 60 \
                                      AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) < 120 \
                                 THEN COALESCE(total_time_ms, 0) ELSE 0 END), 0) as reaching_time, \
                    COALESCE(SUM(CASE WHEN engine_on = 0 AND average_wind_angle_deg IS NOT NULL \
                                      AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) >= 120 \
                                 THEN COALESCE(total_time_ms, 0) ELSE 0 END), 0) as running_time \
                 FROM vessel_status \
                 WHERE DATE(timestamp) = :today AND is_moored = 0",
                mysql::params! { "today" => &today_str },
            )?;
            let (total, sail, motor, upwind, reaching, running, upwind_ms, reaching_ms, running_ms) = row
                .map(|r| {
                    let t: f64 = r.get_opt("total_distance").and_then(|v| v.ok()).unwrap_or(0.0);
                    let s: f64 = r.get_opt("sailing_distance").and_then(|v| v.ok()).unwrap_or(0.0);
                    let m: f64 = r.get_opt("motoring_distance").and_then(|v| v.ok()).unwrap_or(0.0);
                    let u: f64 = r.get_opt("upwind_distance").and_then(|v| v.ok()).unwrap_or(0.0);
                    let rc: f64 = r.get_opt("reaching_distance").and_then(|v| v.ok()).unwrap_or(0.0);
                    let rn: f64 = r.get_opt("running_distance").and_then(|v| v.ok()).unwrap_or(0.0);
                    let ums: u64 = r.get_opt("upwind_time").and_then(|v| v.ok()).unwrap_or(0);
                    let rcms: u64 = r.get_opt("reaching_time").and_then(|v| v.ok()).unwrap_or(0);
                    let rnms: u64 = r.get_opt("running_time").and_then(|v| v.ok()).unwrap_or(0);
                    (t, s, m, u, rc, rn, ums, rcms, rnms)
                })
                .unwrap_or((0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0, 0, 0));
            day_map.insert(today_str, (total, sail, motor, upwind, reaching, running, upwind_ms, reaching_ms, running_ms));
        }
```

Finally, Step 5 (assemble sorted result) destructures `day_map`'s value to build `HeatmapDay` — it currently reads `if let Some(&(total, sail, motor)) = day_map.get(&s) {`. Since `HeatmapDay`/`HeatmapData` stay distance-only (see below), widen only the destructuring pattern to match the new 9-tuple shape, ignoring the 6 new fields with `_`:

```rust
            if let Some(&(total, sail, motor, _, _, _, _, _, _)) = day_map.get(&s) {
```

Do **not** add the 6 fields to `HeatmapDay`/`HeatmapData` or the `days.push(HeatmapDay { ... })` call itself — those stay distance-only, matching the current heatmap widget.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --ignored test_fetch_heatmap_populates_point_of_sail_cache -- --test-threads=1`
Expected: PASS. Also run `cargo test --ignored -- --test-threads=1` to confirm the rest of `fetch_heatmap`'s existing suite (distance-only assertions) still passes unchanged.

- [ ] **Step 5: Commit**

```bash
git add schema.sql src/db/operations/query.rs
git commit -m "feat: cache day-level point-of-sail breakdown in heatmap_cache"
```

---

### Task 9: Year-level point-of-sail (`MonthlyStatistic`, `fetch_monthly_statistics`)

**Files:**
- Modify: `src/db/types.rs` (`MonthlyStatistic` struct)
- Modify: `src/db/operations/query.rs` (`fetch_monthly_statistics`)

**Interfaces:**
- Consumes: `heatmap_cache.upwind_distance_nm/...` (Task 8).
- Produces: `MonthlyStatistic` gains `upwind_distance_nm: f64`, `reaching_distance_nm: f64`, `running_distance_nm: f64`, `upwind_time_ms: u64`, `reaching_time_ms: u64`, `running_time_ms: u64`. Serialized automatically via `GET /api/monthly_statistics` and the MCP `get_monthly_statistics` tool. Consumed by Task 11 (frontend).

- [ ] **Step 1: Write the failing test**

Add to `query.rs`'s `#[ignore]` test group:

```rust
    #[test]
    #[ignore]
    fn test_fetch_monthly_statistics_includes_point_of_sail() {
        let db = setup_db();
        let mut conn = db.pool.get_conn().unwrap();
        conn.exec_drop(
            "INSERT INTO heatmap_cache
                (date, distance_nm, sailing_distance_nm, motoring_distance_nm,
                 upwind_distance_nm, reaching_distance_nm, running_distance_nm,
                 upwind_time_ms, reaching_time_ms, running_time_ms)
             VALUES ('2025-06-15', 6.0, 6.0, 0.0, 2.0, 3.0, 1.0, 60000, 90000, 30000)",
            (),
        ).unwrap();

        let stats = db.fetch_monthly_statistics().unwrap();
        let june_2025 = stats
            .months
            .iter()
            .find(|m| m.year == 2025 && m.month == 6)
            .expect("June 2025 should be present");
        assert_approx_equal(june_2025.upwind_distance_nm, 2.0, 0.001, "upwind_distance_nm");
        assert_approx_equal(june_2025.reaching_distance_nm, 3.0, 0.001, "reaching_distance_nm");
        assert_approx_equal(june_2025.running_distance_nm, 1.0, 0.001, "running_distance_nm");
        assert_eq!(june_2025.upwind_time_ms, 60_000);
        assert_eq!(june_2025.reaching_time_ms, 90_000);
        assert_eq!(june_2025.running_time_ms, 30_000);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --ignored test_fetch_monthly_statistics_includes_point_of_sail -- --test-threads=1`
Expected: FAIL to compile — `MonthlyStatistic` has no field `upwind_distance_nm`.

- [ ] **Step 3: Implement**

In `src/db/types.rs`, extend `MonthlyStatistic`:

```rust
#[derive(Debug, serde::Serialize)]
pub struct MonthlyStatistic {
    pub year: i32,
    pub month: u32,
    pub date: String,
    pub sailing_distance_nm: f64,
    pub motoring_distance_nm: f64,
    pub upwind_distance_nm: f64,
    pub reaching_distance_nm: f64,
    pub running_distance_nm: f64,
    pub upwind_time_ms: u64,
    pub reaching_time_ms: u64,
    pub running_time_ms: u64,
}
```

In `src/db/operations/query.rs`, `fetch_monthly_statistics`:

1. Widen the `heatmap_cache` grouped query and the `vessel_status` live-fallback query, each gaining 6 more `SUM(...)` columns:

```rust
        let results: Vec<mysql::Row> = conn.query(
            r"SELECT YEAR(`date`) as year,
                     MONTH(`date`) as month,
                     SUM(sailing_distance_nm) as sailing_distance,
                     SUM(motoring_distance_nm) as motoring_distance,
                     SUM(upwind_distance_nm) as upwind_distance,
                     SUM(reaching_distance_nm) as reaching_distance,
                     SUM(running_distance_nm) as running_distance,
                     SUM(upwind_time_ms) as upwind_time,
                     SUM(reaching_time_ms) as reaching_time,
                     SUM(running_time_ms) as running_time
              FROM heatmap_cache
              GROUP BY YEAR(`date`), MONTH(`date`)
              ORDER BY year ASC, month ASC",
        )?;
```

```rust
        let live_results: Vec<mysql::Row> = conn.exec(
            r"SELECT YEAR(timestamp) as year,
                     MONTH(timestamp) as month,
                     SUM(CASE WHEN engine_on = 0 THEN COALESCE(total_distance_nm, 0) ELSE 0 END) as sailing_distance,
                     SUM(CASE WHEN engine_on = 1 THEN COALESCE(total_distance_nm, 0) ELSE 0 END) as motoring_distance,
                     SUM(CASE WHEN engine_on = 0 AND average_wind_angle_deg IS NOT NULL
                              AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) <= 60
                              THEN COALESCE(total_distance_nm, 0) ELSE 0 END) as upwind_distance,
                     SUM(CASE WHEN engine_on = 0 AND average_wind_angle_deg IS NOT NULL
                              AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) > 60
                              AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) < 120
                              THEN COALESCE(total_distance_nm, 0) ELSE 0 END) as reaching_distance,
                     SUM(CASE WHEN engine_on = 0 AND average_wind_angle_deg IS NOT NULL
                              AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) >= 120
                              THEN COALESCE(total_distance_nm, 0) ELSE 0 END) as running_distance,
                     SUM(CASE WHEN engine_on = 0 AND average_wind_angle_deg IS NOT NULL
                              AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) <= 60
                              THEN COALESCE(total_time_ms, 0) ELSE 0 END) as upwind_time,
                     SUM(CASE WHEN engine_on = 0 AND average_wind_angle_deg IS NOT NULL
                              AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) > 60
                              AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) < 120
                              THEN COALESCE(total_time_ms, 0) ELSE 0 END) as reaching_time,
                     SUM(CASE WHEN engine_on = 0 AND average_wind_angle_deg IS NOT NULL
                              AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) >= 120
                              THEN COALESCE(total_time_ms, 0) ELSE 0 END) as running_time
              FROM vessel_status
              WHERE is_moored = 0 AND DATE(timestamp) > :since
              GROUP BY YEAR(timestamp), MONTH(timestamp)",
            mysql::params! {
                "since" => last_cached_date.unwrap_or_else(|| "1970-01-01".to_string()),
            },
        )?;
```

2. Widen the `month_data` map's value type from `(f64, f64)` to an 8-tuple `(f64, f64, f64, f64, f64, u64, u64, u64)` (sailing_distance, motoring_distance, upwind_distance, reaching_distance, running_distance, upwind_time, reaching_time, running_time). Today the two populating loops read:

```rust
        let mut month_data: std::collections::HashMap<(i32, u32), (f64, f64)> =
            std::collections::HashMap::new();

        for row in results {
            let year: i32 = row
                .get_opt("year")
                .and_then(|v| v.ok())
                .ok_or(AppError::Database("Missing year".to_string()))?;
            let month: u32 = row
                .get_opt::<u32, _>("month")
                .and_then(|v| v.ok())
                .ok_or(AppError::Database("Missing month".to_string()))?;
            let sailing_distance: f64 = row
                .get_opt::<f64, _>("sailing_distance")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let motoring_distance: f64 = row
                .get_opt::<f64, _>("motoring_distance")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);

            month_data.insert((year, month), (sailing_distance, motoring_distance));
        }

        for row in live_results {
            let year: i32 = row
                .get_opt("year")
                .and_then(|v| v.ok())
                .ok_or(AppError::Database("Missing year".to_string()))?;
            let month: u32 = row
                .get_opt::<u32, _>("month")
                .and_then(|v| v.ok())
                .ok_or(AppError::Database("Missing month".to_string()))?;
            let sailing_distance: f64 = row
                .get_opt::<f64, _>("sailing_distance")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let motoring_distance: f64 = row
                .get_opt::<f64, _>("motoring_distance")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);

            let entry = month_data.entry((year, month)).or_insert((0.0, 0.0));
            entry.0 += sailing_distance;
            entry.1 += motoring_distance;
        }
```

Change them to:

```rust
        let mut month_data: std::collections::HashMap<(i32, u32), (f64, f64, f64, f64, f64, u64, u64, u64)> =
            std::collections::HashMap::new();

        for row in results {
            let year: i32 = row
                .get_opt("year")
                .and_then(|v| v.ok())
                .ok_or(AppError::Database("Missing year".to_string()))?;
            let month: u32 = row
                .get_opt::<u32, _>("month")
                .and_then(|v| v.ok())
                .ok_or(AppError::Database("Missing month".to_string()))?;
            let sailing_distance: f64 = row
                .get_opt::<f64, _>("sailing_distance")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let motoring_distance: f64 = row
                .get_opt::<f64, _>("motoring_distance")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let upwind_distance: f64 = row
                .get_opt::<f64, _>("upwind_distance")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let reaching_distance: f64 = row
                .get_opt::<f64, _>("reaching_distance")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let running_distance: f64 = row
                .get_opt::<f64, _>("running_distance")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let upwind_time: u64 = row
                .get_opt::<u64, _>("upwind_time")
                .and_then(|v| v.ok())
                .unwrap_or(0);
            let reaching_time: u64 = row
                .get_opt::<u64, _>("reaching_time")
                .and_then(|v| v.ok())
                .unwrap_or(0);
            let running_time: u64 = row
                .get_opt::<u64, _>("running_time")
                .and_then(|v| v.ok())
                .unwrap_or(0);

            month_data.insert(
                (year, month),
                (sailing_distance, motoring_distance, upwind_distance, reaching_distance,
                 running_distance, upwind_time, reaching_time, running_time),
            );
        }

        for row in live_results {
            let year: i32 = row
                .get_opt("year")
                .and_then(|v| v.ok())
                .ok_or(AppError::Database("Missing year".to_string()))?;
            let month: u32 = row
                .get_opt::<u32, _>("month")
                .and_then(|v| v.ok())
                .ok_or(AppError::Database("Missing month".to_string()))?;
            let sailing_distance: f64 = row
                .get_opt::<f64, _>("sailing_distance")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let motoring_distance: f64 = row
                .get_opt::<f64, _>("motoring_distance")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let upwind_distance: f64 = row
                .get_opt::<f64, _>("upwind_distance")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let reaching_distance: f64 = row
                .get_opt::<f64, _>("reaching_distance")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let running_distance: f64 = row
                .get_opt::<f64, _>("running_distance")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let upwind_time: u64 = row
                .get_opt::<u64, _>("upwind_time")
                .and_then(|v| v.ok())
                .unwrap_or(0);
            let reaching_time: u64 = row
                .get_opt::<u64, _>("reaching_time")
                .and_then(|v| v.ok())
                .unwrap_or(0);
            let running_time: u64 = row
                .get_opt::<u64, _>("running_time")
                .and_then(|v| v.ok())
                .unwrap_or(0);

            let entry = month_data
                .entry((year, month))
                .or_insert((0.0, 0.0, 0.0, 0.0, 0.0, 0, 0, 0));
            entry.0 += sailing_distance;
            entry.1 += motoring_distance;
            entry.2 += upwind_distance;
            entry.3 += reaching_distance;
            entry.4 += running_distance;
            entry.5 += upwind_time;
            entry.6 += reaching_time;
            entry.7 += running_time;
        }
```

3. Widen the `(sailing_dist, motoring_dist)` destructuring and its `unwrap_or` default in the "generate all months" loop to the 8-tuple, and extend the `MonthlyStatistic { ... }` push. Today this reads:

```rust
                let (sailing_dist, motoring_dist) = month_data
                    .get(&(year, month))
                    .copied()
                    .unwrap_or((0.0, 0.0));

                let date = format!("{:04}-{:02}", year, month);

                all_months.push(MonthlyStatistic {
                    year,
                    month,
                    date,
                    sailing_distance_nm: sailing_dist,
                    motoring_distance_nm: motoring_dist,
                });
```

Change it to:

```rust
                let (
                    sailing_dist,
                    motoring_dist,
                    upwind_dist,
                    reaching_dist,
                    running_dist,
                    upwind_time,
                    reaching_time,
                    running_time,
                ) = month_data
                    .get(&(year, month))
                    .copied()
                    .unwrap_or((0.0, 0.0, 0.0, 0.0, 0.0, 0, 0, 0));

                let date = format!("{:04}-{:02}", year, month);

                all_months.push(MonthlyStatistic {
                    year,
                    month,
                    date,
                    sailing_distance_nm: sailing_dist,
                    motoring_distance_nm: motoring_dist,
                    upwind_distance_nm: upwind_dist,
                    reaching_distance_nm: reaching_dist,
                    running_distance_nm: running_dist,
                    upwind_time_ms: upwind_time,
                    reaching_time_ms: reaching_time,
                    running_time_ms: running_time,
                });
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --ignored test_fetch_monthly_statistics_includes_point_of_sail -- --test-threads=1`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/db/types.rs src/db/operations/query.rs
git commit -m "feat: roll up point-of-sail into monthly statistics"
```

---

### Task 10: Frontend — `trip.html` trip and leg-level display

**Files:**
- Modify: `static/trip.html`

**Interfaces:**
- Consumes: `TripSummary.upwind_distance_nm/reaching_distance_nm/running_distance_nm/upwind_time_ms/reaching_time_ms/running_time_ms` (Task 6, via `GET /api/trip`), `TripLeg`'s same 6 fields (Task 7, via `GET /api/trip_legs`).

- [ ] **Step 1: Add 3 new stat tiles to the markup**

In `static/trip.html`, after the existing Sailing/Motoring Time `card-stats` block (around line 246–263), add a third `card-stats` block:

```html
                    <div class="card-stats">
                        <div class="card-stat" id="upwindTimeStat">
                            <div class="card-stat-label">Upwind</div>
                            <div id="upwindTimeValue" class="card-stat-value"></div>
                        </div>
                        <div class="stat-divider"></div>
                        <div class="card-stat" id="reachingTimeStat">
                            <div class="card-stat-label">Reaching</div>
                            <div id="reachingTimeValue" class="card-stat-value"></div>
                        </div>
                        <div class="stat-divider"></div>
                        <div class="card-stat" id="runningTimeStat">
                            <div class="card-stat-label">Running</div>
                            <div id="runningTimeValue" class="card-stat-value"></div>
                        </div>
                    </div>
```

After the existing Sailing/Motoring Distance `card-stats` block (around line 264–281), add a matching distance block:

```html
                    <div class="card-stats">
                        <div class="card-stat" id="upwindDistanceStat">
                            <div class="card-stat-label">Upwind</div>
                            <div id="upwindDistanceValue" class="card-stat-value"></div>
                        </div>
                        <div class="stat-divider"></div>
                        <div class="card-stat" id="reachingDistanceStat">
                            <div class="card-stat-label">Reaching</div>
                            <div id="reachingDistanceValue" class="card-stat-value"></div>
                        </div>
                        <div class="stat-divider"></div>
                        <div class="card-stat" id="runningDistanceStat">
                            <div class="card-stat-label">Running</div>
                            <div id="runningDistanceValue" class="card-stat-value"></div>
                        </div>
                    </div>
```

- [ ] **Step 2: Thread the fields through `loadTripDetails`'s `tripData` construction**

In the single-leg branch of `loadTripDetails` (around line 789–807), add the 6 fields from `selectedLeg`:

```javascript
                    tripData = {
                        id: currentTrip.id,
                        description: (currentTrip.description || 'Trip ' + currentTrip.id) + ' - Leg ' + selectedLeg.leg_number,
                        start_date: selectedLeg.start_timestamp,
                        end_date: selectedLeg.end_timestamp,
                        total_distance_nm: selectedLeg.total_distance_nm,
                        sailing_distance_nm: selectedLeg.sailing_distance_nm,
                        motoring_distance_nm: selectedLeg.motoring_distance_nm,
                        sailing_time_ms: selectedLeg.sailing_time_ms,
                        motoring_time_ms: selectedLeg.motoring_time_ms,
                        upwind_distance_nm: selectedLeg.upwind_distance_nm,
                        reaching_distance_nm: selectedLeg.reaching_distance_nm,
                        running_distance_nm: selectedLeg.running_distance_nm,
                        upwind_time_ms: selectedLeg.upwind_time_ms,
                        reaching_time_ms: selectedLeg.reaching_time_ms,
                        running_time_ms: selectedLeg.running_time_ms,
                        total_time_ms: selectedLeg.sailing_time_ms + selectedLeg.motoring_time_ms,
                        nav_start_timestamp: selectedLeg.nav_start_timestamp || null,
                        nav_end_timestamp: selectedLeg.nav_end_timestamp || null
                    };
```

The full-trip branch (`tripData = { ...currentTrip, total_time_ms: ... }`, around line 783–786) needs no change — the spread already carries every `TripSummary` field, including the 6 new ones, since `currentTrip` is the raw `/api/trip` response.

- [ ] **Step 3: Populate the tiles in `displayTripInfoAndMap`**

After the existing distance/time percentage `textContent` assignments (around line 883–891), add:

```javascript
            document.getElementById('upwindTimeValue').textContent = formatDuration(trip.upwind_time_ms || 0);
            document.getElementById('reachingTimeValue').textContent = formatDuration(trip.reaching_time_ms || 0);
            document.getElementById('runningTimeValue').textContent = formatDuration(trip.running_time_ms || 0);
            document.getElementById('upwindDistanceValue').textContent = (trip.upwind_distance_nm || 0).toFixed(1) + ' NM';
            document.getElementById('reachingDistanceValue').textContent = (trip.reaching_distance_nm || 0).toFixed(1) + ' NM';
            document.getElementById('runningDistanceValue').textContent = (trip.running_distance_nm || 0).toFixed(1) + ' NM';
```

(`|| 0` guards against a full-trip view of a trip recorded before this feature shipped, where the fields exist but may be `0` from `ALTER TABLE ... DEFAULT 0` — no different from how `sailingDistanceValue` already assumes non-null.)

- [ ] **Step 4: Manual verification**

Run the app locally (`cargo run` or existing dev workflow) against a trip with known mixed-angle sailing data. Open `trip.html?id=<a real trip id>` in a browser. Confirm:
- The 6 new tiles render with non-blank values at the trip level.
- Selecting a single leg (if the trip has multiple legs) updates the 6 tiles to that leg's own breakdown.
- No console errors.

- [ ] **Step 5: Commit**

```bash
git add static/trip.html
git commit -m "feat: show point-of-sail breakdown in trip.html"
```

---

### Task 11: Frontend — `yearly-stats.html` year-level display

**Files:**
- Modify: `static/yearly-stats.html`

**Interfaces:**
- Consumes: `MonthlyStatistic`'s 6 new fields (Task 9, via `GET /api/monthly_statistics`).
- Out of scope: the 3 existing Chart.js charts (`yearlyChart`, `monthlyChart`, `accumulatedChart`) are not extended with new series — the summary cards and table below give full visibility into the new numbers without the added complexity of 3 more dataset/color/legend wiring passes. Can be added later as a separate, focused enhancement if wanted.

- [ ] **Step 1: Add 3 new summary cards**

In `static/yearly-stats.html`, after the existing `summary-grid` cards (around line 144–165), add 3 more `analyticsCard` divs:

```html
                <div class="analyticsCard">
                    <div class="analyticsCardTitle">Total Upwind</div>
                    <div class="analyticsValue" id="totalUpwind">0</div>
                    <div class="analyticsDetail">nm</div>
                </div>
                <div class="analyticsCard">
                    <div class="analyticsCardTitle">Total Reaching</div>
                    <div class="analyticsValue" id="totalReaching">0</div>
                    <div class="analyticsDetail">nm</div>
                </div>
                <div class="analyticsCard">
                    <div class="analyticsCardTitle">Total Running</div>
                    <div class="analyticsValue" id="totalRunning">0</div>
                    <div class="analyticsDetail">nm</div>
                </div>
```

- [ ] **Step 2: Add 3 new table columns**

Extend the `yearlyTable` header (around line 179–190):

```html
            <table class="stats-table" id="yearlyTable">
                <thead>
                    <tr>
                        <th>Year</th>
                        <th>Sailing (nm)</th>
                        <th>Motoring (nm)</th>
                        <th>Upwind (nm)</th>
                        <th>Reaching (nm)</th>
                        <th>Running (nm)</th>
                        <th>Total (nm)</th>
                        <th id="ytdHeader">Jan–Mar (nm)</th>
                    </tr>
                </thead>
                <tbody id="yearlyTableBody"></tbody>
            </table>
```

- [ ] **Step 3: Aggregate the new fields in `processStatistics`/`populateYearlyTable`**

In `processStatistics` (around line 264–277), extend the per-year accumulator:

```javascript
            data.months.forEach(month => {
                if (!yearlyData[month.year]) {
                    yearlyData[month.year] = {
                        sailing: 0,
                        motoring: 0,
                        upwind: 0,
                        reaching: 0,
                        running: 0
                    };
                }
                yearlyData[month.year].sailing += month.sailing_distance_nm;
                yearlyData[month.year].motoring += month.motoring_distance_nm;
                yearlyData[month.year].upwind += month.upwind_distance_nm;
                yearlyData[month.year].reaching += month.reaching_distance_nm;
                yearlyData[month.year].running += month.running_distance_nm;
            });
```

Extend the totals block right after it (around line 279–295):

```javascript
            let totalSailing = 0;
            let totalMotoring = 0;
            let totalUpwind = 0;
            let totalReaching = 0;
            let totalRunning = 0;

            Object.values(yearlyData).forEach(year => {
                totalSailing += year.sailing;
                totalMotoring += year.motoring;
                totalUpwind += year.upwind;
                totalReaching += year.reaching;
                totalRunning += year.running;
            });

            const totalDistance = totalSailing + totalMotoring;
            const sailingPercent = totalDistance > 0 ? ((totalSailing / totalDistance) * 100).toFixed(1) : 0;

            document.getElementById('totalSailing').textContent = Math.round(totalSailing);
            document.getElementById('totalMotoring').textContent = Math.round(totalMotoring);
            document.getElementById('totalDistance').textContent = Math.round(totalDistance);
            document.getElementById('sailingPercent').textContent = sailingPercent + '%';
            document.getElementById('totalUpwind').textContent = Math.round(totalUpwind);
            document.getElementById('totalReaching').textContent = Math.round(totalReaching);
            document.getElementById('totalRunning').textContent = Math.round(totalRunning);
```

In `populateYearlyTable`, find the per-row rendering that currently emits `<td>` cells for sailing/motoring/total (below line 329's `// Find max total and max YTD` — read that loop body before editing) and add 3 more `<td>` cells reading `year.upwind.toFixed(1)`, `year.reaching.toFixed(1)`, `year.running.toFixed(1)` in the same position the header columns were inserted (between Motoring and Total).

- [ ] **Step 4: Manual verification**

Load `yearly-stats.html` in a browser against data seeded by Task 9's test or real trip data. Confirm:
- The 3 new summary cards show non-zero values once at least one trip has classified sailing data.
- The table's 3 new columns line up correctly under their headers and sum consistently with the summary cards.
- No console errors.

- [ ] **Step 5: Commit**

```bash
git add static/yearly-stats.html
git commit -m "feat: show point-of-sail breakdown in yearly-stats.html"
```

---

### Task 12: Backfill historical data (manual, post-deploy — not a code change)

**Files:** none (database operation only, run via the `mariadb` MCP tool or `mysql` CLI against the production database after Tasks 1–11 are deployed)

**Interfaces:** none.

This task has no code to write or commit — it's a one-time data migration, run by a human (or by this assistant, following `DB_ANALYST.md`'s write protocol: preview, show exact SQL, confirm, execute in a transaction, verify) after the schema/code changes from Tasks 1–11 are live in production.

- [ ] **Step 1: Add the `trips` columns to production** (if not already present from Task 3's deploy)

```sql
ALTER TABLE trips
    ADD COLUMN total_distance_upwind DOUBLE NOT NULL DEFAULT 0,
    ADD COLUMN total_distance_reaching DOUBLE NOT NULL DEFAULT 0,
    ADD COLUMN total_distance_running DOUBLE NOT NULL DEFAULT 0,
    ADD COLUMN total_time_upwind BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN total_time_reaching BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN total_time_running BIGINT NOT NULL DEFAULT 0;
```

- [ ] **Step 2: Preview affected trip count**

```sql
SELECT COUNT(*) FROM trips;
```

- [ ] **Step 3: Backfill every trip's point-of-sail totals from `vessel_status`**

```sql
UPDATE trips t SET
  total_distance_upwind = (
    SELECT COALESCE(SUM(CASE WHEN vs.is_moored = 0 AND vs.engine_on != 1
                              AND vs.average_wind_angle_deg IS NOT NULL
                              AND LEAST(vs.average_wind_angle_deg, 360 - vs.average_wind_angle_deg) <= 60
                         THEN vs.total_distance_nm ELSE 0 END), 0)
    FROM vessel_status vs WHERE vs.timestamp BETWEEN t.start_timestamp AND t.end_timestamp
  ),
  total_distance_reaching = (
    SELECT COALESCE(SUM(CASE WHEN vs.is_moored = 0 AND vs.engine_on != 1
                              AND vs.average_wind_angle_deg IS NOT NULL
                              AND LEAST(vs.average_wind_angle_deg, 360 - vs.average_wind_angle_deg) > 60
                              AND LEAST(vs.average_wind_angle_deg, 360 - vs.average_wind_angle_deg) < 120
                         THEN vs.total_distance_nm ELSE 0 END), 0)
    FROM vessel_status vs WHERE vs.timestamp BETWEEN t.start_timestamp AND t.end_timestamp
  ),
  total_distance_running = (
    SELECT COALESCE(SUM(CASE WHEN vs.is_moored = 0 AND vs.engine_on != 1
                              AND vs.average_wind_angle_deg IS NOT NULL
                              AND LEAST(vs.average_wind_angle_deg, 360 - vs.average_wind_angle_deg) >= 120
                         THEN vs.total_distance_nm ELSE 0 END), 0)
    FROM vessel_status vs WHERE vs.timestamp BETWEEN t.start_timestamp AND t.end_timestamp
  ),
  total_time_upwind = (
    SELECT COALESCE(SUM(CASE WHEN vs.is_moored = 0 AND vs.engine_on != 1
                              AND vs.average_wind_angle_deg IS NOT NULL
                              AND LEAST(vs.average_wind_angle_deg, 360 - vs.average_wind_angle_deg) <= 60
                         THEN vs.total_time_ms ELSE 0 END), 0)
    FROM vessel_status vs WHERE vs.timestamp BETWEEN t.start_timestamp AND t.end_timestamp
  ),
  total_time_reaching = (
    SELECT COALESCE(SUM(CASE WHEN vs.is_moored = 0 AND vs.engine_on != 1
                              AND vs.average_wind_angle_deg IS NOT NULL
                              AND LEAST(vs.average_wind_angle_deg, 360 - vs.average_wind_angle_deg) > 60
                              AND LEAST(vs.average_wind_angle_deg, 360 - vs.average_wind_angle_deg) < 120
                         THEN vs.total_time_ms ELSE 0 END), 0)
    FROM vessel_status vs WHERE vs.timestamp BETWEEN t.start_timestamp AND t.end_timestamp
  ),
  total_time_running = (
    SELECT COALESCE(SUM(CASE WHEN vs.is_moored = 0 AND vs.engine_on != 1
                              AND vs.average_wind_angle_deg IS NOT NULL
                              AND LEAST(vs.average_wind_angle_deg, 360 - vs.average_wind_angle_deg) >= 120
                         THEN vs.total_time_ms ELSE 0 END), 0)
    FROM vessel_status vs WHERE vs.timestamp BETWEEN t.start_timestamp AND t.end_timestamp
  );
```

- [ ] **Step 4: Verify**

```sql
SELECT id, description, total_distance_sailed,
       total_distance_upwind, total_distance_reaching, total_distance_running
FROM trips
WHERE total_distance_sailed > 0
ORDER BY end_timestamp DESC
LIMIT 10;
```

Confirm `total_distance_upwind + total_distance_reaching + total_distance_running <= total_distance_sailed` for each row (equality only when every sailing sample in that trip had a wind reading).

- [ ] **Step 5: Clear the two caches so they recompute with the new columns on next access**

```sql
DELETE FROM trip_legs_cache;
DELETE FROM heatmap_cache;
```

No further action needed — `fetch_trip_legs` (Task 7) and `fetch_heatmap`/`fetch_monthly_statistics` (Task 8/9) already recompute and re-cache on the next request for any trip/day with no cache row, exactly as they do today for any other cache miss.
