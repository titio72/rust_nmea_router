// Callers will be wired up in later tasks on this feature branch.
#![allow(dead_code)]

use crate::db::types::VesselDatabase;
use crate::error::AppError;
use chrono::{DateTime, Utc};
use mysql::params;
use mysql::prelude::Queryable;
use serde::{Deserialize, Serialize};

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
pub struct ForecastArea {
    pub id: u32,
    pub lat_min: f64,
    pub lat_max: f64,
    pub lon_min: f64,
    pub lon_max: f64,
    pub created_at: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct GridPointForecast {
    pub lat: f64,
    pub lon: f64,
    pub wind_speed_kn: Option<f64>,
    pub wind_direction_deg: Option<f64>,
    pub wind_gust_kn: Option<f64>,
    pub wave_height_m: Option<f64>,
    pub wave_period_s: Option<f64>,
    pub wave_direction_deg: Option<f64>,
    pub cape_j_kg: Option<f64>,
    pub model: String,
}

#[derive(Debug, Deserialize)]
pub struct NewForecastArea {
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
    pub model: String,
    pub hourly: Vec<ForecastHourlyPoint>,
}

impl VesselDatabase {
    // ── Area CRUD ─────────────────────────────────────────────────────────────

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

    pub fn create_forecast_area(&self, area: &NewForecastArea, created_at: DateTime<Utc>) -> Result<u32, AppError> {
        let mut conn = self.pool.get_conn()?;
        conn.exec_drop(
            "INSERT INTO forecast_area (lat_min, lat_max, lon_min, lon_max, created_at)
             VALUES (:lat_min, :lat_max, :lon_min, :lon_max, :created_at)",
            params! {
                "lat_min" => area.lat_min, "lat_max" => area.lat_max,
                "lon_min" => area.lon_min, "lon_max" => area.lon_max,
                "created_at" => created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            },
        )?;
        Ok(conn.exec_first("SELECT LAST_INSERT_ID()", ())?
            .ok_or_else(|| AppError::Database("No insert ID".into()))?)
    }

    pub fn delete_forecast_area(&self, id: u32) -> Result<bool, AppError> {
        let mut conn = self.pool.get_conn()?;
        conn.exec_drop(
            "DELETE FROM forecast_area WHERE id = :id",
            params! { "id" => id },
        )?;
        Ok(conn.affected_rows() > 0)
    }

    // ── Forecast data ─────────────────────────────────────────────────────────

    pub fn insert_forecast(
        &self,
        area_id: u32,
        model: &str,
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
            "INSERT INTO forecast_fetch (area_id, model, lat, lon, fetched_at, forecast_from, forecast_to)
             VALUES (:area_id, :model, :lat, :lon, :fetched_at, :from, :to)",
            params! {
                "area_id" => area_id,
                "model" => model,
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

    /// Deletes every forecast fetch for `area_id` older than `keep_fetched_at`,
    /// cascading to their hourly rows (FK ON DELETE CASCADE). Called after a
    /// successful refresh so only the most recent fetch per grid point is kept —
    /// storing forecast history is too expensive for the on-board RPi. Returns the
    /// number of fetch rows removed.
    pub fn prune_old_forecasts(
        &self,
        area_id: u32,
        keep_fetched_at: DateTime<Utc>,
    ) -> Result<u64, AppError> {
        let keep = keep_fetched_at.format("%Y-%m-%d %H:%M:%S").to_string();
        let mut conn = self.pool.get_conn()?;
        conn.exec_drop(
            "DELETE FROM forecast_fetch WHERE area_id = :area_id AND fetched_at < :keep",
            params! { "area_id" => area_id, "keep" => &keep },
        )?;
        Ok(conn.affected_rows())
    }

    pub fn get_last_fetch_time(&self) -> Result<Option<DateTime<Utc>>, AppError> {
        let mut conn = self.pool.get_conn()?;
        let row: Option<mysql::Row> = conn.exec_first(
            "SELECT DATE_FORMAT(MAX(fetched_at), '%Y-%m-%dT%H:%i:%SZ') as last_fetch
             FROM forecast_fetch",
            (),
        )?;
        let Some(row) = row else { return Ok(None); };
        let s: Option<String> = row.get::<Option<String>, _>("last_fetch").flatten();
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

    /// Returns the most recent forecast values for every grid point at the given
    /// UTC hour.  `timestamp_iso` is RFC-3339, e.g. "2026-05-14T09:00:00Z".
    /// When AROME data is available for this hour it takes priority; otherwise
    /// ECMWF is returned.
    pub fn get_grid_points_at(
        &self,
        timestamp_iso: &str,
    ) -> Result<Vec<GridPointForecast>, AppError> {
        let ts_db = parse_iso_to_db(timestamp_iso)?;
        let mut conn = self.pool.get_conn()?;
        let rows: Vec<mysql::Row> = conn.exec(
            "SELECT ff.model, ff.lat, ff.lon,
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
                     AND inner_ff.model = ff.model
               )",
            params! { "ts" => &ts_db },
        )?;
        let mut all: Vec<GridPointForecast> = rows.iter()
            .map(|r| -> Result<GridPointForecast, AppError> {
                Ok(GridPointForecast {
                    lat: parse_decimal(r, "lat")?,
                    lon: parse_decimal(r, "lon")?,
                    wind_speed_kn: parse_decimal_opt(r, "wind_speed_kn"),
                    wind_direction_deg: parse_decimal_opt(r, "wind_direction_deg"),
                    wind_gust_kn: parse_decimal_opt(r, "wind_gust_kn"),
                    wave_height_m: parse_decimal_opt(r, "wave_height_m"),
                    wave_period_s: parse_decimal_opt(r, "wave_period_s"),
                    wave_direction_deg: parse_decimal_opt(r, "wave_direction_deg"),
                    cape_j_kg: parse_decimal_opt(r, "cape_j_kg"),
                    model: r.get("model").unwrap_or_else(|| "ecmwf".to_string()),
                })
            })
            .collect::<Result<_, _>>()?;

        // Prefer AROME coverage for this hour; fall back to ECMWF when AROME has none.
        // This is all-or-nothing across the area: if any AROME point exists for the
        // hour, ECMWF points are dropped. Safe while AROME (France-HD) fully covers a
        // regional area; if areas grow to span AROME's edge this would need a
        // per-cell merge. The route/routing path blends per-point in interpolate_blended.
        let has_arome = all.iter().any(|p| p.model == "arome");
        if has_arome {
            all.retain(|p| p.model == "arome");
        }
        Ok(all)
    }

    /// Returns the most recent forecast fetch per (model, lat, lon) grid point globally.
    /// Used by the route forecast endpoint.
    pub fn fetch_forecast_fetches(&self) -> Result<Vec<FetchWithHourly>, AppError> {
        let mut conn = self.pool.get_conn()?;
        let fetch_rows: Vec<mysql::Row> = conn.exec(
            "SELECT ff.id, ff.model, ff.lat, ff.lon FROM forecast_fetch ff
             WHERE ff.fetched_at = (
                 SELECT MAX(inner_ff.fetched_at)
                 FROM forecast_fetch inner_ff
                 WHERE inner_ff.lat = ff.lat
                   AND inner_ff.lon = ff.lon
                   AND inner_ff.model = ff.model
             )
             ORDER BY ff.id",
            (),
        )?;
        let mut fetches = Vec::new();
        for frow in &fetch_rows {
            let fid: u32 = match frow.get("id") { Some(v) => v, None => continue };
            let model: String = frow.get("model").unwrap_or_else(|| "ecmwf".to_string());
            let flat = parse_decimal(frow, "lat")?;
            let flon = parse_decimal(frow, "lon")?;
            let hourly = self.load_hourly(&mut conn, fid)?;
            fetches.push(FetchWithHourly { lat: flat, lon: flon, model, hourly });
        }
        Ok(fetches)
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
            // MariaDB returns DECIMAL as bytes; NULL causes get_opt::<f64> to return Err.
            // Use get_opt::<Vec<u8>> to safely distinguish bytes from NULL.
            match row.get_opt::<Vec<u8>, _>(col) {
                Some(Ok(b)) => String::from_utf8(b)
                    .map_err(|e| AppError::Parse(e.to_string()))?
                    .parse::<f64>()
                    .map_err(|e| AppError::Parse(e.to_string())),
                _ => Err(AppError::Database(format!("Column {col} is NULL or non-convertible"))),
            }
        }
        None => Err(AppError::Database(format!("Column {col} is NULL/missing"))),
    }
}

fn parse_decimal_opt(row: &mysql::Row, col: &str) -> Option<f64> {
    match row.get_opt::<f64, _>(col) {
        Some(Ok(v)) => Some(v),
        Some(Err(_)) => {
            // MariaDB returns DECIMAL as bytes; NULL causes get_opt::<f64> to return Err.
            // Use get_opt::<Vec<u8>> to safely distinguish bytes from NULL.
            match row.get_opt::<Vec<u8>, _>(col) {
                Some(Ok(b)) => String::from_utf8(b).ok()?.parse::<f64>().ok(),
                _ => None,
            }
        }
        None => None,
    }
}

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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::setup_db;
    use chrono::Utc;

    fn make_area(db: &crate::db::types::VesselDatabase) -> u32 {
        db.create_forecast_area(&NewForecastArea {
            lat_min: 43.0, lat_max: 44.0, lon_min: 8.0, lon_max: 9.0,
        }, Utc::now()).unwrap()
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
        db.insert_forecast(area_id, "ecmwf", 43.5, 8.5, Utc::now(), &hourly).unwrap();

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
        db.insert_forecast(area_id, "ecmwf", 43.5, 8.5, Utc::now(), &hourly).unwrap();

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
        db.insert_forecast(area_id, "ecmwf", 43.5, 8.5,
            DateTime::parse_from_rfc3339("2026-05-14T06:00:00Z").unwrap().with_timezone(&Utc),
            &vec![make_hourly(ts, 10.0)]).unwrap();

        // Second (newer) fetch — wind 20 kn — this should win
        db.insert_forecast(area_id, "ecmwf", 43.5, 8.5,
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
        db.insert_forecast(area_id, "ecmwf", 43.2, 8.3, Utc::now(), &hourly).unwrap();
        db.insert_forecast(area_id, "ecmwf", 43.6, 8.7, Utc::now(), &hourly).unwrap();

        let fetches = db.fetch_forecast_fetches().unwrap();
        assert_eq!(fetches.len(), 2);
        assert_eq!(fetches[0].hourly.len(), 1);
    }

    #[test]
    #[ignore]
    fn test_grid_points_prefers_arome_over_ecmwf() {
        let db = setup_db();
        let area_id = make_area(&db);
        let ts = "2026-06-13T09:00:00Z";
        let when = Utc::now();
        db.insert_forecast(area_id, "ecmwf", 43.5, 8.5, when, &vec![make_hourly(ts, 10.0)]).unwrap();
        db.insert_forecast(area_id, "arome", 43.51, 8.51, when, &vec![make_hourly(ts, 18.0)]).unwrap();

        let pts = db.get_grid_points_at(ts).unwrap();
        assert!(!pts.is_empty());
        assert!(pts.iter().all(|p| p.model == "arome"),
            "Expected only AROME points when AROME covers the hour, got {:?}",
            pts.iter().map(|p| &p.model).collect::<Vec<_>>());
    }

    #[test]
    #[ignore]
    fn test_grid_points_falls_back_to_ecmwf_when_no_arome() {
        let db = setup_db();
        let area_id = make_area(&db);
        let ts = "2026-06-13T09:00:00Z";
        db.insert_forecast(area_id, "ecmwf", 43.5, 8.5, Utc::now(), &vec![make_hourly(ts, 10.0)]).unwrap();

        let pts = db.get_grid_points_at(ts).unwrap();
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].model, "ecmwf");
    }

    #[test]
    #[ignore]
    fn test_prune_old_forecasts_keeps_only_latest() {
        let db = setup_db();
        let area_id = make_area(&db);
        let ts = "2026-06-14T09:00:00Z";
        let old = DateTime::parse_from_rfc3339("2026-06-14T06:00:00Z").unwrap().with_timezone(&Utc);
        let new = DateTime::parse_from_rfc3339("2026-06-14T09:00:00Z").unwrap().with_timezone(&Utc);

        // Two fetches for the same grid point, three hours apart.
        db.insert_forecast(area_id, "ecmwf", 43.5, 8.5, old, &vec![make_hourly(ts, 10.0)]).unwrap();
        db.insert_forecast(area_id, "ecmwf", 43.5, 8.5, new, &vec![make_hourly(ts, 20.0)]).unwrap();

        let removed = db.prune_old_forecasts(area_id, new).unwrap();
        assert_eq!(removed, 1, "expected exactly the one older fetch to be removed");

        // Only the latest fetch survives, and its hourly rows cascaded with it.
        let fetches = db.fetch_forecast_fetches().unwrap();
        assert_eq!(fetches.len(), 1);
        assert_eq!(fetches[0].hourly.len(), 1);
        assert!((fetches[0].hourly[0].wind_speed_kn.unwrap() - 20.0).abs() < 0.1,
            "expected the latest fetch (20 kn) to remain, got {:?}",
            fetches[0].hourly[0].wind_speed_kn);
    }
}
