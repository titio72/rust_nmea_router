use crate::db::types::{
    format_duration_ms, FastestSegment, HeatmapData, HeatmapDay, MonthlyStatistic,
    MonthlyStatistics, MultiMetricData, SpeedDistributionData, TrackAnalytics, TrackPoint, TripLeg,
    TripLegsData, TripSummary, VesselDatabase, WebMetricData, WindStatisticsData,
};
use crate::utilities::haversine_distance_nm;
use chrono::{DateTime, NaiveDate, Utc};
use mysql::params;
use mysql::prelude::Queryable;
use std::error::Error;
use tracing::warn;

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

impl VesselDatabase {
    pub fn fetch_trip(
        &self,
        trip_id: u32,
    ) -> Result<Option<TripSummary>, Box<dyn std::error::Error>> {
        let mut conn = self
            .pool
            .get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;

        let row: Option<mysql::Row> = conn
            .exec_first(
                r"SELECT id, description,
                     DATE_FORMAT(start_timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as start_ts,
                     DATE_FORMAT(end_timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as end_ts,
                     total_distance_sailed, total_distance_motoring,
                     (total_distance_sailed + total_distance_motoring) as total_distance,
                     total_time_sailing, total_time_motoring, total_time_moored, uuid
              FROM trips
              WHERE id = :trip_id",
                mysql::params! {
                    "trip_id" => trip_id,
                },
            )
            .map_err(|e| format!("Database query error: {}", e))?;

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
            Ok(Some(trip))
        } else {
            Ok(None)
        }
    }

    /// Fetch trips with optional filtering
    pub fn fetch_trips(
        &self,
        year: Option<i32>,
        last_months: Option<u32>,
    ) -> Result<Vec<TripSummary>, Box<dyn std::error::Error>> {
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

        let mut conn = self
            .pool
            .get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;

        let results: Vec<mysql::Row> = if let Some(year) = year {
            conn.exec(
                format!(
                    "{} YEAR(start_timestamp) = :year ORDER BY start_timestamp DESC",
                    SELECT_TRIPS
                ),
                mysql::params! { "year" => year },
            )
            .map_err(|e| format!("Database query error: {}", e))?
        } else if let Some(months) = last_months {
            conn.exec(
                format!("{} start_timestamp >= DATE_SUB(NOW(), INTERVAL :months MONTH) ORDER BY start_timestamp DESC", SELECT_TRIPS),
                mysql::params! { "months" => months },
            ).map_err(|e| format!("Database query error: {}", e))?
        } else {
            conn.query(format!(
                "{} 1=1 ORDER BY start_timestamp DESC",
                SELECT_TRIPS
            ))
            .map_err(|e| format!("Database query error: {}", e))?
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

    pub fn fetch_trip_by_uuid(
        &self,
        trip_uuid: &str,
    ) -> Result<Option<TripSummary>, Box<dyn std::error::Error>> {
        let mut conn = self
            .pool
            .get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;

        let row: Option<mysql::Row> = conn
            .exec_first(
                r"SELECT id, description,
                     DATE_FORMAT(start_timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as start_ts,
                     DATE_FORMAT(end_timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as end_ts,
                     total_distance_sailed, total_distance_motoring,
                     (total_distance_sailed + total_distance_motoring) as total_distance,
                     total_time_sailing, total_time_motoring, total_time_moored, uuid
              FROM trips
              WHERE uuid = :uuid",
                mysql::params! {
                    "uuid" => trip_uuid,
                },
            )
            .map_err(|e| format!("Database query error: {}", e))?;

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
    pub fn fetch_monthly_statistics(
        &self,
    ) -> Result<MonthlyStatistics, Box<dyn std::error::Error>> {
        let mut conn = self
            .pool
            .get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;

        // Get all trip data grouped by year and month
        let results: Vec<mysql::Row> = conn
            .query(
                r"SELECT YEAR(start_timestamp) as year,
                     MONTH(start_timestamp) as month,
                     SUM(total_distance_sailed) as sailing_distance,
                     SUM(total_distance_motoring) as motoring_distance
              FROM trips
              WHERE start_timestamp >= '2020-01-01'
              GROUP BY YEAR(start_timestamp), MONTH(start_timestamp)
              ORDER BY year ASC, month ASC",
            )
            .map_err(|e| format!("Database query error: {}", e))?;

        // Build a map of (year, month) -> (sailing_distance, motoring_distance)
        let mut month_data: std::collections::HashMap<(i32, u32), (f64, f64)> =
            std::collections::HashMap::new();

        for row in results {
            let year: i32 = row
                .get_opt("year")
                .and_then(|v| v.ok())
                .ok_or("Missing year")?;
            let month: u32 = row
                .get_opt::<u32, _>("month")
                .and_then(|v| v.ok())
                .ok_or("Missing month")?;
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
    ) -> Result<Vec<TrackPoint>, Box<dyn std::error::Error>> {
        let mut conn = self
            .pool
            .get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;

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
            )
            .map_err(|e| format!("Database query error: {}", e))?
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
            )
            .map_err(|e| format!("Database query error: {}", e))?
        } else {
            return Err("Either trip_id or both start and end timestamps are required".into());
        };

        // min_interval_ms derived from max_points interpreted as max samples per hour.
        // e.g. max_points=60 → one sample per minute (3_600_000ms / 60 = 60_000ms).
        // 0 or None means no filtering.
        let min_interval_ms: Option<i64> = max_points
            .filter(|&m| m > 0)
            .map(|m| 3_600_000_i64 / m as i64);

        let mut track: Vec<TrackPoint> = Vec::new();
        let mut last_included_ts: Option<DateTime<Utc>> = None;

        for row in &results {
            let timestamp_str = row
                .get_opt::<String, _>("timestamp")
                .and_then(|v| v.ok())
                .unwrap_or_default();

            // Apply time-based filter when max_points_per_hour is set
            let include = if let Some(min_ms) = min_interval_ms {
                match chrono::DateTime::parse_from_rfc3339(&timestamp_str) {
                    Ok(parsed_ts) => {
                        let parsed_utc = parsed_ts.with_timezone(&Utc);
                        let include = last_included_ts
                            .map(|last| (parsed_utc - last).num_milliseconds() >= min_ms)
                            .unwrap_or(true);
                        if include {
                            last_included_ts = Some(parsed_utc);
                        }
                        include
                    }
                    Err(_) => true, // Unparseable timestamp: include to avoid data loss
                }
            } else {
                true
            };

            if include {
                track.push(TrackPoint {
                    timestamp: timestamp_str,
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
                });
            }
        }

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
    ) -> Result<Vec<WebMetricData>, Box<dyn std::error::Error>> {
        let mut conn = self
            .pool
            .get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;

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
            ).map_err(|e| format!("Database query error: {}", e))?
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
            )
            .map_err(|e| format!("Database query error: {}", e))?
        } else {
            return Err("Either trip_id or both start and end timestamps are required".into());
        };

        let metrics: Vec<WebMetricData> = results
            .iter()
            .map(|row| WebMetricData {
                timestamp: get_or_log(row, "timestamp", String::new(), "fetch_metrics"),
                metric_id: get_or_log(row, "metric_id", String::new(), "fetch_metrics"),
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
    ) -> Result<MultiMetricData, Box<dyn std::error::Error>> {
        if metrics.is_empty() {
            return Err("At least one metric_id is required".into());
        }

        let in_clause = metrics
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<_>>()
            .join(",");

        let mut conn = self
            .pool
            .get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;

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
            ).map_err(|e| format!("Database query error: {}", e))?
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
            )
            .map_err(|e| format!("Database query error: {}", e))?
        } else {
            return Err("Either trip_id or both start and end timestamps are required".into());
        };

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

        Ok(MultiMetricData { metrics: map })
    }

    /// Fetch speed distribution data for a trip
    pub fn fetch_speed_distribution(
        &self,
        trip_id: Option<u32>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    ) -> Result<SpeedDistributionData, Box<dyn std::error::Error>> {
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
        let mut conn = self
            .pool
            .get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;

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
            )
            .map_err(|e| format!("Database query error: {}", e))?
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
            )
            .map_err(|e| format!("Database query error: {}", e))?
        } else {
            return Err("Either trip_id or both start and end timestamps are required".into());
        };

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
    ) -> Result<WindStatisticsData, Box<dyn std::error::Error>> {
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
        let mut conn = self
            .pool
            .get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;

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
            )
            .map_err(|e| format!("Database query error: {}", e))?
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
            )
            .map_err(|e| format!("Database query error: {}", e))?
        } else {
            return Err("Either trip_id or both start and end timestamps are required".into());
        };

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

    /// Fetch trip legs data - divides trip into legs between mooring periods
    pub fn fetch_trip_legs(
        &self,
        trip_id: u32,
    ) -> Result<TripLegsData, Box<dyn std::error::Error>> {
        let mut conn = self
            .pool
            .get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;

        let results: Vec<mysql::Row> = conn
            .exec(
                r"SELECT
                DATE_FORMAT(timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as timestamp,
                is_moored,
                engine_on,
                total_distance_nm,
                total_time_ms
             FROM vessel_status
             WHERE timestamp BETWEEN
                 (SELECT start_timestamp FROM trips WHERE id = :trip_id)
                 AND COALESCE((SELECT end_timestamp FROM trips WHERE id = :trip_id), NOW())
             ORDER BY timestamp",
                mysql::params! { "trip_id" => trip_id },
            )
            .map_err(|e| format!("Database query error: {}", e))?;

        let mut legs = Vec::new();
        let mut in_leg = false;
        let mut leg_start_timestamp = String::new();
        let mut leg_total_distance = 0.0;
        let mut leg_sailing_distance = 0.0;
        let mut leg_motoring_distance = 0.0;
        let mut leg_sailing_time = 0_u64;
        let mut leg_motoring_time = 0_u64;
        let mut leg_number = 0;

        for row in &results {
            let timestamp: String = get_or_log(row, "timestamp", String::new(), "fetch_trip_legs");
            let is_moored: bool = get_or_log(row, "is_moored", false, "fetch_trip_legs");
            let engine_on_u8: u8 = get_or_log(row, "engine_on", 2u8, "fetch_trip_legs"); // 0=off, 1=on, 2=unknown
            let engine_on = engine_on_u8 == 1; // Only treat 1 (On) as true
            let interval_distance: f64 =
                get_or_log(row, "total_distance_nm", 0.0, "fetch_trip_legs");
            let interval_time: u64 = get_or_log(row, "total_time_ms", 0u64, "fetch_trip_legs");

            if is_moored {
                // End current leg if we have one
                if in_leg && leg_total_distance >= 0.5 {
                    leg_number += 1;
                    legs.push(TripLeg {
                        leg_number,
                        start_timestamp: leg_start_timestamp.clone(),
                        end_timestamp: timestamp.clone(),
                        total_distance_nm: leg_total_distance,
                        sailing_distance_nm: leg_sailing_distance,
                        motoring_distance_nm: leg_motoring_distance,
                        sailing_time_ms: leg_sailing_time,
                        motoring_time_ms: leg_motoring_time,
                        sailing_time_formatted: format_duration_ms(leg_sailing_time),
                        motoring_time_formatted: format_duration_ms(leg_motoring_time),
                    });
                }

                // Reset for next leg
                in_leg = false;
                leg_total_distance = 0.0;
                leg_sailing_distance = 0.0;
                leg_motoring_distance = 0.0;
                leg_sailing_time = 0;
                leg_motoring_time = 0;
            } else {
                // Not moored - either starting or continuing a leg
                if !in_leg {
                    // Start a new leg
                    in_leg = true;
                    leg_start_timestamp = timestamp.clone();
                }

                // Accumulate distance and time for this interval
                leg_total_distance += interval_distance;

                if engine_on {
                    leg_motoring_distance += interval_distance;
                    leg_motoring_time += interval_time;
                } else {
                    leg_sailing_distance += interval_distance;
                    leg_sailing_time += interval_time;
                }
            }
        }

        // Handle last leg if trip ended while underway
        if in_leg && leg_total_distance >= 0.5 {
            leg_number += 1;
            let last_timestamp = match results.last() {
                Some(r) => get_or_log(r, "timestamp", String::new(), "fetch_trip_legs"),
                None => String::new(),
            };

            legs.push(TripLeg {
                leg_number,
                start_timestamp: leg_start_timestamp,
                end_timestamp: last_timestamp,
                total_distance_nm: leg_total_distance,
                sailing_distance_nm: leg_sailing_distance,
                motoring_distance_nm: leg_motoring_distance,
                sailing_time_ms: leg_sailing_time,
                motoring_time_ms: leg_motoring_time,
                sailing_time_formatted: format_duration_ms(leg_sailing_time),
                motoring_time_formatted: format_duration_ms(leg_motoring_time),
            });
        }

        Ok(TripLegsData { legs })
    }

    /// Fetch track analytics for a time range - calculates max speed and fastest segments
    pub fn fetch_track_analytics(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<TrackAnalytics, Box<dyn std::error::Error>> {
        let mut conn = self
            .pool
            .get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;

        let results: Vec<mysql::Row> = conn
            .exec(
                r"SELECT
                DATE_FORMAT(vs.timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as timestamp,
                vs.latitude,
                vs.longitude,
                vs.average_speed_kn,
                vs.engine_on,
                vs.is_moored,
                vs.total_distance_nm,
                vs.total_time_ms
             FROM vessel_status vs
             WHERE vs.timestamp BETWEEN :start AND :end
             AND vs.average_speed_kn IS NOT NULL
             ORDER BY vs.timestamp",
                mysql::params! {
                    "start" => start.format("%Y-%m-%d %H:%M:%S").to_string(),
                    "end" => end.format("%Y-%m-%d %H:%M:%S").to_string(),
                },
            )
            .map_err(|e| format!("Database query error: {}", e))?;

        if results.is_empty() {
            return Ok(TrackAnalytics {
                max_speed_kn: None,
                max_speed_timestamp: None,
                average_speed_kn: None,
                average_speed_sailing_kn: None,
                average_speed_motoring_kn: None,
                fastest_1nm: None,
                fastest_5nm: None,
                fastest_10nm: None,
                fastest_25nm: None,
            });
        }

        let mut distance = 0.0;
        let mut time_h = 0.0;
        let mut distance_engine = 0.0;
        let mut time_engine_h = 0.0;
        let mut distance_sailing = 0.0;
        let mut time_sailing_h = 0.0;

        // Find max speed when sailing
        let mut max_speed = None;
        let mut max_speed_timestamp = None;

        // Collect track points
        let mut track_points = Vec::new();
        for row in &results {
            let timestamp: String = row
                .get_opt("timestamp")
                .and_then(|v| v.ok())
                .unwrap_or_default();
            let latitude: Option<f64> = row.get_opt("latitude").and_then(|v| v.ok());
            let longitude: Option<f64> = row.get_opt("longitude").and_then(|v| v.ok());
            let speed: Option<f64> = row.get_opt("average_speed_kn").and_then(|v| v.ok());
            let engine_on: bool = row
                .get_opt("engine_on")
                .and_then(|v| v.ok())
                .map(|v: u8| v == 1) // Only treat 1 (On) as true
                .unwrap_or(false);
            let is_moored: bool = row
                .get_opt("is_moored")
                .and_then(|v| v.ok())
                .unwrap_or(false);
            let sample_distance: f64 = row
                .get_opt("total_distance_nm")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let sample_time_ms: u64 = row
                .get_opt("total_time_ms")
                .and_then(|v| v.ok())
                .unwrap_or(0);

            if let (Some(lat), Some(lon), Some(spd)) = (latitude, longitude, speed) {
                if !engine_on && (max_speed.is_none() || spd > max_speed.unwrap()) {
                    max_speed = Some(spd);
                    max_speed_timestamp = Some(timestamp.clone());
                }
                if !is_moored {
                    distance += sample_distance;
                    time_h += sample_time_ms as f64 / 3600000.0; // Convert ms to hours
                    if engine_on {
                        distance_engine += sample_distance;
                        time_engine_h += sample_time_ms as f64 / 3600000.0;
                    } else {
                        distance_sailing += sample_distance;
                        time_sailing_h += sample_time_ms as f64 / 3600000.0;
                    }
                }
                track_points.push((timestamp, lat, lon, spd, engine_on));
            }
        }

        // Calculate fastest segments for 1NM, 5NM, and 10NM
        let fastest_1nm = find_fastest_segment(&track_points, 1.0);
        let fastest_5nm = find_fastest_segment(&track_points, 5.0);
        let fastest_10nm = find_fastest_segment(&track_points, 10.0);
        let fastest_25nm = find_fastest_segment(&track_points, 25.0);

        Ok(TrackAnalytics {
            max_speed_kn: max_speed,
            max_speed_timestamp,
            average_speed_kn: if time_h > 0.0 {
                Some(distance / time_h)
            } else {
                None
            },
            average_speed_sailing_kn: if time_sailing_h > 0.0 {
                Some(distance_sailing / time_sailing_h)
            } else {
                None
            },
            average_speed_motoring_kn: if time_engine_h > 0.0 {
                Some(distance_engine / time_engine_h)
            } else {
                None
            },
            fastest_1nm,
            fastest_5nm,
            fastest_10nm,
            fastest_25nm,
        })
    }

    /// Fetch heatmap data - distance traveled grouped by day for 365 days before the given date.
    /// Uses a per-day database cache (heatmap_cache) to avoid recomputing past days.
    /// Today is always recomputed fresh since vessel_status data for it is still being written.
    pub fn fetch_heatmap(
        &self,
        end_date: NaiveDate,
    ) -> Result<HeatmapData, Box<dyn std::error::Error>> {
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

        let mut conn = self
            .pool
            .get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;

        // Ensure the cache table exists so existing deployments work without a manual migration
        conn.query_drop(
            r"CREATE TABLE IF NOT EXISTS heatmap_cache (
                date DATE NOT NULL COMMENT 'UTC date of the aggregated sailing distance',
                distance_nm DOUBLE NOT NULL DEFAULT 0 COMMENT 'Total sailing distance in nautical miles',
                PRIMARY KEY (date)
              ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci",
        ).map_err(|e| format!("Failed to ensure heatmap_cache table: {}", e))?;

        // Step 1: Load already-cached days for [start_dt, cache_end]
        let cached_rows: Vec<mysql::Row> = conn
            .exec(
                "SELECT DATE_FORMAT(date, '%Y-%m-%d') as day, distance_nm \
             FROM heatmap_cache WHERE date BETWEEN :start AND :end",
                mysql::params! {
                    "start" => start_dt.to_string(),
                    "end" => cache_end.to_string(),
                },
            )
            .map_err(|e| format!("Database query error (heatmap cache read): {}", e))?;

        let mut day_distances: std::collections::HashMap<String, f64> =
            std::collections::HashMap::new();
        for row in cached_rows {
            let date: String = row.get_opt("day").and_then(|v| v.ok()).unwrap_or_default();
            let dist: f64 = row
                .get_opt("distance_nm")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            day_distances.insert(date, dist);
        }

        // Step 2: Find the earliest missing date in [start_dt, cache_end].
        // All dates from that point on are considered stale — this keeps the recompute
        // query simple (a single range scan) and avoids building an IN list.
        let mut recompute_from: Option<NaiveDate> = None;
        let mut d = start_dt;
        while d <= cache_end {
            if !day_distances.contains_key(&d.format("%Y-%m-%d").to_string()) {
                recompute_from = Some(d);
                break;
            }
            d += chrono::Duration::days(1);
        }

        // Step 3: Recompute from the first missing date to cache_end using a simple range query
        if let Some(from_dt) = recompute_from {
            let results: Vec<mysql::Row> = conn
                .exec(
                    "SELECT DATE_FORMAT(DATE(timestamp), '%Y-%m-%d') as day, \
                        COALESCE(SUM(COALESCE(total_distance_nm, 0)), 0) as total_distance \
                 FROM vessel_status \
                 WHERE timestamp >= :from_dt AND DATE(timestamp) <= :cache_end AND is_moored = 0 \
                 GROUP BY DATE(timestamp)",
                    mysql::params! {
                        "from_dt" => from_dt.to_string(),
                        "cache_end" => cache_end.to_string(),
                    },
                )
                .map_err(|e| format!("Database query error (heatmap recompute): {}", e))?;

            let mut computed: std::collections::HashMap<String, f64> =
                std::collections::HashMap::new();
            for row in results {
                let date: String = row.get_opt("day").and_then(|v| v.ok()).unwrap_or_default();
                let dist: f64 = row
                    .get_opt("total_distance")
                    .and_then(|v| v.ok())
                    .unwrap_or(0.0);
                computed.insert(date, dist);
            }

            // Batch INSERT IGNORE all dates in [from_dt, cache_end] — including 0-distance days
            // so they won't be considered missing on the next call.
            let mut rows: Vec<(String, f64)> = Vec::new();
            let mut d = from_dt;
            while d <= cache_end {
                let s = d.format("%Y-%m-%d").to_string();
                let dist = computed.get(&s).copied().unwrap_or(0.0);
                let dist = if dist.is_finite() { dist } else { 0.0 };
                rows.push((s.clone(), dist));
                // Don't overwrite dates already loaded from cache — INSERT IGNORE
                // preserves them in the DB, and entry() preserves them in memory.
                day_distances.entry(s).or_insert(dist);
                d += chrono::Duration::days(1);
            }

            if !rows.is_empty() {
                conn.exec_batch(
                    "INSERT IGNORE INTO heatmap_cache (date, distance_nm) VALUES (?, ?)",
                    rows.iter().map(|(date, dist)| (date.as_str(), *dist)),
                )
                .map_err(|e| format!("Failed to write heatmap cache: {}", e))?;
            }
        }

        // Step 4: Always recompute today fresh if it falls within the requested window
        if end_dt >= today {
            let today_str = today.format("%Y-%m-%d").to_string();
            let row: Option<mysql::Row> = conn
                .exec_first(
                    "SELECT COALESCE(SUM(COALESCE(total_distance_nm, 0)), 0) as total_distance \
                 FROM vessel_status \
                 WHERE DATE(timestamp) = :today AND is_moored = 0",
                    mysql::params! { "today" => &today_str },
                )
                .map_err(|e| format!("Database query error (heatmap today): {}", e))?;
            let dist: f64 = row
                .and_then(|r| r.get_opt("total_distance").and_then(|v| v.ok()))
                .unwrap_or(0.0);
            day_distances.insert(today_str, dist);
        }

        // Step 5: Assemble sorted result over [start_dt, end_dt]; skip zero-distance days
        let mut days: Vec<HeatmapDay> = Vec::new();
        let mut d = start_dt;
        while d <= end_dt {
            let s = d.format("%Y-%m-%d").to_string();
            if let Some(&dist) = day_distances.get(&s) {
                if dist > 0.0 {
                    days.push(HeatmapDay {
                        date: s,
                        distance_nm: dist,
                    });
                }
            }
            d += chrono::Duration::days(1);
        }

        // Step 6: Compute aggregate statistics
        let mut min_distance: f64 = f64::MAX;
        let mut max_distance: f64 = 0.0;
        let mut total_distance: f64 = 0.0;
        for day in &days {
            total_distance += day.distance_nm;
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
        })
    }

    /// Delete heatmap_cache rows that overlap [start_date, end_date] so they are
    /// recomputed fresh on the next fetch_heatmap() call.
    pub fn invalidate_heatmap_cache(
        &self,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get_conn().map_err(|e| {
            format!(
                "Database connection error (invalidate heatmap cache): {}",
                e
            )
        })?;
        conn.query_drop(
            r"CREATE TABLE IF NOT EXISTS heatmap_cache (
                date DATE NOT NULL,
                distance_nm DOUBLE NOT NULL DEFAULT 0,
                PRIMARY KEY (date)
              ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci",
        )
        .map_err(|e| format!("Failed to ensure heatmap_cache table: {}", e))?;
        conn.exec_drop(
            "DELETE FROM heatmap_cache WHERE date BETWEEN :start AND :end",
            mysql::params! {
                "start" => start_date.format("%Y-%m-%d").to_string(),
                "end"   => end_date.format("%Y-%m-%d").to_string(),
            },
        )
        .map_err(|e| format!("Failed to invalidate heatmap cache: {}", e))?;
        Ok(())
    }

    /// Get system status (tracking and metrics enabled/disabled state)
    pub fn get_system_status(&self, key: &str) -> Result<bool, Box<dyn Error>> {
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
    pub fn set_system_status(&self, key: &str, value: bool) -> Result<(), Box<dyn Error>> {
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
    pub fn set_system_status_string(&self, key: &str, value: &str) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        conn.exec_drop(
            "INSERT INTO system_status (status_key, status_value) VALUES (:key, :value) ON DUPLICATE KEY UPDATE status_value = :value",
            mysql::params! { "key" => key, "value" => value },
        )?;
        Ok(())
    }

    /// Read an arbitrary string value from system_status. Returns None if the key is absent.
    pub fn get_system_status_string(&self, key: &str) -> Result<Option<String>, Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        let row: Option<String> = conn.exec_first(
            "SELECT status_value FROM system_status WHERE status_key = :key",
            mysql::params! { "key" => key },
        )?;
        Ok(row)
    }
}

/// Helper function to find fastest segment for a given target distance
fn find_fastest_segment(
    track_points: &[(String, f64, f64, f64, bool)],
    target_distance_nm: f64,
) -> Option<FastestSegment> {
    if track_points.len() < 2 {
        return None;
    }

    let mut fastest: Option<FastestSegment> = None;

    // Use sliding window approach
    for start_idx in 0..track_points.len() {
        let (start_ts, _start_lat, _start_lon, _, start_engine) = &track_points[start_idx];

        // Skip if motoring
        if *start_engine {
            continue;
        }

        let mut cumulative_distance = 0.0;

        for end_idx in (start_idx + 1)..track_points.len() {
            let (end_ts, end_lat, end_lon, _, end_engine) = &track_points[end_idx];

            // Check if entire segment is sailing
            if *end_engine {
                break;
            }

            // Calculate distance between consecutive points
            let prev_idx = end_idx - 1;
            let (_, prev_lat, prev_lon, _, _) = &track_points[prev_idx];
            let segment_dist = haversine_distance_nm(*prev_lat, *prev_lon, *end_lat, *end_lon);
            cumulative_distance += segment_dist;

            // Check if we've reached or exceeded target distance
            if cumulative_distance >= target_distance_nm {
                // Calculate duration
                let start_time = match chrono::NaiveDateTime::parse_from_str(
                    &start_ts.replace('Z', ""),
                    "%Y-%m-%dT%H:%M:%S%.f",
                ) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                let end_time = match chrono::NaiveDateTime::parse_from_str(
                    &end_ts.replace('Z', ""),
                    "%Y-%m-%dT%H:%M:%S%.f",
                ) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                let duration_ms = (end_time - start_time).num_milliseconds() as u64;

                if duration_ms > 0 {
                    let avg_speed = cumulative_distance / (duration_ms as f64 / 1000.0 / 3600.0);

                    // Check if this is the fastest so far
                    if fastest.is_none() || avg_speed > fastest.as_ref().unwrap().average_speed_kn {
                        fastest = Some(FastestSegment {
                            distance_nm: cumulative_distance,
                            average_speed_kn: avg_speed,
                            duration_ms,
                            start_timestamp: start_ts.clone(),
                            end_timestamp: end_ts.clone(),
                        });
                    }
                }
                break;
            }
        }
    }

    fastest
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
            EngineStatus::Off,
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
    fn test_heatmap_gap_triggers_partial_recompute() {
        let db = setup_db();
        clear_heatmap_cache(&db);

        // Manually pre-load cache for 2020-06-10 only, leaving 11-14 as gaps
        let mut conn = db.pool.get_conn().expect("get_conn");
        conn.query_drop(
            r"CREATE TABLE IF NOT EXISTS heatmap_cache (
                date DATE NOT NULL,
                distance_nm DOUBLE NOT NULL DEFAULT 0,
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
}
