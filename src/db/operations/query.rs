use crate::db::types::{
    format_duration_ms, FastestSegment, HeatmapData, HeatmapDay, MonthlyStatistic,
    MonthlyStatistics, MultiMetricData, NavAnalysisRow, SpeedDistributionData,
    TrackPoint, TripLeg, TripLegsData, TripSummary, VesselDatabase, WebMetricData,
    WindStatisticsData,
};
use crate::error::AppError;
use crate::utilities::haversine_distance_nm;
use chrono::{DateTime, NaiveDate, Utc};
use mysql::params;
use mysql::prelude::Queryable;
use std::time::Instant;
use tracing::{info, warn};

/// Log the elapsed time of a operation/phase pair, with an optional row count.
/// Used to break down where server time goes within a single request, independent
/// of how algorithmically complex the phase is (a tight SQL query and an O(n^2)
/// in-memory loop are both just "elapsed_ms" here).
fn log_timing(operation: &str, phase: &str, start: Instant, rows: Option<usize>) {
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    match rows {
        Some(rows) => info!(operation, phase, rows, elapsed_ms, "timing"),
        None => info!(operation, phase, elapsed_ms, "timing"),
    }
}

/// Get a value from a database row, logging a warning if the default is used.
/// This provides observability for NULL/missing columns without breaking API contracts.
fn get_or_log<T>(row: &mysql::Row, column: &str, default: T, context: &str) -> T
where
    T: mysql::prelude::FromValue,
{
    match row.get_opt::<T, _>(column) {
        Some(Ok(v)) => v,
        Some(Err(e)) => {
            warn!(
                "[{}] Column '{}' conversion error: {}, using default",
                context, column, e
            );
            default
        }
        None => {
            warn!(
                "[{}] Column '{}' is NULL/missing, using default",
                context, column
            );
            default
        }
    }
}

/// Reconstruct an `Option<FastestSegment>` from a `trip_legs_cache` row's `{prefix}_*` columns.
/// All five columns are written together (see `save_trip_legs_to_cache`) so partial-NULL rows
/// only occur pre-migration; treat any missing field as "no segment" rather than panicking.
fn fastest_segment_from_row(row: &mysql::Row, prefix: &str) -> Option<FastestSegment> {
    let distance_nm: Option<f64> = row
        .get_opt(format!("{prefix}_distance_nm").as_str())
        .and_then(|v| v.ok());
    let average_speed_kn: Option<f64> = row
        .get_opt(format!("{prefix}_avg_speed_kn").as_str())
        .and_then(|v| v.ok());
    let duration_ms: Option<u64> = row
        .get_opt(format!("{prefix}_duration_ms").as_str())
        .and_then(|v| v.ok());
    let start_timestamp: Option<String> = row
        .get_opt(format!("{prefix}_start_timestamp").as_str())
        .and_then(|v: Result<Option<String>, _>| v.ok())
        .flatten();
    let end_timestamp: Option<String> = row
        .get_opt(format!("{prefix}_end_timestamp").as_str())
        .and_then(|v: Result<Option<String>, _>| v.ok())
        .flatten();
    match (distance_nm, average_speed_kn, duration_ms, start_timestamp, end_timestamp) {
        (Some(distance_nm), Some(average_speed_kn), Some(duration_ms), Some(start_timestamp), Some(end_timestamp)) => {
            Some(FastestSegment {
                distance_nm,
                average_speed_kn,
                duration_ms,
                start_timestamp,
                end_timestamp,
            })
        }
        _ => None,
    }
}

const NAV_SPEED_THRESHOLD_KN: f64 = 4.0;

/// Downsample a chronologically-ordered sequence to at most `max_points` entries by
/// stride decimation (keep every Nth element, N = ceil(len / max_points)).
/// Unlike a fixed time-interval filter, this caps the *total* output count regardless
/// of how densely or sparsely the input is already sampled — a filter that drops points
/// closer than some interval does nothing when the source is already coarser than that.
fn decimate<T>(items: Vec<T>, max_points: Option<usize>) -> Vec<T> {
    let Some(max_points) = max_points.filter(|&m| m > 0) else {
        return items;
    };
    if items.len() <= max_points {
        return items;
    }
    let stride = items.len().div_ceil(max_points);
    items.into_iter().step_by(stride).collect()
}

/// Parse two ISO-8601 timestamps and return the millisecond difference (b - a).
/// Returns 0 if either is None or unparseable.
fn parse_trim_ms(a: &Option<String>, b: &Option<String>) -> u64 {
    let (Some(a_str), Some(b_str)) = (a.as_ref(), b.as_ref()) else {
        return 0;
    };
    let Ok(ta) = chrono::DateTime::parse_from_rfc3339(a_str) else {
        return 0;
    };
    let Ok(tb) = chrono::DateTime::parse_from_rfc3339(b_str) else {
        return 0;
    };
    let diff = (tb - ta).num_milliseconds();
    if diff > 0 {
        diff as u64
    } else {
        0
    }
}

struct LegRecord {
    timestamp: String,
    speed_kn: f64,
    distance_nm: f64,
    time_ms: u64,
    engine_on: bool,
    lat: Option<f64>,
    lon: Option<f64>,
}

/// Returns (index, detection_method): first engine-off or first speed-above-threshold record.
fn find_nav_start_idx(records: &[LegRecord]) -> (Option<usize>, &'static str) {
    if records.first().map(|r| r.engine_on).unwrap_or(false) {
        let idx = records.iter().position(|r| !r.engine_on);
        (idx, "engine_transition")
    } else {
        let idx = records
            .iter()
            .position(|r| r.speed_kn >= NAV_SPEED_THRESHOLD_KN);
        (idx, "speed_fallback")
    }
}

/// Returns (index, detection_method): last engine-off before final engine-on, or last speed-above-threshold.
fn find_nav_end_idx(records: &[LegRecord]) -> (Option<usize>, &'static str) {
    if records.last().map(|r| r.engine_on).unwrap_or(false) {
        let idx = records.iter().rposition(|r| !r.engine_on);
        (idx, "engine_transition")
    } else {
        let idx = records
            .iter()
            .rposition(|r| r.speed_kn >= NAV_SPEED_THRESHOLD_KN);
        (idx, "speed_fallback")
    }
}

fn finalize_leg(
    records: &[LegRecord],
    leg_number: u32,
    start_lat: Option<f64>,
    start_lon: Option<f64>,
) -> Option<TripLeg> {
    let total_distance: f64 = records.iter().map(|r| r.distance_nm).sum();
    if total_distance < 0.5 {
        return None;
    }

    let mut sailing_distance = 0.0_f64;
    let mut motoring_distance = 0.0_f64;
    let mut sailing_time = 0_u64;
    let mut motoring_time = 0_u64;
    for r in records {
        if r.engine_on {
            motoring_distance += r.distance_nm;
            motoring_time += r.time_ms;
        } else {
            sailing_distance += r.distance_nm;
            sailing_time += r.time_ms;
        }
    }

    let (nav_start_idx, start_method) = find_nav_start_idx(records);
    let (nav_end_idx, end_method) = find_nav_end_idx(records);

    let (
        nav_start_timestamp,
        nav_end_timestamp,
        nav_distance_nm,
        nav_time_ms,
        nav_detection_method,
    ) = match (nav_start_idx, nav_end_idx) {
        (Some(si), Some(ei)) if si <= ei => {
            let nav_dist = records[si..=ei].iter().map(|r| r.distance_nm).sum();
            let nav_time = records[si..=ei].iter().map(|r| r.time_ms).sum();
            let method = if start_method == "engine_transition" && end_method == "engine_transition"
            {
                "engine_transition"
            } else {
                "speed_fallback"
            };
            (
                Some(records[si].timestamp.clone()),
                Some(records[ei].timestamp.clone()),
                nav_dist,
                nav_time,
                Some(method.to_string()),
            )
        }
        _ => (None, None, 0.0, 0, None),
    };

    let end_lat = records.iter().rev().find_map(|r| r.lat);
    let end_lon = records.iter().rev().find_map(|r| r.lon);

    let start_timestamp = records
        .first()
        .map(|r| r.timestamp.clone())
        .unwrap_or_default();
    let end_timestamp = records
        .last()
        .map(|r| r.timestamp.clone())
        .unwrap_or_default();

    // Every record in `records` already belongs to a non-moored stretch (compute_trip_legs only
    // pushes here when !is_moored), so no separate moored filter is needed — unlike the old
    // whole-trip algorithm, which only excluded moored points from the distance/time sums, not
    // from max-speed tracking.
    let mut max_speed_kn: Option<f64> = None;
    let mut max_speed_timestamp: Option<String> = None;
    for r in records {
        if !r.engine_on && (max_speed_kn.is_none() || r.speed_kn > max_speed_kn.unwrap()) {
            max_speed_kn = Some(r.speed_kn);
            max_speed_timestamp = Some(r.timestamp.clone());
        }
    }
    let fastest_1nm = fastest_segment_in_leg(records, 1.0);
    let fastest_5nm = fastest_segment_in_leg(records, 5.0);
    let fastest_10nm = fastest_segment_in_leg(records, 10.0);
    let fastest_25nm = fastest_segment_in_leg(records, 25.0);

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
}

impl VesselDatabase {
    #[cfg(test)]
    pub fn save_trip_legs_to_cache_for_test(
        &self,
        trip_id: u32,
        legs: &[crate::db::types::TripLeg],
    ) -> Result<(), AppError> {
        let mut conn = self.pool.get_conn()?;
        self.save_trip_legs_to_cache(&mut conn, trip_id, legs)
    }

    #[cfg(test)]
    pub fn get_cached_trip_legs_for_test(
        &self,
        trip_id: u32,
    ) -> Result<Option<crate::db::types::TripLegsData>, AppError> {
        let mut conn = self.pool.get_conn()?;
        self.get_cached_trip_legs(&mut conn, trip_id)
    }

    pub fn fetch_trip(&self, trip_id: u32) -> Result<Option<TripSummary>, AppError> {
        let t0 = Instant::now();
        let mut conn = self.pool.get_conn()?;

        let row: Option<mysql::Row> = conn.exec_first(
            r"SELECT id, description,
                     DATE_FORMAT(start_timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as start_ts,
                     DATE_FORMAT(end_timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as end_ts,
                     total_distance_sailed, total_distance_motoring,
                     (total_distance_sailed + total_distance_motoring) as total_distance,
                     (total_time_sailing + total_time_motoring + total_time_moored) as total_time,
                     total_time_sailing, total_time_motoring, total_time_moored, uuid
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
            };
            log_timing("fetch_trip", "total", t0, Some(1));
            Ok(Some(trip))
        } else {
            log_timing("fetch_trip", "total", t0, Some(0));
            Ok(None)
        }
    }

    /// Fetch trips with optional filtering
    pub fn fetch_trips(
        &self,
        year: Option<i32>,
        last_months: Option<u32>,
    ) -> Result<Vec<TripSummary>, AppError> {
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

        let mut conn = self.pool.get_conn()?;

        let results: Vec<mysql::Row> = if let Some(year) = year {
            conn.exec(
                format!(
                    "{} YEAR(start_timestamp) = :year ORDER BY start_timestamp DESC",
                    SELECT_TRIPS
                ),
                mysql::params! { "year" => year },
            )?
        } else if let Some(months) = last_months {
            conn.exec(
                format!("{} start_timestamp >= DATE_SUB(NOW(), INTERVAL :months MONTH) ORDER BY start_timestamp DESC", SELECT_TRIPS),
                mysql::params! { "months" => months },
            )?
        } else {
            conn.query(format!(
                "{} 1=1 ORDER BY start_timestamp DESC",
                SELECT_TRIPS
            ))?
        };

        let trips = results
            .iter()
            .map(|row| TripSummary {
                id: get_or_log(row, "id", 0u32, "fetch_trips"),
                uuid: row
                    .get_opt::<Option<String>, _>("uuid")
                    .and_then(|v| v.ok())
                    .flatten(),
                description: get_or_log(row, "description", String::new(), "fetch_trips"),
                start_date: get_or_log(row, "start_ts", String::new(), "fetch_trips"),
                end_date: get_or_log(row, "end_ts", String::new(), "fetch_trips"),
                total_distance_nm: get_or_log(row, "total_distance", 0.0f64, "fetch_trips"),
                total_time_ms: get_or_log(row, "total_time", 0i64, "fetch_trips"),
                sailing_time_ms: get_or_log(row, "total_time_sailing", 0i64, "fetch_trips"),
                motoring_time_ms: get_or_log(row, "total_time_motoring", 0i64, "fetch_trips"),
                moored_time_ms: get_or_log(row, "total_time_moored", 0i64, "fetch_trips"),
                sailing_distance_nm: get_or_log(
                    row,
                    "total_distance_sailed",
                    0.0f64,
                    "fetch_trips",
                ),
                motoring_distance_nm: get_or_log(
                    row,
                    "total_distance_motoring",
                    0.0f64,
                    "fetch_trips",
                ),
            })
            .collect();

        Ok(trips)
    }

    pub fn fetch_trip_by_uuid(&self, trip_uuid: &str) -> Result<Option<TripSummary>, AppError> {
        let mut conn = self.pool.get_conn()?;

        let row: Option<mysql::Row> = conn.exec_first(
            r"SELECT id, description,
                     DATE_FORMAT(start_timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as start_ts,
                     DATE_FORMAT(end_timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as end_ts,
                     total_distance_sailed, total_distance_motoring,
                     (total_distance_sailed + total_distance_motoring) as total_distance,
                     (total_time_sailing + total_time_motoring + total_time_moored) as total_time,
                     total_time_sailing, total_time_motoring, total_time_moored, uuid
              FROM trips
              WHERE uuid = :uuid",
            mysql::params! {
                "uuid" => trip_uuid,
            },
        )?;

        if let Some(row) = row {
            let trip = TripSummary {
                id: get_or_log(&row, "id", 0u32, "fetch_trip_by_uuid"),
                uuid: row
                    .get_opt::<Option<String>, _>("uuid")
                    .and_then(|v| v.ok())
                    .flatten(),
                description: get_or_log(&row, "description", String::new(), "fetch_trip_by_uuid"),
                start_date: get_or_log(&row, "start_ts", String::new(), "fetch_trip_by_uuid"),
                end_date: get_or_log(&row, "end_ts", String::new(), "fetch_trip_by_uuid"),
                total_distance_nm: get_or_log(&row, "total_distance", 0.0f64, "fetch_trip_by_uuid"),
                total_time_ms: get_or_log(&row, "total_time", 0i64, "fetch_trip_by_uuid"),
                sailing_time_ms: get_or_log(&row, "total_time_sailing", 0i64, "fetch_trip_by_uuid"),
                motoring_time_ms: get_or_log(
                    &row,
                    "total_time_motoring",
                    0i64,
                    "fetch_trip_by_uuid",
                ),
                moored_time_ms: get_or_log(&row, "total_time_moored", 0i64, "fetch_trip_by_uuid"),
                sailing_distance_nm: get_or_log(
                    &row,
                    "total_distance_sailed",
                    0.0f64,
                    "fetch_trip_by_uuid",
                ),
                motoring_distance_nm: get_or_log(
                    &row,
                    "total_distance_motoring",
                    0.0f64,
                    "fetch_trip_by_uuid",
                ),
            };
            Ok(Some(trip))
        } else {
            Ok(None)
        }
    }

    /// Fetch monthly statistics since January 2020
    /// Returns monthly sailed and motored nautical miles, including months with no activity
    pub fn fetch_monthly_statistics(&self) -> Result<MonthlyStatistics, AppError> {
        let mut conn = self.pool.get_conn()?;

        let results: Vec<mysql::Row> = conn.query(
            r"SELECT YEAR(`date`) as year,
                     MONTH(`date`) as month,
                     SUM(sailing_distance_nm) as sailing_distance,
                     SUM(motoring_distance_nm) as motoring_distance
              FROM heatmap_cache
              GROUP BY YEAR(`date`), MONTH(`date`)
              ORDER BY year ASC, month ASC",
        )?;

        // heatmap_cache is only populated lazily (when the heatmap view is requested) and
        // deliberately never caches "today" (see fetch_heatmap). Any day after the last
        // cached date is therefore missing here — most importantly the still-open current
        // day/trip. Fill that gap by summing vessel_status directly for everything after
        // the newest cached date.
        let last_cached_date: Option<String> = conn
            .query_first::<Option<String>, _>(
                r"SELECT DATE_FORMAT(MAX(`date`), '%Y-%m-%d') FROM heatmap_cache",
            )?
            .flatten();

        let live_results: Vec<mysql::Row> = conn.exec(
            r"SELECT YEAR(timestamp) as year,
                     MONTH(timestamp) as month,
                     SUM(CASE WHEN engine_on = 0 THEN COALESCE(total_distance_nm, 0) ELSE 0 END) as sailing_distance,
                     SUM(CASE WHEN engine_on = 1 THEN COALESCE(total_distance_nm, 0) ELSE 0 END) as motoring_distance
              FROM vessel_status
              WHERE is_moored = 0 AND DATE(timestamp) > :since
              GROUP BY YEAR(timestamp), MONTH(timestamp)",
            mysql::params! {
                "since" => last_cached_date.unwrap_or_else(|| "1970-01-01".to_string()),
            },
        )?;

        /*
        // Get all trip data grouped by year and month
        let results: Vec<mysql::Row> = conn.query(
            r"SELECT YEAR(start_timestamp) as year,
                     MONTH(start_timestamp) as month,
                     SUM(total_distance_sailed) as sailing_distance,
                     SUM(total_distance_motoring) as motoring_distance
              FROM trips
              WHERE start_timestamp >= '2020-01-01'
              GROUP BY YEAR(start_timestamp), MONTH(start_timestamp)
              ORDER BY year ASC, month ASC",
        )?;
        */

        // Build a map of (year, month) -> (sailing_distance, motoring_distance)
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

        // Generate all months from January 2020 to now
        use chrono::Datelike;
        let now = chrono::Local::now();
        let current_year = now.year();
        let current_month = now.month();

        let mut all_months = Vec::new();

        for year in 2020..=current_year {
            let start_month = 1;
            let end_month = if year == current_year {
                current_month
            } else {
                12
            };

            for month in start_month..=end_month {
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
            }
        }

        Ok(MonthlyStatistics { months: all_months })
    }

    /// Fetch vessel track data by trip_id or date range
    pub fn fetch_track(
        &self,
        trip_id: Option<u32>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        max_points: Option<usize>,
    ) -> Result<Vec<TrackPoint>, AppError> {
        let t_sql = Instant::now();
        let mut conn = self.pool.get_conn()?;

        let results: Vec<mysql::Row> = if let Some(trip_id) = trip_id {
            conn.exec(
                "SELECT DATE_FORMAT(timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as timestamp,
                        latitude, longitude, average_speed_kn, max_speed_kn,
                        is_moored, engine_on, total_distance_nm, total_time_ms,
                        average_wind_speed_kn, average_wind_angle_deg,
                        cog_deg, average_heading_deg
                 FROM vessel_status
                 WHERE timestamp BETWEEN
                     (SELECT start_timestamp FROM trips WHERE id = :trip_id)
                     AND COALESCE((SELECT end_timestamp FROM trips WHERE id = :trip_id), NOW())
                 ORDER BY timestamp",
                mysql::params! { "trip_id" => trip_id },
            )?
        } else if let (Some(start), Some(end)) = (start, end) {
            conn.exec(
                "SELECT DATE_FORMAT(timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as timestamp,
                        latitude, longitude, average_speed_kn, max_speed_kn, is_moored, engine_on,
                        total_distance_nm, total_time_ms,
                        average_wind_speed_kn, average_wind_angle_deg,
                        cog_deg, average_heading_deg
                 FROM vessel_status WHERE timestamp BETWEEN :start AND :end ORDER BY timestamp",
                mysql::params! {
                    "start" => start.format("%Y-%m-%d %H:%M:%S").to_string(),
                    "end" => end.format("%Y-%m-%d %H:%M:%S").to_string(),
                },
            )?
        } else {
            return Err(AppError::Database(
                "Either trip_id or both start and end timestamps are required".to_string(),
            ));
        };
        log_timing("fetch_track", "sql_query", t_sql, Some(results.len()));
        let t_downsample = Instant::now();

        let track: Vec<TrackPoint> = results
            .iter()
            .map(|row| TrackPoint {
                timestamp: row
                    .get_opt::<String, _>("timestamp")
                    .and_then(|v| v.ok())
                    .unwrap_or_default(),
                latitude: row.get_opt::<f64, _>("latitude").and_then(|v| v.ok()),
                longitude: row.get_opt::<f64, _>("longitude").and_then(|v| v.ok()),
                avg_speed_kn: row
                    .get_opt::<f64, _>("average_speed_kn")
                    .and_then(|v| v.ok()),
                max_speed_kn: row.get_opt::<f64, _>("max_speed_kn").and_then(|v| v.ok()),
                moored: row
                    .get_opt::<i32, _>("is_moored")
                    .and_then(|v| v.ok())
                    .map(|v| v != 0)
                    .unwrap_or(false),
                engine_on: row
                    .get_opt::<u8, _>("engine_on")
                    .and_then(|v| v.ok())
                    .unwrap_or(2), // Default to unknown if not available
                total_distance_nm: row
                    .get_opt::<f64, _>("total_distance_nm")
                    .and_then(|v| v.ok()),
                total_time_ms: row
                    .get_opt::<u64, _>("total_time_ms")
                    .and_then(|v| v.ok())
                    .unwrap_or(0),
                average_wind_speed_kn: row
                    .get_opt::<f64, _>("average_wind_speed_kn")
                    .and_then(|v| v.ok()),
                average_wind_angle_deg: row
                    .get_opt::<f64, _>("average_wind_angle_deg")
                    .and_then(|v| v.ok()),
                cog_deg: row.get_opt::<f64, _>("cog_deg").and_then(|v| v.ok()),
                average_heading_deg: row
                    .get_opt::<f64, _>("average_heading_deg")
                    .and_then(|v| v.ok()),
                polar_speed_kn: None,
                polar_ratio: None,
            })
            .collect();
        let track = decimate(track, max_points);

        log_timing("fetch_track", "downsample", t_downsample, Some(track.len()));
        Ok(track)
    }

    /// Fetch environmental metrics by metric_id with optional trip_id or date range
    pub fn fetch_metrics(
        &self,
        metric: &str,
        trip_id: Option<u32>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        max_points: Option<usize>,
    ) -> Result<Vec<WebMetricData>, AppError> {
        let t_sql = Instant::now();
        let mut conn = self.pool.get_conn()?;

        let results: Vec<mysql::Row> = if let Some(trip_id) = trip_id {
            conn.exec(
                "SELECT DATE_FORMAT(e.timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as timestamp,
                        e.metric_id, e.value_avg, e.value_max, e.value_min
                 FROM environmental_data e
                 WHERE e.timestamp >= (SELECT COALESCE(start_timestamp, NOW()) FROM trips WHERE id = :trip_id)
                   AND e.timestamp <= (SELECT COALESCE(end_timestamp, NOW()) FROM trips WHERE id = :trip_id)
                   AND e.metric_id = :metric
                 ORDER BY e.timestamp",
                mysql::params! { "trip_id" => trip_id, "metric" => metric },
            )?
        } else if let (Some(start), Some(end)) = (start, end) {
            conn.exec(
                "SELECT DATE_FORMAT(timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as timestamp,
                        metric_id, value_avg, value_max, value_min
                 FROM environmental_data
                 WHERE metric_id = :metric AND timestamp BETWEEN :start AND :end
                 ORDER BY timestamp",
                mysql::params! {
                    "metric" => metric,
                    "start" => start.format("%Y-%m-%d %H:%M:%S").to_string(),
                    "end" => end.format("%Y-%m-%d %H:%M:%S").to_string(),
                },
            )?
        } else {
            return Err(AppError::Database(
                "Either trip_id or both start and end timestamps are required".to_string(),
            ));
        };
        log_timing("fetch_metrics", "sql_query", t_sql, Some(results.len()));
        let t_downsample = Instant::now();

        let metrics: Vec<WebMetricData> = results
            .iter()
            .map(|row| WebMetricData {
                timestamp: get_or_log(row, "timestamp", String::new(), "fetch_metrics"),
                // metric_id is TINYINT UNSIGNED — read as u8 then convert to string.
                // get_opt::<String, _> silently fails on integer columns in the mysql crate.
                metric_id: row
                    .get_opt::<u8, _>("metric_id")
                    .and_then(|v| v.ok())
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| {
                        warn!("[fetch_metrics] Column 'metric_id' is NULL/missing or not convertible, using default");
                        String::new()
                    }),
                avg_value: row.get_opt::<f64, _>("value_avg").and_then(|v| v.ok()),
                max_value: row.get_opt::<f64, _>("value_max").and_then(|v| v.ok()),
                min_value: row.get_opt::<f64, _>("value_min").and_then(|v| v.ok()),
            })
            .collect();

        // Downsample if max_points is requested and result exceeds the limit
        let metrics = if let Some(max) = max_points {
            if metrics.len() > max && max > 0 {
                let bucket_size = metrics.len().div_ceil(max);
                metrics
                    .chunks(bucket_size)
                    .map(|chunk| {
                        let timestamp = chunk[0].timestamp.clone();
                        let metric_id = chunk[0].metric_id.clone();
                        let avg_values: Vec<f64> =
                            chunk.iter().filter_map(|p| p.avg_value).collect();
                        let max_values: Vec<f64> =
                            chunk.iter().filter_map(|p| p.max_value).collect();
                        let min_values: Vec<f64> =
                            chunk.iter().filter_map(|p| p.min_value).collect();
                        let avg_value = if avg_values.is_empty() {
                            None
                        } else {
                            Some(avg_values.iter().sum::<f64>() / avg_values.len() as f64)
                        };
                        let max_value = if max_values.is_empty() {
                            None
                        } else {
                            max_values.iter().cloned().reduce(f64::max)
                        };
                        let min_value = if min_values.is_empty() {
                            None
                        } else {
                            min_values.iter().cloned().reduce(f64::min)
                        };
                        WebMetricData {
                            timestamp,
                            metric_id,
                            avg_value,
                            max_value,
                            min_value,
                        }
                    })
                    .collect()
            } else {
                metrics
            }
        } else {
            metrics
        };

        log_timing("fetch_metrics", "downsample", t_downsample, Some(metrics.len()));
        Ok(metrics)
    }

    /// Fetch multiple environmental metrics in a single query and return them as a map of metric_id → time series.
    /// This is more efficient than calling fetch_metrics repeatedly for the same time range.
    pub fn fetch_metrics_batch(
        &self,
        metrics: &[u8],
        trip_id: Option<u32>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        max_points: Option<usize>,
    ) -> Result<MultiMetricData, AppError> {
        if metrics.is_empty() {
            return Err(AppError::Database(
                "At least one metric_id is required".to_string(),
            ));
        }

        let in_clause = metrics
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<_>>()
            .join(",");

        let mut conn = self.pool.get_conn()?;

        let t_sql = Instant::now();
        // in_clause is built from &[u8] typed integers — safe to inline.
        let results: Vec<mysql::Row> = if let Some(trip_id) = trip_id {
            conn.exec(
                format!(
                    "SELECT DATE_FORMAT(e.timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as timestamp, \
                            e.metric_id, e.value_avg, e.value_max, e.value_min \
                     FROM environmental_data e \
                     WHERE e.timestamp >= (SELECT COALESCE(start_timestamp, NOW()) FROM trips WHERE id = :trip_id) \
                       AND e.timestamp <= (SELECT COALESCE(end_timestamp, NOW()) FROM trips WHERE id = :trip_id) \
                       AND e.metric_id IN ({in_clause}) \
                     ORDER BY e.timestamp",
                    in_clause = in_clause
                ),
                mysql::params! { "trip_id" => trip_id },
            )?
        } else if let (Some(start), Some(end)) = (start, end) {
            conn.exec(
                format!(
                    "SELECT DATE_FORMAT(timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as timestamp, \
                            metric_id, value_avg, value_max, value_min \
                     FROM environmental_data \
                     WHERE metric_id IN ({in_clause}) \
                       AND timestamp BETWEEN :start AND :end \
                     ORDER BY timestamp",
                    in_clause = in_clause
                ),
                mysql::params! {
                    "start" => start.format("%Y-%m-%d %H:%M:%S").to_string(),
                    "end" => end.format("%Y-%m-%d %H:%M:%S").to_string(),
                },
            )?
        } else {
            return Err(AppError::Database(
                "Either trip_id or both start and end timestamps are required".to_string(),
            ));
        };
        log_timing("fetch_metrics_batch", "sql_query", t_sql, Some(results.len()));
        let t_downsample = Instant::now();

        // Partition rows into per-metric Vecs
        let mut map: std::collections::HashMap<String, Vec<WebMetricData>> =
            std::collections::HashMap::new();
        for row in &results {
            // metric_id is TINYINT UNSIGNED — read as u8 then convert to string key.
            // get_opt::<String, _> silently fails on integer columns in the mysql crate.
            let metric_id = row
                .get_opt::<u8, _>("metric_id")
                .and_then(|v| v.ok())
                .map(|v| v.to_string())
                .unwrap_or_default();
            let entry = map.entry(metric_id.clone()).or_default();
            entry.push(WebMetricData {
                timestamp: row
                    .get_opt::<String, _>("timestamp")
                    .and_then(|v| v.ok())
                    .unwrap_or_default(),
                metric_id,
                avg_value: row.get_opt::<f64, _>("value_avg").and_then(|v| v.ok()),
                max_value: row.get_opt::<f64, _>("value_max").and_then(|v| v.ok()),
                min_value: row.get_opt::<f64, _>("value_min").and_then(|v| v.ok()),
            });
        }

        // Downsample each metric series independently
        let map = if let Some(max) = max_points {
            if max > 0 {
                map.into_iter()
                    .map(|(key, series)| {
                        let downsampled = if series.len() > max {
                            let bucket_size = series.len().div_ceil(max);
                            series
                                .chunks(bucket_size)
                                .map(|chunk| {
                                    let timestamp = chunk[0].timestamp.clone();
                                    let metric_id = chunk[0].metric_id.clone();
                                    let avg_values: Vec<f64> =
                                        chunk.iter().filter_map(|p| p.avg_value).collect();
                                    let max_values: Vec<f64> =
                                        chunk.iter().filter_map(|p| p.max_value).collect();
                                    let min_values: Vec<f64> =
                                        chunk.iter().filter_map(|p| p.min_value).collect();
                                    WebMetricData {
                                        timestamp,
                                        metric_id,
                                        avg_value: if avg_values.is_empty() {
                                            None
                                        } else {
                                            Some(
                                                avg_values.iter().sum::<f64>()
                                                    / avg_values.len() as f64,
                                            )
                                        },
                                        max_value: if max_values.is_empty() {
                                            None
                                        } else {
                                            max_values.iter().cloned().reduce(f64::max)
                                        },
                                        min_value: if min_values.is_empty() {
                                            None
                                        } else {
                                            min_values.iter().cloned().reduce(f64::min)
                                        },
                                    }
                                })
                                .collect()
                        } else {
                            series
                        };
                        (key, downsampled)
                    })
                    .collect()
            } else {
                map
            }
        } else {
            map
        };

        log_timing(
            "fetch_metrics_batch",
            "downsample",
            t_downsample,
            Some(map.values().map(|v| v.len()).sum()),
        );
        Ok(MultiMetricData { metrics: map })
    }

    /// Fetch speed distribution data for a trip
    pub fn fetch_speed_distribution(
        &self,
        trip_id: Option<u32>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    ) -> Result<SpeedDistributionData, AppError> {
        // Create buckets for speeds from 0 to 10 knots in 0.5 knot increments
        let max_speed = 10.0_f64;
        let bucket_size = 0.5_f64;
        let num_buckets = (max_speed / bucket_size).ceil() as usize;

        let mut sailing_buckets = vec![0.0; num_buckets];
        let mut motoring_buckets = vec![0.0; num_buckets];
        let mut labels = Vec::with_capacity(num_buckets);

        for i in 0..num_buckets {
            let min_speed = i as f64 * bucket_size;
            let max_speed_label = (i + 1) as f64 * bucket_size;
            labels.push(format!("{:.1}-{:.1}", min_speed, max_speed_label));
        }

        // Aggregate on the database side: one row per 0.5-kn speed bucket
        let mut conn = self.pool.get_conn()?;
        let t_sql = Instant::now();

        let results: Vec<mysql::Row> = if let Some(trip_id) = trip_id {
            conn.exec(
                "SELECT FLOOR(average_speed_kn / 0.5) * 0.5 AS speed,
                        SUM(total_distance_nm * IF(engine_on = 1, 0, 1)) AS dist_sail,
                        SUM(total_distance_nm * IF(engine_on = 1, 1, 0)) AS dist_engine
                 FROM vessel_status
                 WHERE timestamp BETWEEN
                     (SELECT start_timestamp FROM trips WHERE id = :trip_id)
                     AND COALESCE((SELECT end_timestamp FROM trips WHERE id = :trip_id), NOW())
                 AND is_moored = 0
                 AND average_speed_kn IS NOT NULL
                 GROUP BY FLOOR(average_speed_kn / 0.5) * 0.5",
                mysql::params! { "trip_id" => trip_id },
            )?
        } else if let (Some(start), Some(end)) = (start, end) {
            conn.exec(
                "SELECT FLOOR(average_speed_kn / 0.5) * 0.5 AS speed,
                        SUM(total_distance_nm * IF(engine_on = 1, 0, 1)) AS dist_sail,
                        SUM(total_distance_nm * IF(engine_on = 1, 1, 0)) AS dist_engine
                 FROM vessel_status
                 WHERE timestamp BETWEEN :start AND :end
                 AND is_moored = 0
                 AND average_speed_kn IS NOT NULL
                 GROUP BY FLOOR(average_speed_kn / 0.5) * 0.5",
                mysql::params! {
                    "start" => start.format("%Y-%m-%d %H:%M:%S").to_string(),
                    "end" => end.format("%Y-%m-%d %H:%M:%S").to_string(),
                },
            )?
        } else {
            return Err(AppError::Database(
                "Either trip_id or both start and end timestamps are required".to_string(),
            ));
        };
        log_timing(
            "fetch_speed_distribution",
            "sql_query",
            t_sql,
            Some(results.len()),
        );

        for row in results {
            let speed: f64 = row
                .get_opt::<f64, _>("speed")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let dist_sail: f64 = row
                .get_opt::<f64, _>("dist_sail")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let dist_engine: f64 = row
                .get_opt::<f64, _>("dist_engine")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);

            let bucket_index = ((speed / bucket_size).round() as usize).min(num_buckets - 1);
            sailing_buckets[bucket_index] += dist_sail;
            motoring_buckets[bucket_index] += dist_engine;
        }

        Ok(SpeedDistributionData {
            labels,
            sailing: sailing_buckets,
            motoring: motoring_buckets,
        })
    }

    /// Fetch wind statistics data for a trip or time range
    pub fn fetch_wind_statistics(
        &self,
        trip_id: Option<u32>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    ) -> Result<WindStatisticsData, AppError> {
        // Create 72 buckets for wind directions (360 degrees / 5 degrees = 72 buckets)
        let bucket_size = 5.0;
        let num_buckets = 72usize;

        let mut wind_distances = vec![0.0; num_buckets];
        let mut max_wind_speeds = vec![0.0; num_buckets];
        let mut directions = Vec::with_capacity(num_buckets);

        for i in 0..num_buckets {
            directions.push(i as f64 * bucket_size);
        }

        // Aggregate on the database side: one row per 5-degree wind-angle bucket.
        // Wind distance = speed (kn) * period duration (h) = speed * total_time_ms / 3_600_000
        let mut conn = self.pool.get_conn()?;
        let t_sql = Instant::now();

        let results: Vec<mysql::Row> = if let Some(trip_id) = trip_id {
            conn.exec(
                "SELECT FLOOR(average_wind_angle_deg / 5.0) * 5.0 AS angle,
                        SUM(average_wind_speed_kn * total_time_ms / 3600000) AS dist_wind,
                        MAX(average_wind_speed_kn) AS max_wind_speed
                 FROM vessel_status
                 WHERE timestamp BETWEEN
                     (SELECT start_timestamp FROM trips WHERE id = :trip_id)
                     AND COALESCE((SELECT end_timestamp FROM trips WHERE id = :trip_id), NOW())
                 AND is_moored = 0
                 AND average_wind_angle_deg IS NOT NULL
                 AND average_wind_speed_kn IS NOT NULL
                 GROUP BY FLOOR(average_wind_angle_deg / 5.0) * 5.0",
                mysql::params! { "trip_id" => trip_id },
            )?
        } else if let (Some(start), Some(end)) = (start, end) {
            conn.exec(
                "SELECT FLOOR(average_wind_angle_deg / 5.0) * 5.0 AS angle,
                        SUM(average_wind_speed_kn * total_time_ms / 3600000) AS dist_wind,
                        MAX(average_wind_speed_kn) AS max_wind_speed
                 FROM vessel_status
                 WHERE timestamp BETWEEN :start AND :end
                 AND is_moored = 0
                 AND average_wind_angle_deg IS NOT NULL
                 AND average_wind_speed_kn IS NOT NULL
                 GROUP BY FLOOR(average_wind_angle_deg / 5.0) * 5.0",
                mysql::params! {
                    "start" => start.format("%Y-%m-%d %H:%M:%S").to_string(),
                    "end" => end.format("%Y-%m-%d %H:%M:%S").to_string(),
                },
            )?
        } else {
            return Err(AppError::Database(
                "Either trip_id or both start and end timestamps are required".to_string(),
            ));
        };
        log_timing(
            "fetch_wind_statistics",
            "sql_query",
            t_sql,
            Some(results.len()),
        );

        for row in results {
            let angle: f64 = row
                .get_opt::<f64, _>("angle")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let dist_wind: f64 = row
                .get_opt::<f64, _>("dist_wind")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let max_wind: f64 = row
                .get_opt::<f64, _>("max_wind_speed")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);

            let normalized = angle % 360.0;
            let bucket_index = ((normalized / bucket_size).floor() as usize).min(num_buckets - 1);
            wind_distances[bucket_index] += dist_wind;
            max_wind_speeds[bucket_index] = f64::max(max_wind_speeds[bucket_index], max_wind);
        }

        Ok(WindStatisticsData {
            directions,
            wind_distances,
            max_wind_speeds,
        })
    }

    /// Fetch trip legs data - divides trip into legs between mooring periods.
    /// Results are cached in trip_legs_cache for closed trips (end_timestamp > 24h ago).
    /// User nav window overrides from trip_legs_nav_overrides are applied after computation.
    pub fn fetch_trip_legs(&self, trip_id: u32) -> Result<TripLegsData, AppError> {
        let t_total = Instant::now();
        let mut conn = self.pool.get_conn()?;

        let t_cache_check = Instant::now();
        let is_closed = self.trip_is_closed(&mut conn, trip_id)?;

        if is_closed {
            let cached = self.get_cached_trip_legs(&mut conn, trip_id)?;
            log_timing(
                "fetch_trip_legs",
                "cache_lookup",
                t_cache_check,
                Some(cached.as_ref().map(|c| c.legs.len()).unwrap_or(0)),
            );
            if let Some(mut cached) = cached {
                let t_nav = Instant::now();
                self.apply_nav_overrides(&mut conn, trip_id, &mut cached.legs)?;
                log_timing("fetch_trip_legs", "nav_overrides", t_nav, Some(cached.legs.len()));
                log_timing("fetch_trip_legs", "total_cache_hit", t_total, Some(cached.legs.len()));
                return Ok(cached);
            }
        } else {
            log_timing("fetch_trip_legs", "cache_lookup", t_cache_check, Some(0));
        }

        let t_compute = Instant::now();
        let mut legs_data = self.compute_trip_legs(&mut conn, trip_id)?;
        log_timing("fetch_trip_legs", "compute", t_compute, Some(legs_data.legs.len()));

        if is_closed {
            if let Err(e) = self.save_trip_legs_to_cache(&mut conn, trip_id, &legs_data.legs) {
                warn!(
                    "Failed to write trip_legs_cache for trip {}: {}",
                    trip_id, e
                );
            }
        }

        let t_nav = Instant::now();
        self.apply_nav_overrides(&mut conn, trip_id, &mut legs_data.legs)?;
        log_timing("fetch_trip_legs", "nav_overrides", t_nav, Some(legs_data.legs.len()));
        log_timing("fetch_trip_legs", "total_computed", t_total, Some(legs_data.legs.len()));
        Ok(legs_data)
    }

    fn apply_nav_overrides(
        &self,
        conn: &mut mysql::PooledConn,
        trip_id: u32,
        legs: &mut [TripLeg],
    ) -> Result<(), AppError> {
        conn.query_drop(
            r"CREATE TABLE IF NOT EXISTS trip_legs_nav_overrides (
                trip_id        INT UNSIGNED NOT NULL,
                leg_number     INT UNSIGNED NOT NULL,
                nav_start      VARCHAR(30)  NULL,
                nav_end        VARCHAR(30)  NULL,
                auto_nav_start VARCHAR(30)  NULL,
                auto_nav_end   VARCHAR(30)  NULL,
                corrected_at   DATETIME(3)  NOT NULL,
                PRIMARY KEY (trip_id, leg_number)
              ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci",
        )?;

        let rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT leg_number, nav_start, nav_end FROM trip_legs_nav_overrides WHERE trip_id = :trip_id",
                mysql::params! { "trip_id" => trip_id },
            )
?;

        for row in &rows {
            let leg_num: u32 = get_or_log(row, "leg_number", 0u32, "apply_nav_overrides");
            let nav_start: Option<String> = row
                .get_opt("nav_start")
                .and_then(|v: Result<Option<String>, _>| v.ok())
                .flatten();
            let nav_end: Option<String> = row
                .get_opt("nav_end")
                .and_then(|v: Result<Option<String>, _>| v.ok())
                .flatten();
            if let Some(leg) = legs.iter_mut().find(|l| l.leg_number == leg_num) {
                leg.nav_start_timestamp = nav_start;
                leg.nav_end_timestamp = nav_end;
                leg.nav_detection_method = Some("user_override".to_string());
            }
        }
        Ok(())
    }

    fn trip_is_closed(&self, conn: &mut mysql::PooledConn, trip_id: u32) -> Result<bool, AppError> {
        let count: u32 = conn
            .exec_first(
                "SELECT COUNT(*) FROM trips WHERE id = :trip_id AND end_timestamp < DATE_SUB(NOW(), INTERVAL 24 HOUR)",
                mysql::params! { "trip_id" => trip_id },
            )
?
            .unwrap_or(0);
        Ok(count > 0)
    }

    fn get_cached_trip_legs(
        &self,
        conn: &mut mysql::PooledConn,
        trip_id: u32,
    ) -> Result<Option<TripLegsData>, AppError> {
        conn.query_drop(
            r"CREATE TABLE IF NOT EXISTS trip_legs_cache (
                trip_id              INT UNSIGNED    NOT NULL,
                leg_number           INT UNSIGNED    NOT NULL,
                start_timestamp      VARCHAR(30)     NOT NULL,
                end_timestamp        VARCHAR(30)     NOT NULL,
                total_distance_nm    DOUBLE          NOT NULL DEFAULT 0,
                sailing_distance_nm  DOUBLE          NOT NULL DEFAULT 0,
                motoring_distance_nm DOUBLE          NOT NULL DEFAULT 0,
                sailing_time_ms      BIGINT UNSIGNED NOT NULL DEFAULT 0,
                motoring_time_ms     BIGINT UNSIGNED NOT NULL DEFAULT 0,
                start_lat            DOUBLE          NULL,
                start_lon            DOUBLE          NULL,
                end_lat              DOUBLE          NULL,
                end_lon              DOUBLE          NULL,
                nav_start_timestamp  VARCHAR(30)     NULL,
                nav_end_timestamp    VARCHAR(30)     NULL,
                nav_distance_nm      DOUBLE          NOT NULL DEFAULT 0,
                nav_time_ms          BIGINT UNSIGNED NOT NULL DEFAULT 0,
                nav_detection_method VARCHAR(20)     NULL,
                max_speed_kn                 DOUBLE          NULL,
                max_speed_timestamp          VARCHAR(30)     NULL,
                fastest_1nm_distance_nm      DOUBLE          NULL,
                fastest_1nm_avg_speed_kn     DOUBLE          NULL,
                fastest_1nm_duration_ms      BIGINT UNSIGNED NULL,
                fastest_1nm_start_timestamp  VARCHAR(30)     NULL,
                fastest_1nm_end_timestamp    VARCHAR(30)     NULL,
                fastest_5nm_distance_nm      DOUBLE          NULL,
                fastest_5nm_avg_speed_kn     DOUBLE          NULL,
                fastest_5nm_duration_ms      BIGINT UNSIGNED NULL,
                fastest_5nm_start_timestamp  VARCHAR(30)     NULL,
                fastest_5nm_end_timestamp    VARCHAR(30)     NULL,
                fastest_10nm_distance_nm     DOUBLE          NULL,
                fastest_10nm_avg_speed_kn    DOUBLE          NULL,
                fastest_10nm_duration_ms     BIGINT UNSIGNED NULL,
                fastest_10nm_start_timestamp VARCHAR(30)     NULL,
                fastest_10nm_end_timestamp   VARCHAR(30)     NULL,
                fastest_25nm_distance_nm     DOUBLE          NULL,
                fastest_25nm_avg_speed_kn    DOUBLE          NULL,
                fastest_25nm_duration_ms     BIGINT UNSIGNED NULL,
                fastest_25nm_start_timestamp VARCHAR(30)     NULL,
                fastest_25nm_end_timestamp   VARCHAR(30)     NULL,
                PRIMARY KEY (trip_id, leg_number)
              ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci",
        )?;
        // Best-effort migrations for columns added in later versions.
        // Silently ignored if already present (MySQL 1060) or on read-only DB users.
        for sql in &[
            "ALTER TABLE trip_legs_cache ADD COLUMN start_lat DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN start_lon DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN end_lat DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN end_lon DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN nav_start_timestamp VARCHAR(30) NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN nav_end_timestamp VARCHAR(30) NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN nav_distance_nm DOUBLE NOT NULL DEFAULT 0",
            "ALTER TABLE trip_legs_cache ADD COLUMN nav_time_ms BIGINT UNSIGNED NOT NULL DEFAULT 0",
            "ALTER TABLE trip_legs_cache ADD COLUMN nav_detection_method VARCHAR(20) NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN max_speed_kn DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN max_speed_timestamp VARCHAR(30) NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_1nm_distance_nm DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_1nm_avg_speed_kn DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_1nm_duration_ms BIGINT UNSIGNED NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_1nm_start_timestamp VARCHAR(30) NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_1nm_end_timestamp VARCHAR(30) NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_5nm_distance_nm DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_5nm_avg_speed_kn DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_5nm_duration_ms BIGINT UNSIGNED NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_5nm_start_timestamp VARCHAR(30) NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_5nm_end_timestamp VARCHAR(30) NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_10nm_distance_nm DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_10nm_avg_speed_kn DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_10nm_duration_ms BIGINT UNSIGNED NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_10nm_start_timestamp VARCHAR(30) NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_10nm_end_timestamp VARCHAR(30) NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_25nm_distance_nm DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_25nm_avg_speed_kn DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_25nm_duration_ms BIGINT UNSIGNED NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_25nm_start_timestamp VARCHAR(30) NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_25nm_end_timestamp VARCHAR(30) NULL",
        ] {
            let _ = conn.query_drop(sql);
        }

        let rows: Vec<mysql::Row> = conn.exec(
            r"SELECT leg_number, start_timestamp, end_timestamp,
                         total_distance_nm, sailing_distance_nm, motoring_distance_nm,
                         sailing_time_ms, motoring_time_ms,
                         start_lat, start_lon, end_lat, end_lon,
                         nav_start_timestamp, nav_end_timestamp,
                         nav_distance_nm, nav_time_ms, nav_detection_method,
                         max_speed_kn, max_speed_timestamp,
                         fastest_1nm_distance_nm, fastest_1nm_avg_speed_kn, fastest_1nm_duration_ms,
                         fastest_1nm_start_timestamp, fastest_1nm_end_timestamp,
                         fastest_5nm_distance_nm, fastest_5nm_avg_speed_kn, fastest_5nm_duration_ms,
                         fastest_5nm_start_timestamp, fastest_5nm_end_timestamp,
                         fastest_10nm_distance_nm, fastest_10nm_avg_speed_kn, fastest_10nm_duration_ms,
                         fastest_10nm_start_timestamp, fastest_10nm_end_timestamp,
                         fastest_25nm_distance_nm, fastest_25nm_avg_speed_kn, fastest_25nm_duration_ms,
                         fastest_25nm_start_timestamp, fastest_25nm_end_timestamp
                  FROM trip_legs_cache
                  WHERE trip_id = :trip_id
                  ORDER BY leg_number",
            mysql::params! { "trip_id" => trip_id },
        )?;

        if rows.is_empty() {
            return Ok(None);
        }

        let legs = rows
            .iter()
            .map(|row| {
                let sailing_time_ms: u64 =
                    get_or_log(row, "sailing_time_ms", 0u64, "get_cached_trip_legs");
                let motoring_time_ms: u64 =
                    get_or_log(row, "motoring_time_ms", 0u64, "get_cached_trip_legs");
                TripLeg {
                    leg_number: get_or_log(row, "leg_number", 0u32, "get_cached_trip_legs"),
                    start_timestamp: get_or_log(
                        row,
                        "start_timestamp",
                        String::new(),
                        "get_cached_trip_legs",
                    ),
                    end_timestamp: get_or_log(
                        row,
                        "end_timestamp",
                        String::new(),
                        "get_cached_trip_legs",
                    ),
                    total_distance_nm: get_or_log(
                        row,
                        "total_distance_nm",
                        0.0f64,
                        "get_cached_trip_legs",
                    ),
                    sailing_distance_nm: get_or_log(
                        row,
                        "sailing_distance_nm",
                        0.0f64,
                        "get_cached_trip_legs",
                    ),
                    motoring_distance_nm: get_or_log(
                        row,
                        "motoring_distance_nm",
                        0.0f64,
                        "get_cached_trip_legs",
                    ),
                    sailing_time_ms,
                    motoring_time_ms,
                    sailing_time_formatted: format_duration_ms(sailing_time_ms),
                    motoring_time_formatted: format_duration_ms(motoring_time_ms),
                    start_lat: row.get_opt("start_lat").and_then(|v| v.ok()),
                    start_lon: row.get_opt("start_lon").and_then(|v| v.ok()),
                    end_lat: row.get_opt("end_lat").and_then(|v| v.ok()),
                    end_lon: row.get_opt("end_lon").and_then(|v| v.ok()),
                    nav_start_timestamp: row
                        .get_opt("nav_start_timestamp")
                        .and_then(|v: Result<Option<String>, _>| v.ok())
                        .flatten(),
                    nav_end_timestamp: row
                        .get_opt("nav_end_timestamp")
                        .and_then(|v: Result<Option<String>, _>| v.ok())
                        .flatten(),
                    nav_distance_nm: get_or_log(
                        row,
                        "nav_distance_nm",
                        0.0f64,
                        "get_cached_trip_legs",
                    ),
                    nav_time_ms: get_or_log(row, "nav_time_ms", 0u64, "get_cached_trip_legs"),
                    nav_detection_method: row
                        .get_opt("nav_detection_method")
                        .and_then(|v: Result<Option<String>, _>| v.ok())
                        .flatten(),
                    max_speed_kn: row.get_opt("max_speed_kn").and_then(|v| v.ok()),
                    max_speed_timestamp: row
                        .get_opt("max_speed_timestamp")
                        .and_then(|v: Result<Option<String>, _>| v.ok())
                        .flatten(),
                    fastest_1nm: fastest_segment_from_row(row, "fastest_1nm"),
                    fastest_5nm: fastest_segment_from_row(row, "fastest_5nm"),
                    fastest_10nm: fastest_segment_from_row(row, "fastest_10nm"),
                    fastest_25nm: fastest_segment_from_row(row, "fastest_25nm"),
                }
            })
            .collect();

        Ok(Some(TripLegsData { legs }))
    }

    fn save_trip_legs_to_cache(
        &self,
        conn: &mut mysql::PooledConn,
        trip_id: u32,
        legs: &[TripLeg],
    ) -> Result<(), AppError> {
        if legs.is_empty() {
            return Ok(());
        }
        conn.exec_batch(
            r"INSERT IGNORE INTO trip_legs_cache
                (trip_id, leg_number, start_timestamp, end_timestamp,
                 total_distance_nm, sailing_distance_nm, motoring_distance_nm,
                 sailing_time_ms, motoring_time_ms,
                 start_lat, start_lon, end_lat, end_lon,
                 nav_start_timestamp, nav_end_timestamp,
                 nav_distance_nm, nav_time_ms, nav_detection_method,
                 max_speed_kn, max_speed_timestamp,
                 fastest_1nm_distance_nm, fastest_1nm_avg_speed_kn, fastest_1nm_duration_ms,
                 fastest_1nm_start_timestamp, fastest_1nm_end_timestamp,
                 fastest_5nm_distance_nm, fastest_5nm_avg_speed_kn, fastest_5nm_duration_ms,
                 fastest_5nm_start_timestamp, fastest_5nm_end_timestamp,
                 fastest_10nm_distance_nm, fastest_10nm_avg_speed_kn, fastest_10nm_duration_ms,
                 fastest_10nm_start_timestamp, fastest_10nm_end_timestamp,
                 fastest_25nm_distance_nm, fastest_25nm_avg_speed_kn, fastest_25nm_duration_ms,
                 fastest_25nm_start_timestamp, fastest_25nm_end_timestamp)
              VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
                      ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
                for segment in [&leg.fastest_1nm, &leg.fastest_5nm, &leg.fastest_10nm, &leg.fastest_25nm] {
                    match segment {
                        Some(s) => {
                            values.push(s.distance_nm.into());
                            values.push(s.average_speed_kn.into());
                            values.push(s.duration_ms.into());
                            values.push(s.start_timestamp.as_str().into());
                            values.push(s.end_timestamp.as_str().into());
                        }
                        None => {
                            for _ in 0..5 {
                                values.push(mysql::Value::NULL);
                            }
                        }
                    }
                }
                values
            }),
        )?;
        Ok(())
    }

    pub fn invalidate_trip_legs_cache(&self, trip_id: u32) -> Result<(), AppError> {
        let mut conn = self.pool.get_conn()?;
        conn.query_drop(
            r"CREATE TABLE IF NOT EXISTS trip_legs_cache (
                trip_id              INT UNSIGNED    NOT NULL,
                leg_number           INT UNSIGNED    NOT NULL,
                start_timestamp      VARCHAR(30)     NOT NULL,
                end_timestamp        VARCHAR(30)     NOT NULL,
                total_distance_nm    DOUBLE          NOT NULL DEFAULT 0,
                sailing_distance_nm  DOUBLE          NOT NULL DEFAULT 0,
                motoring_distance_nm DOUBLE          NOT NULL DEFAULT 0,
                sailing_time_ms      BIGINT UNSIGNED NOT NULL DEFAULT 0,
                motoring_time_ms     BIGINT UNSIGNED NOT NULL DEFAULT 0,
                start_lat            DOUBLE          NULL,
                start_lon            DOUBLE          NULL,
                end_lat              DOUBLE          NULL,
                end_lon              DOUBLE          NULL,
                max_speed_kn                 DOUBLE          NULL,
                max_speed_timestamp          VARCHAR(30)     NULL,
                fastest_1nm_distance_nm      DOUBLE          NULL,
                fastest_1nm_avg_speed_kn     DOUBLE          NULL,
                fastest_1nm_duration_ms      BIGINT UNSIGNED NULL,
                fastest_1nm_start_timestamp  VARCHAR(30)     NULL,
                fastest_1nm_end_timestamp    VARCHAR(30)     NULL,
                fastest_5nm_distance_nm      DOUBLE          NULL,
                fastest_5nm_avg_speed_kn     DOUBLE          NULL,
                fastest_5nm_duration_ms      BIGINT UNSIGNED NULL,
                fastest_5nm_start_timestamp  VARCHAR(30)     NULL,
                fastest_5nm_end_timestamp    VARCHAR(30)     NULL,
                fastest_10nm_distance_nm     DOUBLE          NULL,
                fastest_10nm_avg_speed_kn    DOUBLE          NULL,
                fastest_10nm_duration_ms     BIGINT UNSIGNED NULL,
                fastest_10nm_start_timestamp VARCHAR(30)     NULL,
                fastest_10nm_end_timestamp   VARCHAR(30)     NULL,
                fastest_25nm_distance_nm     DOUBLE          NULL,
                fastest_25nm_avg_speed_kn    DOUBLE          NULL,
                fastest_25nm_duration_ms     BIGINT UNSIGNED NULL,
                fastest_25nm_start_timestamp VARCHAR(30)     NULL,
                fastest_25nm_end_timestamp   VARCHAR(30)     NULL,
                PRIMARY KEY (trip_id, leg_number)
              ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci",
        )?;
        conn.exec_drop(
            "DELETE FROM trip_legs_cache WHERE trip_id = :trip_id",
            mysql::params! { "trip_id" => trip_id },
        )?;
        Ok(())
    }

    /// Populate trip_legs_cache for all closed trips that have no cached legs yet.
    /// Returns the number of trips whose legs were computed and stored.
    #[allow(dead_code)]
    pub fn backfill_trip_legs_cache(&self) -> Result<usize, AppError> {
        let mut conn = self.pool.get_conn()?;

        let trip_ids: Vec<u32> = conn.query(
            r"SELECT t.id FROM trips t
                  WHERE t.end_timestamp < DATE_SUB(NOW(), INTERVAL 24 HOUR)
                    AND NOT EXISTS (
                      SELECT 1 FROM trip_legs_cache c WHERE c.trip_id = t.id
                    )
                  ORDER BY t.id",
        )?;

        let count = trip_ids.len();
        for trip_id in trip_ids {
            if let Err(e) = self.fetch_trip_legs(trip_id) {
                warn!(
                    "backfill_trip_legs_cache: failed for trip {}: {}",
                    trip_id, e
                );
            }
        }
        Ok(count)
    }

    /// Return nav window analysis rows for one trip or all closed trips.
    /// Used for backtesting: shows auto-detected windows, trimmed durations, and override status.
    pub fn fetch_nav_analysis(
        &self,
        trip_id: Option<u32>,
    ) -> Result<Vec<NavAnalysisRow>, AppError> {
        let mut conn = self.pool.get_conn()?;

        // Ensure overrides table exists (best-effort).
        let _ = conn.query_drop(
            r"CREATE TABLE IF NOT EXISTS trip_legs_nav_overrides (
                trip_id        INT UNSIGNED NOT NULL,
                leg_number     INT UNSIGNED NOT NULL,
                nav_start      VARCHAR(30)  NULL,
                nav_end        VARCHAR(30)  NULL,
                auto_nav_start VARCHAR(30)  NULL,
                auto_nav_end   VARCHAR(30)  NULL,
                corrected_at   DATETIME(3)  NOT NULL,
                PRIMARY KEY (trip_id, leg_number)
              ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci",
        );

        let rows: Vec<mysql::Row> = if let Some(id) = trip_id {
            conn.exec(
                r"SELECT
                    c.trip_id,
                    c.leg_number,
                    c.start_timestamp   AS leg_start,
                    c.end_timestamp     AS leg_end,
                    COALESCE(c.sailing_time_ms, 0) + COALESCE(c.motoring_time_ms, 0) AS leg_duration_ms,
                    c.nav_start_timestamp,
                    c.nav_end_timestamp,
                    c.nav_detection_method,
                    IF(o.trip_id IS NOT NULL, 1, 0) AS has_override
                  FROM trip_legs_cache c
                  LEFT JOIN trip_legs_nav_overrides o
                    ON o.trip_id = c.trip_id AND o.leg_number = c.leg_number
                  WHERE c.trip_id = :trip_id
                  ORDER BY c.trip_id, c.leg_number",
                mysql::params! { "trip_id" => id },
            )
?
        } else {
            conn.query(
                r"SELECT
                    c.trip_id,
                    c.leg_number,
                    c.start_timestamp   AS leg_start,
                    c.end_timestamp     AS leg_end,
                    COALESCE(c.sailing_time_ms, 0) + COALESCE(c.motoring_time_ms, 0) AS leg_duration_ms,
                    c.nav_start_timestamp,
                    c.nav_end_timestamp,
                    c.nav_detection_method,
                    IF(o.trip_id IS NOT NULL, 1, 0) AS has_override
                  FROM trip_legs_cache c
                  LEFT JOIN trip_legs_nav_overrides o
                    ON o.trip_id = c.trip_id AND o.leg_number = c.leg_number
                  ORDER BY c.trip_id, c.leg_number",
            )
?
        };

        let result = rows
            .iter()
            .map(|row| {
                let leg_start: String =
                    get_or_log(row, "leg_start", String::new(), "fetch_nav_analysis");
                let leg_end: String =
                    get_or_log(row, "leg_end", String::new(), "fetch_nav_analysis");
                let leg_duration_ms: u64 =
                    get_or_log(row, "leg_duration_ms", 0u64, "fetch_nav_analysis");
                let nav_start: Option<String> = row
                    .get_opt("nav_start_timestamp")
                    .and_then(|v: Result<Option<String>, _>| v.ok())
                    .flatten();
                let nav_end: Option<String> = row
                    .get_opt("nav_end_timestamp")
                    .and_then(|v: Result<Option<String>, _>| v.ok())
                    .flatten();

                let trimmed_start_ms = parse_trim_ms(&Some(leg_start.clone()), &nav_start);
                let trimmed_end_ms = parse_trim_ms(&nav_end, &Some(leg_end.clone()));

                NavAnalysisRow {
                    trip_id: get_or_log(row, "trip_id", 0u32, "fetch_nav_analysis"),
                    leg_number: get_or_log(row, "leg_number", 0u32, "fetch_nav_analysis"),
                    leg_start,
                    leg_end,
                    leg_duration_ms,
                    nav_start,
                    nav_end,
                    nav_detection_method: row
                        .get_opt("nav_detection_method")
                        .and_then(|v: Result<Option<String>, _>| v.ok())
                        .flatten(),
                    trimmed_start_ms,
                    trimmed_end_ms,
                    has_override: get_or_log(row, "has_override", 0u8, "fetch_nav_analysis") != 0,
                }
            })
            .collect();

        Ok(result)
    }

    fn compute_trip_legs(
        &self,
        conn: &mut mysql::PooledConn,
        trip_id: u32,
    ) -> Result<TripLegsData, AppError> {
        let results: Vec<mysql::Row> = conn.exec(
            r"SELECT
                DATE_FORMAT(timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as timestamp,
                latitude,
                longitude,
                is_moored,
                engine_on,
                total_distance_nm,
                total_time_ms,
                average_speed_kn
             FROM vessel_status
             WHERE timestamp BETWEEN
                 (SELECT start_timestamp FROM trips WHERE id = :trip_id)
                 AND COALESCE((SELECT end_timestamp FROM trips WHERE id = :trip_id), NOW())
             ORDER BY timestamp",
            mysql::params! { "trip_id" => trip_id },
        )?;

        let mut legs = Vec::new();
        let mut leg_number = 0_u32;
        let mut current_leg: Vec<LegRecord> = Vec::new();
        let mut in_leg = false;
        let mut last_lat: Option<f64> = None;
        let mut last_lon: Option<f64> = None;
        let mut leg_start_lat: Option<f64> = None;
        let mut leg_start_lon: Option<f64> = None;

        for row in &results {
            let timestamp: String = get_or_log(row, "timestamp", String::new(), "fetch_trip_legs");
            let lat: Option<f64> = row.get_opt("latitude").and_then(|v| v.ok());
            let lon: Option<f64> = row.get_opt("longitude").and_then(|v| v.ok());
            if lat.is_some() {
                last_lat = lat;
            }
            if lon.is_some() {
                last_lon = lon;
            }
            let is_moored: bool = get_or_log(row, "is_moored", false, "fetch_trip_legs");
            let engine_on_u8: u8 = get_or_log(row, "engine_on", 2u8, "fetch_trip_legs");
            let engine_on = engine_on_u8 == 1;
            let interval_distance: f64 =
                get_or_log(row, "total_distance_nm", 0.0, "fetch_trip_legs");
            let interval_time: u64 = get_or_log(row, "total_time_ms", 0u64, "fetch_trip_legs");
            let speed_kn: f64 = row
                .get_opt::<f64, _>("average_speed_kn")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);

            if is_moored {
                if in_leg {
                    leg_number += 1;
                    if let Some(leg) =
                        finalize_leg(&current_leg, leg_number, leg_start_lat, leg_start_lon)
                    {
                        legs.push(leg);
                    } else {
                        leg_number -= 1;
                    }
                    current_leg.clear();
                    in_leg = false;
                    leg_start_lat = None;
                    leg_start_lon = None;
                }
            } else {
                if !in_leg {
                    in_leg = true;
                    leg_start_lat = last_lat;
                    leg_start_lon = last_lon;
                }
                current_leg.push(LegRecord {
                    timestamp,
                    speed_kn,
                    distance_nm: interval_distance,
                    time_ms: interval_time,
                    engine_on,
                    lat: last_lat,
                    lon: last_lon,
                });
            }
        }

        if in_leg && !current_leg.is_empty() {
            leg_number += 1;
            if let Some(leg) = finalize_leg(&current_leg, leg_number, leg_start_lat, leg_start_lon)
            {
                legs.push(leg);
            }
        }

        Ok(TripLegsData { legs })
    }

    /// Fetch heatmap data - distance traveled grouped by day for 365 days before the given date.
    /// Uses a per-day database cache (heatmap_cache) to avoid recomputing past days.
    /// Today is always recomputed fresh since vessel_status data for it is still being written.
    pub fn fetch_heatmap(&self, end_date: NaiveDate) -> Result<HeatmapData, AppError> {
        let end_dt = end_date;
        let start_dt = end_dt - chrono::Duration::days(365);

        // Today in UTC — never cache today since vessel_status data is still being written
        let today = chrono::Utc::now().date_naive();
        // Only cache days strictly before today
        let cache_end = if end_dt < today {
            end_dt
        } else {
            today - chrono::Duration::days(1)
        };

        let mut conn = self.pool.get_conn()?;

        // Ensure the cache table exists; add new columns for existing deployments
        conn.query_drop(
            r"CREATE TABLE IF NOT EXISTS heatmap_cache (
                date DATE NOT NULL COMMENT 'UTC date of the aggregated sailing distance',
                distance_nm DOUBLE NOT NULL DEFAULT 0 COMMENT 'Total distance in nautical miles',
                sailing_distance_nm DOUBLE NOT NULL DEFAULT 0 COMMENT 'Distance with engine off in nautical miles',
                motoring_distance_nm DOUBLE NOT NULL DEFAULT 0 COMMENT 'Distance with engine on in nautical miles',
                PRIMARY KEY (date)
              ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci",
        )?;
        // Best-effort migration for columns added in later versions; ignored if already present.
        let _ = conn.query_drop(
            "ALTER TABLE heatmap_cache \
             ADD COLUMN sailing_distance_nm DOUBLE NOT NULL DEFAULT 0, \
             ADD COLUMN motoring_distance_nm DOUBLE NOT NULL DEFAULT 0",
        );

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

        // Step 2: Find the earliest missing date in [start_dt, cache_end].
        // All dates from that point on are considered stale — this keeps the recompute
        // query simple (a single range scan) and avoids building an IN list.
        let mut recompute_from: Option<NaiveDate> = None;
        let mut d = start_dt;
        while d <= cache_end {
            if !day_map.contains_key(&d.format("%Y-%m-%d").to_string()) {
                recompute_from = Some(d);
                break;
            }
            d += chrono::Duration::days(1);
        }

        // Step 3: Recompute from the first missing date to cache_end using a simple range query
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
                // Don't overwrite dates already loaded from cache — INSERT IGNORE
                // preserves them in the DB, and entry() preserves them in memory.
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

        // Step 4: Always recompute today fresh if it falls within the requested window
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

        // Step 5: Assemble sorted result over [start_dt, end_dt]; skip zero-distance days
        let mut days: Vec<HeatmapDay> = Vec::new();
        let mut d = start_dt;
        while d <= end_dt {
            let s = d.format("%Y-%m-%d").to_string();
            if let Some(&(total, sail, motor)) = day_map.get(&s) {
                if total > 0.0 {
                    days.push(HeatmapDay {
                        date: s,
                        distance_nm: total,
                        sailing_distance_nm: sail,
                        motoring_distance_nm: motor,
                    });
                }
            }
            d += chrono::Duration::days(1);
        }

        // Step 6: Compute aggregate statistics
        let mut min_distance: f64 = f64::MAX;
        let mut max_distance: f64 = 0.0;
        let mut total_distance: f64 = 0.0;
        let mut total_sailing_distance: f64 = 0.0;
        let mut total_motoring_distance: f64 = 0.0;
        for day in &days {
            total_distance += day.distance_nm;
            total_sailing_distance += day.sailing_distance_nm;
            total_motoring_distance += day.motoring_distance_nm;
            min_distance = min_distance.min(day.distance_nm);
            max_distance = max_distance.max(day.distance_nm);
        }
        if min_distance == f64::MAX {
            min_distance = 0.0;
        }

        Ok(HeatmapData {
            days,
            min_distance,
            max_distance,
            total_distance,
            total_sailing_distance,
            total_motoring_distance,
        })
    }

    /// Delete heatmap_cache rows that overlap [start_date, end_date] so they are
    /// recomputed fresh on the next fetch_heatmap() call.
    pub fn invalidate_heatmap_cache(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<(), AppError> {
        let mut conn = self.pool.get_conn()?;
        conn.query_drop(
            r"CREATE TABLE IF NOT EXISTS heatmap_cache (
                date DATE NOT NULL,
                distance_nm DOUBLE NOT NULL DEFAULT 0,
                sailing_distance_nm DOUBLE NOT NULL DEFAULT 0,
                motoring_distance_nm DOUBLE NOT NULL DEFAULT 0,
                PRIMARY KEY (date)
              ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci",
        )?;
        conn.exec_drop(
            "DELETE FROM heatmap_cache WHERE date BETWEEN :start AND :end",
            mysql::params! {
                "start" => start_date.format("%Y-%m-%d").to_string(),
                "end"   => end_date.format("%Y-%m-%d").to_string(),
            },
        )?;
        Ok(())
    }

    /// Get system status (tracking and metrics enabled/disabled state)
    pub fn get_system_status(&self, key: &str) -> Result<bool, AppError> {
        let cache = self
            .system_status_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(&cached) = cache.get(key) {
            Ok(cached)
        } else {
            Ok(true) // Default to true if not found in cache for backward compatibility
        }
    }

    /// Set system status (tracking and metrics enabled/disabled state)
    pub fn set_system_status(&self, key: &str, value: bool) -> Result<(), AppError> {
        let mut conn = self.pool.get_conn()?;
        let value_str = if value { "1" } else { "0" };

        // Update database first
        conn.exec_drop(
            "INSERT INTO system_status (status_key, status_value) VALUES (:key, :value) ON DUPLICATE KEY UPDATE status_value = :value",
            mysql::params! {
                "key" => key,
                "value" => value_str,
            },
        )?;

        // Update cache to stay in sync
        let mut cache = self
            .system_status_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.insert(key.to_string(), value);

        Ok(())
    }

    /// Write an arbitrary string value to system_status without touching the bool cache.
    pub fn set_system_status_string(&self, key: &str, value: &str) -> Result<(), AppError> {
        let mut conn = self.pool.get_conn()?;
        conn.exec_drop(
            "INSERT INTO system_status (status_key, status_value) VALUES (:key, :value) ON DUPLICATE KEY UPDATE status_value = :value",
            mysql::params! { "key" => key, "value" => value },
        )?;
        Ok(())
    }

    /// Read an arbitrary string value from system_status. Returns None if the key is absent.
    pub fn get_system_status_string(&self, key: &str) -> Result<Option<String>, AppError> {
        let mut conn = self.pool.get_conn()?;
        let row: Option<String> = conn.exec_first(
            "SELECT status_value FROM system_status WHERE status_key = :key",
            mysql::params! { "key" => key },
        )?;
        Ok(row)
    }
}

/// Find the fastest continuous segment of at least `target_distance_nm` within a single leg's
/// records, considering only maximal runs where `engine_on` is false — a segment can never
/// include a motoring point, matching the semantics of the original per-trip algorithm. O(leg
/// length) per target distance via a monotonic two-pointer within each run: the window's start
/// and end indices only ever advance, never reset backward.
fn fastest_segment_in_leg(records: &[LegRecord], target_distance_nm: f64) -> Option<FastestSegment> {
    let mut best: Option<FastestSegment> = None;
    let mut run_start = 0;
    while run_start < records.len() {
        if records[run_start].engine_on {
            run_start += 1;
            continue;
        }
        let mut run_end = run_start;
        while run_end < records.len() && !records[run_end].engine_on {
            run_end += 1;
        }
        if let Some(candidate) = fastest_in_run(&records[run_start..run_end], target_distance_nm) {
            let better = best
                .as_ref()
                .map(|b| candidate.average_speed_kn > b.average_speed_kn)
                .unwrap_or(true);
            if better {
                best = Some(candidate);
            }
        }
        run_start = run_end;
    }
    best
}

/// Two-pointer scan within a single engine-off run: for each `right`, shrink `left` as far as
/// possible while the window still covers `target_distance_nm`. `left` only ever advances across
/// the whole run, so this is O(run length), not O(run length^2).
fn fastest_in_run(run: &[LegRecord], target_distance_nm: f64) -> Option<FastestSegment> {
    if run.len() < 2 {
        return None;
    }
    let edge_dist: Vec<f64> = (0..run.len() - 1)
        .map(|i| {
            haversine_distance_nm(
                run[i].lat.unwrap_or(0.0),
                run[i].lon.unwrap_or(0.0),
                run[i + 1].lat.unwrap_or(0.0),
                run[i + 1].lon.unwrap_or(0.0),
            )
        })
        .collect();

    let mut best: Option<FastestSegment> = None;
    let mut left = 0usize;
    let mut window_dist = 0.0;

    for right in 1..run.len() {
        window_dist += edge_dist[right - 1];
        while left < right && window_dist - edge_dist[left] >= target_distance_nm {
            window_dist -= edge_dist[left];
            left += 1;
        }
        if window_dist >= target_distance_nm {
            if let Some(candidate) = segment_from_window(run, left, right, window_dist) {
                let better = best
                    .as_ref()
                    .map(|b| candidate.average_speed_kn > b.average_speed_kn)
                    .unwrap_or(true);
                if better {
                    best = Some(candidate);
                }
            }
        }
    }
    best
}

fn segment_from_window(
    run: &[LegRecord],
    left: usize,
    right: usize,
    distance_nm: f64,
) -> Option<FastestSegment> {
    let start_time = chrono::NaiveDateTime::parse_from_str(
        &run[left].timestamp.replace('Z', ""),
        "%Y-%m-%dT%H:%M:%S%.f",
    )
    .ok()?;
    let end_time = chrono::NaiveDateTime::parse_from_str(
        &run[right].timestamp.replace('Z', ""),
        "%Y-%m-%dT%H:%M:%S%.f",
    )
    .ok()?;
    let duration_ms = (end_time - start_time).num_milliseconds().max(0) as u64;
    if duration_ms == 0 {
        return None;
    }
    let average_speed_kn = distance_nm / (duration_ms as f64 / 1000.0 / 3600.0);
    Some(FastestSegment {
        distance_nm,
        average_speed_kn,
        duration_ms,
        start_timestamp: run[left].timestamp.clone(),
        end_timestamp: run[right].timestamp.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::Add;
    use std::time::{Duration, SystemTime};

    #[cfg(test)]
    use crate::config::Config;
    #[cfg(test)]
    use crate::db::test_helpers::{
        add_test_trip, add_test_vessel_status, assert_approx_equal, reset_test_db, setup_test_db,
    };
    #[cfg(test)]
    use crate::utilities::EngineStatus;

    fn synthetic_leg_constant_speed(n: usize, speed_kn: f64) -> Vec<LegRecord> {
        let interval_s: f64 = 10.0;
        let dist_per_point = speed_kn * interval_s / 3600.0; // nm per 10s interval
        let deg_per_nm = 1.0 / 60.0; // ~1 nm per 1/60 degree of latitude
        let base = chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        (0..n)
            .map(|i| LegRecord {
                timestamp: (base + chrono::Duration::seconds(i as i64 * interval_s as i64))
                    .format("%Y-%m-%dT%H:%M:%S.000Z")
                    .to_string(),
                speed_kn,
                distance_nm: dist_per_point,
                time_ms: (interval_s * 1000.0) as u64,
                engine_on: false,
                lat: Some(40.0 + i as f64 * dist_per_point * deg_per_nm),
                lon: Some(2.0),
            })
            .collect()
    }

    fn synthetic_leg_with_engine_gap(
        n: usize,
        speed_kn: f64,
        gap_start: usize,
        gap_end: usize,
    ) -> Vec<LegRecord> {
        let mut records = synthetic_leg_constant_speed(n, speed_kn);
        for r in records.iter_mut().take(gap_end).skip(gap_start) {
            r.engine_on = true;
        }
        records
    }

    fn synthetic_leg_becalmed_stretch(
        n: usize,
        speed_kn: f64,
        becalm_start: usize,
        becalm_end: usize,
    ) -> Vec<LegRecord> {
        let mut records = synthetic_leg_constant_speed(n, speed_kn);
        let interval_s: f64 = 10.0;
        let dist_per_point = speed_kn * interval_s / 3600.0;
        let deg_per_nm = 1.0 / 60.0;
        let frozen_lat = records[becalm_start].lat;
        let missed_distance = (becalm_end - becalm_start) as f64 * dist_per_point * deg_per_nm;
        for (i, r) in records.iter_mut().enumerate() {
            if i >= becalm_start && i < becalm_end {
                // Freeze position and distance during becalmed stretch
                r.lat = frozen_lat;
                r.distance_nm = 0.0;
            } else if i >= becalm_end {
                // Offset post-becalmed positions by the distance not traveled during the stretch
                if let Some(lat) = r.lat {
                    r.lat = Some(lat - missed_distance);
                }
            }
        }
        records
    }

    #[test]
    fn fastest_segment_in_leg_is_linear_not_quadratic_on_becalmed_stretch() {
        // Regression test for the original O(n^2) blowup: a long becalmed (near-zero-distance,
        // engine-off) stretch must complete near-instantly now that the algorithm is a genuine
        // two-pointer. An accidental revert to nested-loop behavior makes this test visibly slow.
        let records = synthetic_leg_becalmed_stretch(20_000, 6.0, 100, 19_900);
        let start = std::time::Instant::now();
        let _ = fastest_segment_in_leg(&records, 25.0);
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() < 500,
            "fastest_segment_in_leg took {:?} on 20k becalmed points — looks quadratic",
            elapsed
        );
    }

    #[test]
    fn finalize_leg_populates_speed_records() {
        let records = synthetic_leg_constant_speed(400, 6.0); // 400 * 10s = ~1.1h, ~2.5nm total
        let leg = finalize_leg(&records, 1, records[0].lat, records[0].lon)
            .expect("leg should finalize — total distance exceeds the 0.5nm minimum");

        assert!(leg.max_speed_kn.is_some());
        assert!((leg.max_speed_kn.unwrap() - 6.0).abs() < 0.01);
        assert!(leg.max_speed_timestamp.is_some());

        // Total leg distance is ~2.5nm (400 * 6.0 * 10/3600), so a 1nm segment must exist...
        assert!(leg.fastest_1nm.is_some());
        let seg = leg.fastest_1nm.as_ref().unwrap();
        assert!((seg.average_speed_kn - 6.0).abs() < 0.1);
        // ...but 25nm never fits in a 2.5nm leg.
        assert!(leg.fastest_25nm.is_none());
    }

    #[test]
    fn fastest_segment_in_leg_never_spans_an_engine_on_gap() {
        // 500 points at 6kn, 10s interval => ~0.0167nm/point. Engine on for indices
        // [200, 250) splits the leg into a leading engine-off run [0, 200) (~3.3nm) and a
        // trailing engine-off run [250, 500) (~4.2nm). fastest_segment_in_leg scopes its
        // two-pointer search to each engine-off run independently; a regression that ignored
        // engine_on and scanned the whole leg as one run could stitch together a "segment"
        // that silently skips over the motoring gap in the middle.
        let records = synthetic_leg_with_engine_gap(500, 6.0, 200, 250);

        for target_nm in [1.0, 2.0, 3.0] {
            if let Some(seg) = fastest_segment_in_leg(&records, target_nm) {
                let start_idx = records
                    .iter()
                    .position(|r| r.timestamp == seg.start_timestamp)
                    .expect("segment start_timestamp must match a record");
                let end_idx = records
                    .iter()
                    .position(|r| r.timestamp == seg.end_timestamp)
                    .expect("segment end_timestamp must match a record");
                assert!(
                    end_idx < 200 || start_idx >= 250,
                    "segment [{start_idx}, {end_idx}] for target {target_nm}nm straddles the engine-on gap [200, 250)"
                );
            }
        }
    }

    #[test]
    #[ignore]
    fn test_trip_legs_cache_round_trips_speed_records() {
        let db = setup_db();
        const ONE_HOUR_S: u64 = 3600;

        let start_time = SystemTime::now().add(Duration::from_secs(48 * ONE_HOUR_S));
        let end_time = start_time.add(Duration::from_secs(2 * ONE_HOUR_S));

        let trip_id = add_test_trip(
            &db,
            "Speed Record Cache Test".to_string(),
            start_time,
            end_time,
            10.5,
            2.3,
            3600000,
            600000,
            0,
        )
        .expect("Failed to insert test trip");

        // A steady 6kn run for 2 hours (~12nm) — long enough that fastest_1nm/5nm/10nm all exist,
        // fastest_25nm does not. add_test_vessel_status's own signature: (db, timestamp, latitude,
        // longitude, average_speed_kn, max_speed_kn, average_wind_speed_kn, average_wind_angle_deg,
        // is_moored, engine_on, total_distance_nm, total_time_ms, cog_deg, average_heading_deg).
        let mut current_time = start_time;
        let mut lat = 41.0;
        let interval_s = 30u64;
        let dist_per_interval_nm = 6.0 * interval_s as f64 / 3600.0; // 0.05nm per 30s at 6kn
        while current_time < end_time {
            add_test_vessel_status(
                &db,
                current_time,
                lat,
                2.0,
                6.0,
                6.0,
                None,
                None,
                false,
                EngineStatus::Off,
                dist_per_interval_nm,
                interval_s * 1000,
                None,
                None,
            )
            .expect("Failed to insert vessel status");
            current_time = current_time.add(Duration::from_secs(interval_s));
            lat += dist_per_interval_nm / 60.0; // ~1 nm per 1/60 degree of latitude
        }

        // fetch_trip_legs always computes fresh regardless of is_closed — only the caching step is
        // conditional — so this exercises finalize_leg's new fields without depending on wall-clock
        // trip closure timing.
        let legs_data = db.fetch_trip_legs(trip_id).expect("fetch_trip_legs failed");
        assert!(!legs_data.legs.is_empty(), "expected at least one leg");
        let leg = &legs_data.legs[0];
        assert!(leg.max_speed_kn.is_some(), "max_speed_kn should be populated");
        assert!(leg.fastest_1nm.is_some(), "fastest_1nm should be populated for a 12nm run");
        assert!(leg.fastest_5nm.is_some(), "fastest_5nm should be populated for a 12nm run");
        assert!(leg.fastest_25nm.is_none(), "fastest_25nm should be absent for a 12nm run");

        let fastest_1nm_before = leg.fastest_1nm.clone();
        let max_speed_before = leg.max_speed_kn;

        // Exercise the cache write/read path directly (mirrors how get_cached_trip_legs is reached
        // for closed trips) via the #[cfg(test)] wrappers added below. get_cached_trip_legs_for_test
        // is called first purely for its CREATE TABLE/ALTER TABLE side effects: invalidate_trip_legs_cache
        // only runs CREATE TABLE IF NOT EXISTS, which is a no-op when the table already exists in an
        // older shape, so on a database whose trip_legs_cache predates the fastest-segment columns
        // (e.g. freshly created from schema.sql without applying the "For existing databases, run:"
        // ALTER block), get_cached_trip_legs's CREATE+ALTER is the only thing that actually adds
        // them — it must run at least once before save_trip_legs_to_cache, otherwise the INSERT
        // below could target a table still missing those columns. invalidate_trip_legs_cache then
        // clears any stale row for this trip_id — this matters because reset_test_db() doesn't
        // truncate trip_legs_cache, and TRUNCATE TABLE trips resets AUTO_INCREMENT, so a fresh
        // test run can reissue a trip_id a previous run already cached legs under; without this,
        // save's INSERT IGNORE would silently keep that stale row and the round-trip assertions
        // below would compare against old data.
        db.get_cached_trip_legs_for_test(trip_id)
            .expect("get_cached_trip_legs failed (pre-save schema migration)");
        db.invalidate_trip_legs_cache(trip_id)
            .expect("invalidate_trip_legs_cache failed (pre-save cleanup)");
        db.save_trip_legs_to_cache_for_test(trip_id, &legs_data.legs)
            .expect("save_trip_legs_to_cache failed");
        let cached = db
            .get_cached_trip_legs_for_test(trip_id)
            .expect("get_cached_trip_legs failed")
            .expect("expected a cached row");
        assert_eq!(cached.legs[0].fastest_1nm, fastest_1nm_before);
        assert_eq!(cached.legs[0].max_speed_kn, max_speed_before);
    }

    fn setup_db() -> VesselDatabase {
        let config = Config::load_for_context(None)
            .expect("Failed to load test config - ensure test_config.json exists");
        let db_url = config.database.connection.connection_url();
        let db = setup_test_db(&db_url).expect(
            "Failed to setup test database - ensure MySQL is running and test database exists",
        );
        reset_test_db(&db).expect("Failed to reset test database");
        db
    }

    #[test]
    #[ignore]
    fn test_get_track() {
        let db = setup_db();
        const ONE_HOUR_S: u64 = 3600;

        // Create a test trip with vessel status records
        let start_time = SystemTime::now();
        let end_time = start_time.add(Duration::from_secs(2 * ONE_HOUR_S));

        let trip_id = add_test_trip(
            &db,
            "Track Test Trip".to_string(),
            start_time,
            end_time,
            10.5,
            2.3,
            3600000,
            600000,
            0,
        )
        .expect("Failed to insert test trip");

        // Add multiple vessel status records to create a track
        let mut current_time = start_time;
        let mut lat = 41.0;
        let mut lon = 2.0;
        while current_time < end_time {
            add_test_vessel_status(
                &db,
                current_time,
                lat,
                lon,
                6.5,
                7.2,
                Some(12.0),
                Some(45.0),
                false,
                EngineStatus::Off,
                0.5,
                600000,
                Some(90.0),
                Some(92.0),
            )
            .expect("Failed to insert vessel status");

            current_time = current_time.add(Duration::from_secs(600)); // Every 10 minutes
            lat += 0.01;
            lon += 0.01;
        }

        // Fetch track by trip_id
        let points = db
            .fetch_track(Some(trip_id), None, None, None)
            .expect("Failed to fetch track");

        // Verify track has multiple points
        assert!(!points.is_empty(), "Track should not be empty");
        assert!(points.len() >= 2, "Track should have at least 2 points");

        // Verify first and last points are reasonable
        let first = &points[0];
        let last = &points[points.len() - 1];

        assert_approx_equal(
            first.latitude.expect("First point should have latitude"),
            41.0,
            0.1,
            "First point latitude",
        );
        assert_approx_equal(
            first.longitude.expect("First point should have longitude"),
            2.0,
            0.1,
            "First point longitude",
        );
        assert_approx_equal(
            last.latitude.expect("Last point should have latitude"),
            41.0 + 0.01 * 2.0,
            0.1,
            "Last point latitude",
        );
        assert_approx_equal(
            last.longitude.expect("Last point should have longitude"),
            2.0 + 0.01 * 2.0,
            0.1,
            "Last point longitude",
        );
    }

    #[test]
    fn decimate_caps_total_count_even_when_source_is_already_sparse() {
        // Regression test: vessel_status rows can already be spaced further apart (e.g.
        // 10s underway) than the max_points-implied sampling rate (e.g. 600 points/hour
        // = one every 6s), so a rate-based filter lets every row through unfiltered.
        // decimate() must still cap the *total* output length regardless of input spacing.
        let items: Vec<u32> = (0..32_257).collect();
        let result = decimate(items, Some(600));
        assert!(
            result.len() <= 600,
            "expected at most 600 points, got {}",
            result.len()
        );
    }

    #[test]
    fn decimate_preserves_order_with_even_stride() {
        let items: Vec<u32> = (0..12).collect();
        let result = decimate(items, Some(4));
        assert_eq!(result, vec![0, 3, 6, 9]);
    }

    #[test]
    fn decimate_is_noop_when_under_the_limit() {
        let items: Vec<u32> = (0..5).collect();
        let result = decimate(items.clone(), Some(600));
        assert_eq!(result, items);
    }

    #[test]
    fn decimate_is_noop_when_max_points_is_none() {
        let items: Vec<u32> = (0..10_000).collect();
        let result = decimate(items.clone(), None);
        assert_eq!(result, items);
    }

    #[test]
    #[ignore]
    fn test_system_status_set() {
        let db = setup_db();

        // Test setting to true
        db.set_system_status("tracking_enabled", true)
            .expect("Failed to set tracking_enabled to true");
        assert!(
            db.get_system_status("tracking_enabled")
                .expect("Failed to get tracking_enabled"),
            "tracking_enabled should be true"
        );

        // Test setting to false
        db.set_system_status("tracking_enabled", false)
            .expect("Failed to set tracking_enabled to false");
        assert!(
            !db.get_system_status("tracking_enabled")
                .expect("Failed to get tracking_enabled"),
            "tracking_enabled should be false"
        );

        // Test different key
        db.set_system_status("metrics_enabled", true)
            .expect("Failed to set metrics_enabled");
        assert!(
            db.get_system_status("metrics_enabled")
                .expect("Failed to get metrics_enabled"),
            "metrics_enabled should be true"
        );
    }

    #[test]
    #[ignore]
    fn test_system_status_default() {
        let db = setup_db();

        // Test that non-existent keys default to true
        let result = db
            .get_system_status("a_key_that_does_not_exist")
            .expect("Failed to get status");
        assert!(result, "Non-existent keys should default to true");
    }

    #[test]
    #[ignore]
    fn test_system_status_persistence() {
        let db = setup_db();

        // Set a status
        db.set_system_status("test_key", true)
            .expect("Failed to set test_key");
        assert!(
            db.get_system_status("test_key")
                .expect("Failed to get test_key"),
            "test_key should be true after setting"
        );

        // Create a new database instance (simulates application restart)
        let config = Config::load_for_context(None).expect("Failed to load test config");
        let db_url = config.database.connection.connection_url();
        let db2 = VesselDatabase::new(
            &db_url,
            config.database.connection.pool_min,
            config.database.connection.pool_max,
        )
        .expect("Failed to create second database instance");

        // Verify value persists
        assert!(
            db2.get_system_status("test_key")
                .expect("Failed to get test_key from new instance"),
            "test_key should persist across database instances"
        );
    }

    #[test]
    #[ignore]
    fn test_export_trip() {
        use std::fs;
        use std::path::PathBuf;

        let db = setup_db();
        const ONE_HOUR_S: u64 = 3600;

        // Create a test trip
        let start_time = SystemTime::now();
        let end_time = start_time.add(Duration::from_secs(3 * ONE_HOUR_S));

        let trip_id = add_test_trip(
            &db,
            "Export Test Trip".to_string(),
            start_time,
            end_time,
            15.5,
            3.2,
            6000000,
            1200000,
            0,
        )
        .expect("Failed to insert test trip") as i64;

        // Add some vessel status records
        let mut current_time = start_time;
        let mut lat = 40.5;
        let mut lon = 1.5;
        while current_time < end_time {
            add_test_vessel_status(
                &db,
                current_time,
                lat,
                lon,
                6.5,
                7.2,
                Some(12.0),
                Some(45.0),
                false,
                EngineStatus::Off,
                0.5,
                1800000,
                Some(90.0),
                Some(92.0),
            )
            .expect("Failed to insert vessel status");

            current_time = current_time.add(Duration::from_secs(1800)); // Every 30 minutes
            lat += 0.05;
            lon += 0.05;
        }

        // Export trip
        let export_path = PathBuf::from("/tmp/test_trip_export.json");
        let _ = fs::remove_file(&export_path); // Clean up previous run

        let result = db.export_trip(trip_id, &export_path);
        assert!(result.is_ok(), "Export should succeed: {:?}", result.err());

        // Verify file exists and has content
        assert!(export_path.exists(), "Export file should exist");
        let metadata = fs::metadata(&export_path).expect("Failed to get export file metadata");
        assert!(metadata.len() > 0, "Export file should not be empty");

        // Verify JSON structure
        let contents = fs::read_to_string(&export_path).expect("Failed to read export file");
        let json: serde_json::Value =
            serde_json::from_str(&contents).expect("Export file should contain valid JSON");

        assert!(json["trip"].is_object(), "Should have trip object");
        assert_eq!(json["trip"]["id"], trip_id, "Trip ID should match");
        assert!(
            json["trip"]["desc"].is_string(),
            "Trip should have description"
        );
        assert_eq!(
            json["trip"]["desc"], "Export Test Trip",
            "Trip description should match"
        );
        assert!(
            json["trip"]["start"].is_string(),
            "Trip should have start_timestamp"
        );
        assert!(
            json["trip"]["end"].is_string(),
            "Trip should have end_timestamp"
        );

        // Verify arrays exist
        assert!(json["vs"].is_array(), "Should have vessel_statuses array");
        assert!(
            json["em"].is_array(),
            "Should have environmental_metrics array"
        );
        assert!(
            json["meta"].is_object(),
            "Should have export_metadata object"
        );

        // Clean up
        fs::remove_file(&export_path).expect("Should be able to delete test file");
    }

    #[test]
    #[ignore]
    fn test_fetch_trip_by_uuid() {
        let db = setup_db();

        let start_time = SystemTime::now();
        let end_time = start_time + std::time::Duration::from_secs(3600);

        // Insert a trip; add_test_trip generates a random UUID internally
        let trip_id = add_test_trip(
            &db,
            "UUID Lookup Test".to_string(),
            start_time,
            end_time,
            5.0,
            1.0,
            1800000,
            600000,
            0,
        )
        .expect("Failed to insert test trip");

        // Retrieve the UUID that was stored for this trip
        let mut conn = db.pool.get_conn().expect("Failed to get connection");
        let stored_uuid: Option<String> = conn
            .exec_first(
                "SELECT uuid FROM trips WHERE id = :id",
                mysql::params! { "id" => trip_id },
            )
            .expect("Query failed");
        let stored_uuid = stored_uuid.expect("UUID must not be NULL");

        // fetch_trip_by_uuid should return the same trip
        let result = db
            .fetch_trip_by_uuid(&stored_uuid)
            .expect("fetch_trip_by_uuid failed");

        let trip = result.expect("Trip should be found by UUID");
        assert_eq!(trip.id, trip_id, "Returned trip ID must match");
        assert_eq!(trip.description, "UUID Lookup Test");
        assert_eq!(
            trip.uuid,
            Some(stored_uuid.clone()),
            "Returned uuid must match"
        );

        // Looking up a non-existent UUID must return None
        let missing = db
            .fetch_trip_by_uuid("00000000-0000-0000-0000-000000000000")
            .expect("fetch_trip_by_uuid should not error on missing uuid");
        assert!(missing.is_none(), "Non-existent UUID must return None");
    }

    // -----------------------------------------------------------------------
    // Heatmap caching tests
    // All use past dates (2020-06-xx) so they always fall into the "cacheable"
    // branch — today's date is never 2020-06-xx in production.
    // -----------------------------------------------------------------------

    fn clear_heatmap_cache(db: &VesselDatabase) {
        let mut conn = db.pool.get_conn().expect("get_conn");
        conn.query_drop("DELETE FROM heatmap_cache")
            .expect("clear heatmap_cache");
    }

    /// Insert a vessel_status row with a specific UTC timestamp and distance.
    /// Sets is_moored = 0 so the heatmap query picks the row up.
    fn add_heatmap_status(db: &VesselDatabase, ts: SystemTime, distance_nm: f64) {
        add_heatmap_status_engine(db, ts, distance_nm, EngineStatus::Off);
    }

    fn add_heatmap_status_engine(
        db: &VesselDatabase,
        ts: SystemTime,
        distance_nm: f64,
        engine: EngineStatus,
    ) {
        add_test_vessel_status(
            db,
            ts,
            41.0,
            2.0,
            5.0,
            6.0,
            None,
            None,
            false,
            engine,
            distance_nm,
            600_000,
            None,
            None,
        )
        .expect("add_test_vessel_status");
    }

    #[test]
    #[ignore]
    fn test_heatmap_empty() {
        let db = setup_db();
        clear_heatmap_cache(&db);
        // No vessel_status rows → HeatmapData with all zeros and empty days
        let end = chrono::NaiveDate::from_ymd_opt(2020, 6, 30).unwrap();
        let result = db.fetch_heatmap(end).expect("fetch_heatmap");
        assert!(result.days.is_empty(), "No sailing days expected");
        assert_eq!(result.total_distance, 0.0);
        assert_eq!(result.min_distance, 0.0);
        assert_eq!(result.max_distance, 0.0);
    }

    #[test]
    #[ignore]
    fn test_heatmap_populates_cache() {
        let db = setup_db();
        clear_heatmap_cache(&db);

        // Add two vessel_status rows on 2020-06-15
        let ts = SystemTime::UNIX_EPOCH + Duration::from_secs(1592179200); // 2020-06-15 00:00:00 UTC
        add_heatmap_status(&db, ts, 10.0);
        add_heatmap_status(&db, ts + Duration::from_secs(3600), 5.0);

        let end = chrono::NaiveDate::from_ymd_opt(2020, 6, 30).unwrap();
        let result = db.fetch_heatmap(end).expect("fetch_heatmap");

        // The day should appear with sum 15 nm
        let day = result.days.iter().find(|d| d.date == "2020-06-15");
        assert!(day.is_some(), "2020-06-15 should be in heatmap days");
        assert_approx_equal(
            day.unwrap().distance_nm,
            15.0,
            0.001,
            "distance for 2020-06-15",
        );

        // Cache row must have been written
        let mut conn = db.pool.get_conn().expect("get_conn");
        let cached: Option<f64> = conn
            .exec_first(
                "SELECT distance_nm FROM heatmap_cache WHERE date = '2020-06-15'",
                mysql::Params::Empty,
            )
            .expect("cache query");
        assert!(
            cached.is_some(),
            "heatmap_cache row should exist for 2020-06-15"
        );
        assert_approx_equal(
            cached.unwrap(),
            15.0,
            0.001,
            "cached distance for 2020-06-15",
        );
    }

    #[test]
    #[ignore]
    fn test_heatmap_cache_hit_no_recompute() {
        let db = setup_db();
        clear_heatmap_cache(&db);

        let ts = SystemTime::UNIX_EPOCH + Duration::from_secs(1592179200); // 2020-06-15
        add_heatmap_status(&db, ts, 10.0);

        let end = chrono::NaiveDate::from_ymd_opt(2020, 6, 30).unwrap();

        // First call — populates cache
        let first = db.fetch_heatmap(end).expect("first fetch_heatmap");
        let first_dist = first
            .days
            .iter()
            .find(|d| d.date == "2020-06-15")
            .map(|d| d.distance_nm)
            .unwrap_or(0.0);

        // Add another vessel_status row for the same day — if the cache is hit
        // this new row must NOT affect the result.
        add_heatmap_status(&db, ts + Duration::from_secs(7200), 99.0);

        // Second call — should read from cache, ignore the new row
        let second = db.fetch_heatmap(end).expect("second fetch_heatmap");
        let second_dist = second
            .days
            .iter()
            .find(|d| d.date == "2020-06-15")
            .map(|d| d.distance_nm)
            .unwrap_or(0.0);

        assert_approx_equal(
            first_dist,
            second_dist,
            0.001,
            "second call must return cached value, not recomputed",
        );
    }

    #[test]
    #[ignore]
    fn test_heatmap_invalidate_forces_recompute() {
        let db = setup_db();
        clear_heatmap_cache(&db);

        let ts = SystemTime::UNIX_EPOCH + Duration::from_secs(1592179200); // 2020-06-15
        add_heatmap_status(&db, ts, 10.0);

        let end = chrono::NaiveDate::from_ymd_opt(2020, 6, 30).unwrap();

        // Prime the cache
        db.fetch_heatmap(end).expect("prime fetch_heatmap");

        // Insert more data then invalidate the cache
        add_heatmap_status(&db, ts + Duration::from_secs(7200), 20.0);
        let d = chrono::NaiveDate::from_ymd_opt(2020, 6, 15).unwrap();
        db.invalidate_heatmap_cache(d, d)
            .expect("invalidate_heatmap_cache");

        // After invalidation the recompute should include the new row
        let result = db
            .fetch_heatmap(end)
            .expect("post-invalidate fetch_heatmap");
        let dist = result
            .days
            .iter()
            .find(|d| d.date == "2020-06-15")
            .map(|d| d.distance_nm)
            .unwrap_or(0.0);
        assert_approx_equal(dist, 30.0, 0.001, "recomputed distance after invalidation");
    }

    #[test]
    #[ignore]
    fn test_heatmap_zero_distance_days_not_in_result() {
        let db = setup_db();
        clear_heatmap_cache(&db);

        // 2020-06-15: non-zero distance; 2020-06-16: zero distance (moored row via 0.0 nm)
        let ts15 = SystemTime::UNIX_EPOCH + Duration::from_secs(1592179200); // 2020-06-15
        let ts16 = ts15 + Duration::from_secs(86400); // 2020-06-16
        add_heatmap_status(&db, ts15, 5.0);
        add_heatmap_status(&db, ts16, 0.0);

        let end = chrono::NaiveDate::from_ymd_opt(2020, 6, 30).unwrap();
        let result = db.fetch_heatmap(end).expect("fetch_heatmap");

        let has_15 = result.days.iter().any(|d| d.date == "2020-06-15");
        let has_16 = result.days.iter().any(|d| d.date == "2020-06-16");
        assert!(has_15, "2020-06-15 with non-zero distance should appear");
        assert!(
            !has_16,
            "2020-06-16 with zero distance must not appear in result"
        );
    }

    #[test]
    #[ignore]
    fn test_heatmap_sailing_motoring_split() {
        let db = setup_db();
        clear_heatmap_cache(&db);

        // 2020-06-15: 10 nm sailing (engine off) + 6 nm motoring (engine on)
        let ts = SystemTime::UNIX_EPOCH + Duration::from_secs(1592179200); // 2020-06-15 00:00:00 UTC
        add_heatmap_status_engine(&db, ts, 10.0, EngineStatus::Off);
        add_heatmap_status_engine(&db, ts + Duration::from_secs(3600), 6.0, EngineStatus::On);

        let end = chrono::NaiveDate::from_ymd_opt(2020, 6, 30).unwrap();
        let result = db.fetch_heatmap(end).expect("fetch_heatmap");

        let day = result
            .days
            .iter()
            .find(|d| d.date == "2020-06-15")
            .expect("2020-06-15 should appear");

        assert_approx_equal(day.distance_nm, 16.0, 0.001, "total distance");
        assert_approx_equal(day.sailing_distance_nm, 10.0, 0.001, "sailing distance");
        assert_approx_equal(day.motoring_distance_nm, 6.0, 0.001, "motoring distance");
        assert_approx_equal(
            result.total_sailing_distance,
            10.0,
            0.001,
            "aggregate sailing",
        );
        assert_approx_equal(
            result.total_motoring_distance,
            6.0,
            0.001,
            "aggregate motoring",
        );
    }

    #[test]
    #[ignore]
    fn test_heatmap_gap_triggers_partial_recompute() {
        let db = setup_db();
        clear_heatmap_cache(&db);

        // Manually pre-load cache for 2020-06-10 only, leaving 11-14 as gaps
        let mut conn = db.pool.get_conn().expect("get_conn");
        conn.query_drop(
            r"CREATE TABLE IF NOT EXISTS heatmap_cache (
                date DATE NOT NULL,
                distance_nm DOUBLE NOT NULL DEFAULT 0,
                sailing_distance_nm DOUBLE NOT NULL DEFAULT 0,
                motoring_distance_nm DOUBLE NOT NULL DEFAULT 0,
                PRIMARY KEY (date)
              ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci",
        )
        .expect("ensure table");
        conn.exec_drop(
            "INSERT IGNORE INTO heatmap_cache (date, distance_nm) VALUES ('2020-06-10', 7.0)",
            mysql::Params::Empty,
        )
        .expect("insert cache row");

        // Add vessel_status for 2020-06-12 (a gap day)
        let ts12 = SystemTime::UNIX_EPOCH + Duration::from_secs(1591920000); // 2020-06-12 00:00:00 UTC
        add_heatmap_status(&db, ts12, 12.0);

        let end = chrono::NaiveDate::from_ymd_opt(2020, 6, 20).unwrap();
        let result = db.fetch_heatmap(end).expect("fetch_heatmap");

        // The pre-cached day-10 value must survive
        let dist10 = result
            .days
            .iter()
            .find(|d| d.date == "2020-06-10")
            .map(|d| d.distance_nm);
        assert!(dist10.is_some(), "2020-06-10 cached value should appear");
        assert_approx_equal(dist10.unwrap(), 7.0, 0.001, "2020-06-10 distance");

        // Gap day 2020-06-12 must be recomputed from vessel_status
        let dist12 = result
            .days
            .iter()
            .find(|d| d.date == "2020-06-12")
            .map(|d| d.distance_nm);
        assert!(dist12.is_some(), "2020-06-12 recomputed day should appear");
        assert_approx_equal(dist12.unwrap(), 12.0, 0.001, "2020-06-12 distance");
    }

    #[test]
    #[ignore]
    fn test_monthly_statistics_includes_uncached_today() {
        let db = setup_db();
        clear_heatmap_cache(&db);

        // Add vessel_status for "today" without ever populating heatmap_cache
        // (mirrors production: nobody has viewed the heatmap since this data arrived).
        let now = SystemTime::now();
        add_heatmap_status_engine(&db, now, 20.0, EngineStatus::Off);
        add_heatmap_status_engine(&db, now, 5.0, EngineStatus::On);

        let stats = db
            .fetch_monthly_statistics()
            .expect("fetch_monthly_statistics");

        use chrono::Datelike;
        let today = chrono::Utc::now();
        let this_month = stats
            .months
            .iter()
            .find(|m| m.year == today.year() && m.month == today.month())
            .expect("current month should be present");

        assert_approx_equal(
            this_month.sailing_distance_nm,
            20.0,
            0.001,
            "current month sailing distance should include uncached today",
        );
        assert_approx_equal(
            this_month.motoring_distance_nm,
            5.0,
            0.001,
            "current month motoring distance should include uncached today",
        );
    }
}
