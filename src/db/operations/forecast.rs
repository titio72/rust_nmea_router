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
pub struct TripForecastArea {
    pub id: u32,
    pub trip_id: u32,
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
    /// All forecast_fetch records for the trip, with their hourly data
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

    pub fn delete_forecast_area(&self, id: u32) -> Result<bool, AppError> {
        let mut conn = self.pool.get_conn()?;
        conn.exec_drop(
            "DELETE FROM trip_forecast_area WHERE id = :id",
            params! { "id" => id },
        )?;
        Ok(conn.affected_rows() > 0)
    }

    // ── Forecast data ─────────────────────────────────────────────────────────

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

    pub fn get_last_fetch_time(&self, trip_id: u32) -> Result<Option<DateTime<Utc>>, AppError> {
        let mut conn = self.pool.get_conn()?;
        let row: Option<mysql::Row> = conn.exec_first(
            "SELECT DATE_FORMAT(MAX(fetched_at), '%Y-%m-%dT%H:%i:%SZ') as last_fetch
             FROM forecast_fetch WHERE trip_id = :trip_id",
            params! { "trip_id" => trip_id },
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

    pub fn get_active_trip_id(&self) -> Result<Option<u32>, AppError> {
        let mut conn = self.pool.get_conn()?;
        let row: Option<mysql::Row> = conn.exec_first(
            "SELECT id FROM trips
             WHERE end_timestamp >= DATE_SUB(NOW(), INTERVAL 24 HOUR)
             ORDER BY end_timestamp DESC LIMIT 1",
            (),
        )?;
        Ok(row.and_then(|r| r.get("id")))
    }

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

        let fetches = self.fetch_forecast_fetches(trip_id)?;

        Ok(Some(TripForecastInputs { trip_start, trip_end, track, fetches }))
    }

    /// Returns the most recent forecast values for every grid point of `trip_id`
    /// at the given UTC hour.  `timestamp_iso` is RFC-3339, e.g. "2026-05-14T09:00:00Z".
    pub fn get_grid_points_at(
        &self,
        trip_id: u32,
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
             WHERE ff.trip_id = :trip_id
               AND fh.timestamp = :ts
               AND ff.fetched_at = (
                   SELECT MAX(inner_ff.fetched_at)
                   FROM forecast_fetch inner_ff
                   WHERE inner_ff.trip_id = ff.trip_id
                     AND inner_ff.lat = ff.lat
                     AND inner_ff.lon = ff.lon
               )",
            params! { "trip_id" => trip_id, "ts" => &ts_db },
        )?;
        rows.iter()
            .map(|r| {
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
                })
            })
            .collect()
    }

    /// Loads all FetchWithHourly records for a trip without loading the vessel track.
    /// Returns only the latest fetch per (lat, lon) grid point.
    /// Used by the route forecast endpoint.
    pub fn fetch_forecast_fetches(
        &self,
        trip_id: u32,
    ) -> Result<Vec<FetchWithHourly>, AppError> {
        let mut conn = self.pool.get_conn()?;
        let fetch_rows: Vec<mysql::Row> = conn.exec(
            "SELECT ff.id, ff.lat, ff.lon FROM forecast_fetch ff
             WHERE ff.trip_id = :trip_id
               AND ff.fetched_at = (
                   SELECT MAX(inner_ff.fetched_at)
                   FROM forecast_fetch inner_ff
                   WHERE inner_ff.trip_id = ff.trip_id
                     AND inner_ff.lat = ff.lat
                     AND inner_ff.lon = ff.lon
               )
             ORDER BY ff.id",
            params! { "trip_id" => trip_id },
        )?;
        let mut fetches = Vec::new();
        for frow in &fetch_rows {
            let fid: u32 = match frow.get("id") { Some(v) => v, None => continue };
            let flat = parse_decimal(frow, "lat")?;
            let flon = parse_decimal(frow, "lon")?;
            let hourly = self.load_hourly(&mut conn, fid)?;
            fetches.push(FetchWithHourly { lat: flat, lon: flon, hourly });
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

        db.delete_forecast_area(area_id).unwrap();

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

    #[test]
    #[ignore]
    fn test_get_grid_points_at_returns_latest_fetch() {
        let db = setup_db();
        let trip_id = make_trip(&db);
        let area_id = db.create_forecast_area(&NewTripForecastArea {
            trip_id, lat_min: 43.0, lat_max: 44.0, lon_min: 8.0, lon_max: 9.0,
        }).unwrap();

        let ts = "2026-05-14T09:00:00Z";
        let hourly = vec![ForecastHourlyPoint {
            timestamp: ts.to_string(),
            wind_speed_kn: Some(10.0),
            wind_direction_deg: Some(90.0),
            wind_gust_kn: Some(14.0),
            wave_height_m: Some(1.0),
            wave_period_s: Some(6.0),
            wave_direction_deg: Some(95.0),
            cape_j_kg: Some(50.0),
        }];
        // First (older) fetch — wind 10 kn
        db.insert_forecast(trip_id, area_id, 43.5, 8.5, DateTime::parse_from_rfc3339("2026-05-14T06:00:00Z").unwrap().with_timezone(&Utc), &hourly).unwrap();

        let hourly2 = vec![ForecastHourlyPoint {
            timestamp: ts.to_string(),
            wind_speed_kn: Some(20.0),
            wind_direction_deg: Some(180.0),
            wind_gust_kn: Some(25.0),
            wave_height_m: Some(1.5),
            wave_period_s: Some(7.0),
            wave_direction_deg: Some(185.0),
            cape_j_kg: Some(100.0),
        }];
        // Second (newer) fetch — wind 20 kn — this should win
        db.insert_forecast(trip_id, area_id, 43.5, 8.5, DateTime::parse_from_rfc3339("2026-05-14T09:00:00Z").unwrap().with_timezone(&Utc), &hourly2).unwrap();

        let pts = db.get_grid_points_at(trip_id, ts).unwrap();
        assert_eq!(pts.len(), 1);
        assert!((pts[0].wind_speed_kn.unwrap() - 20.0).abs() < 0.1, "Expected latest fetch (20 kn), got {:?}", pts[0].wind_speed_kn);
    }

    #[test]
    #[ignore]
    fn test_fetch_forecast_fetches_returns_all_grid_points() {
        let db = setup_db();
        let trip_id = make_trip(&db);
        let area_id = db.create_forecast_area(&NewTripForecastArea {
            trip_id, lat_min: 43.0, lat_max: 44.0, lon_min: 8.0, lon_max: 9.0,
        }).unwrap();

        let hourly = vec![ForecastHourlyPoint {
            timestamp: "2026-05-14T09:00:00Z".to_string(),
            wind_speed_kn: Some(12.0), wind_direction_deg: Some(90.0),
            wind_gust_kn: Some(15.0), wave_height_m: Some(1.0),
            wave_period_s: Some(6.0), wave_direction_deg: Some(95.0),
            cape_j_kg: Some(0.0),
        }];
        db.insert_forecast(trip_id, area_id, 43.2, 8.3, Utc::now(), &hourly).unwrap();
        db.insert_forecast(trip_id, area_id, 43.6, 8.7, Utc::now(), &hourly).unwrap();

        let fetches = db.fetch_forecast_fetches(trip_id).unwrap();
        assert_eq!(fetches.len(), 2);
        assert_eq!(fetches[0].hourly.len(), 1);
    }
}
