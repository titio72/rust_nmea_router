/// Test helpers for database operations
/// Provides utilities to create, populate, and verify test data
use crate::config::Config;
use crate::db::types::VesselDatabase;
use crate::position_utils::Position;
use crate::trip::Trip;
use crate::utilities::{haversine_distance_nm, haversine_heading, EngineStatus};
use mysql::params;
use mysql::prelude::Queryable;
use std::error::Error;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ============================================================================
// DATABASE LIFECYCLE FUNCTIONS
// ============================================================================

/// Helper to setup database for tests
pub fn setup_db() -> VesselDatabase {
    let config = Config::load_for_context(None)
        .expect("Failed to load test config - ensure test_config.json exists");
    let db_url = config.database.connection.connection_url();
    let db = setup_test_db(&db_url)
        .expect("Failed to setup test database - ensure MySQL is running and test database exists");
    reset_test_db(&db).expect("Failed to reset test database");
    db
}

/// Reset the test database - drops all data but keeps schema
pub fn reset_test_db(db: &VesselDatabase) -> Result<(), Box<dyn Error>> {
    let mut conn = db.pool.get_conn()?;

    // Disable foreign key checks temporarily
    conn.query_drop("SET FOREIGN_KEY_CHECKS = 0")?;

    // Truncate all tables
    conn.query_drop("TRUNCATE TABLE environmental_data")?;
    conn.query_drop("TRUNCATE TABLE vessel_status")?;
    conn.query_drop("TRUNCATE TABLE trips")?;
    conn.query_drop("TRUNCATE TABLE system_status")?;
    conn.query_drop("TRUNCATE TABLE forecast_hourly")?;
    conn.query_drop("TRUNCATE TABLE forecast_fetch")?;
    conn.query_drop("TRUNCATE TABLE forecast_poi")?;

    // Re-enable foreign key checks
    conn.query_drop("SET FOREIGN_KEY_CHECKS = 1")?;

    Ok(())
}

/// Setup test database - creates schema if needed
pub fn setup_test_db(connection_url: &str) -> Result<VesselDatabase, Box<dyn Error>> {
    let db = VesselDatabase::new(connection_url, 2, 10)?;

    // Ensure all tables exist (they should from schema.sql, but safe to check)
    let mut conn = db.pool.get_conn()?;

    // Check if tables exist, create if not (basic check)
    let tables: Vec<String> = conn.query("SHOW TABLES")?;

    if !tables.contains(&"vessel_status".to_string()) {
        return Err("Database schema not initialized. Please run schema.sql first.".into());
    }

    Ok(db)
}

/// Teardown test database - drops all data (same as reset for now)
#[allow(dead_code)]
pub fn teardown_test_db(db: &VesselDatabase) -> Result<(), Box<dyn Error>> {
    reset_test_db(db)
}

// ============================================================================
// DATA GENERATION HELPERS
// ============================================================================
/// Add a test trip to the databases
#[allow(clippy::too_many_arguments)]
pub fn add_test_trip(
    db: &VesselDatabase,
    description: String,
    start_timestamp: SystemTime,
    end_timestamp: SystemTime,
    total_distance_sailed: f64,
    total_distance_motoring: f64,
    total_time_sailing: u64,
    total_time_motoring: u64,
    total_time_moored: u64,
) -> Result<u32, Box<dyn Error>> {
    let mut conn = db.pool.get_conn()?;

    let start_dt = chrono::DateTime::<chrono::Utc>::from(start_timestamp);
    let end_dt = chrono::DateTime::<chrono::Utc>::from(end_timestamp);

    conn.exec_drop(
        r"INSERT INTO trips
            (description, start_timestamp, end_timestamp,
             total_distance_sailed, total_distance_motoring,
             total_time_sailing, total_time_motoring, total_time_moored, uuid)
        VALUES (:description, :start_ts, :end_ts, :dist_sailed, :dist_motoring, :time_sailing, :time_motoring, :time_moored, :uuid)",
        params! {
            "description" => description,
            "start_ts" => start_dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            "end_ts" => end_dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            "dist_sailed" => total_distance_sailed,
            "dist_motoring" => total_distance_motoring,
            "time_sailing" => total_time_sailing,
            "time_motoring" => total_time_motoring,
            "time_moored" => total_time_moored,
            "uuid" => uuid::Uuid::new_v4().to_string(),
        },
    )?;

    let trip_id = conn.last_insert_id() as u32;
    Ok(trip_id)
}

/// Add a test vessel status record to the database
#[allow(clippy::too_many_arguments)]
pub fn add_test_vessel_status(
    db: &VesselDatabase,
    timestamp: SystemTime,
    latitude: f64,
    longitude: f64,
    average_speed_kn: f64,
    max_speed_kn: f64,
    average_wind_speed_kn: Option<f64>,
    average_wind_angle_deg: Option<f64>,
    is_moored: bool,
    engine_on: EngineStatus,
    total_distance_nm: f64,
    total_time_ms: u64,
    cog_deg: Option<f64>,
    average_heading_deg: Option<f64>,
) -> Result<u32, Box<dyn Error>> {
    let mut conn = db.pool.get_conn()?;

    let dt = chrono::DateTime::<chrono::Utc>::from(timestamp);

    conn.exec_drop(
        r"INSERT INTO vessel_status
            (timestamp, latitude, longitude, average_speed_kn, max_speed_kn,
             average_wind_speed_kn, average_wind_angle_deg, is_moored, engine_on,
             total_distance_nm, total_time_ms, cog_deg, average_heading_deg)
        VALUES (:ts, :lat, :lon, :avg_spd, :max_spd, :wind_spd, :wind_ang, :moored, :engine, :dist, :time, :cog, :hdg)",
        params! {
            "ts" => dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            "lat" => latitude,
            "lon" => longitude,
            "avg_spd" => average_speed_kn,
            "max_spd" => max_speed_kn,
            "wind_spd" => average_wind_speed_kn,
            "wind_ang" => average_wind_angle_deg,
            "moored" => is_moored,
            "engine" => engine_on.as_u8(),
            "dist" => total_distance_nm,
            "time" => total_time_ms,
            "cog" => cog_deg,
            "hdg" => average_heading_deg,
        },
    )?;

    let id = conn.last_insert_id() as u32;
    Ok(id)
}

/// Add test environmental data to the database
pub fn add_test_env(
    db: &VesselDatabase,
    timestamp: SystemTime,
    metric_id: u8,
    value_avg: Option<f64>,
    value_max: Option<f64>,
    value_min: Option<f64>,
    unit: &str,
) -> Result<u32, Box<dyn Error>> {
    let mut conn = db.pool.get_conn()?;

    let dt = chrono::DateTime::<chrono::Utc>::from(timestamp);

    conn.exec_drop(
        r"INSERT INTO environmental_data
            (timestamp, metric_id, value_avg, value_max, value_min, unit)
        VALUES (:ts, :metric, :avg, :max, :min, :unit)",
        params! {
            "ts" => dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            "metric" => metric_id,
            "avg" => value_avg,
            "max" => value_max,
            "min" => value_min,
            "unit" => unit,
        },
    )?;

    let id = conn.last_insert_id() as u32;
    Ok(id)
}

// ============================================================================
// POSITION & NAVIGATION UTILITIES
// ============================================================================

/// Calculate position P1 when traveling from position P0 on bearing H at speed V for time T
/// Uses Haversine formula for great circle navigation
///
/// # Arguments
/// * `p0` - Starting position
/// * `bearing_deg` - True bearing in degrees (0 = North, 90 = East)
/// * `speed_kn` - Speed in knots
/// * `duration_s` - Duration in seconds
///
/// # Returns
/// New position after traveling
pub fn calculate_position_from_bearing(
    p0: Position,
    bearing_deg: f64,
    speed_kn: f64,
    duration_s: f64,
) -> Position {
    let distance_nm = speed_kn * (duration_s / 3600.0);

    // Earth radius in nautical miles
    let earth_radius_nm = 3440.065;

    let lat1 = p0.latitude.to_radians();
    let lon1 = p0.longitude.to_radians();
    let bearing_rad = bearing_deg.to_radians();
    let angular_distance = distance_nm / earth_radius_nm;

    let lat2 = (lat1.sin() * angular_distance.cos()
        + lat1.cos() * angular_distance.sin() * bearing_rad.cos())
    .asin();

    let lon2 = lon1
        + (bearing_rad.sin() * angular_distance.sin() * lat1.cos())
            .atan2(angular_distance.cos() - lat1.sin() * lat2.sin());

    Position {
        latitude: lat2.to_degrees(),
        longitude: lon2.to_degrees(),
    }
}

/// Generate a track (vector of position/timestamp pairs) from P0 to P1 at constant speed
///
/// # Arguments
/// * `p0` - Starting position
/// * `p1` - Ending position
/// * `speed_kn` - Speed in knots
/// * `interval_s` - Time interval between points in seconds
/// * `start_time` - Starting timestamp
///
/// # Returns
/// Vector of (Position, SystemTime) pairs
pub fn generate_track(
    p0: Position,
    p1: Position,
    speed_kn: f64,
    interval_s: u64,
    start_time: SystemTime,
) -> Vec<(Position, SystemTime)> {
    let mut track = Vec::new();

    let total_distance_nm =
        haversine_distance_nm(p0.latitude, p0.longitude, p1.latitude, p1.longitude);

    let bearing_deg = haversine_heading(p0.latitude, p0.longitude, p1.latitude, p1.longitude);

    let total_time_hours = total_distance_nm / speed_kn;
    let total_time_s = (total_time_hours * 3600.0) as u64;
    let num_points = (total_time_s / interval_s) + 1;

    for i in 0..num_points {
        let elapsed_s = i * interval_s;
        let fraction = elapsed_s as f64 / total_time_s as f64;

        // Calculate intermediate position
        let pos = if fraction >= 1.0 {
            p1
        } else {
            calculate_position_from_bearing(p0, bearing_deg, speed_kn, elapsed_s as f64)
        };

        let timestamp = start_time + Duration::from_secs(elapsed_s);
        track.push((pos, timestamp));
    }

    track
}

/// Generate realistic wind data with variation
/// Returns (speed_kn, angle_deg) with some randomness
#[allow(dead_code)]
pub fn generate_realistic_wind(
    base_speed_kn: f64,
    base_angle_deg: f64,
    variation: f64,
) -> (f64, f64) {
    use rand::Rng;
    let mut rng = rand::thread_rng();

    let speed_variation = rng.gen_range(-variation..variation);
    let angle_variation = rng.gen_range(-10.0..10.0);

    let speed = (base_speed_kn + speed_variation).max(0.0);
    let angle = (base_angle_deg + angle_variation + 360.0) % 360.0;

    (speed, angle)
}

// ============================================================================
// DATA RETRIEVAL & VERIFICATION FUNCTIONS
// ============================================================================

/// Fetch a trip by its start timestamp (for test verification)
pub fn fetch_trip_by_timestamp(
    db: &VesselDatabase,
    start_timestamp: SystemTime,
) -> Result<Option<Trip>, Box<dyn Error>> {
    let mut conn = db.pool.get_conn()?;

    let dt = chrono::DateTime::<chrono::Utc>::from(start_timestamp);
    let timestamp_str = dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string();

    let row: Option<mysql::Row> = conn.exec_first(
        r"SELECT id, description,
                 UNIX_TIMESTAMP(start_timestamp) as start_ts,
                 UNIX_TIMESTAMP(end_timestamp) as end_ts,
                 total_distance_sailed, total_distance_motoring,
                 total_time_sailing, total_time_motoring, total_time_moored
          FROM trips
          WHERE start_timestamp = ?",
        (timestamp_str,),
    )?;

    if let Some(mut row) = row {
        let id: i64 = row.take("id").ok_or("Missing id")?;
        let description: String = row.take("description").ok_or("Missing description")?;
        let start_ts: f64 = row.take("start_ts").ok_or("Missing start_ts")?;
        let end_ts: f64 = row.take("end_ts").ok_or("Missing end_ts")?;
        let total_distance_sailed: f64 = row
            .take("total_distance_sailed")
            .ok_or("Missing total_distance_sailed")?;
        let total_distance_motoring: f64 = row
            .take("total_distance_motoring")
            .ok_or("Missing total_distance_motoring")?;
        let total_time_sailing: u64 = row
            .take("total_time_sailing")
            .ok_or("Missing total_time_sailing")?;
        let total_time_motoring: u64 = row
            .take("total_time_motoring")
            .ok_or("Missing total_time_motoring")?;
        let total_time_moored: u64 = row
            .take("total_time_moored")
            .ok_or("Missing total_time_moored")?;

        Ok(Some(Trip {
            id: Some(id),
            uuid: uuid::Uuid::new_v4().to_string(),
            description,
            start_timestamp: UNIX_EPOCH + Duration::from_secs_f64(start_ts),
            end_timestamp: UNIX_EPOCH + Duration::from_secs_f64(end_ts),
            total_distance_sailed,
            total_distance_motoring,
            total_time_sailing,
            total_time_motoring,
            total_time_moored,
        }))
    } else {
        Ok(None)
    }
}

/// Fetch vessel status by timestamp (for test verification)
#[derive(Debug)]
pub struct VesselStatusRecord {
    #[allow(dead_code)]
    pub id: u32,
    #[allow(dead_code)]
    pub timestamp: SystemTime,
    #[allow(dead_code)]
    pub latitude: Option<f64>,
    #[allow(dead_code)]
    pub longitude: Option<f64>,
    #[allow(dead_code)]
    pub average_speed_kn: f64,
    #[allow(dead_code)]
    pub max_speed_kn: f64,
    #[allow(dead_code)]
    pub average_wind_speed_kn: Option<f64>,
    #[allow(dead_code)]
    pub average_wind_angle_deg: Option<f64>,
    #[allow(dead_code)]
    pub is_moored: bool,
    #[allow(dead_code)]
    pub engine_on: EngineStatus,
    #[allow(dead_code)]
    pub total_distance_nm: f64,
    #[allow(dead_code)]
    pub total_time_ms: u64,
    #[allow(dead_code)]
    pub cog_deg: Option<f64>,
    #[allow(dead_code)]
    pub average_heading_deg: Option<f64>,
}

pub fn fetch_vessel_status_by_id(
    db: &VesselDatabase,
    id: u32,
) -> Result<Option<VesselStatusRecord>, Box<dyn Error>> {
    let mut conn = db.pool.get_conn()?;

    let row: Option<mysql::Row> = conn.exec_first(
        r"SELECT id, UNIX_TIMESTAMP(timestamp) as ts,
                 latitude, longitude, average_speed_kn, max_speed_kn,
                 average_wind_speed_kn, average_wind_angle_deg, is_moored, engine_on,
                 total_distance_nm, total_time_ms, cog_deg, average_heading_deg
          FROM vessel_status
          WHERE id = ?",
        (id,),
    )?;

    if let Some(mut row) = row {
        let id: u32 = row.take("id").ok_or("Missing id")?;
        let ts: f64 = row.take("ts").ok_or("Missing ts")?;
        let latitude: Option<f64> = row.take("latitude");
        let longitude: Option<f64> = row.take("longitude");

        // Safely convert numeric fields that might be different types in different MySQL versions
        let average_speed_kn: f64 = {
            match row.take::<mysql::Value, _>("average_speed_kn") {
                Some(mysql::Value::Float(f)) => f as f64,
                Some(mysql::Value::Double(d)) => d,
                Some(mysql::Value::Int(i)) => i as f64,
                Some(mysql::Value::UInt(u)) => u as f64,
                Some(mysql::Value::Bytes(b)) => {
                    // DECIMAL columns are returned as Bytes, need to parse as string
                    let s = String::from_utf8(b)?;
                    s.parse::<f64>()?
                }
                Some(mysql::Value::NULL) => return Err("average_speed_kn is NULL".into()),
                None => return Err("Missing average_speed_kn".into()),
                Some(v) => {
                    return Err(format!("Unexpected type for average_speed_kn: {:?}", v).into())
                }
            }
        };

        let max_speed_kn: f64 = {
            match row.take::<mysql::Value, _>("max_speed_kn") {
                Some(mysql::Value::Float(f)) => f as f64,
                Some(mysql::Value::Double(d)) => d,
                Some(mysql::Value::Int(i)) => i as f64,
                Some(mysql::Value::UInt(u)) => u as f64,
                Some(mysql::Value::Bytes(b)) => {
                    let s = String::from_utf8(b)?;
                    s.parse::<f64>()?
                }
                Some(mysql::Value::NULL) => return Err("max_speed_kn is NULL".into()),
                None => return Err("Missing max_speed_kn".into()),
                Some(v) => return Err(format!("Unexpected type for max_speed_kn: {:?}", v).into()),
            }
        };

        let average_wind_speed_kn: Option<f64> = {
            match row.take::<mysql::Value, _>("average_wind_speed_kn") {
                Some(mysql::Value::NULL) | None => None,
                Some(mysql::Value::Float(f)) => Some(f as f64),
                Some(mysql::Value::Double(d)) => Some(d),
                Some(mysql::Value::Int(i)) => Some(i as f64),
                Some(mysql::Value::UInt(u)) => Some(u as f64),
                Some(mysql::Value::Bytes(b)) => String::from_utf8(b)
                    .ok()
                    .and_then(|s| s.parse::<f64>().ok()),
                _ => None,
            }
        };

        let average_wind_angle_deg: Option<f64> = {
            match row.take::<mysql::Value, _>("average_wind_angle_deg") {
                Some(mysql::Value::NULL) | None => None,
                Some(mysql::Value::Float(f)) => Some(f as f64),
                Some(mysql::Value::Double(d)) => Some(d),
                Some(mysql::Value::Int(i)) => Some(i as f64),
                Some(mysql::Value::UInt(u)) => Some(u as f64),
                Some(mysql::Value::Bytes(b)) => String::from_utf8(b)
                    .ok()
                    .and_then(|s| s.parse::<f64>().ok()),
                _ => None,
            }
        };

        let is_moored: bool = row.take("is_moored").ok_or("Missing is_moored")?;
        let engine_on_u8: u8 = row.take("engine_on").ok_or("Missing engine_on")?;

        let total_distance_nm: f64 = {
            match row.take::<mysql::Value, _>("total_distance_nm") {
                Some(mysql::Value::Float(f)) => f as f64,
                Some(mysql::Value::Double(d)) => d,
                Some(mysql::Value::Int(i)) => i as f64,
                Some(mysql::Value::UInt(u)) => u as f64,
                Some(mysql::Value::Bytes(b)) => {
                    let s = String::from_utf8(b)?;
                    s.parse::<f64>()?
                }
                Some(mysql::Value::NULL) => return Err("total_distance_nm is NULL".into()),
                None => return Err("Missing total_distance_nm".into()),
                Some(v) => {
                    return Err(format!("Unexpected type for total_distance_nm: {:?}", v).into())
                }
            }
        };

        let total_time_ms: u64 = row.take("total_time_ms").ok_or("Missing total_time_ms")?;

        let cog_deg: Option<f64> = {
            match row.take::<mysql::Value, _>("cog_deg") {
                Some(mysql::Value::NULL) | None => None,
                Some(mysql::Value::Float(f)) => Some(f as f64),
                Some(mysql::Value::Double(d)) => Some(d),
                Some(mysql::Value::Int(i)) => Some(i as f64),
                Some(mysql::Value::UInt(u)) => Some(u as f64),
                Some(mysql::Value::Bytes(b)) => String::from_utf8(b)
                    .ok()
                    .and_then(|s| s.parse::<f64>().ok()),
                _ => None,
            }
        };

        let average_heading_deg: Option<f64> = {
            match row.take::<mysql::Value, _>("average_heading_deg") {
                Some(mysql::Value::NULL) | None => None,
                Some(mysql::Value::Float(f)) => Some(f as f64),
                Some(mysql::Value::Double(d)) => Some(d),
                Some(mysql::Value::Int(i)) => Some(i as f64),
                Some(mysql::Value::UInt(u)) => Some(u as f64),
                Some(mysql::Value::Bytes(b)) => String::from_utf8(b)
                    .ok()
                    .and_then(|s| s.parse::<f64>().ok()),
                _ => None,
            }
        };

        Ok(Some(VesselStatusRecord {
            id,
            timestamp: UNIX_EPOCH + Duration::from_secs_f64(ts),
            latitude,
            longitude,
            average_speed_kn,
            max_speed_kn,
            average_wind_speed_kn,
            average_wind_angle_deg,
            is_moored,
            engine_on: EngineStatus::from_u8(engine_on_u8),
            total_distance_nm,
            total_time_ms,
            cog_deg,
            average_heading_deg,
        }))
    } else {
        Ok(None)
    }
}

pub fn fetch_vessel_status_by_timestamp(
    db: &VesselDatabase,
    timestamp: SystemTime,
) -> Result<Option<VesselStatusRecord>, Box<dyn Error>> {
    let mut conn = db.pool.get_conn()?;

    let dt = chrono::DateTime::<chrono::Utc>::from(timestamp);
    let timestamp_str = dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string();

    let row: Option<mysql::Row> = conn.exec_first(
        r"SELECT id, UNIX_TIMESTAMP(timestamp) as ts,
                 latitude, longitude, average_speed_kn, max_speed_kn,
                 average_wind_speed_kn, average_wind_angle_deg, is_moored, engine_on,
                 total_distance_nm, total_time_ms, cog_deg, average_heading_deg
          FROM vessel_status
          WHERE timestamp = ?",
        (timestamp_str,),
    )?;

    if let Some(mut row) = row {
        let id: u32 = row.take("id").ok_or("Missing id")?;
        let ts: f64 = row.take("ts").ok_or("Missing ts")?;
        let latitude: Option<f64> = row.take("latitude");
        let longitude: Option<f64> = row.take("longitude");

        // Safely convert numeric fields that might be different types in different MySQL versions
        let average_speed_kn: f64 = {
            match row.take::<mysql::Value, _>("average_speed_kn") {
                Some(mysql::Value::Float(f)) => f as f64,
                Some(mysql::Value::Double(d)) => d,
                Some(mysql::Value::Int(i)) => i as f64,
                Some(mysql::Value::UInt(u)) => u as f64,
                Some(mysql::Value::Bytes(b)) => {
                    // DECIMAL columns are returned as Bytes, need to parse as string
                    let s = String::from_utf8(b)?;
                    s.parse::<f64>()?
                }
                Some(mysql::Value::NULL) => return Err("average_speed_kn is NULL".into()),
                None => return Err("Missing average_speed_kn".into()),
                Some(v) => {
                    return Err(format!("Unexpected type for average_speed_kn: {:?}", v).into())
                }
            }
        };

        let max_speed_kn: f64 = {
            match row.take::<mysql::Value, _>("max_speed_kn") {
                Some(mysql::Value::Float(f)) => f as f64,
                Some(mysql::Value::Double(d)) => d,
                Some(mysql::Value::Int(i)) => i as f64,
                Some(mysql::Value::UInt(u)) => u as f64,
                Some(mysql::Value::Bytes(b)) => {
                    let s = String::from_utf8(b)?;
                    s.parse::<f64>()?
                }
                Some(mysql::Value::NULL) => return Err("max_speed_kn is NULL".into()),
                None => return Err("Missing max_speed_kn".into()),
                Some(v) => return Err(format!("Unexpected type for max_speed_kn: {:?}", v).into()),
            }
        };

        let average_wind_speed_kn: Option<f64> = {
            match row.take::<mysql::Value, _>("average_wind_speed_kn") {
                Some(mysql::Value::NULL) | None => None,
                Some(mysql::Value::Float(f)) => Some(f as f64),
                Some(mysql::Value::Double(d)) => Some(d),
                Some(mysql::Value::Int(i)) => Some(i as f64),
                Some(mysql::Value::UInt(u)) => Some(u as f64),
                Some(mysql::Value::Bytes(b)) => String::from_utf8(b)
                    .ok()
                    .and_then(|s| s.parse::<f64>().ok()),
                _ => None,
            }
        };

        let average_wind_angle_deg: Option<f64> = {
            match row.take::<mysql::Value, _>("average_wind_angle_deg") {
                Some(mysql::Value::NULL) | None => None,
                Some(mysql::Value::Float(f)) => Some(f as f64),
                Some(mysql::Value::Double(d)) => Some(d),
                Some(mysql::Value::Int(i)) => Some(i as f64),
                Some(mysql::Value::UInt(u)) => Some(u as f64),
                Some(mysql::Value::Bytes(b)) => String::from_utf8(b)
                    .ok()
                    .and_then(|s| s.parse::<f64>().ok()),
                _ => None,
            }
        };

        let is_moored: bool = row.take("is_moored").ok_or("Missing is_moored")?;
        let engine_on_u8: u8 = row.take("engine_on").ok_or("Missing engine_on")?;

        let total_distance_nm: f64 = {
            match row.take::<mysql::Value, _>("total_distance_nm") {
                Some(mysql::Value::Float(f)) => f as f64,
                Some(mysql::Value::Double(d)) => d,
                Some(mysql::Value::Int(i)) => i as f64,
                Some(mysql::Value::UInt(u)) => u as f64,
                Some(mysql::Value::Bytes(b)) => {
                    let s = String::from_utf8(b)?;
                    s.parse::<f64>()?
                }
                Some(mysql::Value::NULL) => return Err("total_distance_nm is NULL".into()),
                None => return Err("Missing total_distance_nm".into()),
                Some(v) => {
                    return Err(format!("Unexpected type for total_distance_nm: {:?}", v).into())
                }
            }
        };

        let total_time_ms: u64 = row.take("total_time_ms").ok_or("Missing total_time_ms")?;

        let cog_deg: Option<f64> = {
            match row.take::<mysql::Value, _>("cog_deg") {
                Some(mysql::Value::NULL) | None => None,
                Some(mysql::Value::Float(f)) => Some(f as f64),
                Some(mysql::Value::Double(d)) => Some(d),
                Some(mysql::Value::Int(i)) => Some(i as f64),
                Some(mysql::Value::UInt(u)) => Some(u as f64),
                Some(mysql::Value::Bytes(b)) => String::from_utf8(b)
                    .ok()
                    .and_then(|s| s.parse::<f64>().ok()),
                _ => None,
            }
        };

        let average_heading_deg: Option<f64> = {
            match row.take::<mysql::Value, _>("average_heading_deg") {
                Some(mysql::Value::NULL) | None => None,
                Some(mysql::Value::Float(f)) => Some(f as f64),
                Some(mysql::Value::Double(d)) => Some(d),
                Some(mysql::Value::Int(i)) => Some(i as f64),
                Some(mysql::Value::UInt(u)) => Some(u as f64),
                Some(mysql::Value::Bytes(b)) => String::from_utf8(b)
                    .ok()
                    .and_then(|s| s.parse::<f64>().ok()),
                _ => None,
            }
        };

        Ok(Some(VesselStatusRecord {
            id,
            timestamp: UNIX_EPOCH + Duration::from_secs_f64(ts),
            latitude,
            longitude,
            average_speed_kn,
            max_speed_kn,
            average_wind_speed_kn,
            average_wind_angle_deg,
            is_moored,
            engine_on: EngineStatus::from_u8(engine_on_u8),
            total_distance_nm,
            total_time_ms,
            cog_deg,
            average_heading_deg,
        }))
    } else {
        Ok(None)
    }
}

/// Fetch environmental data by timestamp and metric_id (for test verification)
#[derive(Debug)]
pub struct EnvironmentalRecord {
    #[allow(dead_code)]
    pub id: u32,
    #[allow(dead_code)]
    pub timestamp: SystemTime,
    #[allow(dead_code)]
    pub metric_id: u8,
    #[allow(dead_code)]
    pub value_avg: Option<f64>,
    #[allow(dead_code)]
    pub value_max: Option<f64>,
    #[allow(dead_code)]
    pub value_min: Option<f64>,
    #[allow(dead_code)]
    pub unit: Option<String>,
}

pub fn fetch_env_data_by_timestamp(
    db: &VesselDatabase,
    timestamp: SystemTime,
    metric_id: u8,
) -> Result<Option<EnvironmentalRecord>, Box<dyn Error>> {
    let mut conn = db.pool.get_conn()?;

    let dt = chrono::DateTime::<chrono::Utc>::from(timestamp);
    let timestamp_str = dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string();

    let row: Option<mysql::Row> = conn.exec_first(
        r"SELECT id, UNIX_TIMESTAMP(timestamp) as ts, metric_id,
                 value_avg, value_max, value_min, unit
          FROM environmental_data
          WHERE timestamp = ? AND metric_id = ?",
        (timestamp_str, metric_id),
    )?;

    if let Some(mut row) = row {
        let id: u32 = row.take("id").ok_or("Missing id")?;
        let ts: f64 = row.take("ts").ok_or("Missing ts")?;
        let metric_id: u8 = row.take("metric_id").ok_or("Missing metric_id")?;
        let value_avg: Option<f64> = row.take("value_avg");
        let value_max: Option<f64> = row.take("value_max");
        let value_min: Option<f64> = row.take("value_min");
        let unit: Option<String> = row.take("unit");

        Ok(Some(EnvironmentalRecord {
            id,
            timestamp: UNIX_EPOCH + Duration::from_secs_f64(ts),
            metric_id,
            value_avg,
            value_max,
            value_min,
            unit,
        }))
    } else {
        Ok(None)
    }
}

pub struct TestLeg {
    #[allow(dead_code)]
    pub start_position: Position,
    #[allow(dead_code)]
    pub start_time: SystemTime,
    #[allow(dead_code)]
    pub end_position: Position,
    #[allow(dead_code)]
    pub end_time: SystemTime,
    #[allow(dead_code)]
    pub wind_speed_kn: f64,
    #[allow(dead_code)]
    pub wind_angle_deg: f64,
    #[allow(dead_code)]
    pub heading_deg_at_mooring: f64,
}

impl TestLeg {
    pub fn new(
        start_position: Position,
        start_time: SystemTime,
        end_position: Position,
        end_time: SystemTime,
        wind_speed_kn: f64,
        wind_angle_deg: f64,
        heading_deg_at_mooring: f64,
    ) -> Self {
        Self {
            start_position,
            start_time,
            end_position,
            end_time,
            wind_speed_kn,
            wind_angle_deg,
            heading_deg_at_mooring,
        }
    }
}

/// Populate a multi-leg trip with realistic vessel status records and mooring periods
///
/// Each leg is represented as (start_position, start_time, end_position, end_time, wind_speed_kn, wind_angle_deg)
///
/// The function will:
/// 1. Generate a moored status 5 minutes before the first leg
/// 2. For each leg, generate vessel status records at 30-second intervals
/// 3. Calculate speed from leg distance and time
/// 4. Between legs (non-final), generate moored records at 5-minute intervals
/// 5. After the last leg, generate two moored records 5 minutes apart
/// 6. Create the trip record with correct distance and time totals
///
/// Details:
/// - heading is set to COG value
/// - max SOG is set to average SOG + 10%
/// - wind data persists to mooring periods (last leg's wind used)
///
/// # Arguments
/// * `db` - Database connection
/// * `trip_name` - Name/description of the trip
/// * `legs` - Vector of (start_pos, start_time, end_pos, end_time, wind_speed_kn, wind_angle_deg)
///
/// # Returns
/// Trip ID of the inserted trip
pub fn populate_multi_leg_trip(
    heading_at_start: f64,
    db: &VesselDatabase,
    trip_name: String,
    legs: Vec<TestLeg>,
) -> Result<u32, Box<dyn Error>> {
    if legs.is_empty() {
        return Err("At least one leg is required".into());
    }

    let trip_start_pos = legs[0].start_position;
    let trip_start_time = legs[0].start_time - Duration::from_secs(5 * 60); // 5 minutes before first leg
    let trip_end_time = legs[legs.len() - 1].end_time + Duration::from_secs(10 * 60); // After final moored period

    let mut total_distance_sailed = 0.0;
    let mut total_time_sailing = 0u64;
    let mut total_time_moored = 0u64;

    let mut last_wind_speed = 0.0;
    let mut last_wind_angle = 0.0;

    // Initial moored status 5 minutes before first leg
    add_test_vessel_status(
        db,
        trip_start_time,
        trip_start_pos.latitude,
        trip_start_pos.longitude,
        0.0,
        0.0,
        Some(0.0),
        Some(0.0),
        true,
        EngineStatus::Off,
        0.0,
        30 * 1000, // the first record simulates the start of the app, where the first moored record is 30 seconds long to indicate the vessel was moored before the trip started
        Some(heading_at_start),
        Some(heading_at_start),
    )?;
    total_time_moored += 30 * 1000; // the first record simulates the start of the app, where the first moored record is 30 seconds long to indicate the vessel was moored before the trip started
    let mut last_report_time = trip_start_time;

    // Process each leg
    for (leg_idx, leg) in legs.iter().enumerate() {
        let is_last_leg = leg_idx == legs.len() - 1;
        last_wind_speed = leg.wind_speed_kn;
        last_wind_angle = leg.wind_angle_deg;

        // Calculate leg distance and speed
        let leg_distance = haversine_distance_nm(
            leg.start_position.latitude,
            leg.start_position.longitude,
            leg.end_position.latitude,
            leg.end_position.longitude,
        );

        let leg_duration = leg
            .end_time
            .duration_since(leg.start_time)
            .unwrap_or(Duration::from_secs(0));
        let leg_duration_hours = leg_duration.as_secs_f64() / 3600.0;
        let leg_speed_kn = if leg_duration_hours > 0.0 {
            leg_distance / leg_duration_hours
        } else {
            0.0
        };

        // Generate track at 30-second intervals
        let track = generate_track(
            leg.start_position,
            leg.end_position,
            leg_speed_kn,
            30,
            leg.start_time,
        );

        // Generate vessel status records for this leg
        for i in 0..track.len() {
            let (pos, timestamp) = track[i];

            let segment_distance = if i > 0 {
                pos.distance_to_nm(&track[i - 1].0)
            } else {
                0.0
            };

            let segment_time_ms = timestamp
                .duration_since(last_report_time)
                .unwrap_or(Duration::from_secs(0))
                .as_millis() as u64;
            last_report_time = timestamp;

            let cog = if i > 0 {
                Some(track[i - 1].0.course_from_deg(&pos))
            } else {
                Some(heading_at_start)
            };

            // Calculate actual speed for this segment
            let segment_speed = if segment_time_ms > 0 {
                let hours = segment_time_ms as f64 / (3600.0 * 1000.0);
                if hours > 0.0 {
                    segment_distance / hours
                } else {
                    0.0
                }
            } else {
                0.0
            };

            add_test_vessel_status(
                db,
                timestamp,
                pos.latitude,
                pos.longitude,
                segment_speed,
                segment_speed * 1.1, // max speed = avg + 10%
                Some(leg.wind_speed_kn),
                Some(leg.wind_angle_deg),
                false,
                EngineStatus::Off,
                segment_distance,
                segment_time_ms,
                cog,
                cog, // heading = COG
            )?;

            total_distance_sailed += segment_distance;
            total_time_sailing += segment_time_ms;
        }

        // Between legs: generate moored records until next leg starts
        if !is_last_leg {
            let next_leg_start = legs[leg_idx + 1].start_time;
            let moored_pos = legs[leg_idx].end_position;
            let mut current_time = last_report_time;

            // add 5 minutes to current_time to simulate time taken to moor after leg ends
            current_time += Duration::from_secs(5 * 60);

            while current_time < next_leg_start {
                let remaining = next_leg_start
                    .duration_since(current_time)
                    .unwrap_or(Duration::from_secs(0));
                let interval_ms = std::cmp::min(remaining.as_millis(), 5 * 60 * 1000) as u64;

                if interval_ms > 0 {
                    last_report_time = current_time;
                    add_test_vessel_status(
                        db,
                        current_time,
                        moored_pos.latitude,
                        moored_pos.longitude,
                        0.0,
                        0.0,
                        Some(last_wind_speed),
                        Some(last_wind_angle),
                        true,
                        EngineStatus::Off,
                        0.0,
                        interval_ms,
                        leg.heading_deg_at_mooring.into(),
                        leg.heading_deg_at_mooring.into(),
                    )?;

                    total_time_moored += interval_ms;
                    current_time += Duration::from_millis(interval_ms);
                } else {
                    break;
                }
            }
        }
    }

    // Final moored period - two records 5 minutes apart
    let final_pos = legs[legs.len() - 1].end_position;
    let final_time = last_report_time + Duration::from_secs(5 * 60);

    add_test_vessel_status(
        db,
        final_time,
        final_pos.latitude,
        final_pos.longitude,
        0.0,
        0.0,
        Some(last_wind_speed),
        Some(last_wind_angle),
        true,
        EngineStatus::Off,
        0.0,
        5 * 60 * 1000,
        legs[legs.len() - 1].heading_deg_at_mooring.into(),
        legs[legs.len() - 1].heading_deg_at_mooring.into(),
    )?;
    total_time_moored += 5 * 60 * 1000;

    add_test_vessel_status(
        db,
        final_time + Duration::from_secs(5 * 60),
        final_pos.latitude,
        final_pos.longitude,
        0.0,
        0.0,
        Some(last_wind_speed),
        Some(last_wind_angle),
        true,
        EngineStatus::Off,
        0.0,
        5 * 60 * 1000,
        legs[legs.len() - 1].heading_deg_at_mooring.into(),
        legs[legs.len() - 1].heading_deg_at_mooring.into(),
    )?;
    total_time_moored += 5 * 60 * 1000;

    // Create trip record
    let trip_id = add_test_trip(
        db,
        trip_name,
        trip_start_time,
        trip_end_time,
        total_distance_sailed,
        0.0, // No motoring for multi-leg sailing trip
        total_time_sailing,
        0,
        total_time_moored,
    )?;

    Ok(trip_id)
}

// ============================================================================
// TEST ASSERTIONS HELPERS
// ============================================================================

/// Assert that two floating point values are approximately equal
#[allow(dead_code)]
pub fn assert_approx_equal(a: f64, b: f64, epsilon: f64, message: &str) {
    let diff = (a - b).abs();
    assert!(
        diff < epsilon,
        "{}: expected {} ≈ {}, but difference is {} (epsilon: {})",
        message,
        a,
        b,
        diff,
        epsilon
    );
}

/// Assert that two optional floating point values are approximately equal
#[allow(dead_code)]
pub fn assert_option_approx_equal(a: Option<f64>, b: Option<f64>, epsilon: f64, message: &str) {
    match (a, b) {
        (Some(av), Some(bv)) => assert_approx_equal(av, bv, epsilon, message),
        (None, None) => {}
        _ => panic!("{}: values don't match - {:?} vs {:?}", message, a, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_position_from_bearing() {
        // Start at 0°N, 0°E
        let p0 = Position {
            latitude: 0.0,
            longitude: 0.0,
        };

        // Travel north at 6 knots for 10 minutes (1/6 hour)
        let p1 = calculate_position_from_bearing(p0, 0.0, 6.0, 600.0);

        // Should travel 1 nautical mile north
        // 1 NM ≈ 1/60 degree latitude
        assert_approx_equal(
            p1.latitude,
            1.0 / 60.0,
            0.0001,
            "Latitude after moving north",
        );
        assert_approx_equal(p1.longitude, 0.0, 0.0001, "Longitude should remain same");
    }

    #[test]
    fn test_generate_track() {
        let p0 = Position {
            latitude: 40.0,
            longitude: -70.0,
        };
        let p1 = Position {
            latitude: 40.0,
            longitude: -69.0,
        };
        let start_time = UNIX_EPOCH + Duration::from_secs(1000000);

        let track = generate_track(p0, p1, 6.0, 600, start_time);

        // Should have multiple waypoints
        assert!(track.len() > 1, "Track should have multiple points");

        // First point should be at start
        assert_approx_equal(track[0].0.latitude, p0.latitude, 0.0001, "First point lat");
        assert_approx_equal(
            track[0].0.longitude,
            p0.longitude,
            0.0001,
            "First point lon",
        );

        // Last point should be at end (with some tolerance for interpolation)
        let last = track.last().unwrap();
        assert_approx_equal(last.0.latitude, p1.latitude, 0.05, "Last point lat");
        assert_approx_equal(last.0.longitude, p1.longitude, 0.05, "Last point lon");
    }
}
