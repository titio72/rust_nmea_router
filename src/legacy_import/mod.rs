// Shared data types for importing pre-2020 trips from the legacy `nmearouter`
// database (old `trip`/`track`/`meteo` tables) into the current schema.
// See docs/superpowers/specs/2026-09-04-legacy-trip-import-design.md.

pub mod geometry;
pub mod transform;
pub mod source;

#[derive(Debug, Clone)]
pub struct LegacyTrip {
    pub id: i64,
    pub description: Option<String>,
    pub from_ts: chrono::NaiveDateTime,
    pub to_ts: chrono::NaiveDateTime,
}

#[derive(Debug, Clone)]
pub struct LegacyTrackRow {
    pub ts: chrono::NaiveDateTime,
    pub lat: f64,
    pub lon: f64,
    /// 0 = underway, non-zero = anchored/moored.
    pub anchor: i32,
    /// Seconds since the previous track row; NULL on the first row of a trip.
    pub d_time: Option<i64>,
    /// Nautical miles since the previous track row.
    pub dist: Option<f64>,
    pub speed: f64,
    pub max_speed: f64,
    /// 0 = off, 1 = on, 2 = unknown (same encoding as engine_on).
    pub engine: u8,
}

#[derive(Debug, Clone)]
pub struct LegacyMeteoRow {
    pub ts: chrono::NaiveDateTime,
    pub metric_id: u8,
    pub v: f64,
    pub v_min: Option<f64>,
    pub v_max: Option<f64>,
    pub unit: Option<String>,
}
