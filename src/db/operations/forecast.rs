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
        if conn.affected_rows() == 0 {
            return Err(AppError::Database(format!("Forecast POI {} not found", id)));
        }
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

        let delta = 0.02_f64;
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
