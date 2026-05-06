# Meteo Forecast Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add weather forecast fetch (Open-Meteo ECMWF IFS HRES 9km + WAM), POI management, and visual overlay of forecast data on trip charts.

**Architecture:** Backend fetches forecast data from two Open-Meteo endpoints per trigger, stores it in three new DB tables (forecast_poi, forecast_fetch, forecast_hourly), and serves it via six new REST endpoints. A dedicated meteo.html page handles POI management and forecast display; trip.html overlays forecast data on existing wind charts using IDW spatial interpolation.

**Tech Stack:** Rust (reqwest 0.12, chrono, mysql), Axum, MariaDB, Chart.js (already loaded in trip.html), Leaflet (already used in trip.html), vanilla JavaScript.

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `schema.sql` | Modify | Add 3 forecast tables at end |
| `src/db/operations/forecast.rs` | Create | POI CRUD, forecast insert, data/overlay queries |
| `src/db/operations/mod.rs` | Modify | Add `pub mod forecast;` |
| `src/db/test_helpers.rs` | Modify | Truncate forecast tables in `reset_test_db` |
| `src/forecast.rs` | Create | Open-Meteo HTTP fetch, IDW interpolation, trip overlay computation |
| `src/main.rs` | Modify | Add `mod forecast;` |
| `src/web/api.rs` | Modify | 6 new handler functions + route registration |
| `static/meteo.html` | Create | POI manager, fetch panel, 4-chart forecast viewer |
| `static/trip.html` | Modify | Forecast overlay on wind charts, wave + CAPE panels |

---

## Task 1: DB Schema — Add Forecast Tables

**Files:**
- Modify: `schema.sql`

- [ ] **Step 1: Append the three forecast tables to schema.sql**

Add at the end of `schema.sql`:

```sql
-- ============================================================================
-- FORECAST POI TABLE
-- ============================================================================
CREATE TABLE IF NOT EXISTS forecast_poi (
    id          INT AUTO_INCREMENT PRIMARY KEY,
    name        VARCHAR(100) NOT NULL,
    lat         DECIMAL(9,6) NOT NULL,
    lon         DECIMAL(9,6) NOT NULL,
    created_at  DATETIME NOT NULL,
    INDEX idx_name (name)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='Named points of interest that guide forecast fetch locations';

-- ============================================================================
-- FORECAST FETCH TABLE
-- ============================================================================
CREATE TABLE IF NOT EXISTS forecast_fetch (
    id              INT AUTO_INCREMENT PRIMARY KEY,
    lat             DECIMAL(9,6) NOT NULL,
    lon             DECIMAL(9,6) NOT NULL,
    fetched_at      DATETIME NOT NULL,
    forecast_from   DATETIME NOT NULL,
    forecast_to     DATETIME NOT NULL,
    INDEX idx_fetched_at (fetched_at),
    INDEX idx_lat_lon (lat, lon)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='One record per fetch operation; coordinates stored directly, no FK to forecast_poi';

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

- [ ] **Step 2: Apply the schema to your development database**

```bash
mysql -u nmea -p nmea_router < schema.sql
```

Expected: no errors; `SHOW TABLES` includes `forecast_poi`, `forecast_fetch`, `forecast_hourly`.

- [ ] **Step 3: Commit**

```bash
git add schema.sql
git commit -m "feat: add forecast_poi, forecast_fetch, forecast_hourly tables"
```

---

## Task 2: DB Operations — Forecast CRUD and Queries

**Files:**
- Create: `src/db/operations/forecast.rs`
- Modify: `src/db/operations/mod.rs`
- Modify: `src/db/test_helpers.rs`

- [ ] **Step 1: Add `pub mod forecast;` to the operations module**

In `src/db/operations/mod.rs`, add after the last `pub mod` line:

```rust
pub mod forecast;
```

- [ ] **Step 2: Write the failing DB tests**

Create `src/db/operations/forecast.rs` with just the types and test stubs that will fail until the implementations exist:

```rust
use crate::db::types::VesselDatabase;
use crate::error::AppError;
use chrono::{DateTime, Utc};
use mysql::params;
use mysql::prelude::Queryable;
use serde::{Deserialize, Serialize};

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
pub struct ForecastPoi {
    pub id: u32,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct NewForecastPoi {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
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
    /// All forecast_fetch records predating trip start, with their hourly data
    pub fetches: Vec<FetchWithHourly>,
}

#[derive(Debug, Serialize)]
pub struct ForecastData {
    pub fetch_id: u32,
    pub lat: f64,
    pub lon: f64,
    pub fetched_at: String,
    pub hourly: Vec<ForecastHourlyPoint>,
}

// Placeholder impl block — steps below fill these in
impl VesselDatabase {}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::setup_db;

    #[test]
    #[ignore]
    fn test_poi_create_list_delete() {
        let db = setup_db();

        let id = db.create_forecast_poi("Ajaccio", 41.9267, 8.7369).unwrap();
        assert!(id > 0);

        let pois = db.list_forecast_pois().unwrap();
        assert_eq!(pois.len(), 1);
        assert_eq!(pois[0].name, "Ajaccio");

        db.delete_forecast_poi(id).unwrap();
        let pois = db.list_forecast_pois().unwrap();
        assert!(pois.is_empty());
    }

    #[test]
    #[ignore]
    fn test_insert_and_query_forecast_data() {
        let db = setup_db();

        let fetched_at = Utc::now();
        let hourly = vec![
            ForecastHourlyPoint {
                timestamp: "2026-05-06T06:00:00Z".to_string(),
                wind_speed_kn: Some(12.0),
                wind_direction_deg: Some(180.0),
                wind_gust_kn: Some(16.0),
                wave_height_m: Some(1.5),
                wave_period_s: Some(7.0),
                wave_direction_deg: Some(190.0),
                cape_j_kg: Some(0.0),
            },
        ];

        db.insert_forecast(41.9267, 8.7369, fetched_at, &hourly).unwrap();

        let data = db.fetch_forecast_data(41.9267, 8.7369).unwrap();
        assert!(data.is_some());
        let data = data.unwrap();
        assert_eq!(data.hourly.len(), 1);
        assert_eq!(data.hourly[0].wind_speed_kn, Some(12.0));
    }

    #[test]
    #[ignore]
    fn test_fetch_trip_forecast_inputs_empty_when_no_fetches() {
        let db = setup_db();
        use crate::db::test_helpers::add_test_trip;
        use std::time::{Duration, UNIX_EPOCH};

        let start = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let end = start + Duration::from_secs(3600);
        add_test_trip(&db, "test".to_string(), start, end, 10.0, 0.0, 3_600_000, 0, 0).unwrap();

        // Fetch trips to get the ID
        let trips = db.fetch_trips(None, None).unwrap();
        let trip_id = trips[0].id;

        let inputs = db.fetch_trip_forecast_inputs(trip_id).unwrap();
        // Trip exists but no forecasts → inputs is Some with empty fetches
        assert!(inputs.is_some());
        assert!(inputs.unwrap().fetches.is_empty());
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

```bash
cargo test -- --test-threads=1 --include-ignored db::operations::forecast::tests
```

Expected: compile errors on missing methods `create_forecast_poi`, `list_forecast_pois`, `delete_forecast_poi`, `insert_forecast`, `fetch_forecast_data`, `fetch_trip_forecast_inputs`.

- [ ] **Step 4: Implement the six DB methods**

Replace the empty `impl VesselDatabase {}` block in `src/db/operations/forecast.rs`:

```rust
impl VesselDatabase {
    pub fn list_forecast_pois(&self) -> Result<Vec<ForecastPoi>, AppError> {
        let mut conn = self.pool.get_conn()?;
        let rows: Vec<mysql::Row> = conn.exec(
            "SELECT id, name, lat, lon,
                    DATE_FORMAT(created_at, '%Y-%m-%dT%H:%i:%SZ') as created_at
             FROM forecast_poi ORDER BY name",
            (),
        )?;
        rows.iter()
            .map(|row| {
                Ok(ForecastPoi {
                    id: row.get("id").ok_or_else(|| AppError::Database("Missing id".into()))?,
                    name: row.get("name").ok_or_else(|| AppError::Database("Missing name".into()))?,
                    lat: parse_decimal(row, "lat")?,
                    lon: parse_decimal(row, "lon")?,
                    created_at: row.get("created_at").unwrap_or_default(),
                })
            })
            .collect()
    }

    pub fn create_forecast_poi(&self, name: &str, lat: f64, lon: f64) -> Result<u32, AppError> {
        let mut conn = self.pool.get_conn()?;
        conn.exec_drop(
            "INSERT INTO forecast_poi (name, lat, lon, created_at) VALUES (:name, :lat, :lon, NOW())",
            params! { "name" => name, "lat" => lat, "lon" => lon },
        )?;
        let id: u64 = conn
            .exec_first("SELECT LAST_INSERT_ID()", ())?
            .ok_or_else(|| AppError::Database("No insert ID returned".into()))?;
        Ok(id as u32)
    }

    pub fn delete_forecast_poi(&self, id: u32) -> Result<(), AppError> {
        let mut conn = self.pool.get_conn()?;
        conn.exec_drop(
            "DELETE FROM forecast_poi WHERE id = :id",
            params! { "id" => id },
        )?;
        Ok(())
    }

    /// Insert one forecast_fetch + all hourly rows in a single transaction.
    pub fn insert_forecast(
        &self,
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
            "INSERT INTO forecast_fetch (lat, lon, fetched_at, forecast_from, forecast_to)
             VALUES (:lat, :lon, :fetched_at, :from, :to)",
            params! {
                "lat" => lat, "lon" => lon,
                "fetched_at" => &fetched_at_str,
                "from" => &from_str,
                "to" => &to_str,
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
                    "fid" => fetch_id,
                    "ts" => &ts_str,
                    "ws" => pt.wind_speed_kn,
                    "wd" => pt.wind_direction_deg,
                    "wg" => pt.wind_gust_kn,
                    "wh" => pt.wave_height_m,
                    "wp" => pt.wave_period_s,
                    "wdir" => pt.wave_direction_deg,
                    "cape" => pt.cape_j_kg,
                },
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Return the most recent forecast for the nearest location within 1 NM.
    pub fn fetch_forecast_data(&self, lat: f64, lon: f64) -> Result<Option<ForecastData>, AppError> {
        let mut conn = self.pool.get_conn()?;

        // Find the most recent fetch within ~1NM (≈0.0167°) using a bounding box,
        // then sort by fetched_at DESC to get the latest.
        let delta = 0.02_f64; // slightly over 1NM — exact haversine check done in Rust
        let rows: Vec<mysql::Row> = conn.exec(
            "SELECT id, lat, lon,
                    DATE_FORMAT(fetched_at, '%Y-%m-%dT%H:%i:%SZ') as fetched_at
             FROM forecast_fetch
             WHERE lat BETWEEN :lat_min AND :lat_max
               AND lon BETWEEN :lon_min AND :lon_max
             ORDER BY fetched_at DESC
             LIMIT 20",
            params! {
                "lat_min" => lat - delta, "lat_max" => lat + delta,
                "lon_min" => lon - delta, "lon_max" => lon + delta,
            },
        )?;

        use crate::utilities::haversine_distance_nm;
        let nearest = rows.iter().find(|row| {
            let rlat = parse_decimal(row, "lat").unwrap_or(f64::MAX);
            let rlon = parse_decimal(row, "lon").unwrap_or(f64::MAX);
            haversine_distance_nm(lat, lon, rlat, rlon) <= 1.0
        });

        let Some(fetch_row) = nearest else { return Ok(None); };

        let fetch_id: u32 = fetch_row.get("id").ok_or_else(|| AppError::Database("Missing id".into()))?;
        let fetch_lat = parse_decimal(fetch_row, "lat")?;
        let fetch_lon = parse_decimal(fetch_row, "lon")?;
        let fetched_at: String = fetch_row.get("fetched_at").unwrap_or_default();

        let hourly = self.load_hourly(&mut conn, fetch_id)?;

        Ok(Some(ForecastData { fetch_id, lat: fetch_lat, lon: fetch_lon, fetched_at, hourly }))
    }

    /// Load all data needed by `compute_trip_overlay` in `src/forecast.rs`.
    pub fn fetch_trip_forecast_inputs(&self, trip_id: u32) -> Result<Option<TripForecastInputs>, AppError> {
        let mut conn = self.pool.get_conn()?;

        // 1. Get trip timestamps
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

        // 2. Get track points
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

        // 3. Get all forecast_fetch records predating trip start
        let fetch_rows: Vec<mysql::Row> = conn.exec(
            "SELECT id, lat, lon FROM forecast_fetch WHERE fetched_at < :trip_start",
            params! { "trip_start" => start_str },
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

/// Parse a DECIMAL column (stored as Bytes in mysql crate) into f64.
fn parse_decimal(row: &mysql::Row, col: &str) -> Result<f64, AppError> {
    match row.get_opt::<f64, _>(col) {
        Some(Ok(v)) => Ok(v),
        Some(Err(_)) => {
            // Fallback: DECIMAL may come back as Bytes
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
    // Try RFC3339 first ("2026-05-06T00:00:00Z")
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc).format("%Y-%m-%d %H:%M:%S").to_string());
    }
    // Fall back to Open-Meteo format ("2026-05-06T00:00")
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M")
        .map(|ndt| {
            DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .map_err(|e| AppError::Parse(format!("Cannot parse timestamp '{}': {}", s, e)))
}
```

- [ ] **Step 5: Update `reset_test_db` to truncate forecast tables**

In `src/db/test_helpers.rs`, in the `reset_test_db` function, add these lines after the existing TRUNCATE statements (before the `SET FOREIGN_KEY_CHECKS = 1`):

```rust
    conn.query_drop("TRUNCATE TABLE forecast_hourly")?;
    conn.query_drop("TRUNCATE TABLE forecast_fetch")?;
    conn.query_drop("TRUNCATE TABLE forecast_poi")?;
```

- [ ] **Step 6: Run the failing tests**

```bash
cargo test -- --test-threads=1 --include-ignored db::operations::forecast::tests
```

Expected: tests compile and run; all three pass.

- [ ] **Step 7: Commit**

```bash
git add src/db/operations/forecast.rs src/db/operations/mod.rs src/db/test_helpers.rs
git commit -m "feat: add forecast DB operations (POI CRUD, insert, query)"
```

---

## Task 3: Fetch Module — Open-Meteo HTTP and IDW Interpolation

**Files:**
- Create: `src/forecast.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add `mod forecast;` to main.rs**

In `src/main.rs`, add after the other `mod` declarations (e.g., after `mod utilities;`):

```rust
pub mod forecast;
```

- [ ] **Step 2: Write failing unit tests for IDW and overlay**

Create `src/forecast.rs` with types, stubs, and tests:

```rust
use crate::db::operations::forecast::{FetchWithHourly, ForecastHourlyPoint, TripForecastInputs};
use crate::error::AppError;
use crate::utilities::haversine_distance_nm;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

// ── Public API types ──────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct FetchedForecast {
    pub lat: f64,
    pub lon: f64,
    pub fetched_at: DateTime<Utc>,
    pub hourly: Vec<ForecastHourlyPoint>,
}

#[derive(Debug, serde::Serialize)]
pub struct FetchPoiResult {
    pub lat: f64,
    pub lon: f64,
    pub status: String,
}

#[derive(Debug, serde::Serialize, Clone)]
pub struct TripOverlayPoint {
    pub timestamp: String,
    pub wind_speed_kn: Option<f64>,
    pub wind_direction_deg: Option<f64>,
    pub wind_gust_kn: Option<f64>,
    pub wave_height_m: Option<f64>,
    pub wave_period_s: Option<f64>,
    pub wave_direction_deg: Option<f64>,
    pub cape_j_kg: Option<f64>,
}

// ── Open-Meteo deserialisation types (private) ────────────────────────────────

#[derive(Debug, Deserialize)]
struct MeteoHourly {
    time: Vec<String>,
    wind_speed_10m: Option<Vec<Option<f64>>>,
    wind_direction_10m: Option<Vec<Option<f64>>>,
    wind_gusts_10m: Option<Vec<Option<f64>>>,
    cape: Option<Vec<Option<f64>>>,
}

#[derive(Debug, Deserialize)]
struct MeteoResponse {
    latitude: f64,
    longitude: f64,
    hourly: MeteoHourly,
}

#[derive(Debug, Deserialize)]
struct MarineHourly {
    time: Vec<String>,
    wave_height: Option<Vec<Option<f64>>>,
    wave_period: Option<Vec<Option<f64>>>,
    wave_direction: Option<Vec<Option<f64>>>,
}

#[derive(Debug, Deserialize)]
struct MarineResponse {
    latitude: f64,
    longitude: f64,
    hourly: MarineHourly,
}

/// Handles both single-object and array responses from Open-Meteo.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

impl<T> OneOrMany<T> {
    fn into_vec(self) -> Vec<T> {
        match self {
            Self::One(v) => vec![v],
            Self::Many(v) => v,
        }
    }
}

// ── Public functions (stubs to be filled in) ──────────────────────────────────

pub async fn fetch_from_open_meteo(
    _coords: &[(f64, f64)],
) -> Result<Vec<FetchedForecast>, AppError> {
    unimplemented!()
}

pub fn compute_trip_overlay(inputs: &TripForecastInputs) -> Vec<TripOverlayPoint> {
    unimplemented!()
}

fn interpolate_idw(
    target_lat: f64,
    target_lon: f64,
    samples: &[(f64, f64, ForecastHourlyPoint)],
) -> Option<ForecastHourlyPoint> {
    unimplemented!()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(wind_speed: f64, wind_dir: f64, gust: f64, wave_h: f64, wave_p: f64, wave_dir: f64, cape: f64) -> ForecastHourlyPoint {
        ForecastHourlyPoint {
            timestamp: "2026-05-06T06:00:00Z".to_string(),
            wind_speed_kn: Some(wind_speed),
            wind_direction_deg: Some(wind_dir),
            wind_gust_kn: Some(gust),
            wave_height_m: Some(wave_h),
            wave_period_s: Some(wave_p),
            wave_direction_deg: Some(wave_dir),
            cape_j_kg: Some(cape),
        }
    }

    #[test]
    fn test_idw_no_samples_returns_none() {
        let result = interpolate_idw(43.0, 9.0, &[]);
        assert!(result.is_none());
    }

    #[test]
    fn test_idw_beyond_25nm_returns_none() {
        // ~50NM away from target
        let sample = (43.0 + 0.9, 9.0, pt(10.0, 180.0, 14.0, 1.0, 7.0, 185.0, 0.0));
        let result = interpolate_idw(43.0, 9.0, &[sample]);
        assert!(result.is_none());
    }

    #[test]
    fn test_idw_single_source_returns_its_values() {
        // ~5NM away
        let sample = (43.0 + 0.08, 9.0, pt(12.0, 180.0, 16.0, 1.5, 7.0, 190.0, 100.0));
        let result = interpolate_idw(43.0, 9.0, &[sample]).unwrap();
        assert!((result.wind_speed_kn.unwrap() - 12.0).abs() < 0.01);
        assert!((result.wind_direction_deg.unwrap() - 180.0).abs() < 0.01);
    }

    #[test]
    fn test_idw_two_equidistant_sources_averages_scalars() {
        // Both ~8NM away on opposite sides of target latitude
        let s1 = (43.0 + 0.13, 9.0, pt(10.0, 0.0, 14.0, 1.0, 6.0, 0.0, 0.0));
        let s2 = (43.0 - 0.13, 9.0, pt(20.0, 0.0, 26.0, 3.0, 8.0, 0.0, 0.0));
        let result = interpolate_idw(43.0, 9.0, &[s1, s2]).unwrap();
        let ws = result.wind_speed_kn.unwrap();
        assert!((ws - 15.0).abs() < 0.5, "Expected ~15kn, got {}", ws);
    }

    #[test]
    fn test_idw_angular_wraparound_at_north() {
        // Two sources: 350° and 10° equidistant → result should be near 0° not 180°
        let s1 = (43.0 + 0.13, 9.0, pt(10.0, 350.0, 14.0, 1.0, 6.0, 350.0, 0.0));
        let s2 = (43.0 - 0.13, 9.0, pt(10.0,  10.0, 14.0, 1.0, 6.0,  10.0, 0.0));
        let result = interpolate_idw(43.0, 9.0, &[s1, s2]).unwrap();
        let wd = result.wind_direction_deg.unwrap();
        // Result should be ~0° (or 360°), definitely not ~180°
        assert!(wd < 20.0 || wd > 340.0, "Expected ~0°, got {}°", wd);
    }
}
```

- [ ] **Step 3: Run tests to confirm failures**

```bash
cargo test forecast::tests
```

Expected: compile error on `unimplemented!()` panic / missing function bodies.

- [ ] **Step 4: Implement `interpolate_idw`**

Replace the `interpolate_idw` stub:

```rust
const MAX_DISTANCE_NM: f64 = 25.0;

fn interpolate_idw(
    target_lat: f64,
    target_lon: f64,
    samples: &[(f64, f64, ForecastHourlyPoint)],
) -> Option<ForecastHourlyPoint> {
    let within: Vec<(f64, &ForecastHourlyPoint)> = samples
        .iter()
        .filter_map(|(lat, lon, pt)| {
            let d = haversine_distance_nm(target_lat, target_lon, *lat, *lon);
            if d <= MAX_DISTANCE_NM { Some((d, pt)) } else { None }
        })
        .collect();

    if within.is_empty() {
        return None;
    }

    // If the boat is essentially at a forecast point, return it directly.
    if let Some((_, pt)) = within.iter().find(|(d, _)| *d < 0.01) {
        return Some((*pt).clone());
    }

    let weights: Vec<f64> = within.iter().map(|(d, _)| 1.0 / (d * d)).collect();
    let w_total: f64 = weights.iter().sum();

    let scalar_idw = |get: &dyn Fn(&ForecastHourlyPoint) -> Option<f64>| -> Option<f64> {
        let pairs: Vec<(f64, f64)> = within
            .iter()
            .zip(&weights)
            .filter_map(|((_, pt), w)| get(pt).map(|v| (v, *w)))
            .collect();
        if pairs.is_empty() { return None; }
        let sum_w: f64 = pairs.iter().map(|(_, w)| w).sum();
        Some(pairs.iter().map(|(v, w)| v * w).sum::<f64>() / sum_w)
    };

    let angular_idw = |get: &dyn Fn(&ForecastHourlyPoint) -> Option<f64>| -> Option<f64> {
        let pairs: Vec<(f64, f64)> = within
            .iter()
            .zip(&weights)
            .filter_map(|((_, pt), w)| get(pt).map(|deg| (deg.to_radians(), *w)))
            .collect();
        if pairs.is_empty() { return None; }
        let sin_sum: f64 = pairs.iter().map(|(r, w)| r.sin() * w).sum();
        let cos_sum: f64 = pairs.iter().map(|(r, w)| r.cos() * w).sum();
        let deg = sin_sum.atan2(cos_sum).to_degrees();
        Some(if deg < 0.0 { deg + 360.0 } else { deg })
    };

    Some(ForecastHourlyPoint {
        timestamp: within[0].1.timestamp.clone(),
        wind_speed_kn: scalar_idw(&|p| p.wind_speed_kn),
        wind_direction_deg: angular_idw(&|p| p.wind_direction_deg),
        wind_gust_kn: scalar_idw(&|p| p.wind_gust_kn),
        wave_height_m: scalar_idw(&|p| p.wave_height_m),
        wave_period_s: scalar_idw(&|p| p.wave_period_s),
        wave_direction_deg: angular_idw(&|p| p.wave_direction_deg),
        cape_j_kg: scalar_idw(&|p| p.cape_j_kg),
    })
}
```

- [ ] **Step 5: Run IDW tests**

```bash
cargo test forecast::tests
```

Expected: all 4 tests pass.

- [ ] **Step 6: Implement `compute_trip_overlay`**

Replace the `compute_trip_overlay` stub:

```rust
pub fn compute_trip_overlay(inputs: &TripForecastInputs) -> Vec<TripOverlayPoint> {
    let mut result = Vec::new();
    let mut hour = inputs.trip_start;

    while hour <= inputs.trip_end {
        let boat_pos = nearest_track_pos(&inputs.track, hour);

        if let Some((boat_lat, boat_lon)) = boat_pos {
            let samples: Vec<(f64, f64, ForecastHourlyPoint)> = inputs
                .fetches
                .iter()
                .filter_map(|fetch| {
                    nearest_hourly(&fetch.hourly, hour)
                        .map(|pt| (fetch.lat, fetch.lon, pt))
                })
                .collect();

            let interp = interpolate_idw(boat_lat, boat_lon, &samples);
            result.push(TripOverlayPoint {
                timestamp: hour.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                wind_speed_kn: interp.as_ref().and_then(|p| p.wind_speed_kn),
                wind_direction_deg: interp.as_ref().and_then(|p| p.wind_direction_deg),
                wind_gust_kn: interp.as_ref().and_then(|p| p.wind_gust_kn),
                wave_height_m: interp.as_ref().and_then(|p| p.wave_height_m),
                wave_period_s: interp.as_ref().and_then(|p| p.wave_period_s),
                wave_direction_deg: interp.as_ref().and_then(|p| p.wave_direction_deg),
                cape_j_kg: interp.as_ref().and_then(|p| p.cape_j_kg),
            });
        }

        hour = hour + Duration::hours(1);
    }

    result
}

fn nearest_track_pos(
    track: &[(f64, f64, DateTime<Utc>)],
    ts: DateTime<Utc>,
) -> Option<(f64, f64)> {
    track
        .iter()
        .min_by_key(|(_, _, t)| (*t - ts).num_seconds().unsigned_abs())
        .filter(|(_, _, t)| (*t - ts).num_seconds().abs() < 7200)
        .map(|(lat, lon, _)| (*lat, *lon))
}

fn nearest_hourly(hourly: &[ForecastHourlyPoint], ts: DateTime<Utc>) -> Option<ForecastHourlyPoint> {
    hourly
        .iter()
        .min_by_key(|p| {
            DateTime::parse_from_rfc3339(&p.timestamp)
                .map(|t| (t.with_timezone(&Utc) - ts).num_seconds().unsigned_abs())
                .unwrap_or(u64::MAX)
        })
        .filter(|p| {
            DateTime::parse_from_rfc3339(&p.timestamp)
                .map(|t| (t.with_timezone(&Utc) - ts).num_seconds().abs() < 7200)
                .unwrap_or(false)
        })
        .cloned()
}
```

- [ ] **Step 7: Implement `fetch_from_open_meteo`**

Replace the `fetch_from_open_meteo` stub:

```rust
pub async fn fetch_from_open_meteo(
    coords: &[(f64, f64)],
) -> Result<Vec<FetchedForecast>, AppError> {
    if coords.is_empty() {
        return Ok(vec![]);
    }

    let lats: String = coords.iter().map(|(lat, _)| lat.to_string()).collect::<Vec<_>>().join(",");
    let lons: String = coords.iter().map(|(_, lon)| lon.to_string()).collect::<Vec<_>>().join(",");

    let client = reqwest::Client::new();
    let fetched_at = Utc::now();

    // Forecast (wind + CAPE)
    let meteo_url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&models=ecmwf_ifs&hourly=wind_speed_10m,wind_direction_10m,wind_gusts_10m,cape&wind_speed_unit=kn&forecast_days=7&timezone=UTC",
        lats, lons
    );
    let meteo_raw: serde_json::Value = client
        .get(&meteo_url)
        .send()
        .await
        .map_err(|e| AppError::Io(format!("Open-Meteo forecast request failed: {}", e)))?
        .json()
        .await
        .map_err(|e| AppError::Parse(format!("Open-Meteo forecast parse failed: {}", e)))?;
    let meteo_responses: Vec<MeteoResponse> = serde_json::from_value::<OneOrMany<MeteoResponse>>(meteo_raw)
        .map_err(|e| AppError::Parse(e.to_string()))?
        .into_vec();

    // Marine (waves)
    let marine_url = format!(
        "https://marine-api.open-meteo.com/v1/marine?latitude={}&longitude={}&models=ecmwf_wam&hourly=wave_height,wave_period,wave_direction&forecast_days=7&timezone=UTC",
        lats, lons
    );
    let marine_raw: serde_json::Value = client
        .get(&marine_url)
        .send()
        .await
        .map_err(|e| AppError::Io(format!("Open-Meteo marine request failed: {}", e)))?
        .json()
        .await
        .map_err(|e| AppError::Parse(format!("Open-Meteo marine parse failed: {}", e)))?;
    let marine_responses: Vec<MarineResponse> = serde_json::from_value::<OneOrMany<MarineResponse>>(marine_raw)
        .map_err(|e| AppError::Parse(e.to_string()))?
        .into_vec();

    // Merge by index
    let mut results = Vec::new();
    for (i, (coord_lat, coord_lon)) in coords.iter().enumerate() {
        let Some(meteo) = meteo_responses.get(i) else { continue };
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

        let _ = (*coord_lat, *coord_lon); // suppress unused warning
    }

    Ok(results)
}
```

- [ ] **Step 8: Build to confirm no compile errors**

```bash
cargo build 2>&1 | head -30
```

Expected: no errors (warnings may appear).

- [ ] **Step 9: Run all forecast tests**

```bash
cargo test forecast::tests
```

Expected: all 4 IDW tests pass.

- [ ] **Step 10: Commit**

```bash
git add src/forecast.rs src/main.rs
git commit -m "feat: add Open-Meteo fetch module and IDW interpolation"
```

---

## Task 4: API Endpoints — 6 Handlers and Route Registration

**Files:**
- Modify: `src/web/api.rs`

- [ ] **Step 1: Add new query/body types near the top of api.rs**

After the existing `#[derive(Debug, Deserialize)]` query structs (around line 100), add:

```rust
#[derive(Debug, Deserialize)]
pub struct ForecastPoiIdQuery {
    pub id: u32,
}

#[derive(Debug, Deserialize)]
pub struct ForecastDataQuery {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Deserialize)]
pub struct ForecastTripOverlayQuery {
    pub trip_id: u32,
}

#[derive(Debug, Deserialize)]
pub struct FetchForecastBody {
    pub poi_ids: Vec<u32>,
}
```

- [ ] **Step 2: Add the 6 handler functions**

Add these after the last existing handler function (before `create_api_router`):

```rust
pub async fn get_forecast_pois(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<crate::db::operations::forecast::ForecastPoi>>>, StatusCode> {
    match state.db().list_forecast_pois() {
        Ok(pois) => Ok(Json(ApiResponse::ok(pois))),
        Err(e) => {
            error!(error = %e, "Failed to list forecast POIs");
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

pub async fn create_forecast_poi(
    State(state): State<AppState>,
    Json(body): Json<crate::db::operations::forecast::NewForecastPoi>,
) -> Result<Json<ApiResponse<u32>>, StatusCode> {
    match state.db().create_forecast_poi(&body.name, body.lat, body.lon) {
        Ok(id) => Ok(Json(ApiResponse::ok(id))),
        Err(e) => {
            error!(error = %e, "Failed to create forecast POI");
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

pub async fn delete_forecast_poi(
    State(state): State<AppState>,
    Query(params): Query<ForecastPoiIdQuery>,
) -> Result<Json<ApiResponse<String>>, StatusCode> {
    match state.db().delete_forecast_poi(params.id) {
        Ok(_) => Ok(Json(ApiResponse::ok("deleted".to_string()))),
        Err(e) => {
            error!(error = %e, poi_id = params.id, "Failed to delete forecast POI");
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

pub async fn post_forecast_fetch(
    State(state): State<AppState>,
    Json(body): Json<FetchForecastBody>,
) -> Result<Json<ApiResponse<Vec<crate::forecast::FetchPoiResult>>>, StatusCode> {
    // Resolve POI IDs → coordinates
    let all_pois = match state.db().list_forecast_pois() {
        Ok(p) => p,
        Err(e) => return Ok(Json(ApiResponse::error(e.to_string()))),
    };
    let coords: Vec<(f64, f64)> = all_pois
        .iter()
        .filter(|p| body.poi_ids.contains(&p.id))
        .map(|p| (p.lat, p.lon))
        .collect();

    if coords.is_empty() {
        return Ok(Json(ApiResponse::error("No matching POIs found".to_string())));
    }

    let fetched = match crate::forecast::fetch_from_open_meteo(&coords).await {
        Ok(f) => f,
        Err(e) => return Ok(Json(ApiResponse::error(e.to_string()))),
    };

    let mut results = Vec::new();
    for forecast in fetched {
        let lat = forecast.lat;
        let lon = forecast.lon;
        let status = match state.db().insert_forecast(lat, lon, forecast.fetched_at, &forecast.hourly) {
            Ok(_) => "ok".to_string(),
            Err(e) => format!("error: {}", e),
        };
        results.push(crate::forecast::FetchPoiResult { lat, lon, status });
    }

    Ok(Json(ApiResponse::ok(results)))
}

pub async fn get_forecast_data(
    State(state): State<AppState>,
    Query(params): Query<ForecastDataQuery>,
) -> Result<Json<ApiResponse<Option<crate::db::operations::forecast::ForecastData>>>, StatusCode> {
    match state.db().fetch_forecast_data(params.lat, params.lon) {
        Ok(data) => Ok(Json(ApiResponse::ok(data))),
        Err(e) => {
            error!(error = %e, "Failed to fetch forecast data");
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

pub async fn get_forecast_trip_overlay(
    State(state): State<AppState>,
    Query(params): Query<ForecastTripOverlayQuery>,
) -> Result<Json<ApiResponse<Vec<crate::forecast::TripOverlayPoint>>>, StatusCode> {
    let inputs = match state.db().fetch_trip_forecast_inputs(params.trip_id) {
        Ok(Some(i)) => i,
        Ok(None) => return Ok(Json(ApiResponse::ok(vec![]))),
        Err(e) => {
            error!(error = %e, trip_id = params.trip_id, "Failed to fetch trip forecast inputs");
            return Ok(Json(ApiResponse::error(e.to_string())));
        }
    };
    let overlay = crate::forecast::compute_trip_overlay(&inputs);
    Ok(Json(ApiResponse::ok(overlay)))
}
```

- [ ] **Step 3: Register the 6 routes in `create_api_router`**

In `create_api_router`, add to the read-only block (the main `Router::new()` chain, before `if !read_only`):

```rust
        .route("/forecast/pois", get(get_forecast_pois))
        .route("/forecast/data", get(get_forecast_data))
        .route("/forecast/trip-overlay", get(get_forecast_trip_overlay))
```

And add to the `if !read_only` block (alongside the other write routes):

```rust
            .route("/forecast/pois", post(create_forecast_poi))
            .route("/forecast/pois", delete(delete_forecast_poi))
            .route("/forecast/fetch", post(post_forecast_fetch))
```

- [ ] **Step 4: Build and check for errors**

```bash
cargo build 2>&1 | head -30
```

Expected: builds cleanly.

- [ ] **Step 5: Commit**

```bash
git add src/web/api.rs
git commit -m "feat: add 6 forecast API endpoints"
```

---

## Task 5: meteo.html — Dedicated Weather Page

**Files:**
- Create: `static/meteo.html`

- [ ] **Step 1: Create meteo.html**

Create `static/meteo.html` with the full page. Follow project UI conventions: 1500px wide, `shared-theme.js`, `shared.css`, `header-bar`, `level-1-container`. Use Leaflet (CDN link, same as in trip.html) and Chart.js (already at `/libs/chart.min.js`).

Check the Leaflet CDN link used in trip.html:
```bash
grep -i "leaflet" /home/aboni/dev/rust_nmea_router/static/trip.html | head -5
```

Then create `static/meteo.html`:

```html
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Meteo - NMEA Router</title>
    <link rel="icon" type="image/png" href="/images/nmeasail.png">
    <link rel="stylesheet" href="/shared.css">
    <!-- Replace with the exact Leaflet link found in trip.html -->
    <link rel="stylesheet" href="https://unpkg.com/leaflet@1.9.4/dist/leaflet.css" />
    <script src="/js/shared-theme.js"></script>
    <script src="/libs/chart.min.js"></script>
    <script src="/libs/chartjs-adapter-date-fns.bundle.min.js"></script>
    <script src="https://unpkg.com/leaflet@1.9.4/dist/leaflet.js"></script>
    <style>
        #poiMap { height: 300px; width: 100%; border-radius: 6px; }
        .poi-table { width: 100%; border-collapse: collapse; }
        .poi-table th, .poi-table td { padding: 8px 12px; text-align: left; border-bottom: 1px solid var(--border-color); font-size: 13px; }
        .poi-table th { font-weight: bold; color: var(--text-bold); }
        .fetch-log { font-size: 12px; color: var(--text-secondary); margin-top: 8px; max-height: 120px; overflow-y: auto; }
        .fetch-log-entry { padding: 2px 0; }
        .fetch-log-entry.ok { color: var(--link-color); }
        .fetch-log-entry.error { color: #e74c3c; }
        .chart-panel { margin-top: 20px; }
        .chart-panel canvas { height: 180px !important; }
        .cape-bar { display: flex; height: 24px; border-radius: 4px; overflow: hidden; margin-top: 4px; }
        .cape-bar-segment { flex: 1; }
    </style>
</head>
<body>
<div class="header-bar">
    <div class="header-left">
        <img id="brandLogo" src="/images/nmeasail.png" alt="Logo" class="brand-logo">
        <span class="header-title">Meteo Forecast</span>
    </div>
    <div class="header-right">
        <a href="/" class="nav-link">Dashboard</a>
        <button id="themeBtn" class="theme-toggle" onclick="toggleTheme()">🌙</button>
    </div>
</div>

<div style="max-width:1500px; margin:0 auto; padding:20px;">

    <!-- ── POI Manager ── -->
    <div class="level-1-container" style="margin-bottom:20px;">
        <h2 style="font-size:16px; font-weight:bold; margin-bottom:15px; color:var(--text-bold);">Points of Interest</h2>
        <div style="display:grid; grid-template-columns:1fr 1fr; gap:20px;">
            <div>
                <table class="poi-table">
                    <thead><tr><th>Name</th><th>Lat</th><th>Lon</th><th></th></tr></thead>
                    <tbody id="poiTableBody"></tbody>
                </table>
                <div style="margin-top:16px;">
                    <strong style="font-size:13px;">Add POI</strong>
                    <div style="display:flex; gap:8px; margin-top:8px; align-items:center; flex-wrap:wrap;">
                        <input id="poiName" type="text" placeholder="Name" style="flex:2; min-width:100px; padding:6px; border:1px solid var(--border-color); border-radius:4px; background:var(--bg-primary); color:var(--text-primary);">
                        <input id="poiLat" type="number" step="0.0001" placeholder="Lat" style="flex:1; min-width:80px; padding:6px; border:1px solid var(--border-color); border-radius:4px; background:var(--bg-primary); color:var(--text-primary);">
                        <input id="poiLon" type="number" step="0.0001" placeholder="Lon" style="flex:1; min-width:80px; padding:6px; border:1px solid var(--border-color); border-radius:4px; background:var(--bg-primary); color:var(--text-primary);">
                        <button onclick="addPoi()" style="padding:6px 14px; background:var(--link-color); color:#fff; border:none; border-radius:4px; cursor:pointer;">Add</button>
                    </div>
                    <div style="font-size:12px; color:var(--text-secondary); margin-top:6px;">Click the map to set coordinates.</div>
                </div>
            </div>
            <div id="poiMap"></div>
        </div>
    </div>

    <!-- ── Fetch Panel ── -->
    <div class="level-1-container" style="margin-bottom:20px;">
        <h2 style="font-size:16px; font-weight:bold; margin-bottom:15px; color:var(--text-bold);">Fetch Forecast</h2>
        <div id="fetchPoiList" style="margin-bottom:12px;"></div>
        <button onclick="fetchForecast()" style="padding:8px 20px; background:var(--link-color); color:#fff; border:none; border-radius:4px; cursor:pointer; font-size:14px;">Fetch Forecast (7 days)</button>
        <div class="fetch-log" id="fetchLog"></div>
    </div>

    <!-- ── Forecast Viewer ── -->
    <div class="level-1-container">
        <h2 style="font-size:16px; font-weight:bold; margin-bottom:15px; color:var(--text-bold);">Forecast Viewer</h2>
        <div style="margin-bottom:12px;">
            <label style="font-size:13px; color:var(--text-secondary);">Select POI: </label>
            <select id="viewerPoiSelect" onchange="loadForecastViewer()" style="padding:6px; border:1px solid var(--border-color); border-radius:4px; background:var(--bg-primary); color:var(--text-primary);">
                <option value="">— choose —</option>
            </select>
        </div>
        <div id="forecastViewer" style="display:none;">
            <div class="chart-panel">
                <div style="font-size:13px; font-weight:bold; color:var(--text-bold); margin-bottom:4px;">Wind Speed & Gusts (knots)</div>
                <canvas id="windSpeedForecastChart"></canvas>
            </div>
            <div class="chart-panel">
                <div style="font-size:13px; font-weight:bold; color:var(--text-bold); margin-bottom:4px;">Wind Direction (°)</div>
                <canvas id="windDirForecastChart"></canvas>
            </div>
            <div class="chart-panel">
                <div style="font-size:13px; font-weight:bold; color:var(--text-bold); margin-bottom:4px;">Wave Height & Period</div>
                <canvas id="waveForecastChart"></canvas>
            </div>
            <div class="chart-panel">
                <div style="font-size:13px; font-weight:bold; color:var(--text-bold); margin-bottom:4px;">CAPE (J/kg) — Thunderstorm Risk</div>
                <canvas id="capeForecastChart"></canvas>
            </div>
        </div>
    </div>
</div>

<script>
    let pois = [];
    let poiMap = null;
    let poiMarker = null;
    let forecastCharts = {};

    async function apiFetch(url, opts = {}) {
        const r = await fetch(url, opts);
        return r.json();
    }

    async function loadPois() {
        const resp = await apiFetch('/api/forecast/pois');
        pois = resp.data || [];
        renderPoiTable();
        renderFetchPoiList();
        renderViewerSelect();
    }

    function renderPoiTable() {
        const tbody = document.getElementById('poiTableBody');
        tbody.innerHTML = pois.map(p => `
            <tr>
                <td>${p.name}</td>
                <td>${p.lat.toFixed(4)}</td>
                <td>${p.lon.toFixed(4)}</td>
                <td><button onclick="deletePoi(${p.id})" style="padding:2px 8px; font-size:12px; cursor:pointer; background:transparent; border:1px solid #e74c3c; border-radius:3px; color:#e74c3c;">Delete</button></td>
            </tr>`).join('');
    }

    function renderFetchPoiList() {
        document.getElementById('fetchPoiList').innerHTML = pois.map(p => `
            <label style="display:inline-flex; align-items:center; gap:6px; margin-right:16px; font-size:13px;">
                <input type="checkbox" class="fetchPoiCheck" value="${p.id}" checked> ${p.name}
            </label>`).join('');
    }

    function renderViewerSelect() {
        const sel = document.getElementById('viewerPoiSelect');
        const prev = sel.value;
        sel.innerHTML = '<option value="">— choose —</option>' +
            pois.map(p => `<option value="${p.id}" data-lat="${p.lat}" data-lon="${p.lon}">${p.name}</option>`).join('');
        if (prev) sel.value = prev;
    }

    async function addPoi() {
        const name = document.getElementById('poiName').value.trim();
        const lat = parseFloat(document.getElementById('poiLat').value);
        const lon = parseFloat(document.getElementById('poiLon').value);
        if (!name || isNaN(lat) || isNaN(lon)) { alert('Name, lat, and lon are required.'); return; }
        await apiFetch('/api/forecast/pois', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ name, lat, lon }),
        });
        document.getElementById('poiName').value = '';
        await loadPois();
    }

    async function deletePoi(id) {
        await apiFetch(`/api/forecast/pois?id=${id}`, { method: 'DELETE' });
        await loadPois();
    }

    async function fetchForecast() {
        const checked = [...document.querySelectorAll('.fetchPoiCheck:checked')].map(el => parseInt(el.value));
        if (!checked.length) { alert('Select at least one POI.'); return; }
        const log = document.getElementById('fetchLog');
        log.innerHTML = '<div class="fetch-log-entry">Fetching…</div>';
        const resp = await apiFetch('/api/forecast/fetch', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ poi_ids: checked }),
        });
        if (resp.status === 'ok') {
            log.innerHTML = (resp.data || []).map(r => `
                <div class="fetch-log-entry ${r.status === 'ok' ? 'ok' : 'error'}">
                    (${r.lat.toFixed(3)}, ${r.lon.toFixed(3)}): ${r.status}
                </div>`).join('');
        } else {
            log.innerHTML = `<div class="fetch-log-entry error">${resp.error}</div>`;
        }
    }

    async function loadForecastViewer() {
        const sel = document.getElementById('viewerPoiSelect');
        const opt = sel.selectedOptions[0];
        if (!opt || !opt.value) { document.getElementById('forecastViewer').style.display = 'none'; return; }
        const lat = parseFloat(opt.dataset.lat);
        const lon = parseFloat(opt.dataset.lon);
        const resp = await apiFetch(`/api/forecast/data?lat=${lat}&lon=${lon}`);
        if (resp.status !== 'ok' || !resp.data) {
            document.getElementById('forecastViewer').style.display = 'none';
            return;
        }
        const hourly = resp.data.hourly;
        document.getElementById('forecastViewer').style.display = 'block';
        renderForecastCharts(hourly);
    }

    function renderForecastCharts(hourly) {
        Object.values(forecastCharts).forEach(c => c.destroy());
        forecastCharts = {};

        const times = hourly.map(p => new Date(p.timestamp));
        const colors = window.getChartColors ? getChartColors() : { text: '#333', grid: '#eee' };

        const timeOpts = {
            type: 'time',
            time: { unit: 'day', displayFormats: { day: 'MMM d' } },
            ticks: { color: colors.text },
            grid: { color: colors.grid },
        };

        // Wind speed + gusts
        forecastCharts.wind = new Chart(document.getElementById('windSpeedForecastChart'), {
            type: 'line',
            data: {
                labels: times,
                datasets: [
                    { label: 'Wind (kn)', data: hourly.map(p => p.wind_speed_kn), borderColor: '#27ae60', backgroundColor: 'rgba(39,174,96,0.1)', fill: true, tension: 0.2, pointRadius: 0, borderWidth: 1.5 },
                    { label: 'Gust (kn)', data: hourly.map(p => p.wind_gust_kn), borderColor: '#e67e22', borderDash: [4, 4], fill: false, tension: 0.2, pointRadius: 0, borderWidth: 1.5 },
                ]
            },
            options: { maintainAspectRatio: false, scales: { x: timeOpts, y: { beginAtZero: true, ticks: { color: colors.text }, grid: { color: colors.grid } } }, plugins: { legend: { labels: { color: colors.text } } } }
        });

        // Wind direction
        forecastCharts.windDir = new Chart(document.getElementById('windDirForecastChart'), {
            type: 'scatter',
            data: { datasets: [{ label: 'Wind Dir (°)', data: times.map((t, i) => ({ x: t, y: hourly[i].wind_direction_deg })), borderColor: '#3498db', pointRadius: 2, showLine: true, tension: 0.1 }] },
            options: { maintainAspectRatio: false, scales: { x: timeOpts, y: { min: 0, max: 360, ticks: { color: colors.text, stepSize: 90 }, grid: { color: colors.grid } } }, plugins: { legend: { display: false } } }
        });

        // Waves
        forecastCharts.wave = new Chart(document.getElementById('waveForecastChart'), {
            type: 'line',
            data: {
                labels: times,
                datasets: [
                    { label: 'Wave Height (m)', data: hourly.map(p => p.wave_height_m), borderColor: '#2980b9', backgroundColor: 'rgba(41,128,185,0.1)', fill: true, tension: 0.2, pointRadius: 0, borderWidth: 1.5, yAxisID: 'y' },
                    { label: 'Period (s)', data: hourly.map(p => p.wave_period_s), borderColor: '#8e44ad', borderDash: [4, 4], fill: false, tension: 0.2, pointRadius: 0, borderWidth: 1.5, yAxisID: 'y2' },
                ]
            },
            options: { maintainAspectRatio: false, scales: { x: timeOpts, y: { beginAtZero: true, ticks: { color: colors.text }, grid: { color: colors.grid } }, y2: { position: 'right', beginAtZero: true, ticks: { color: colors.text }, grid: { drawOnChartArea: false } } }, plugins: { legend: { labels: { color: colors.text } } } }
        });

        // CAPE
        forecastCharts.cape = new Chart(document.getElementById('capeForecastChart'), {
            type: 'bar',
            data: {
                labels: times,
                datasets: [{
                    label: 'CAPE (J/kg)',
                    data: hourly.map(p => p.cape_j_kg),
                    backgroundColor: hourly.map(p => {
                        const v = p.cape_j_kg || 0;
                        if (v >= 1500) return '#e74c3c';
                        if (v >= 500) return '#f39c12';
                        return '#27ae60';
                    }),
                }]
            },
            options: { maintainAspectRatio: false, scales: { x: timeOpts, y: { beginAtZero: true, ticks: { color: colors.text }, grid: { color: colors.grid } } }, plugins: { legend: { display: false } } }
        });
    }

    // Map initialisation
    function initMap() {
        poiMap = L.map('poiMap').setView([43.0, 9.0], 6);
        L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
            attribution: '© OpenStreetMap contributors', maxZoom: 16
        }).addTo(poiMap);
        poiMap.on('click', e => {
            document.getElementById('poiLat').value = e.latlng.lat.toFixed(5);
            document.getElementById('poiLon').value = e.latlng.lng.toFixed(5);
            if (poiMarker) poiMarker.remove();
            poiMarker = L.marker(e.latlng).addTo(poiMap);
        });
    }

    initMap();
    loadPois();
</script>
</body>
</html>
```

- [ ] **Step 2: Start the dev server and verify the page loads**

```bash
cargo run &
```

Open `http://localhost:<port>/meteo.html` in a browser and confirm:
- Three panels render with correct layout
- The Leaflet map loads
- Clicking the map sets lat/lon fields
- "Add POI" creates a POI and it appears in the table
- "Delete" removes it
- The viewer select updates when POIs are added

- [ ] **Step 3: Commit**

```bash
git add static/meteo.html
git commit -m "feat: add meteo.html with POI manager, fetch panel, and 4-chart forecast viewer"
```

---

## Task 6: trip.html — Forecast Overlay

**Files:**
- Modify: `static/trip.html`

- [ ] **Step 1: Add forecast canvas elements after the existing charts container**

In `trip.html`, find the `chartsContainerLeg` div and the section around `initializeChartsLeg`. After the block that renders `renderLegCharts`, add two new canvas elements for wave and CAPE (these are hidden until data is available):

Find the line that calls `initializeChartsTrip` (around line 474) and add two new chart IDs:

```javascript
// In initializeChartsTrip, add:
addChart(chartsContainer, 'waveForecastChart');
addChart(chartsContainer, 'capeForecastChart');
```

- [ ] **Step 2: Load and cache forecast overlay data when a trip is selected**

Find the `renderTripCharts` function (around line 959). Add at the top of that function:

```javascript
async function renderTripCharts(trip, trackData, startTime, endTime, isFullTrip) {
    // --- existing code ---
    // Add this near the top, after existing variable declarations:
    let forecastOverlay = [];
    try {
        const overlayResp = await fetch(`/api/forecast/trip-overlay?trip_id=${trip.id}`);
        const overlayJson = await overlayResp.json();
        if (overlayJson.status === 'ok' && overlayJson.data && overlayJson.data.length > 0) {
            forecastOverlay = overlayJson.data;
        }
    } catch (e) {
        // No forecast data — continue without it
    }
    // ... rest of function
}
```

- [ ] **Step 3: Overlay forecast wind speed and gusts on the existing wind speed chart**

Find `createWindSpeedChart` (around line 1406). After the existing dataset array, add forecast datasets when available. Replace the function to accept an optional `forecastData` parameter:

```javascript
function createWindSpeedChart(trackData, forecastData = []) {
    const ctx = document.getElementById('windSpeedChart').getContext('2d');
    const colors = getChartColors();

    const validData = trackData.filter(p => p.average_wind_speed_kn != null);
    const chartData = validData.map(p => ({ x: new Date(p.timestamp), y: p.average_wind_speed_kn }));

    const datasets = [{
        label: 'Wind Speed (knots)',
        data: chartData,
        borderColor: '#27ae60',
        backgroundColor: 'rgba(39, 174, 96, 0.1)',
        fill: true, tension: 0.1, pointRadius: 0, borderWidth: 1, pointHoverRadius: 5
    }];

    if (forecastData.length > 0) {
        const fcValid = forecastData.filter(p => p.wind_speed_kn != null);
        datasets.push({
            label: 'Forecast Wind (kn)',
            data: fcValid.map(p => ({ x: new Date(p.timestamp), y: p.wind_speed_kn })),
            borderColor: '#27ae60', borderDash: [5, 5], fill: false, tension: 0.2, pointRadius: 0, borderWidth: 1.5
        });
        const gustValid = forecastData.filter(p => p.wind_gust_kn != null);
        datasets.push({
            label: 'Forecast Gust (kn)',
            data: gustValid.map(p => ({ x: new Date(p.timestamp), y: p.wind_gust_kn })),
            borderColor: '#e67e22', borderDash: [3, 3], fill: false, tension: 0.2, pointRadius: 0, borderWidth: 1.5
        });
    }

    return new Chart(ctx, {
        type: 'line',
        data: { datasets },
        options: {
            maintainAspectRatio: false,
            scales: {
                y: { beginAtZero: true, title: { display: true, text: 'Wind Speed (knots)', color: colors.text }, ticks: { color: colors.text }, grid: { color: colors.grid } },
                x: getTimeXScale(colors)
            },
            plugins: {
                legend: { display: forecastData.length > 0, labels: { color: colors.text } },
                title: { display: true, text: 'True Wind Speed (TWS)', color: colors.text }
            }
        }
    });
}
```

Update the call site in `renderLegCharts`:
```javascript
allCharts.windSpeed = createWindSpeedChart(trackData, forecastOverlay);
```

- [ ] **Step 4: Overlay forecast wind direction on the wind direction chart**

Find `createWindDirectionChart` (around line 1464). Add a `forecastData` parameter and a dashed forecast dataset following the same pattern as Step 3:

```javascript
function createWindDirectionChart(trackData, normalized = false, forecastData = []) {
    const ctx = document.getElementById('windDirectionChart').getContext('2d');
    const colors = getChartColors();

    const validData = trackData.filter(p => p.average_wind_angle_deg != null);
    const chartData = validData.map(p => {
        let direction = p.average_wind_angle_deg;
        if (normalized && direction > 180) direction -= 360;
        return { x: new Date(p.timestamp), y: direction };
    });

    const datasets = [{
        label: 'Wind Direction (°)',
        data: chartData,
        borderColor: '#2980b9',
        backgroundColor: 'rgba(41, 128, 185, 0.1)',
        fill: true, tension: 0.1, pointRadius: 0, borderWidth: 1, pointHoverRadius: 5
    }];

    if (forecastData.length > 0) {
        const fcValid = forecastData.filter(p => p.wind_direction_deg != null);
        datasets.push({
            label: 'Forecast Dir (°)',
            data: fcValid.map(p => {
                let d = p.wind_direction_deg;
                if (normalized && d > 180) d -= 360;
                return { x: new Date(p.timestamp), y: d };
            }),
            borderColor: '#2980b9', borderDash: [5, 5], fill: false, tension: 0.2, pointRadius: 0, borderWidth: 1.5
        });
    }

    // ... keep the rest of the existing Chart constructor unchanged, just replace data.datasets with datasets
    // and set plugins.legend.display: forecastData.length > 0
}
```

Update the call site:
```javascript
allCharts.windDirection = createWindDirectionChart(trackData, windDirectionNormalized, forecastOverlay);
```

Also update the call site inside the scale toggle handler (search for the second call to `createWindDirectionChart` around line 1241):
```javascript
allCharts.windDirection = createWindDirectionChart(currentTrackData, windDirectionNormalized, forecastOverlay);
```

- [ ] **Step 5: Add wave forecast and CAPE panels to renderTripCharts**

At the end of `renderTripCharts`, after the existing chart renders, add:

```javascript
    // Wave forecast panel (hidden if no data)
    const waveData = forecastOverlay.filter(p => p.wave_height_m != null);
    if (waveData.length > 0) {
        const waveCtx = document.getElementById('waveForecastChart').getContext('2d');
        allCharts.waveForecast = new Chart(waveCtx, {
            type: 'line',
            data: {
                datasets: [
                    {
                        label: 'Wave Height (m)',
                        data: waveData.map(p => ({ x: new Date(p.timestamp), y: p.wave_height_m })),
                        borderColor: '#2980b9', backgroundColor: 'rgba(41,128,185,0.1)',
                        fill: true, tension: 0.2, pointRadius: 0, borderWidth: 1.5, yAxisID: 'y'
                    },
                    {
                        label: 'Period (s)',
                        data: forecastOverlay.filter(p => p.wave_period_s != null).map(p => ({ x: new Date(p.timestamp), y: p.wave_period_s })),
                        borderColor: '#8e44ad', borderDash: [4, 4],
                        fill: false, tension: 0.2, pointRadius: 0, borderWidth: 1.5, yAxisID: 'y2'
                    },
                ]
            },
            options: {
                maintainAspectRatio: false,
                scales: {
                    x: getTimeXScale(colors),
                    y: { beginAtZero: true, title: { display: true, text: 'Height (m)', color: colors.text }, ticks: { color: colors.text }, grid: { color: colors.grid } },
                    y2: { position: 'right', beginAtZero: true, title: { display: true, text: 'Period (s)', color: colors.text }, ticks: { color: colors.text }, grid: { drawOnChartArea: false } }
                },
                plugins: { legend: { labels: { color: colors.text } }, title: { display: true, text: 'Wave Forecast', color: colors.text } }
            }
        });
        document.getElementById('waveForecastChart').closest('.chart-wrapper').style.display = '';
    } else {
        const el = document.getElementById('waveForecastChart');
        if (el) el.closest('.chart-wrapper').style.display = 'none';
    }

    // CAPE forecast panel (hidden if no data)
    const capeData = forecastOverlay.filter(p => p.cape_j_kg != null);
    if (capeData.length > 0) {
        const capeCtx = document.getElementById('capeForecastChart').getContext('2d');
        allCharts.capeForecast = new Chart(capeCtx, {
            type: 'bar',
            data: {
                datasets: [{
                    label: 'CAPE (J/kg)',
                    data: capeData.map(p => ({ x: new Date(p.timestamp), y: p.cape_j_kg })),
                    backgroundColor: capeData.map(p => {
                        if ((p.cape_j_kg || 0) >= 1500) return '#e74c3c';
                        if ((p.cape_j_kg || 0) >= 500) return '#f39c12';
                        return '#27ae60';
                    }),
                }]
            },
            options: {
                maintainAspectRatio: false,
                scales: {
                    x: getTimeXScale(colors),
                    y: { beginAtZero: true, ticks: { color: colors.text }, grid: { color: colors.grid } }
                },
                plugins: {
                    legend: { display: false },
                    title: { display: true, text: 'CAPE — Thunderstorm Risk (J/kg)', color: colors.text }
                }
            }
        });
        document.getElementById('capeForecastChart').closest('.chart-wrapper').style.display = '';
    } else {
        const el = document.getElementById('capeForecastChart');
        if (el) el.closest('.chart-wrapper').style.display = 'none';
    }
```

- [ ] **Step 6: Test the trip page with a trip that has no forecast data**

Start the dev server, open a trip in `trip.html`, and confirm:
- The wave and CAPE panels are hidden (no data)
- The wind charts render normally without any dashed lines
- No JavaScript errors in the browser console

- [ ] **Step 7: Test with forecast data (manual integration test)**

1. Go to `meteo.html`, add a POI near one of your test trips, fetch forecast
2. Open the trip in `trip.html`
3. Confirm dashed forecast lines appear on wind speed and direction charts
4. Confirm wave height + period chart appears
5. Confirm CAPE bar chart appears

- [ ] **Step 8: Commit**

```bash
git add static/trip.html
git commit -m "feat: add forecast overlay to trip wind charts, wave, and CAPE panels"
```

---

## Self-Review Checklist

- [ ] All 6 spec API endpoints implemented: ✓ pois GET/POST/DELETE, fetch POST, data GET, trip-overlay GET
- [ ] DB schema matches spec (3 tables, correct columns and types): ✓
- [ ] Forecast data has no FK to forecast_poi: ✓ (coordinates stored directly in forecast_fetch)
- [ ] IDW uses angular averaging for direction fields: ✓ (`angular_idw` uses atan2)
- [ ] Wind speed units: knots throughout (Open-Meteo `wind_speed_unit=kn`, DB `wind_speed_kn`): ✓
- [ ] All DB timestamps UTC: ✓ (all DATE_FORMAT with Z suffix, all inserts use UTC)
- [ ] Forecast overlay hidden when no data: ✓ (wave + CAPE panels hidden, no empty states)
- [ ] Trip overlay uses most recent fetch predating trip start: ✓ (`fetched_at < trip_start` in query)
- [ ] IDW 25NM radius for trip overlay, 1NM radius for `/forecast/data`: ✓
- [ ] reqwest already in Cargo.toml: ✓ (v0.12, json + rustls-tls features)
