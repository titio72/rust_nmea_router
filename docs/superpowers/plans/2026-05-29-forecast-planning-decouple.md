# Forecast & Planning — Trip Decoupling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove all trip coupling from the forecast and planning subsystems so forecast areas are a global application concept, the planner is a top-level nav page, and the trip page is free of forecast UI.

**Architecture:** Rename `trip_forecast_area` → `forecast_area` (drop `trip_id`), drop `trip_id` from `forecast_fetch`, update all Rust callers (DB layer, poller, API handlers), move forecast area management from `trip.html` to `plan.html`, and add "Forecast" to the main nav.

**Tech Stack:** Rust/Axum backend, MariaDB (mysql crate), vanilla JS / Leaflet frontend.

**Spec:** `docs/superpowers/specs/2026-05-29-forecast-planning-decoupled-design.md`

---

## File Map

| File | Change |
|---|---|
| `schema.sql` | Update table definitions (rename, drop trip_id columns) |
| `schema_migration_forecast_decouple.sql` | New migration script |
| `src/db/operations/forecast.rs` | Rename structs, drop trip_id from all function signatures and SQL, remove dead functions and TripForecastInputs, update tests |
| `src/forecast.rs` | Remove TripForecastInputs import, TripOverlayPoint struct, compute_trip_overlay fn |
| `src/forecast_poller.rs` | Remove active-trip check, update all function calls to drop trip_id |
| `src/web/api.rs` | Remove ForecastTripOverlayQuery, ForecastAreaTripQuery; drop trip_id from ForecastGridPointsQuery/ForecastRouteQuery/OptimalRouteQuery; update all forecast handlers; remove trip-overlay route |
| `static/js/shared-theme.js` | Add "Forecast" nav item |
| `static/plan.html` | Remove trip dependency; add forecast area management section; update all API calls |
| `static/trip.html` | Remove forecastAreasContainer div, forecast overlay fetch, waveForecast/capeForecaset charts, all forecast JS functions |

---

## Task 1: Write migration SQL and update schema.sql

**Files:**
- Create: `schema_migration_forecast_decouple.sql`
- Modify: `schema.sql`

- [ ] **Step 1: Create the migration file**

Create `schema_migration_forecast_decouple.sql` with this content:

```sql
-- Forecast areas become global (no trip relationship)

-- 1. Rename trip_forecast_area → forecast_area, drop trip_id
RENAME TABLE trip_forecast_area TO forecast_area;
ALTER TABLE forecast_area DROP FOREIGN KEY fk_forecast_area_trip;
ALTER TABLE forecast_area DROP INDEX idx_trip_id;
ALTER TABLE forecast_area DROP COLUMN trip_id;

-- 2. Drop trip_id from forecast_fetch (keep area_id FK intact)
ALTER TABLE forecast_fetch DROP FOREIGN KEY fk_forecast_fetch_trip;
ALTER TABLE forecast_fetch DROP INDEX idx_trip_id;
ALTER TABLE forecast_fetch DROP COLUMN trip_id;

-- 3. Truncate stale forecast data (rows carry trip context that no longer applies)
SET FOREIGN_KEY_CHECKS = 0;
TRUNCATE TABLE forecast_hourly;
TRUNCATE TABLE forecast_fetch;
SET FOREIGN_KEY_CHECKS = 1;
```

- [ ] **Step 2: Update schema.sql**

In `schema.sql`, replace the `trip_forecast_area` definition (lines ~227–240):

```sql
-- OLD:
CREATE TABLE IF NOT EXISTS trip_forecast_area (
    id         INT AUTO_INCREMENT PRIMARY KEY,
    trip_id    INT NOT NULL,
    lat_min    DECIMAL(9,6) NOT NULL,
    ...
    INDEX idx_trip_id (trip_id)
);

-- NEW:
CREATE TABLE IF NOT EXISTS forecast_area (
    id         INT AUTO_INCREMENT PRIMARY KEY,
    lat_min    DECIMAL(9,6) NOT NULL,
    lat_max    DECIMAL(9,6) NOT NULL,
    lon_min    DECIMAL(9,6) NOT NULL,
    lon_max    DECIMAL(9,6) NOT NULL,
    created_at DATETIME NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
```

Replace the `forecast_fetch` definition (lines ~242–255):

```sql
-- Remove trip_id column and its FK/index. Keep area_id FK.
CREATE TABLE IF NOT EXISTS forecast_fetch (
    id              INT AUTO_INCREMENT PRIMARY KEY,
    area_id         INT NOT NULL,
    lat             DECIMAL(9,6) NOT NULL,
    lon             DECIMAL(9,6) NOT NULL,
    fetched_at      DATETIME NOT NULL,
    forecast_from   DATETIME NOT NULL,
    forecast_to     DATETIME NOT NULL,
    INDEX idx_area_id (area_id),
    INDEX idx_fetched_at (fetched_at),
    CONSTRAINT fk_forecast_fetch_area FOREIGN KEY (area_id) REFERENCES forecast_area(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
```

- [ ] **Step 3: Verify no syntax errors**

```bash
mysql --user=<user> --password <dbname> < schema_migration_forecast_decouple.sql
```

Expected: no errors. (Or dry-run by reviewing with `--verbose` if you don't want to apply yet.)

- [ ] **Step 4: Commit**

```bash
git add schema_migration_forecast_decouple.sql schema.sql
git commit -m "chore: migration + schema — decouple forecast_area from trips"
```

---

## Task 2: Refactor DB layer — structs, functions, tests

**Files:**
- Modify: `src/db/operations/forecast.rs`

- [ ] **Step 1: Write the updated tests first**

Replace the entire `#[cfg(test)] mod tests` block (lines ~441–607) with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::setup_db;
    use chrono::Utc;

    fn make_area(db: &crate::db::types::VesselDatabase) -> u32 {
        db.create_forecast_area(&NewForecastArea {
            lat_min: 43.0, lat_max: 44.0, lon_min: 8.0, lon_max: 9.0,
        }).unwrap()
    }

    fn make_hourly(ts: &str, wind_kn: f64) -> ForecastHourlyPoint {
        ForecastHourlyPoint {
            timestamp: ts.to_string(),
            wind_speed_kn: Some(wind_kn),
            wind_direction_deg: Some(180.0),
            wind_gust_kn: Some(wind_kn + 3.0),
            wave_height_m: Some(1.0),
            wave_period_s: Some(6.0),
            wave_direction_deg: Some(185.0),
            cape_j_kg: Some(0.0),
        }
    }

    #[test]
    #[ignore]
    fn test_area_create_list_delete() {
        let db = setup_db();
        let id = make_area(&db);
        assert!(id > 0);

        let areas = db.list_forecast_areas().unwrap();
        assert_eq!(areas.len(), 1);
        assert!((areas[0].lat_min - 43.0).abs() < 0.001);

        db.delete_forecast_area(id).unwrap();
        assert!(db.list_forecast_areas().unwrap().is_empty());
    }

    #[test]
    #[ignore]
    fn test_insert_forecast_and_fetch_fetches() {
        let db = setup_db();
        let area_id = make_area(&db);
        let hourly = vec![make_hourly("2026-05-11T06:00:00Z", 12.0)];
        db.insert_forecast(area_id, 43.5, 8.5, Utc::now(), &hourly).unwrap();

        let fetches = db.fetch_forecast_fetches().unwrap();
        assert_eq!(fetches.len(), 1);
        assert_eq!(fetches[0].hourly.len(), 1);
    }

    #[test]
    #[ignore]
    fn test_delete_area_cascades_to_fetches() {
        let db = setup_db();
        let area_id = make_area(&db);
        let hourly = vec![make_hourly("2026-05-11T06:00:00Z", 10.0)];
        db.insert_forecast(area_id, 43.5, 8.5, Utc::now(), &hourly).unwrap();

        db.delete_forecast_area(area_id).unwrap();

        let fetches = db.fetch_forecast_fetches().unwrap();
        assert!(fetches.is_empty());
    }

    #[test]
    #[ignore]
    fn test_get_last_fetch_time_none_when_empty() {
        let db = setup_db();
        let last = db.get_last_fetch_time().unwrap();
        assert!(last.is_none());
    }

    #[test]
    #[ignore]
    fn test_get_grid_points_at_returns_latest_fetch() {
        let db = setup_db();
        let area_id = make_area(&db);
        let ts = "2026-05-14T09:00:00Z";

        // First (older) fetch — wind 10 kn
        db.insert_forecast(area_id, 43.5, 8.5,
            DateTime::parse_from_rfc3339("2026-05-14T06:00:00Z").unwrap().with_timezone(&Utc),
            &vec![make_hourly(ts, 10.0)]).unwrap();

        // Second (newer) fetch — wind 20 kn — this should win
        db.insert_forecast(area_id, 43.5, 8.5,
            DateTime::parse_from_rfc3339("2026-05-14T09:00:00Z").unwrap().with_timezone(&Utc),
            &vec![make_hourly(ts, 20.0)]).unwrap();

        let pts = db.get_grid_points_at(ts).unwrap();
        assert_eq!(pts.len(), 1);
        assert!((pts[0].wind_speed_kn.unwrap() - 20.0).abs() < 0.1,
            "Expected latest fetch (20 kn), got {:?}", pts[0].wind_speed_kn);
    }

    #[test]
    #[ignore]
    fn test_fetch_forecast_fetches_returns_all_grid_points() {
        let db = setup_db();
        let area_id = make_area(&db);
        let hourly = vec![make_hourly("2026-05-14T09:00:00Z", 12.0)];
        db.insert_forecast(area_id, 43.2, 8.3, Utc::now(), &hourly).unwrap();
        db.insert_forecast(area_id, 43.6, 8.7, Utc::now(), &hourly).unwrap();

        let fetches = db.fetch_forecast_fetches().unwrap();
        assert_eq!(fetches.len(), 2);
        assert_eq!(fetches[0].hourly.len(), 1);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail as expected (can't compile yet)**

```bash
cargo test --package nmea_router db::operations::forecast::tests 2>&1 | head -30
```

Expected: compile errors referencing `NewForecastArea`, changed signatures — not logic errors. This confirms the test targets the right symbols.

- [ ] **Step 3: Update public types**

In `src/db/operations/forecast.rs`, replace the `TripForecastArea`, `NewTripForecastArea`, and `TripForecastInputs` type definitions:

```rust
// REMOVE TripForecastInputs entirely (lines ~65–73):
// pub struct TripForecastInputs { ... }

// REPLACE TripForecastArea (lines ~13–22) with:
#[derive(Debug, Serialize, Clone)]
pub struct ForecastArea {
    pub id: u32,
    pub lat_min: f64,
    pub lat_max: f64,
    pub lon_min: f64,
    pub lon_max: f64,
    pub created_at: String,
}

// REPLACE NewTripForecastArea (lines ~37–44) with:
#[derive(Debug, Deserialize)]
pub struct NewForecastArea {
    pub lat_min: f64,
    pub lat_max: f64,
    pub lon_min: f64,
    pub lon_max: f64,
}
```

- [ ] **Step 4: Update `list_forecast_areas`**

Replace the function signature and body:

```rust
pub fn list_forecast_areas(&self) -> Result<Vec<ForecastArea>, AppError> {
    let mut conn = self.pool.get_conn()?;
    let rows: Vec<mysql::Row> = conn.exec(
        "SELECT id, lat_min, lat_max, lon_min, lon_max,
                DATE_FORMAT(created_at, '%Y-%m-%dT%H:%i:%SZ') as created_at
         FROM forecast_area ORDER BY id",
        (),
    )?;
    rows.iter()
        .map(|row| Ok(ForecastArea {
            id: row.get("id").ok_or_else(|| AppError::Database("Missing id".into()))?,
            lat_min: parse_decimal(row, "lat_min")?,
            lat_max: parse_decimal(row, "lat_max")?,
            lon_min: parse_decimal(row, "lon_min")?,
            lon_max: parse_decimal(row, "lon_max")?,
            created_at: row.get("created_at").unwrap_or_default(),
        }))
        .collect()
}
```

- [ ] **Step 5: Update `create_forecast_area`**

```rust
pub fn create_forecast_area(&self, area: &NewForecastArea) -> Result<u32, AppError> {
    let mut conn = self.pool.get_conn()?;
    conn.exec_drop(
        "INSERT INTO forecast_area (lat_min, lat_max, lon_min, lon_max, created_at)
         VALUES (:lat_min, :lat_max, :lon_min, :lon_max, NOW())",
        params! {
            "lat_min" => area.lat_min, "lat_max" => area.lat_max,
            "lon_min" => area.lon_min, "lon_max" => area.lon_max,
        },
    )?;
    Ok(conn.exec_first("SELECT LAST_INSERT_ID()", ())?
        .ok_or_else(|| AppError::Database("No insert ID".into()))?)
}
```

- [ ] **Step 6: Update `insert_forecast` (drop `trip_id`)**

Replace the signature and INSERT statement:

```rust
pub fn insert_forecast(
    &self,
    area_id: u32,
    lat: f64,
    lon: f64,
    fetched_at: DateTime<Utc>,
    hourly: &[ForecastHourlyPoint],
) -> Result<(), AppError> {
```

And the INSERT inside the function body:

```rust
tx.exec_drop(
    "INSERT INTO forecast_fetch (area_id, lat, lon, fetched_at, forecast_from, forecast_to)
     VALUES (:area_id, :lat, :lon, :fetched_at, :from, :to)",
    params! {
        "area_id" => area_id,
        "lat" => lat, "lon" => lon,
        "fetched_at" => &fetched_at_str,
        "from" => &from_str, "to" => &to_str,
    },
)?;
```

- [ ] **Step 7: Update `get_last_fetch_time` (drop `trip_id`)**

```rust
pub fn get_last_fetch_time(&self) -> Result<Option<DateTime<Utc>>, AppError> {
    let mut conn = self.pool.get_conn()?;
    let row: Option<mysql::Row> = conn.exec_first(
        "SELECT DATE_FORMAT(MAX(fetched_at), '%Y-%m-%dT%H:%i:%SZ') as last_fetch
         FROM forecast_fetch",
        (),
    )?;
    // ... rest of function unchanged (reads "last_fetch" from row)
```

- [ ] **Step 8: Update `get_forecast_counts` (drop `trip_id`)**

```rust
pub fn get_forecast_counts(&self) -> Result<(u64, u64), AppError> {
    let mut conn = self.pool.get_conn()?;
    let area_count: u64 = conn
        .exec_first("SELECT COUNT(*) as cnt FROM forecast_area", ())?
        .and_then(|r: mysql::Row| r.get("cnt"))
        .unwrap_or(0);
    let point_count: u64 = conn
        .exec_first("SELECT COUNT(DISTINCT lat, lon) as cnt FROM forecast_fetch", ())?
        .and_then(|r: mysql::Row| r.get("cnt"))
        .unwrap_or(0);
    Ok((area_count, point_count))
}
```

- [ ] **Step 9: Update `get_grid_points_at` (drop `trip_id`)**

Replace signature and SQL:

```rust
pub fn get_grid_points_at(
    &self,
    timestamp_iso: &str,
) -> Result<Vec<GridPointForecast>, AppError> {
    let ts_db = parse_iso_to_db(timestamp_iso)?;
    let mut conn = self.pool.get_conn()?;
    let rows: Vec<mysql::Row> = conn.exec(
        "SELECT ff.lat, ff.lon,
                fh.wind_speed_kn, fh.wind_direction_deg, fh.wind_gust_kn,
                fh.wave_height_m, fh.wave_period_s, fh.wave_direction_deg, fh.cape_j_kg
         FROM forecast_fetch ff
         JOIN forecast_hourly fh ON fh.fetch_id = ff.id
         WHERE fh.timestamp = :ts
           AND ff.fetched_at = (
               SELECT MAX(inner_ff.fetched_at)
               FROM forecast_fetch inner_ff
               WHERE inner_ff.lat = ff.lat
                 AND inner_ff.lon = ff.lon
           )",
        params! { "ts" => &ts_db },
    )?;
    // ... rest of function unchanged
```

- [ ] **Step 10: Update `fetch_forecast_fetches` (drop `trip_id`)**

Replace signature and SQL:

```rust
pub fn fetch_forecast_fetches(&self) -> Result<Vec<FetchWithHourly>, AppError> {
    let mut conn = self.pool.get_conn()?;
    let fetch_rows: Vec<mysql::Row> = conn.exec(
        "SELECT ff.id, ff.lat, ff.lon FROM forecast_fetch ff
         WHERE ff.fetched_at = (
             SELECT MAX(inner_ff.fetched_at)
             FROM forecast_fetch inner_ff
             WHERE inner_ff.lat = ff.lat
               AND inner_ff.lon = ff.lon
         )
         ORDER BY ff.id",
        (),
    )?;
    // ... rest of function unchanged (loads hourly per fetch)
```

- [ ] **Step 11: Remove dead functions**

Delete these function bodies entirely from `src/db/operations/forecast.rs`:
- `fetch_trip_forecast_inputs` (all lines)
- `get_active_trip_id` (all lines)

- [ ] **Step 12: Verify the file compiles in isolation**

```bash
cargo check --package nmea_router 2>&1 | grep "forecast"
```

Expected: errors in `forecast.rs`, `forecast_poller.rs`, and `api.rs` (callers not updated yet) — NOT errors inside `db/operations/forecast.rs` itself. If you see errors inside the module, fix them before proceeding.

- [ ] **Step 13: Run DB tests**

```bash
cargo test -- --test-threads=1 --include-ignored db::operations::forecast::tests 2>&1
```

Expected: all 6 tests pass.

- [ ] **Step 14: Commit**

```bash
git add src/db/operations/forecast.rs
git commit -m "refactor: decouple forecast DB layer from trips — drop trip_id from all functions"
```

---

## Task 3: Clean up forecast.rs

**Files:**
- Modify: `src/forecast.rs`

- [ ] **Step 1: Remove dead imports and types**

At the top of `src/forecast.rs` (line 1), change:

```rust
// FROM:
use crate::db::operations::forecast::{FetchWithHourly, ForecastHourlyPoint, TripForecastInputs};

// TO:
use crate::db::operations::forecast::{FetchWithHourly, ForecastHourlyPoint};
```

- [ ] **Step 2: Remove `TripOverlayPoint` struct and `compute_trip_overlay` function**

Delete:
- The `TripOverlayPoint` struct (lines ~18–28)
- The `compute_trip_overlay` function (lines ~255–285 approximately)

- [ ] **Step 3: Verify compilation**

```bash
cargo check --package nmea_router 2>&1 | grep "forecast"
```

Expected: errors only in `api.rs` (still references `TripOverlayPoint` and `compute_trip_overlay`). No errors in `forecast.rs`.

- [ ] **Step 4: Commit**

```bash
git add src/forecast.rs
git commit -m "refactor: remove TripOverlayPoint and compute_trip_overlay from forecast.rs"
```

---

## Task 4: Refactor forecast_poller.rs

**Files:**
- Modify: `src/forecast_poller.rs`

- [ ] **Step 1: Remove the active-trip check block**

Delete these lines from `run_poller` (approximately lines 29–41):

```rust
// DELETE this entire block:
let active_trip = {
    let db = db.read().unwrap_or_else(|e| e.into_inner());
    db.get_active_trip_id().unwrap_or(None)
};
let Some(trip_id) = active_trip else {
    debug!("Forecast poller: no active trip, sleeping {}s", IDLE_CHECK_SECS);
    tokio::time::sleep(tokio::time::Duration::from_secs(IDLE_CHECK_SECS)).await;
    continue;
};
```

- [ ] **Step 2: Update `list_forecast_areas` call**

```rust
// FROM:
let areas = {
    let db = db.read().unwrap_or_else(|e| e.into_inner());
    db.list_forecast_areas(trip_id).unwrap_or_default()
};

// TO:
let areas = {
    let db = db.read().unwrap_or_else(|e| e.into_inner());
    db.list_forecast_areas().unwrap_or_default()
};
```

Update the debug log below it (remove `trip_id` from structured fields):

```rust
debug!("Forecast poller: no areas defined, sleeping {}s", IDLE_CHECK_SECS);
```

- [ ] **Step 3: Update `get_last_fetch_time` call**

```rust
// FROM:
db.get_last_fetch_time(trip_id).unwrap_or(None)
// TO:
db.get_last_fetch_time().unwrap_or(None)
```

- [ ] **Step 4: Update `insert_forecast` call**

```rust
// FROM:
if let Err(e) = db.insert_forecast(
    trip_id, area.id, f.lat, f.lon, fetched_at, &f.hourly,
) {
    warn!(trip_id, area_id = area.id, error = %e, "Failed to store forecast point");
}

// TO:
if let Err(e) = db.insert_forecast(
    area.id, f.lat, f.lon, fetched_at, &f.hourly,
) {
    warn!(area_id = area.id, error = %e, "Failed to store forecast point");
}
```

- [ ] **Step 5: Remove all `trip_id` structured log fields**

Search for `trip_id,` in the poller file and remove those log fields. They appear in several `info!` and `warn!` calls. Example:

```rust
// FROM:
info!(trip_id, area_count = areas.len(), "Forecast poller: triggering fetch");
// TO:
info!(area_count = areas.len(), "Forecast poller: triggering fetch");
```

Apply the same removal to all other `info!`/`warn!` calls that reference `trip_id` in this file.

- [ ] **Step 6: Verify compilation**

```bash
cargo check --package nmea_router 2>&1 | grep -E "forecast_poller|error"
```

Expected: no errors in `forecast_poller.rs`. Remaining errors are in `api.rs`.

- [ ] **Step 7: Commit**

```bash
git add src/forecast_poller.rs
git commit -m "refactor: remove active-trip gate from forecast poller"
```

---

## Task 5: Refactor api.rs — query structs, handlers, route wiring

**Files:**
- Modify: `src/web/api.rs`

- [ ] **Step 1: Remove and update query structs**

In `src/web/api.rs` (lines ~237–289):

```rust
// REMOVE entirely:
pub struct ForecastTripOverlayQuery { pub trip_id: u32, }
pub struct ForecastAreaTripQuery { pub trip_id: u32, }

// UPDATE ForecastGridPointsQuery — drop trip_id:
#[derive(Debug, Deserialize)]
pub struct ForecastGridPointsQuery {
    pub timestamp: String,
}

// UPDATE ForecastRouteQuery — drop trip_id:
#[derive(Debug, Deserialize)]
pub struct ForecastRouteQuery {
    pub waypoints: String,
    pub departure: String,
    pub motoring_speed_kn: f64,
    #[serde(default = "default_polar_efficiency")]
    pub polar_efficiency: f64,
    #[serde(default)]
    pub min_sail_speed_kn: f64,
}

// UPDATE OptimalRouteQuery — drop trip_id:
#[derive(Debug, Deserialize)]
pub struct OptimalRouteQuery {
    pub from_lat: f64,
    pub from_lon: f64,
    pub to_lat: f64,
    pub to_lon: f64,
    pub departure: String,
    pub motoring_speed_kn: f64,
    #[serde(default = "default_polar_efficiency")]
    pub polar_efficiency: f64,
    #[serde(default)]
    pub min_sail_speed_kn: f64,
    #[serde(default)]
    pub sail_weight_kn: f64,
}
```

- [ ] **Step 2: Remove `get_forecast_trip_overlay` handler**

Delete the entire function (lines ~1537–1551):

```rust
// DELETE:
pub async fn get_forecast_trip_overlay(
    State(state): State<AppState>,
    Query(params): Query<ForecastTripOverlayQuery>,
) -> Result<Json<ApiResponse<Vec<crate::forecast::TripOverlayPoint>>>, StatusCode> {
    ...
}
```

- [ ] **Step 3: Update `get_forecast_areas`**

```rust
pub async fn get_forecast_areas(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<crate::db::operations::forecast::ForecastArea>>>, StatusCode> {
    match state.db().list_forecast_areas() {
        Ok(areas) => Ok(Json(ApiResponse::ok(areas))),
        Err(e) => {
            error!(error = %e, "Failed to list forecast areas");
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}
```

- [ ] **Step 4: Update `create_forecast_area`**

```rust
pub async fn create_forecast_area(
    State(state): State<AppState>,
    Json(body): Json<crate::db::operations::forecast::NewForecastArea>,
) -> Result<Json<ApiResponse<u32>>, StatusCode> {
    match state.db().create_forecast_area(&body) {
        Ok(id) => Ok(Json(ApiResponse::ok(id))),
        Err(e) => {
            error!(error = %e, "Failed to create forecast area");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
```

- [ ] **Step 5: Update `get_forecast_status`**

```rust
pub async fn get_forecast_status(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<ForecastStatusResponse>>, StatusCode> {
    let poller = state.poller_status.lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    let (area_count, point_count) = state.db().get_forecast_counts().unwrap_or((0, 0));
    Ok(Json(ApiResponse::ok(ForecastStatusResponse {
        online: poller.online,
        last_fetch: poller.last_fetch.map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        next_fetch: poller.next_fetch.map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        area_count,
        point_count,
    })))
}
```

- [ ] **Step 6: Update `refresh_forecast`**

```rust
pub async fn refresh_forecast(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<String>>, StatusCode> {
    let areas = state.db().list_forecast_areas().unwrap_or_default();
    if areas.is_empty() {
        return Ok(Json(ApiResponse::error("No forecast areas defined".to_string())));
    }
    let mut total_points = 0usize;
    for area in &areas {
        match crate::forecast::fetch_area_forecast(
            area.lat_min, area.lat_max, area.lon_min, area.lon_max,
        )
        .await
        {
            Ok(forecasts) => {
                let fetched_at = chrono::Utc::now();
                let db = state.db();
                for f in &forecasts {
                    if let Err(e) = db.insert_forecast(
                        area.id, f.lat, f.lon, fetched_at, &f.hourly,
                    ) {
                        warn!(area_id = area.id, error = %e,
                              "refresh_forecast: failed to store point");
                    }
                }
                total_points += forecasts.len();
                let mut s = state.poller_status.lock().unwrap_or_else(|p| p.into_inner());
                s.online = true;
                s.last_fetch = Some(fetched_at);
                s.next_fetch = Some(fetched_at + chrono::Duration::seconds(3 * 3600));
            }
            Err(e) => {
                warn!(area_id = area.id, error = %e, "refresh_forecast: fetch failed");
                state.poller_status.lock().unwrap_or_else(|p| p.into_inner()).online = false;
                return Ok(Json(ApiResponse::error(format!("Fetch failed for area {}: {}", area.id, e))));
            }
        }
    }
    Ok(Json(ApiResponse::ok(format!("{} grid points fetched", total_points))))
}
```

- [ ] **Step 7: Update `get_forecast_grid_points`**

```rust
pub async fn get_forecast_grid_points(
    State(state): State<AppState>,
    Query(params): Query<ForecastGridPointsQuery>,
) -> Result<Json<ApiResponse<Vec<crate::db::operations::forecast::GridPointForecast>>>, StatusCode> {
    match state.db().get_grid_points_at(&params.timestamp) {
        Ok(pts) => Ok(Json(ApiResponse::ok(pts))),
        Err(e) => {
            error!(error = %e, "Failed to get grid points");
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}
```

- [ ] **Step 8: Update `get_forecast_route`**

Remove `params.trip_id` from the `fetch_forecast_fetches` call:

```rust
let fetches = match state.db().fetch_forecast_fetches() {
    Ok(f) => f,
    Err(e) => {
        error!(error = %e, "Failed to load forecast fetches for route");
        return Ok(Json(ApiResponse::error(e.to_string())));
    }
};
```

- [ ] **Step 9: Update `get_optimal_route`**

Same change — remove `params.trip_id`:

```rust
let fetches = match state.db().fetch_forecast_fetches() {
    Ok(f) => f,
    Err(e) => {
        error!(error = %e, "Failed to load forecast fetches for optimal route");
        return Ok(Json(ApiResponse::error(e.to_string())));
    }
};
```

Update the empty-fetch error message:

```rust
if fetches.is_empty() {
    return Ok(Json(ApiResponse::error(
        "No forecast data available".to_string()
    )));
}
```

- [ ] **Step 10: Remove the trip-overlay route**

In the route registration section (around line 1805), delete:

```rust
.route("/forecast/trip-overlay", get(get_forecast_trip_overlay))
```

- [ ] **Step 11: Build and verify**

```bash
cargo build 2>&1 | grep -E "^error"
```

Expected: clean build (zero errors).

- [ ] **Step 12: Commit**

```bash
git add src/web/api.rs
git commit -m "refactor: remove trip_id from all forecast API handlers and query structs"
```

---

## Task 6: Add "Forecast" nav item

**Files:**
- Modify: `static/js/shared-theme.js`

- [ ] **Step 1: Add the nav item**

In `static/js/shared-theme.js`, add `forecast` to the `navItems` array (line ~135):

```js
const navItems = [
    { href: '/', label: 'Trips', page: 'trips', roHidden: false },
    { href: '/realtime.html', label: 'Monitor', page: 'monitor', roHidden: true },
    { href: '/ais.html', label: 'AIS', page: 'ais', roHidden: true },
    { href: '/yearly-stats.html', label: 'Stats', page: 'stats', roHidden: false },
    { href: '/plan.html', label: 'Forecast', page: 'forecast', roHidden: false },   // ← add this
    { href: '/signalk-browser.html', label: 'SignalK Browser', page: 'signalk-browser', roHidden: true },
    { href: '/backup.html', label: 'Backup', page: 'backup', roHidden: true }
];
```

- [ ] **Step 2: Commit**

```bash
git add static/js/shared-theme.js
git commit -m "feat: add Forecast nav item to shared header"
```

---

## Task 7: Refactor plan.html — remove trip dependency and add area management

**Files:**
- Modify: `static/plan.html`

This task has two parts: (A) removing the trip dependency from the existing page, and (B) adding the forecast area management section that was previously in trip.html. Do them as one edit to avoid a broken intermediate state.

- [ ] **Step 1: Update page title and header call**

```html
<!-- Line 6 — FROM: -->
<title>Trip Planning - NMEA Router</title>
<!-- TO: -->
<title>Forecast & Planning - NMEA Router</title>
```

```js
// Line 180 — FROM:
document.getElementById('headerContainer').innerHTML = createHeaderBar('trips');
// TO:
document.getElementById('headerContainer').innerHTML = createHeaderBar('forecast');
```

- [ ] **Step 2: Remove trip URL param and its guard**

Delete lines ~183–189:

```js
// DELETE:
const urlParams = new URLSearchParams(window.location.search);
const tripId = Number(urlParams.get('id'));

if (!tripId) {
    document.getElementById('dayTabsContainer').innerHTML =
        '<span style="color:var(--text-secondary); font-size:13px;">No trip specified. Open this page from a trip.</span>';
}
```

- [ ] **Step 3: Remove trip-related state variables**

Delete these variable declarations from the State section (~lines 206–212):

```js
// DELETE:
let tripInfo = null;
let tripStart = null;
let tripEnd = null;
let tripTrack = [];
let boatMarker = null;
```

- [ ] **Step 4: Remove trip-related functions**

Delete these entire functions:
- `loadTripInfo()` (~lines 268–274)
- `loadTripTrack()` (~lines 276–288)
- `currentBoatPos()` (~lines 291–303)
- `updateBoatMarker()` (~lines 305–325)

- [ ] **Step 5: Update `init()` — remove trip calls**

In `init()`, delete:

```js
// DELETE these two lines:
await loadTripTrack();
updateBoatMarker();
```

- [ ] **Step 6: Replace `loadAvailableDays()` with trip-free version**

Replace the entire `loadAvailableDays()` function (~lines 327–396) with:

```js
async function loadAvailableDays() {
    try {
        const statusResp = await fetch('/api/forecast/status');
        const statusJson = await statusResp.json();
        if (!statusJson.data || statusJson.data.point_count === 0) {
            document.getElementById('dayTabsContainer').innerHTML =
                '<span style="font-size:13px; color:var(--text-secondary);">' +
                'No forecast data. Draw a bounding box in the Forecast Areas section below.</span>';
            return;
        }

        // Build day tabs: today UTC through today + 7 days
        const today = new Date();
        today.setUTCHours(0, 0, 0, 0);
        availableDays = [];
        for (let i = 0; i < 8; i++) {
            const d = new Date(today);
            d.setUTCDate(today.getUTCDate() + i);
            availableDays.push({
                date: d,
                label: d.toLocaleDateString('en-GB', {
                    weekday: 'short', day: 'numeric', month: 'short', timeZone: 'UTC'
                })
            });
        }
        renderDayTabs();
        jumpToNow();
    } catch (err) {
        console.error('Failed to load available days', err);
        document.getElementById('dayTabsContainer').innerHTML =
            '<span style="font-size:13px; color:var(--text-secondary);">Failed to load forecast data. Please refresh.</span>';
    }
}
```

- [ ] **Step 7: Update all API calls to drop `trip_id`**

Find and update each occurrence:

```js
// Line ~330 (inside old loadAvailableDays — already replaced above)

// Line ~588:
// FROM: `/api/forecast/grid-points?trip_id=${tripId}&timestamp=...`
// TO:   `/api/forecast/grid-points?timestamp=${encodeURIComponent(ts)}`

// Line ~846:
// FROM: `/api/forecast/route?trip_id=${tripId}&...`
// TO:   `/api/forecast/route?...`  (keep all other params, remove trip_id only)

// Line ~893:
// FROM: `/api/forecast/optimal-route?trip_id=${tripId}&...`
// TO:   `/api/forecast/optimal-route?...`
```

- [ ] **Step 8: Update localStorage keys (remove trip_id)**

```js
// FROM: localStorage.removeItem(`plan_route_${tripId}`)
// TO:   localStorage.removeItem('plan_route')

// FROM: localStorage.setItem(`plan_route_${tripId}`, ...)
// TO:   localStorage.setItem('plan_route', ...)

// FROM: localStorage.getItem(`plan_route_${tripId}`)
// TO:   localStorage.getItem('plan_route')
```

Also remove any remaining `if (!tripId) return;` guards (around lines ~1102, 1112).

- [ ] **Step 9: Replace `if (tripId) init()` with unconditional call**

```js
// FROM:
if (tripId) init();
// TO:
init();
```

- [ ] **Step 10: Add the Forecast Areas management section**

Add this HTML block immediately after the closing `</div>` of the route bar section (before the map div):

```html
<!-- Forecast Areas management -->
<div class="level-1-container" style="margin-bottom:10px; padding:14px 20px;">
    <div style="display:flex; justify-content:space-between; align-items:center; margin-bottom:12px;">
        <h2 style="font-size:16px; font-weight:bold; color:var(--text-bold); margin:0;">Forecast Areas</h2>
        <div style="display:flex; align-items:center; gap:14px;">
            <span id="forecastPollerStatus" style="font-size:12px; color:var(--text-secondary);"></span>
            <button id="forecastRefreshBtn" onclick="refreshForecast()"
                style="padding:5px 14px; background:var(--bg-secondary); color:var(--text-secondary);
                       border:1px solid var(--border-color); border-radius:4px; cursor:pointer; font-size:13px;">
                ↻ Refresh
            </button>
        </div>
    </div>
    <div style="display:grid; grid-template-columns:1fr 1fr; gap:20px;">
        <div>
            <div id="forecastAreaList" style="margin-bottom:12px;"></div>
            <button id="drawAreaBtn" onclick="startDrawArea()"
                style="padding:6px 14px; background:var(--link-color); color:#fff; border:none;
                       border-radius:4px; cursor:pointer; font-size:13px;">
                Draw Area
            </button>
            <button id="cancelDrawBtn" onclick="cancelDrawArea()"
                style="display:none; padding:6px 14px; background:#e74c3c; color:#fff; border:none;
                       border-radius:4px; cursor:pointer; font-size:13px; margin-left:8px;">
                Cancel
            </button>
            <span id="drawAreaHint"
                style="font-size:12px; color:var(--text-secondary); margin-left:10px; display:none;">
                Click and drag on the map to draw a bounding box
            </span>
        </div>
        <div id="forecastAreaMapEl"
             style="height:200px; border-radius:6px; border:1px solid var(--border-color);"></div>
    </div>
</div>
```

- [ ] **Step 11: Add forecast area management JS**

Add this `<script>` block just before the closing `</body>` tag (or append to the existing inline script):

```js
// ── Forecast Area Management ──────────────────────────────────────────────────

let forecastAreaMap = null;
let forecastAreaRectangles = [];
let forecastDrawOverlay = null;
let drawStart = null;
let drawRect = null;
let isDrawing = false;
let forecastStatusInterval = null;

function initForecastAreaMap() {
    if (forecastAreaMap) { forecastAreaMap.remove(); }
    forecastAreaMap = L.map('forecastAreaMapEl').setView([43.0, 9.0], 5);
    L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
        attribution: '© OpenStreetMap', maxZoom: 16
    }).addTo(forecastAreaMap);
    forecastDrawOverlay = document.createElement('div');
    forecastDrawOverlay.style.cssText =
        'position:absolute;inset:0;z-index:10000;display:none;cursor:crosshair;';
    forecastAreaMap.getContainer().appendChild(forecastDrawOverlay);
    forecastDrawOverlay.addEventListener('mousedown', onAreaMapMouseDown);
}

function startDrawArea() {
    isDrawing = true;
    document.getElementById('cancelDrawBtn').style.display = '';
    document.getElementById('drawAreaBtn').style.display = 'none';
    document.getElementById('drawAreaHint').style.display = '';
    forecastDrawOverlay.style.display = 'block';
}

function cancelDrawArea() {
    isDrawing = false;
    drawStart = null;
    if (drawRect) { forecastAreaMap.removeLayer(drawRect); drawRect = null; }
    document.getElementById('cancelDrawBtn').style.display = 'none';
    document.getElementById('drawAreaBtn').style.display = '';
    document.getElementById('drawAreaHint').style.display = 'none';
    forecastDrawOverlay.style.display = 'none';
}

function overlayLatLng(e) {
    const r = forecastDrawOverlay.getBoundingClientRect();
    return forecastAreaMap.containerPointToLatLng(L.point(e.clientX - r.left, e.clientY - r.top));
}

function onAreaMapMouseDown(e) {
    e.preventDefault();
    e.stopPropagation();
    drawStart = overlayLatLng(e);
    if (drawRect) { forecastAreaMap.removeLayer(drawRect); }
    drawRect = L.rectangle([drawStart, drawStart], {
        color: '#3b82f6', weight: 2, fillOpacity: 0.15, interactive: false
    }).addTo(forecastAreaMap);
    document.addEventListener('mousemove', onAreaMapMouseMove);
    document.addEventListener('mouseup', onAreaMapMouseUp);
}

function onAreaMapMouseMove(e) {
    if (!drawStart || !drawRect) return;
    drawRect.setBounds(L.latLngBounds(drawStart, overlayLatLng(e)));
}

async function onAreaMapMouseUp(e) {
    document.removeEventListener('mousemove', onAreaMapMouseMove);
    document.removeEventListener('mouseup', onAreaMapMouseUp);
    if (!drawStart || !drawRect) return;
    const bounds = drawRect.getBounds();
    const lat_min = Math.min(bounds.getSouthWest().lat, bounds.getNorthEast().lat);
    const lat_max = Math.max(bounds.getSouthWest().lat, bounds.getNorthEast().lat);
    const lon_min = Math.min(bounds.getSouthWest().lng, bounds.getNorthEast().lng);
    const lon_max = Math.max(bounds.getSouthWest().lng, bounds.getNorthEast().lng);
    cancelDrawArea();
    if (Math.abs(lat_max - lat_min) < 0.01 || Math.abs(lon_max - lon_min) < 0.01) return;
    try {
        await fetch('/api/forecast/areas', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ lat_min, lat_max, lon_min, lon_max }),
        });
        await loadForecastAreas();
        await updateForecastStatus();
    } catch (err) {
        console.error('Failed to save forecast area', err);
    }
}

async function deleteForecastArea(id) {
    try {
        await fetch('/api/forecast/areas?id=' + id, { method: 'DELETE' });
        await loadForecastAreas();
        await updateForecastStatus();
    } catch (err) {
        console.error('Failed to delete forecast area', err);
    }
}

function renderForecastAreaList(areas) {
    const container = document.getElementById('forecastAreaList');
    if (!areas.length) {
        container.innerHTML =
            '<div style="font-size:13px; color:var(--text-secondary);">No areas defined. Draw a bounding box on the map.</div>';
        return;
    }
    container.innerHTML = areas.map(a => `
        <div style="display:flex; justify-content:space-between; align-items:center;
                    background:var(--bg-secondary); padding:6px 10px; border-radius:5px;
                    margin-bottom:5px; font-size:13px;">
            <span>
                <strong>Area ${a.id}</strong>
                <span style="color:var(--text-secondary); font-size:11px; margin-left:8px;">
                    ${a.lat_min.toFixed(2)}–${a.lat_max.toFixed(2)}°N &nbsp;
                    ${a.lon_min.toFixed(2)}–${a.lon_max.toFixed(2)}°E
                </span>
            </span>
            <button onclick="deleteForecastArea(${Number(a.id)})"
                style="background:transparent; border:1px solid #e74c3c; color:#e74c3c;
                       border-radius:3px; padding:1px 8px; font-size:11px; cursor:pointer;">
                Delete
            </button>
        </div>`).join('');
}

function renderForecastAreasOnMap(areas) {
    forecastAreaRectangles.forEach(r => forecastAreaMap.removeLayer(r));
    forecastAreaRectangles = [];
    areas.forEach(a => {
        const r = L.rectangle(
            [[a.lat_min, a.lon_min], [a.lat_max, a.lon_max]],
            { color: '#3b82f6', weight: 2, fillOpacity: 0.1 }
        ).addTo(forecastAreaMap);
        forecastAreaRectangles.push(r);
    });
    if (areas.length) {
        const bounds = L.latLngBounds(
            areas.map(a => [[a.lat_min, a.lon_min], [a.lat_max, a.lon_max]]).flat()
        );
        forecastAreaMap.fitBounds(bounds, { padding: [20, 20] });
    }
}

async function loadForecastAreas() {
    try {
        const resp = await fetch('/api/forecast/areas');
        const json = await resp.json();
        const areas = json.data || [];
        renderForecastAreaList(areas);
        renderForecastAreasOnMap(areas);
    } catch (err) {
        console.error('Failed to load forecast areas', err);
        renderForecastAreaList([]);
    }
}

async function updateForecastStatus() {
    try {
        const resp = await fetch('/api/forecast/status');
        const json = await resp.json();
        const s = json.data;
        if (!s) return;
        const statusEl = document.getElementById('forecastPollerStatus');
        const onlineBadge = s.online
            ? '<span style="background:#14532d;color:#86efac;padding:2px 8px;border-radius:10px;font-size:11px;">● Fetching every 3h</span>'
            : '<span style="background:#7c2d12;color:#fdba74;padding:2px 8px;border-radius:10px;font-size:11px;">⚠ Offline — retrying</span>';
        const lastFetch = s.last_fetch
            ? 'Last fetch: ' + new Date(s.last_fetch).toLocaleTimeString() + ' UTC'
            : 'No fetch yet';
        const nextFetch = s.next_fetch
            ? ' · Next: ' + new Date(s.next_fetch).toLocaleTimeString()
            : '';
        statusEl.innerHTML =
            onlineBadge + ' <span style="font-size:11px;color:var(--text-secondary);margin-left:8px;">' +
            lastFetch + nextFetch + ' · ' + s.point_count + ' pts</span>';
    } catch (_) {}
}

async function refreshForecast() {
    const btn = document.getElementById('forecastRefreshBtn');
    const orig = btn.textContent;
    btn.disabled = true;
    btn.textContent = '↻ Fetching…';
    try {
        const resp = await fetch('/api/forecast/refresh', { method: 'POST' });
        const json = await resp.json();
        btn.textContent = json.status === 'ok' ? '✓ Done' : '✗ Error';
        setTimeout(() => { btn.textContent = orig; btn.disabled = false; },
                   json.status === 'ok' ? 2000 : 3000);
    } catch (_) {
        btn.textContent = '✗ Error';
        setTimeout(() => { btn.textContent = orig; btn.disabled = false; }, 3000);
    }
    await updateForecastStatus();
    await loadAvailableDays();
}

// Init area management after the main map is ready
window.addEventListener('load', () => {
    requestAnimationFrame(() => {
        initForecastAreaMap();
        loadForecastAreas();
        updateForecastStatus();
        if (forecastStatusInterval) clearInterval(forecastStatusInterval);
        forecastStatusInterval = setInterval(updateForecastStatus, 60000);
    });
});
```

- [ ] **Step 12: Open plan.html in the browser and verify**

Start the server (`cargo run`) and open `http://localhost:<port>/plan.html`.

- "Forecast" is highlighted in the nav bar
- Forecast Areas section shows with empty area list and Draw Area button
- Draw a bounding box — it should appear in the area list and mini-map
- If forecast data exists: day tabs and hour slider appear, wind arrows render on scrub
- Route planning still works (Plan Route → click FROM → click TO → Compute)

- [ ] **Step 13: Commit**

```bash
git add static/plan.html
git commit -m "feat: decouple plan.html from trips — add area management, remove trip dependency"
```

---

## Task 8: Clean up trip.html — remove all forecast UI

**Files:**
- Modify: `static/trip.html`

- [ ] **Step 1: Remove the `forecastAreasContainer` HTML block**

Delete the entire div (lines ~317–351):

```html
<!-- DELETE this entire block: -->
<div class="level-1-container" id="forecastAreasContainer" style="display:none; margin-top:20px;">
    ...
</div>
```

- [ ] **Step 2: Remove the `waveForecastChart` and `capeForecastChart` chart setup**

In `initializeChartsTrip()` (line ~440), delete:

```js
// DELETE:
addChart(chartsContainer, 'waveForecastChart');
addChart(chartsContainer, 'capeForecastChart');
// Hide wave and CAPE forecast panels until data is available
const waveCard = document.getElementById('waveForecastChart').closest('.app-card');
if (waveCard) waveCard.style.display = 'none';
const capeCard = document.getElementById('capeForecastChart').closest('.app-card');
if (capeCard) capeCard.style.display = 'none';
```

- [ ] **Step 3: Remove `forecastOverlay` state variable**

```js
// DELETE line ~559:
let forecastOverlay = [];
```

- [ ] **Step 4: Remove the forecast overlay fetch block**

In `loadTripDetails()` (lines ~716–728), delete:

```js
// DELETE:
// Fetch forecast overlay (best-effort — continue without it on any error)
forecastOverlay = [];
try {
    const overlayResp = await fetch('/api/forecast/trip-overlay?trip_id=' + tripId);
    if (overlayResp.ok) {
        const overlayJson = await overlayResp.json();
        if (overlayJson.status === 'ok' && Array.isArray(overlayJson.data) && overlayJson.data.length > 0) {
            forecastOverlay = overlayJson.data;
        }
    }
} catch (e) {
    // No forecast data — continue without it
}
```

- [ ] **Step 5: Remove forecast parameters from chart creation calls**

```js
// Line ~862 — FROM:
allCharts.windSpeed = createWindSpeedChart(trackData, forecastOverlay);
// TO:
allCharts.windSpeed = createWindSpeedChart(trackData);

// Line ~863 — FROM:
allCharts.windDirection = createWindDirectionChart(trackData, windDirectionNormalized, forecastOverlay);
// TO:
allCharts.windDirection = createWindDirectionChart(trackData, windDirectionNormalized);

// Line ~1290 — FROM:
allCharts.windDirection = createWindDirectionChart(currentTrackData, windDirectionNormalized, forecastOverlay);
// TO:
allCharts.windDirection = createWindDirectionChart(currentTrackData, windDirectionNormalized);
```

- [ ] **Step 6: Remove `renderWaveForecastChart` and `renderCapeForecastChart` calls and render call sites**

```js
// Lines ~972–973 — DELETE:
renderWaveForecastChart(forecastOverlay);
renderCapeForecastChart(forecastOverlay);
```

- [ ] **Step 7: Remove `initForecastAreasSection` call**

```js
// Lines ~807–810 — DELETE:
// Init forecast areas section
const tripEndMs = tripData.end_date ? new Date(tripData.end_date).getTime() : 0;
const isActive = (Date.now() - tripEndMs) < 86_400_000;
initForecastAreasSection(tripId, isActive);
```

Note: if `isActive` is used elsewhere in `loadTripDetails`, keep that declaration — otherwise delete it.

- [ ] **Step 8: Remove forecast area JS functions and state variables**

Delete the entire block from line ~2557 to the end of `initForecastAreasSection` (line ~2815):

```js
// DELETE all of these (lines ~2557–2812):
let forecastAreaMap = null;
let forecastAreaRectangles = [];
let forecastDrawOverlay = null;
let drawStart = null;
let drawRect = null;
let isDrawing = false;
let currentTripId = null;
let isActiveTripForForecast = false;
let forecastStatusInterval = null;

function initForecastAreaMap() { ... }
function startDrawArea() { ... }
function cancelDrawArea() { ... }
function overlayLatLng() { ... }
function onMapMouseDown() { ... }
function onMapMouseMove() { ... }
function onMapMouseUp() { ... }  // (whatever the actual fn name is)
async function deleteForecastArea() { ... }
function renderForecastAreaList() { ... }
function renderForecastAreasOnMap() { ... }
async function loadForecastAreas() { ... }
async function updateForecastStatus() { ... }
async function refreshForecast() { ... }
function initForecastAreasSection() { ... }
```

Also delete `renderWaveForecastChart` and `renderCapeForecastChart` function bodies (lines ~980–1066 approximately).

- [ ] **Step 9: Verify no broken references**

```bash
grep -n "forecastOverlay\|forecastArea\|waveForecastChart\|capeForecastChart\|initForecastAreas\|renderWaveForecast\|renderCapeForecast\|trip-overlay\|trip_overlay" static/trip.html
```

Expected: no output (all references removed).

- [ ] **Step 10: Open trip.html in browser and verify**

Open a trip. Confirm:
- No forecast-related UI visible anywhere on the page
- No console errors about missing functions or elements
- Wind speed and direction charts render normally (without dashed forecast lines)

- [ ] **Step 11: Commit**

```bash
git add static/trip.html
git commit -m "feat: remove all forecast UI from trip.html"
```

---

## Self-Review Checklist

- [x] **Spec coverage:** Schema (Task 1), DB layer (Task 2), forecast.rs (Task 3), poller (Task 4), API (Task 5), nav (Task 6), plan.html (Task 7), trip.html (Task 8) — all spec sections covered.
- [x] **No placeholders:** All code blocks are complete.
- [x] **Type consistency:** `ForecastArea`/`NewForecastArea` introduced in Task 2 and referenced by name in Task 5. `fetch_forecast_fetches()` (no-arg) used consistently in Tasks 4 and 5.
- [x] **`TripForecastArea` fully renamed:** Only Task 2 touches this; Task 5 updates the handler return type from `TripForecastArea` to `ForecastArea`.
