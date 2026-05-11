# Trip Forecast Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the manual POI-based forecast system with automatic trip-area forecasting — users draw bounding boxes on the trip page, and the server polls Open-Meteo every 3 hours while a trip is active.

**Architecture:** A background Tokio task (`src/forecast_poller.rs`) queries the DB for the active trip's areas, calls the Open-Meteo bbox API once per area (2 HTTP calls each), and stores all returned 9 km grid points as `forecast_fetch` / `forecast_hourly` rows tagged with `trip_id` + `area_id`. The trip-overlay endpoint and IDW interpolation are unchanged; only the DB query that feeds them changes (filter by `trip_id` instead of time-window + distance). The meteo page and its POI system are removed entirely.

**Tech Stack:** Rust (tokio, reqwest, mysql, chrono, axum), Leaflet (rectangle draw in trip.html), Chart.js (existing), vanilla JS.

**Spec:** `docs/superpowers/specs/2026-05-11-trip-forecast-redesign-design.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `schema.sql` | Modify | Add `trip_forecast_area`, modify `forecast_fetch`, drop `forecast_poi` |
| `src/db/operations/forecast.rs` | Rewrite | Area CRUD, updated insert/fetch, active-trip query |
| `src/db/test_helpers.rs` | Modify | Replace `forecast_poi` truncation with `trip_forecast_area` |
| `src/forecast.rs` | Modify | Replace coord-list fetch with bbox fetch |
| `src/forecast_poller.rs` | Create | Background 3h poll loop + `ForecastPollerStatus` |
| `src/web/api.rs` | Modify | Replace POI/fetch endpoints with area/status endpoints; add `poller_status` to `AppState` |
| `src/web/server.rs` | Modify | Add `poller_status` to `AppState`, spawn poller task |
| `src/main.rs` | Modify | Add `mod forecast_poller` |
| `static/meteo.html` | Delete | Removed entirely |
| `static/js/shared-theme.js` | Modify | Remove Meteo nav item, bump asset version |
| `static/trip.html` | Modify | Add Leaflet, add Forecast Areas section below trip container |

---

## Task 1: Schema migration

**Files:**
- Modify: `schema.sql`

- [ ] **Step 1: Write the migration SQL**

Create file `schema_migration_forecast_redesign.sql`:

```sql
-- Migration: forecast redesign
-- Run once against the production database

SET FOREIGN_KEY_CHECKS = 0;

-- 1. Remove incompatible old data
TRUNCATE TABLE forecast_hourly;
TRUNCATE TABLE forecast_fetch;
DROP TABLE IF EXISTS forecast_poi;

-- 2. Add new columns to forecast_fetch
ALTER TABLE forecast_fetch
  ADD COLUMN trip_id INT NOT NULL AFTER lon,
  ADD COLUMN area_id INT NOT NULL AFTER trip_id,
  ADD INDEX idx_trip_id (trip_id),
  ADD INDEX idx_area_id (area_id);

-- 3. Create trip_forecast_area
CREATE TABLE IF NOT EXISTS trip_forecast_area (
    id         INT AUTO_INCREMENT PRIMARY KEY,
    trip_id    INT NOT NULL,
    lat_min    DECIMAL(9,6) NOT NULL,
    lat_max    DECIMAL(9,6) NOT NULL,
    lon_min    DECIMAL(9,6) NOT NULL,
    lon_max    DECIMAL(9,6) NOT NULL,
    created_at DATETIME NOT NULL,
    INDEX idx_trip_id (trip_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- 4. Add FK from forecast_fetch to trip_forecast_area (cascade delete)
ALTER TABLE forecast_fetch
  ADD CONSTRAINT fk_forecast_fetch_area
    FOREIGN KEY (area_id) REFERENCES trip_forecast_area(id) ON DELETE CASCADE;

SET FOREIGN_KEY_CHECKS = 1;
```

- [ ] **Step 2: Update `schema.sql` — replace the three forecast table blocks**

Find the block starting at the `forecast_poi` table comment and replace the entire three-table block (forecast_poi + forecast_fetch + forecast_hourly) with:

```sql
-- ============================================================================
-- TRIP FORECAST AREA TABLE
-- ============================================================================
CREATE TABLE IF NOT EXISTS trip_forecast_area (
    id         INT AUTO_INCREMENT PRIMARY KEY,
    trip_id    INT NOT NULL,
    lat_min    DECIMAL(9,6) NOT NULL,
    lat_max    DECIMAL(9,6) NOT NULL,
    lon_min    DECIMAL(9,6) NOT NULL,
    lon_max    DECIMAL(9,6) NOT NULL,
    created_at DATETIME NOT NULL,
    INDEX idx_trip_id (trip_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='Bounding boxes defining the forecast area for a trip';

-- ============================================================================
-- FORECAST FETCH TABLE
-- ============================================================================
CREATE TABLE IF NOT EXISTS forecast_fetch (
    id              INT AUTO_INCREMENT PRIMARY KEY,
    trip_id         INT NOT NULL,
    area_id         INT NOT NULL,
    lat             DECIMAL(9,6) NOT NULL,
    lon             DECIMAL(9,6) NOT NULL,
    fetched_at      DATETIME NOT NULL,
    forecast_from   DATETIME NOT NULL,
    forecast_to     DATETIME NOT NULL,
    INDEX idx_trip_id (trip_id),
    INDEX idx_area_id (area_id),
    INDEX idx_fetched_at (fetched_at),
    CONSTRAINT fk_forecast_fetch_area FOREIGN KEY (area_id) REFERENCES trip_forecast_area(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='One record per grid point per fetch operation, tagged with trip and area';

-- ============================================================================
-- FORECAST HOURLY TABLE
-- ============================================================================
CREATE TABLE IF NOT EXISTS forecast_hourly (
    id                  INT AUTO_INCREMENT PRIMARY KEY,
    fetch_id            INT NOT NULL,
    timestamp           DATETIME NOT NULL,
    wind_speed_kn       DECIMAL(6,2),
    wind_direction_deg  DECIMAL(5,1),
    wind_gust_kn        DECIMAL(6,2),
    wave_height_m       DECIMAL(5,2),
    wave_period_s       DECIMAL(5,2),
    wave_direction_deg  DECIMAL(5,1),
    cape_j_kg           DECIMAL(8,2),
    INDEX idx_fetch_ts (fetch_id, timestamp),
    CONSTRAINT fk_forecast_hourly_fetch FOREIGN KEY (fetch_id) REFERENCES forecast_fetch(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='One row per forecasted hour per fetch; all timestamps UTC';
```

- [ ] **Step 3: Commit**

```bash
git add schema.sql schema_migration_forecast_redesign.sql
git commit -m "feat: schema — trip_forecast_area, trip_id/area_id on forecast_fetch, drop forecast_poi"
```

---

## Task 2: DB operations — forecast areas

**Files:**
- Rewrite: `src/db/operations/forecast.rs`
- Modify: `src/db/test_helpers.rs`

- [ ] **Step 1: Write the failing DB tests**

Replace the entire test module in `src/db/operations/forecast.rs` with these tests first (they will fail to compile until the implementation exists):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::setup_db;
    use crate::db::test_helpers::add_test_trip;
    use std::time::{Duration, UNIX_EPOCH};

    fn make_trip(db: &crate::db::types::VesselDatabase) -> u32 {
        let start = UNIX_EPOCH + Duration::from_secs(1_746_000_000);
        let end = start + Duration::from_secs(3600);
        add_test_trip(db, "test".to_string(), start, end, 10.0, 0.0, 3_600_000, 0, 0).unwrap();
        let trips = db.fetch_trips(None, None).unwrap();
        trips[0].id
    }

    #[test]
    #[ignore]
    fn test_area_create_list_delete() {
        let db = setup_db();
        let trip_id = make_trip(&db);

        let area = NewTripForecastArea {
            trip_id,
            lat_min: 43.0, lat_max: 44.0, lon_min: 8.0, lon_max: 9.0,
        };
        let id = db.create_forecast_area(&area).unwrap();
        assert!(id > 0);

        let areas = db.list_forecast_areas(trip_id).unwrap();
        assert_eq!(areas.len(), 1);
        assert_eq!(areas[0].trip_id, trip_id);

        db.delete_forecast_area(id).unwrap();
        assert!(db.list_forecast_areas(trip_id).unwrap().is_empty());
    }

    #[test]
    #[ignore]
    fn test_insert_forecast_and_trip_inputs() {
        let db = setup_db();
        let trip_id = make_trip(&db);

        let area_id = db.create_forecast_area(&NewTripForecastArea {
            trip_id, lat_min: 43.0, lat_max: 44.0, lon_min: 8.0, lon_max: 9.0,
        }).unwrap();

        let hourly = vec![ForecastHourlyPoint {
            timestamp: "2026-05-11T06:00:00Z".to_string(),
            wind_speed_kn: Some(12.0),
            wind_direction_deg: Some(180.0),
            wind_gust_kn: Some(16.0),
            wave_height_m: Some(1.5),
            wave_period_s: Some(7.0),
            wave_direction_deg: Some(190.0),
            cape_j_kg: Some(0.0),
        }];
        db.insert_forecast(trip_id, area_id, 43.5, 8.5, Utc::now(), &hourly).unwrap();

        let inputs = db.fetch_trip_forecast_inputs(trip_id).unwrap();
        assert!(inputs.is_some());
        assert_eq!(inputs.unwrap().fetches.len(), 1);
    }

    #[test]
    #[ignore]
    fn test_delete_area_cascades_to_fetches() {
        let db = setup_db();
        let trip_id = make_trip(&db);

        let area_id = db.create_forecast_area(&NewTripForecastArea {
            trip_id, lat_min: 43.0, lat_max: 44.0, lon_min: 8.0, lon_max: 9.0,
        }).unwrap();

        let hourly = vec![ForecastHourlyPoint {
            timestamp: "2026-05-11T06:00:00Z".to_string(),
            wind_speed_kn: Some(10.0),
            wind_direction_deg: Some(90.0),
            wind_gust_kn: Some(13.0),
            wave_height_m: Some(1.0),
            wave_period_s: Some(6.0),
            wave_direction_deg: Some(95.0),
            cape_j_kg: Some(0.0),
        }];
        db.insert_forecast(trip_id, area_id, 43.5, 8.5, Utc::now(), &hourly).unwrap();

        // Delete area → should cascade to forecast_fetch + forecast_hourly
        db.delete_forecast_area(area_id).unwrap();

        // Fetches for this trip should now be empty
        let inputs = db.fetch_trip_forecast_inputs(trip_id).unwrap();
        assert!(inputs.unwrap().fetches.is_empty());
    }

    #[test]
    #[ignore]
    fn test_get_last_fetch_time_none_when_empty() {
        let db = setup_db();
        let trip_id = make_trip(&db);
        let last = db.get_last_fetch_time(trip_id).unwrap();
        assert!(last.is_none());
    }

    // IDW tests — keep from old impl, unchanged
    #[test]
    fn test_idw_no_samples_returns_none() {
        use crate::forecast::tests::interpolate_idw_test;
        assert!(interpolate_idw_test(43.0, 9.0, &[]).is_none());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test --lib db::operations::forecast::tests 2>&1 | grep -E "error|FAILED|ok"
```

Expected: compile error (types not found yet)

- [ ] **Step 3: Rewrite `src/db/operations/forecast.rs`**

Replace the entire file:

```rust
use crate::db::types::VesselDatabase;
use crate::error::AppError;
use chrono::{DateTime, Utc};
use mysql::params;
use mysql::prelude::Queryable;
use serde::{Deserialize, Serialize};

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
pub struct TripForecastArea {
    pub id: u32,
    pub trip_id: u32,
    pub lat_min: f64,
    pub lat_max: f64,
    pub lon_min: f64,
    pub lon_max: f64,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct NewTripForecastArea {
    pub trip_id: u32,
    pub lat_min: f64,
    pub lat_max: f64,
    pub lon_min: f64,
    pub lon_max: f64,
}

#[derive(Debug, Serialize, Clone)]
pub struct ForecastHourlyPoint {
    pub timestamp: String,
    pub wind_speed_kn: Option<f64>,
    pub wind_direction_deg: Option<f64>,
    pub wind_gust_kn: Option<f64>,
    pub wave_height_m: Option<f64>,
    pub wave_period_s: Option<f64>,
    pub wave_direction_deg: Option<f64>,
    pub cape_j_kg: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct FetchWithHourly {
    pub lat: f64,
    pub lon: f64,
    pub hourly: Vec<ForecastHourlyPoint>,
}

#[derive(Debug)]
pub struct TripForecastInputs {
    pub trip_start: DateTime<Utc>,
    pub trip_end: DateTime<Utc>,
    /// (lat, lon, timestamp) for each vessel_status track point
    pub track: Vec<(f64, f64, DateTime<Utc>)>,
    /// All forecast_fetch records for this trip, with their hourly data
    pub fetches: Vec<FetchWithHourly>,
}

impl VesselDatabase {
    // ── Area CRUD ─────────────────────────────────────────────────────────────

    pub fn list_forecast_areas(&self, trip_id: u32) -> Result<Vec<TripForecastArea>, AppError> {
        let mut conn = self.pool.get_conn()?;
        let rows: Vec<mysql::Row> = conn.exec(
            "SELECT id, trip_id, lat_min, lat_max, lon_min, lon_max,
                    DATE_FORMAT(created_at, '%Y-%m-%dT%H:%i:%SZ') as created_at
             FROM trip_forecast_area WHERE trip_id = :trip_id ORDER BY id",
            params! { "trip_id" => trip_id },
        )?;
        rows.iter()
            .map(|row| {
                Ok(TripForecastArea {
                    id: row.get("id").ok_or_else(|| AppError::Database("Missing id".into()))?,
                    trip_id: row.get("trip_id").ok_or_else(|| AppError::Database("Missing trip_id".into()))?,
                    lat_min: parse_decimal(row, "lat_min")?,
                    lat_max: parse_decimal(row, "lat_max")?,
                    lon_min: parse_decimal(row, "lon_min")?,
                    lon_max: parse_decimal(row, "lon_max")?,
                    created_at: row.get("created_at").unwrap_or_default(),
                })
            })
            .collect()
    }

    pub fn create_forecast_area(&self, area: &NewTripForecastArea) -> Result<u32, AppError> {
        let mut conn = self.pool.get_conn()?;
        conn.exec_drop(
            "INSERT INTO trip_forecast_area (trip_id, lat_min, lat_max, lon_min, lon_max, created_at)
             VALUES (:trip_id, :lat_min, :lat_max, :lon_min, :lon_max, NOW())",
            params! {
                "trip_id" => area.trip_id,
                "lat_min" => area.lat_min, "lat_max" => area.lat_max,
                "lon_min" => area.lon_min, "lon_max" => area.lon_max,
            },
        )?;
        let id: u64 = conn
            .exec_first("SELECT LAST_INSERT_ID()", ())?
            .ok_or_else(|| AppError::Database("No insert ID returned".into()))?;
        Ok(id as u32)
    }

    pub fn delete_forecast_area(&self, id: u32) -> Result<(), AppError> {
        let mut conn = self.pool.get_conn()?;
        conn.exec_drop(
            "DELETE FROM trip_forecast_area WHERE id = :id",
            params! { "id" => id },
        )?;
        if conn.affected_rows() == 0 {
            return Err(AppError::Database(format!("Forecast area {} not found", id)));
        }
        Ok(())
    }

    // ── Forecast data ─────────────────────────────────────────────────────────

    /// Insert one forecast_fetch + all hourly rows in a single transaction.
    pub fn insert_forecast(
        &self,
        trip_id: u32,
        area_id: u32,
        lat: f64,
        lon: f64,
        fetched_at: DateTime<Utc>,
        hourly: &[ForecastHourlyPoint],
    ) -> Result<(), AppError> {
        if hourly.is_empty() {
            return Ok(());
        }
        let forecast_from = &hourly[0].timestamp;
        let forecast_to = &hourly[hourly.len() - 1].timestamp;

        let fetched_at_str = fetched_at.format("%Y-%m-%d %H:%M:%S").to_string();
        let from_str = parse_iso_to_db(forecast_from)?;
        let to_str = parse_iso_to_db(forecast_to)?;

        let mut conn = self.pool.get_conn()?;
        let mut tx = conn.start_transaction(mysql::TxOpts::default())?;

        tx.exec_drop(
            "INSERT INTO forecast_fetch (trip_id, area_id, lat, lon, fetched_at, forecast_from, forecast_to)
             VALUES (:trip_id, :area_id, :lat, :lon, :fetched_at, :from, :to)",
            params! {
                "trip_id" => trip_id, "area_id" => area_id,
                "lat" => lat, "lon" => lon,
                "fetched_at" => &fetched_at_str,
                "from" => &from_str, "to" => &to_str,
            },
        )?;
        let fetch_id: u64 = tx
            .exec_first("SELECT LAST_INSERT_ID()", ())?
            .ok_or_else(|| AppError::Database("No insert ID".into()))?;

        for pt in hourly {
            let ts_str = parse_iso_to_db(&pt.timestamp)?;
            tx.exec_drop(
                "INSERT INTO forecast_hourly
                 (fetch_id, timestamp, wind_speed_kn, wind_direction_deg, wind_gust_kn,
                  wave_height_m, wave_period_s, wave_direction_deg, cape_j_kg)
                 VALUES (:fid, :ts, :ws, :wd, :wg, :wh, :wp, :wdir, :cape)",
                params! {
                    "fid" => fetch_id, "ts" => &ts_str,
                    "ws" => pt.wind_speed_kn, "wd" => pt.wind_direction_deg,
                    "wg" => pt.wind_gust_kn, "wh" => pt.wave_height_m,
                    "wp" => pt.wave_period_s, "wdir" => pt.wave_direction_deg,
                    "cape" => pt.cape_j_kg,
                },
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Return the most recent `fetched_at` for any forecast_fetch row for this trip.
    pub fn get_last_fetch_time(&self, trip_id: u32) -> Result<Option<DateTime<Utc>>, AppError> {
        let mut conn = self.pool.get_conn()?;
        let row: Option<mysql::Row> = conn.exec_first(
            "SELECT DATE_FORMAT(MAX(fetched_at), '%Y-%m-%dT%H:%i:%SZ') as last_fetch
             FROM forecast_fetch WHERE trip_id = :trip_id",
            params! { "trip_id" => trip_id },
        )?;
        let Some(row) = row else { return Ok(None); };
        let s: Option<String> = row.get("last_fetch");
        match s {
            None => Ok(None),
            Some(s) => {
                let dt = DateTime::parse_from_rfc3339(&s)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| AppError::Parse(e.to_string()))?;
                Ok(Some(dt))
            }
        }
    }

    /// Return the ID of the most recently active trip (end_timestamp within the last 2 hours).
    pub fn get_active_trip_id(&self) -> Result<Option<u32>, AppError> {
        let mut conn = self.pool.get_conn()?;
        let row: Option<mysql::Row> = conn.exec_first(
            "SELECT id FROM trips
             WHERE end_timestamp >= DATE_SUB(NOW(), INTERVAL 2 HOUR)
             ORDER BY end_timestamp DESC LIMIT 1",
            (),
        )?;
        Ok(row.and_then(|r| r.get("id")))
    }

    /// Return (area_count, point_count) for the status endpoint.
    pub fn get_forecast_counts(&self, trip_id: u32) -> Result<(u64, u64), AppError> {
        let mut conn = self.pool.get_conn()?;
        let area_row: Option<mysql::Row> = conn.exec_first(
            "SELECT COUNT(*) as cnt FROM trip_forecast_area WHERE trip_id = :trip_id",
            params! { "trip_id" => trip_id },
        )?;
        let area_count: u64 = area_row.and_then(|r| r.get("cnt")).unwrap_or(0);

        let point_row: Option<mysql::Row> = conn.exec_first(
            "SELECT COUNT(DISTINCT lat, lon) as cnt FROM forecast_fetch WHERE trip_id = :trip_id",
            params! { "trip_id" => trip_id },
        )?;
        let point_count: u64 = point_row.and_then(|r| r.get("cnt")).unwrap_or(0);

        Ok((area_count, point_count))
    }

    /// Load all data needed by `compute_trip_overlay` in `src/forecast.rs`.
    pub fn fetch_trip_forecast_inputs(&self, trip_id: u32) -> Result<Option<TripForecastInputs>, AppError> {
        let mut conn = self.pool.get_conn()?;

        let trip_row: Option<mysql::Row> = conn.exec_first(
            "SELECT DATE_FORMAT(start_timestamp, '%Y-%m-%dT%H:%i:%SZ') as start_ts,
                    DATE_FORMAT(end_timestamp,   '%Y-%m-%dT%H:%i:%SZ') as end_ts
             FROM trips WHERE id = :id",
            params! { "id" => trip_id },
        )?;
        let Some(trip_row) = trip_row else { return Ok(None); };
        let start_str: String = trip_row.get("start_ts").unwrap_or_default();
        let end_str: String = trip_row.get("end_ts").unwrap_or_default();

        let trip_start = DateTime::parse_from_rfc3339(&start_str)
            .map(|d| d.with_timezone(&Utc))
            .map_err(|e| AppError::Parse(e.to_string()))?;
        let trip_end = DateTime::parse_from_rfc3339(&end_str)
            .map(|d| d.with_timezone(&Utc))
            .map_err(|e| AppError::Parse(e.to_string()))?;

        let track_rows: Vec<mysql::Row> = conn.exec(
            "SELECT latitude, longitude,
                    DATE_FORMAT(timestamp, '%Y-%m-%dT%H:%i:%SZ') as ts
             FROM vessel_status
             WHERE timestamp BETWEEN :start AND :end
               AND latitude IS NOT NULL AND longitude IS NOT NULL
             ORDER BY timestamp",
            params! { "start" => &start_str, "end" => &end_str },
        )?;
        let track: Vec<(f64, f64, DateTime<Utc>)> = track_rows
            .iter()
            .filter_map(|r| {
                let lat: f64 = r.get("latitude")?;
                let lon: f64 = r.get("longitude")?;
                let ts_str: String = r.get("ts")?;
                let ts = DateTime::parse_from_rfc3339(&ts_str).ok()?.with_timezone(&Utc);
                Some((lat, lon, ts))
            })
            .collect();

        // Filter by trip_id (replaces old time-window + proximity filter)
        let fetch_rows: Vec<mysql::Row> = conn.exec(
            "SELECT id, lat, lon FROM forecast_fetch WHERE trip_id = :trip_id",
            params! { "trip_id" => trip_id },
        )?;

        let mut fetches = Vec::new();
        for frow in &fetch_rows {
            let fid: u32 = match frow.get("id") { Some(v) => v, None => continue };
            let flat = parse_decimal(frow, "lat").unwrap_or(0.0);
            let flon = parse_decimal(frow, "lon").unwrap_or(0.0);
            let hourly = self.load_hourly(&mut conn, fid)?;
            fetches.push(FetchWithHourly { lat: flat, lon: flon, hourly });
        }

        Ok(Some(TripForecastInputs { trip_start, trip_end, track, fetches }))
    }

    fn load_hourly(
        &self,
        conn: &mut mysql::PooledConn,
        fetch_id: u32,
    ) -> Result<Vec<ForecastHourlyPoint>, AppError> {
        let rows: Vec<mysql::Row> = conn.exec(
            "SELECT DATE_FORMAT(timestamp, '%Y-%m-%dT%H:%i:%SZ') as ts,
                    wind_speed_kn, wind_direction_deg, wind_gust_kn,
                    wave_height_m, wave_period_s, wave_direction_deg, cape_j_kg
             FROM forecast_hourly
             WHERE fetch_id = :fid ORDER BY timestamp",
            params! { "fid" => fetch_id },
        )?;
        Ok(rows
            .iter()
            .map(|r| ForecastHourlyPoint {
                timestamp: r.get("ts").unwrap_or_default(),
                wind_speed_kn: parse_decimal_opt(r, "wind_speed_kn"),
                wind_direction_deg: parse_decimal_opt(r, "wind_direction_deg"),
                wind_gust_kn: parse_decimal_opt(r, "wind_gust_kn"),
                wave_height_m: parse_decimal_opt(r, "wave_height_m"),
                wave_period_s: parse_decimal_opt(r, "wave_period_s"),
                wave_direction_deg: parse_decimal_opt(r, "wave_direction_deg"),
                cape_j_kg: parse_decimal_opt(r, "cape_j_kg"),
            })
            .collect())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_decimal(row: &mysql::Row, col: &str) -> Result<f64, AppError> {
    match row.get_opt::<f64, _>(col) {
        Some(Ok(v)) => Ok(v),
        Some(Err(_)) => {
            let b: Vec<u8> = row.get(col).unwrap_or_default();
            String::from_utf8(b)
                .map_err(|e| AppError::Parse(e.to_string()))?
                .parse::<f64>()
                .map_err(|e| AppError::Parse(e.to_string()))
        }
        None => Err(AppError::Database(format!("Column {} is NULL/missing", col))),
    }
}

fn parse_decimal_opt(row: &mysql::Row, col: &str) -> Option<f64> {
    match row.get_opt::<f64, _>(col) {
        Some(Ok(v)) => Some(v),
        Some(Err(_)) => {
            let b: Vec<u8> = row.get(col)?;
            String::from_utf8(b).ok()?.parse::<f64>().ok()
        }
        None => None,
    }
}

/// Parse ISO 8601 (with or without seconds/Z) to DB DATETIME string.
pub fn parse_iso_to_db(s: &str) -> Result<String, AppError> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc).format("%Y-%m-%d %H:%M:%S").to_string());
    }
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M")
        .map(|ndt| {
            DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .map_err(|e| AppError::Parse(format!("Cannot parse timestamp '{}': {}", s, e)))
}
```

Then add the test module (from Step 1) at the bottom.

- [ ] **Step 4: Update `src/db/test_helpers.rs` — replace `forecast_poi` truncation**

Find and replace:
```rust
    conn.query_drop("TRUNCATE TABLE forecast_hourly")?;
    conn.query_drop("TRUNCATE TABLE forecast_fetch")?;
    conn.query_drop("TRUNCATE TABLE forecast_poi")?;
```
With:
```rust
    conn.query_drop("TRUNCATE TABLE forecast_hourly")?;
    conn.query_drop("TRUNCATE TABLE forecast_fetch")?;
    conn.query_drop("TRUNCATE TABLE trip_forecast_area")?;
```

- [ ] **Step 5: Run the unit tests (non-DB)**

```bash
cargo test --lib 2>&1 | grep -E "error|FAILED|test result"
```

Expected: all non-ignored tests pass, 0 errors

- [ ] **Step 6: Run the DB integration tests**

```bash
cargo test --lib db::operations::forecast::tests -- --test-threads=1 --include-ignored 2>&1 | grep -E "error|FAILED|ok|test result"
```

Expected: all 4 new tests pass

- [ ] **Step 7: Commit**

```bash
git add src/db/operations/forecast.rs src/db/test_helpers.rs
git commit -m "feat: DB — TripForecastArea CRUD, updated insert_forecast/fetch_trip_forecast_inputs, active-trip query"
```

---

## Task 3: Open-Meteo bbox fetch

**Files:**
- Modify: `src/forecast.rs`

- [ ] **Step 1: Write the failing test**

Add to the test module in `src/forecast.rs`:

```rust
    #[test]
    fn test_bbox_url_contains_expected_params() {
        // Verifies the URL builder produces a bbox-style URL (no comma-separated points)
        let url = build_meteo_bbox_url(43.0, 44.0, 8.0, 9.0);
        assert!(url.contains("latitude_min=43"), "url: {}", url);
        assert!(url.contains("latitude_max=44"), "url: {}", url);
        assert!(url.contains("longitude_min=8"), "url: {}", url);
        assert!(url.contains("longitude_max=9"), "url: {}", url);
        assert!(!url.contains(','), "URL should not have comma-separated coords: {}", url);
    }
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test --lib forecast::tests::test_bbox_url_contains_expected_params 2>&1 | grep -E "error|FAILED|ok"
```

Expected: compile error (`build_meteo_bbox_url` not found)

- [ ] **Step 3: Replace `fetch_from_open_meteo` with `fetch_area_forecast` + `build_meteo_bbox_url`**

In `src/forecast.rs`, replace the `fetch_from_open_meteo` function and add the two URL builder helpers:

```rust
// ── URL builders (pub(crate) for testability) ─────────────────────────────────

pub(crate) fn build_meteo_bbox_url(lat_min: f64, lat_max: f64, lon_min: f64, lon_max: f64) -> String {
    // Open-Meteo bbox API: verify parameter names at https://open-meteo.com/en/docs
    format!(
        "https://api.open-meteo.com/v1/forecast\
         ?latitude_min={lat_min}&latitude_max={lat_max}\
         &longitude_min={lon_min}&longitude_max={lon_max}\
         &models=ecmwf_ifs\
         &hourly=wind_speed_10m,wind_direction_10m,wind_gusts_10m,cape\
         &wind_speed_unit=kn&forecast_days=7&timezone=UTC",
        lat_min = lat_min, lat_max = lat_max,
        lon_min = lon_min, lon_max = lon_max,
    )
}

pub(crate) fn build_marine_bbox_url(lat_min: f64, lat_max: f64, lon_min: f64, lon_max: f64) -> String {
    format!(
        "https://marine-api.open-meteo.com/v1/marine\
         ?latitude_min={lat_min}&latitude_max={lat_max}\
         &longitude_min={lon_min}&longitude_max={lon_max}\
         &models=ecmwf_wam\
         &hourly=wave_height,wave_period,wave_direction\
         &forecast_days=7&timezone=UTC",
        lat_min = lat_min, lat_max = lat_max,
        lon_min = lon_min, lon_max = lon_max,
    )
}

/// Fetch forecast for a bounding box. Returns one `FetchedForecast` per ECMWF 9km grid point
/// within the box. Makes exactly 2 HTTP calls (forecast + marine).
pub async fn fetch_area_forecast(
    lat_min: f64,
    lat_max: f64,
    lon_min: f64,
    lon_max: f64,
) -> Result<Vec<FetchedForecast>, AppError> {
    let client = reqwest::Client::new();
    let fetched_at = Utc::now();

    // Forecast (wind + CAPE)
    let meteo_url = build_meteo_bbox_url(lat_min, lat_max, lon_min, lon_max);
    let meteo_resp = client
        .get(&meteo_url)
        .send()
        .await
        .map_err(|e| AppError::Io(format!("Open-Meteo forecast request failed: {}", e)))?;
    if !meteo_resp.status().is_success() {
        return Err(AppError::Io(format!("Open-Meteo forecast returned HTTP {}", meteo_resp.status())));
    }
    let meteo_raw: serde_json::Value = meteo_resp
        .json()
        .await
        .map_err(|e| AppError::Parse(format!("Open-Meteo forecast parse failed: {}", e)))?;
    let meteo_responses: Vec<MeteoResponse> =
        serde_json::from_value::<OneOrMany<MeteoResponse>>(meteo_raw)
            .map_err(|e| AppError::Parse(e.to_string()))?
            .into_vec();

    // Marine (waves)
    let marine_url = build_marine_bbox_url(lat_min, lat_max, lon_min, lon_max);
    let marine_resp = client
        .get(&marine_url)
        .send()
        .await
        .map_err(|e| AppError::Io(format!("Open-Meteo marine request failed: {}", e)))?;
    if !marine_resp.status().is_success() {
        return Err(AppError::Io(format!("Open-Meteo marine returned HTTP {}", marine_resp.status())));
    }
    let marine_raw: serde_json::Value = marine_resp
        .json()
        .await
        .map_err(|e| AppError::Parse(format!("Open-Meteo marine parse failed: {}", e)))?;
    let marine_responses: Vec<MarineResponse> =
        serde_json::from_value::<OneOrMany<MarineResponse>>(marine_raw)
            .map_err(|e| AppError::Parse(e.to_string()))?
            .into_vec();

    // Merge by index — same grid, same order
    let mut results = Vec::new();
    for (i, meteo) in meteo_responses.iter().enumerate() {
        let marine = marine_responses.get(i);
        let n = meteo.hourly.time.len();
        let mut hourly = Vec::with_capacity(n);

        for j in 0..n {
            let timestamp = format!("{}:00Z", meteo.hourly.time[j]);
            hourly.push(ForecastHourlyPoint {
                timestamp,
                wind_speed_kn: meteo.hourly.wind_speed_10m.as_ref().and_then(|v| v.get(j).copied().flatten()),
                wind_direction_deg: meteo.hourly.wind_direction_10m.as_ref().and_then(|v| v.get(j).copied().flatten()),
                wind_gust_kn: meteo.hourly.wind_gusts_10m.as_ref().and_then(|v| v.get(j).copied().flatten()),
                wave_height_m: marine.and_then(|m| m.hourly.wave_height.as_ref()?.get(j).copied().flatten()),
                wave_period_s: marine.and_then(|m| m.hourly.wave_period.as_ref()?.get(j).copied().flatten()),
                wave_direction_deg: marine.and_then(|m| m.hourly.wave_direction.as_ref()?.get(j).copied().flatten()),
                cape_j_kg: meteo.hourly.cape.as_ref().and_then(|v| v.get(j).copied().flatten()),
            });
        }

        results.push(FetchedForecast {
            lat: meteo.latitude,
            lon: meteo.longitude,
            fetched_at,
            hourly,
        });
    }

    Ok(results)
}
```

Also remove the old `fetch_from_open_meteo` function, the `FetchPoiResult` type, and the `#![allow(dead_code)]` attribute at the top. Keep `FetchedForecast`, `TripOverlayPoint`, all private serde types, `compute_trip_overlay`, and all IDW functions unchanged.

Add to the test module (to expose `interpolate_idw` for the DB test module):
```rust
    // Re-export for use in db::operations::forecast tests
    pub fn interpolate_idw_test(
        lat: f64, lon: f64, samples: &[(f64, f64, ForecastHourlyPoint)]
    ) -> Option<ForecastHourlyPoint> {
        interpolate_idw(lat, lon, samples)
    }
```

- [ ] **Step 4: Run tests**

```bash
cargo test --lib forecast::tests 2>&1 | grep -E "error|FAILED|ok|test result"
```

Expected: all forecast tests pass (including the new `test_bbox_url_contains_expected_params`)

- [ ] **Step 5: Commit**

```bash
git add src/forecast.rs
git commit -m "feat: forecast — replace coord-list API with bbox fetch (fetch_area_forecast)"
```

---

## Task 4: Background poller

**Files:**
- Create: `src/forecast_poller.rs`
- Modify: `src/main.rs` (add `mod forecast_poller`)
- Modify: `src/web/api.rs` (add `poller_status` field to `AppState`)
- Modify: `src/web/server.rs` (create `ForecastPollerStatus`, spawn poller, pass to `AppState`)

- [ ] **Step 1: Write the test**

Create `src/forecast_poller.rs` with just the types and a test for status serialisation:

```rust
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ForecastPollerStatus {
    pub online: bool,
    pub last_fetch: Option<DateTime<Utc>>,
    pub next_fetch: Option<DateTime<Utc>>,
}

impl Default for ForecastPollerStatus {
    fn default() -> Self {
        Self { online: true, last_fetch: None, next_fetch: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_status_serialises() {
        let s = ForecastPollerStatus::default();
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"online\":true"));
        assert!(json.contains("\"last_fetch\":null"));
    }
}
```

- [ ] **Step 2: Run test to verify it compiles and passes**

```bash
cargo test --lib forecast_poller::tests 2>&1 | grep -E "error|FAILED|ok"
```

Expected: PASS

- [ ] **Step 3: Write the full poller implementation**

Replace the contents of `src/forecast_poller.rs` with:

```rust
use crate::db::VesselDatabase;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::{Arc, Mutex, RwLock};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize)]
pub struct ForecastPollerStatus {
    pub online: bool,
    pub last_fetch: Option<DateTime<Utc>>,
    pub next_fetch: Option<DateTime<Utc>>,
}

impl Default for ForecastPollerStatus {
    fn default() -> Self {
        Self { online: true, last_fetch: None, next_fetch: None }
    }
}

const FETCH_INTERVAL_SECS: i64 = 3 * 3600; // 3 hours between fetches
const IDLE_CHECK_SECS: u64 = 300;          // 5 min when no trip/areas
const RETRY_SECS: u64 = 900;               // 15 min on connectivity failure

pub async fn run_poller(
    db: Arc<RwLock<VesselDatabase>>,
    status: Arc<Mutex<ForecastPollerStatus>>,
) {
    info!("Forecast poller started");
    loop {
        // 1. Find active trip
        let active_trip = {
            let db = db.read().unwrap_or_else(|e| e.into_inner());
            db.get_active_trip_id().unwrap_or(None)
        };
        let Some(trip_id) = active_trip else {
            tokio::time::sleep(tokio::time::Duration::from_secs(IDLE_CHECK_SECS)).await;
            continue;
        };

        // 2. Check areas
        let areas = {
            let db = db.read().unwrap_or_else(|e| e.into_inner());
            db.list_forecast_areas(trip_id).unwrap_or_default()
        };
        if areas.is_empty() {
            tokio::time::sleep(tokio::time::Duration::from_secs(IDLE_CHECK_SECS)).await;
            continue;
        }

        // 3. Check time since last fetch
        let last_fetch = {
            let db = db.read().unwrap_or_else(|e| e.into_inner());
            db.get_last_fetch_time(trip_id).unwrap_or(None)
        };
        let now = Utc::now();
        if let Some(last) = last_fetch {
            let elapsed = (now - last).num_seconds();
            if elapsed < FETCH_INTERVAL_SECS {
                let wait_secs = (FETCH_INTERVAL_SECS - elapsed) as u64;
                let next = last + chrono::Duration::seconds(FETCH_INTERVAL_SECS);
                {
                    let mut s = status.lock().unwrap();
                    s.next_fetch = Some(next);
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(wait_secs)).await;
                continue;
            }
        }

        // 4. Fetch each area
        let mut fetch_error = false;
        'areas: for area in &areas {
            match crate::forecast::fetch_area_forecast(
                area.lat_min, area.lat_max, area.lon_min, area.lon_max,
            )
            .await
            {
                Ok(forecasts) => {
                    {
                        let mut s = status.lock().unwrap();
                        s.online = true;
                    }
                    let fetched_at = Utc::now();
                    let db = db.read().unwrap_or_else(|e| e.into_inner());
                    for f in &forecasts {
                        if let Err(e) = db.insert_forecast(
                            trip_id, area.id, f.lat, f.lon, fetched_at, &f.hourly,
                        ) {
                            warn!("Failed to store forecast point for trip {}: {}", trip_id, e);
                        }
                    }
                    info!(
                        "Forecast fetched for trip {} area {}: {} grid points",
                        trip_id, area.id, forecasts.len()
                    );
                }
                Err(e) => {
                    warn!("Forecast fetch failed for trip {} area {}: {}", trip_id, area.id, e);
                    {
                        let mut s = status.lock().unwrap();
                        s.online = false;
                    }
                    fetch_error = true;
                    break 'areas;
                }
            }
        }

        if fetch_error {
            tokio::time::sleep(tokio::time::Duration::from_secs(RETRY_SECS)).await;
            continue; // re-check active trip + areas + timing
        }

        // 5. Success — update status and sleep 3h
        let next = Utc::now() + chrono::Duration::seconds(FETCH_INTERVAL_SECS);
        {
            let mut s = status.lock().unwrap();
            s.last_fetch = Some(Utc::now());
            s.next_fetch = Some(next);
            s.online = true;
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(FETCH_INTERVAL_SECS as u64)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_status_serialises() {
        let s = ForecastPollerStatus::default();
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"online\":true"));
        assert!(json.contains("\"last_fetch\":null"));
    }
}
```

- [ ] **Step 4: Add `mod forecast_poller` to `src/main.rs`**

Add after `pub mod forecast;`:
```rust
mod forecast_poller;
```

- [ ] **Step 5: Add `poller_status` to `AppState` in `src/web/api.rs`**

Change the `AppState` struct:
```rust
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<RwLock<VesselDatabase>>,
    pub config: Arc<Config>,
    pub signalk_broadcast: Arc<SignalKBroadcastChannels>,
    pub backup_in_progress: Arc<AtomicBool>,
    pub jwt_secret: Arc<JwtSecret>,
    pub ais_cache: Arc<std::sync::Mutex<AisTargetCache>>,
    pub poller_status: Arc<std::sync::Mutex<crate::forecast_poller::ForecastPollerStatus>>,
}
```

- [ ] **Step 6: Update `src/web/server.rs` — create status, pass to AppState, spawn poller**

Add the import at the top of `src/web/server.rs`:
```rust
use crate::forecast_poller::{ForecastPollerStatus, run_poller};
```

In `start_web_server`, after `let jwt_secret = ...`:
```rust
    let poller_status = Arc::new(std::sync::Mutex::new(ForecastPollerStatus::default()));
```

Update the `AppState` construction:
```rust
    let state = AppState {
        db: db.clone(),
        config,
        signalk_broadcast,
        backup_in_progress: Arc::new(AtomicBool::new(false)),
        jwt_secret,
        ais_cache,
        poller_status: poller_status.clone(),
    };
```

After the `cleanup_old_exports` spawn:
```rust
    let poller_db = db.clone();
    let poller_status_arc = poller_status.clone();
    tokio::spawn(async move {
        run_poller(poller_db, poller_status_arc).await;
    });
```

- [ ] **Step 7: Build to check for compile errors**

```bash
cargo build 2>&1 | grep -E "^error"
```

Expected: no errors

- [ ] **Step 8: Commit**

```bash
git add src/forecast_poller.rs src/main.rs src/web/api.rs src/web/server.rs
git commit -m "feat: forecast poller — 3h background task, ForecastPollerStatus in AppState"
```

---

## Task 5: API endpoints

**Files:**
- Modify: `src/web/api.rs`

- [ ] **Step 1: Remove old forecast query/body types**

Find and delete these structs from `src/web/api.rs`:
```rust
pub struct ForecastPoiIdQuery { ... }
pub struct ForecastDataQuery { ... }
pub struct FetchForecastBody { ... }
```

Keep `ForecastTripOverlayQuery` — it's still used.

- [ ] **Step 2: Add new forecast query/body types**

Add after the remaining forecast struct:
```rust
#[derive(Debug, Deserialize)]
pub struct ForecastAreaIdQuery {
    pub id: u32,
}

#[derive(Debug, Deserialize)]
pub struct ForecastAreaTripQuery {
    pub trip_id: u32,
}

#[derive(Debug, Serialize)]
pub struct ForecastStatusResponse {
    pub online: bool,
    pub last_fetch: Option<String>,
    pub next_fetch: Option<String>,
    pub area_count: u64,
    pub point_count: u64,
}
```

- [ ] **Step 3: Remove old forecast handler functions**

Delete these functions entirely from `src/web/api.rs`:
- `get_forecast_pois`
- `create_forecast_poi`
- `delete_forecast_poi`
- `post_forecast_fetch`
- `get_forecast_data`

- [ ] **Step 4: Add new forecast handler functions**

Add before `pub fn create_api_router`:

```rust
pub async fn get_forecast_areas(
    State(state): State<AppState>,
    Query(params): Query<ForecastAreaTripQuery>,
) -> Result<Json<ApiResponse<Vec<crate::db::operations::forecast::TripForecastArea>>>, StatusCode> {
    match state.db().list_forecast_areas(params.trip_id) {
        Ok(areas) => Ok(Json(ApiResponse::ok(areas))),
        Err(e) => {
            error!(error = %e, trip_id = params.trip_id, "Failed to list forecast areas");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn create_forecast_area(
    State(state): State<AppState>,
    Json(body): Json<crate::db::operations::forecast::NewTripForecastArea>,
) -> Result<Json<ApiResponse<u32>>, StatusCode> {
    match state.db().create_forecast_area(&body) {
        Ok(id) => Ok(Json(ApiResponse::ok(id))),
        Err(e) => {
            error!(error = %e, "Failed to create forecast area");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn delete_forecast_area(
    State(state): State<AppState>,
    Query(params): Query<ForecastAreaIdQuery>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    match state.db().delete_forecast_area(params.id) {
        Ok(()) => Ok(Json(ApiResponse::ok(()))),
        Err(e) => {
            error!(error = %e, area_id = params.id, "Failed to delete forecast area");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn get_forecast_status(
    State(state): State<AppState>,
    Query(params): Query<ForecastAreaTripQuery>,
) -> Result<Json<ApiResponse<ForecastStatusResponse>>, StatusCode> {
    let poller = state.poller_status.lock().unwrap().clone();
    let (area_count, point_count) = state.db().get_forecast_counts(params.trip_id).unwrap_or((0, 0));
    Ok(Json(ApiResponse::ok(ForecastStatusResponse {
        online: poller.online,
        last_fetch: poller.last_fetch.map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        next_fetch: poller.next_fetch.map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        area_count,
        point_count,
    })))
}
```

- [ ] **Step 5: Update route registration in `create_api_router`**

Find the read-only router block and replace the old forecast routes:
```rust
        // Old — remove these:
        .route("/forecast/pois", get(get_forecast_pois))
        .route("/forecast/data", get(get_forecast_data))
        .route("/forecast/trip-overlay", get(get_forecast_trip_overlay))

        // New — replace with:
        .route("/forecast/areas", get(get_forecast_areas))
        .route("/forecast/status", get(get_forecast_status))
        .route("/forecast/trip-overlay", get(get_forecast_trip_overlay))
```

In the write-only block, replace:
```rust
        // Old — remove these:
        .route("/forecast/pois", post(create_forecast_poi))
        .route("/forecast/pois", delete(delete_forecast_poi))
        .route("/forecast/fetch", post(post_forecast_fetch))

        // New — replace with:
        .route("/forecast/areas", post(create_forecast_area))
        .route("/forecast/areas", delete(delete_forecast_area))
```

- [ ] **Step 6: Build to verify**

```bash
cargo build 2>&1 | grep -E "^error"
```

Expected: no errors

- [ ] **Step 7: Commit**

```bash
git add src/web/api.rs
git commit -m "feat: API — replace POI/fetch endpoints with area CRUD + status endpoint"
```

---

## Task 6: Remove meteo.html, update navigation

**Files:**
- Delete: `static/meteo.html`
- Modify: `static/js/shared-theme.js`
- Modify: all `*.html` files that reference `shared-theme.js?v=2`

- [ ] **Step 1: Delete `static/meteo.html`**

```bash
rm /home/aboni/dev/rust_nmea_router/static/meteo.html
```

- [ ] **Step 2: Remove Meteo from nav in `shared-theme.js`**

Find and delete this line from the `navItems` array:
```javascript
        { href: '/meteo.html', label: 'Meteo', page: 'meteo', roHidden: false },
```

- [ ] **Step 3: Bump the version query param on `shared-theme.js` references**

The current version is `?v=2`. Bump to `?v=3` across all HTML files:

```bash
cd static && sed -i 's|shared-theme\.js?v=2"|shared-theme.js?v=3"|g' *.html
```

Verify:
```bash
grep "shared-theme.js" static/*.html | grep -v "?v=3"
```

Expected: no output (all references are now `?v=3`)

- [ ] **Step 4: Commit**

```bash
git add static/js/shared-theme.js static/*.html
git rm static/meteo.html
git commit -m "feat: remove meteo.html and Meteo nav item, bump shared-theme.js version"
```

---

## Task 7: Trip page — Forecast Areas section

**Files:**
- Modify: `static/trip.html`

- [ ] **Step 1: Add Leaflet to `<head>`**

In `static/trip.html`, add after the existing `<link rel="stylesheet" href="/shared.css">`:

```html
<link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/leaflet/1.9.4/leaflet.min.css" />
```

Add before `</head>` (or near the other script tags):
```html
<script src="https://cdnjs.cloudflare.com/ajax/libs/leaflet/1.9.4/leaflet.min.js"></script>
```

- [ ] **Step 2: Add the Forecast Areas HTML block**

Add this block after `</div><!-- end tripContainer -->` (line 313), just before the first `<script>` tag:

```html
<div class="level-1-container" id="forecastAreasContainer" style="display:none; margin-top:20px;">
    <div style="display:flex; justify-content:space-between; align-items:center; margin-bottom:15px;">
        <h2 style="font-size:16px; font-weight:bold; color:var(--text-bold); margin:0;">Forecast Areas</h2>
        <span id="forecastPollerStatus" style="font-size:12px; color:var(--text-secondary);"></span>
    </div>
    <div style="display:grid; grid-template-columns:1fr 1fr; gap:20px;">
        <div>
            <div id="forecastAreaList" style="margin-bottom:12px;"></div>
            <div id="forecastAreaControls" style="display:none;">
                <button id="drawAreaBtn" onclick="startDrawArea()"
                    style="padding:6px 14px; background:var(--link-color); color:#fff; border:none; border-radius:4px; cursor:pointer; font-size:13px;">
                    Draw Area
                </button>
                <button id="cancelDrawBtn" onclick="cancelDrawArea()"
                    style="display:none; padding:6px 14px; background:#e74c3c; color:#fff; border:none; border-radius:4px; cursor:pointer; font-size:13px; margin-left:8px;">
                    Cancel
                </button>
                <span id="drawAreaHint" style="font-size:12px; color:var(--text-secondary); margin-left:10px; display:none;">
                    Click and drag on the map to draw a bounding box
                </span>
            </div>
        </div>
        <div id="forecastAreaMap" style="height:260px; border-radius:6px; border:1px solid var(--border-color);"></div>
    </div>
</div>
```

- [ ] **Step 3: Add forecast area JS functions**

In the `<script>` block of `trip.html`, add these functions near the bottom (before the closing `</script>`):

```javascript
// ── Forecast Areas ────────────────────────────────────────────────────────────

let forecastAreaMap = null;
let forecastAreaRectangles = [];
let drawRect = null;
let drawStart = null;
let currentTripId = null;
let isActiveTripForForecast = false;
let forecastStatusInterval = null;

function initForecastAreaMap() {
    if (forecastAreaMap) { forecastAreaMap.remove(); }
    forecastAreaMap = L.map('forecastAreaMap').setView([43.0, 9.0], 5);
    L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
        attribution: '© OpenStreetMap', maxZoom: 16
    }).addTo(forecastAreaMap);

    forecastAreaMap.on('mousedown', onMapMouseDown);
    forecastAreaMap.on('mousemove', onMapMouseMove);
    forecastAreaMap.on('mouseup', onMapMouseUp);
}

let isDrawing = false;

function startDrawArea() {
    isDrawing = true;
    document.getElementById('cancelDrawBtn').style.display = '';
    document.getElementById('drawAreaBtn').style.display = 'none';
    document.getElementById('drawAreaHint').style.display = '';
    forecastAreaMap.dragging.disable();
}

function cancelDrawArea() {
    isDrawing = false;
    drawStart = null;
    if (drawRect) { forecastAreaMap.removeLayer(drawRect); drawRect = null; }
    document.getElementById('cancelDrawBtn').style.display = 'none';
    document.getElementById('drawAreaBtn').style.display = '';
    document.getElementById('drawAreaHint').style.display = 'none';
    forecastAreaMap.dragging.enable();
}

function onMapMouseDown(e) {
    if (!isDrawing) return;
    drawStart = e.latlng;
    if (drawRect) { forecastAreaMap.removeLayer(drawRect); }
    drawRect = L.rectangle([drawStart, drawStart], {
        color: '#3b82f6', weight: 2, fillOpacity: 0.15
    }).addTo(forecastAreaMap);
}

function onMapMouseMove(e) {
    if (!isDrawing || !drawStart || !drawRect) return;
    drawRect.setBounds(L.latLngBounds(drawStart, e.latlng));
}

async function onMapMouseUp(e) {
    if (!isDrawing || !drawStart) return;
    const end = e.latlng;
    cancelDrawArea();

    const lat_min = Math.min(drawStart.lat, end.lat);
    const lat_max = Math.max(drawStart.lat, end.lat);
    const lon_min = Math.min(drawStart.lng, end.lng);
    const lon_max = Math.max(drawStart.lng, end.lng);

    if (Math.abs(lat_max - lat_min) < 0.05 || Math.abs(lon_max - lon_min) < 0.05) {
        // Too small — ignore
        return;
    }

    try {
        await fetch('/api/forecast/areas', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ trip_id: currentTripId, lat_min, lat_max, lon_min, lon_max }),
        });
        await loadForecastAreas(currentTripId);
    } catch (e) {
        console.error('Failed to save forecast area', e);
    }
}

async function deleteForecastArea(id) {
    try {
        await fetch('/api/forecast/areas?id=' + id, { method: 'DELETE' });
        await loadForecastAreas(currentTripId);
    } catch (e) {
        console.error('Failed to delete forecast area', e);
    }
}

function renderForecastAreaList(areas) {
    const container = document.getElementById('forecastAreaList');
    if (!areas.length) {
        container.innerHTML = '<div style="font-size:13px; color:var(--text-secondary);">No areas defined. Draw a bounding box on the map.</div>';
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
            ${isActiveTripForForecast
                ? `<button onclick="deleteForecastArea(${a.id})"
                       style="background:transparent; border:1px solid #e74c3c; color:#e74c3c;
                              border-radius:3px; padding:1px 8px; font-size:11px; cursor:pointer;">
                       Delete
                   </button>`
                : ''}
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
        const bounds = L.latLngBounds(areas.map(a => [
            [a.lat_min, a.lon_min], [a.lat_max, a.lon_max]
        ]).flat());
        forecastAreaMap.fitBounds(bounds, { padding: [20, 20] });
    }
}

async function loadForecastAreas(tripId) {
    try {
        const resp = await fetch('/api/forecast/areas?trip_id=' + tripId);
        const json = await resp.json();
        const areas = json.data || [];
        renderForecastAreaList(areas);
        renderForecastAreasOnMap(areas);
    } catch (e) {
        console.error('Failed to load forecast areas', e);
    }
}

async function updateForecastStatus(tripId) {
    try {
        const resp = await fetch('/api/forecast/status?trip_id=' + tripId);
        const json = await resp.json();
        const s = json.data;
        if (!s) return;
        const onlineBadge = s.online
            ? '<span style="background:#14532d;color:#86efac;padding:2px 8px;border-radius:10px;font-size:11px;">● Fetching every 3h</span>'
            : '<span style="background:#7c2d12;color:#fdba74;padding:2px 8px;border-radius:10px;font-size:11px;">⚠ Offline — retrying</span>';
        const lastFetch = s.last_fetch
            ? 'Last fetch: ' + new Date(s.last_fetch).toLocaleTimeString() + ' UTC'
            : 'No fetch yet';
        const nextFetch = s.next_fetch
            ? ' · Next: ' + new Date(s.next_fetch).toLocaleTimeString()
            : '';
        document.getElementById('forecastPollerStatus').innerHTML =
            onlineBadge + ' <span style="font-size:11px;color:var(--text-secondary);margin-left:8px;">' +
            lastFetch + nextFetch + ' · ' + s.point_count + ' pts</span>';
    } catch (_) {}
}

function initForecastAreasSection(tripId, isActive) {
    currentTripId = tripId;
    isActiveTripForForecast = isActive;

    document.getElementById('forecastAreasContainer').style.display = '';
    document.getElementById('forecastAreaControls').style.display = isActive ? '' : 'none';

    initForecastAreaMap();
    loadForecastAreas(tripId);
    updateForecastStatus(tripId);

    if (forecastStatusInterval) clearInterval(forecastStatusInterval);
    forecastStatusInterval = setInterval(() => updateForecastStatus(tripId), 60000);
}
```

- [ ] **Step 4: Call `initForecastAreasSection` when a trip loads**

Find the section in `loadTripDetails` (or equivalent) where the trip data is available and chart rendering occurs. It should be around the line that calls `createWindSpeedChart`. After the chart rendering calls, add:

```javascript
                // Init forecast areas section
                const tripEndMs = tripData.end_date ? new Date(tripData.end_date).getTime() : 0;
                const isActive = (Date.now() - tripEndMs) < 7_200_000; // within 2h
                initForecastAreasSection(tripId, isActive);
```

The exact location is after `allCharts.windSpeed = createWindSpeedChart(...)` and before the `renderWaveForecastChart` call.

- [ ] **Step 5: Build and verify**

```bash
cargo build 2>&1 | grep -E "^error"
```

Expected: no errors

- [ ] **Step 6: Commit**

```bash
git add static/trip.html
git commit -m "feat: trip page — Forecast Areas section with Leaflet map, rectangle draw, status polling"
```

---

## Self-Review Checklist

Before declaring done, verify:
- [ ] `cargo build` passes with zero errors
- [ ] `cargo test --lib` passes (non-DB tests)
- [ ] `schema_migration_forecast_redesign.sql` exists and is runnable
- [ ] No references to `forecast_poi`, `ForecastPoi`, `fetch_from_open_meteo`, `FetchPoiResult` remain in `src/`
- [ ] `meteo.html` is gone
- [ ] `shared-theme.js` nav has no Meteo entry
- [ ] All HTML files reference `shared-theme.js?v=3`
