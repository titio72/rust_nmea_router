use crate::db::types::{
    VesselDatabase, TripSummary, TrackPoint, WebMetricData, SpeedDistributionData,
    WindStatisticsData, TripLeg, TripLegsData, HeatmapDay, HeatmapData, 
    FastestSegment, TrackAnalytics, MonthlyStatistic, MonthlyStatistics, 
    format_duration_ms,
};
use std::error::Error;
use crate::utilities::haversine_distance_nm;
use mysql::params;
use mysql::prelude::Queryable;

impl VesselDatabase {
    pub fn fetch_trip(&self, trip_id: u32) -> Result<Option<TripSummary>, Box<dyn std::error::Error>> {
        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        let row: Option<mysql::Row> = conn.exec_first(
            r"SELECT id, description, 
                     DATE_FORMAT(start_timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as start_ts,
                     DATE_FORMAT(end_timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as end_ts,
                     total_distance_sailed, total_distance_motoring,
                     (total_distance_sailed + total_distance_motoring) as total_distance,
                     total_time_sailing, total_time_motoring, total_time_moored
              FROM trips
              WHERE id = :trip_id",
            mysql::params! {
                "trip_id" => trip_id,
            },
        ).map_err(|e| format!("Database query error: {}", e))?;
        
        if let Some(row) = row {
            let trip = TripSummary {
                id: row.get_opt("id").and_then(|v| v.ok()).unwrap_or(0),
                description: row.get_opt::<String, _>("description").and_then(|v| v.ok()).unwrap_or_default(),
                start_date: row.get_opt::<String, _>("start_ts").and_then(|v| v.ok()).unwrap_or_default(),
                end_date: row.get_opt::<String, _>("end_ts").and_then(|v| v.ok()).unwrap_or_default(),
                total_distance_nm: row.get_opt::<f64, _>("total_distance").and_then(|v| v.ok()).unwrap_or(0.0),
                total_time_ms: row.get_opt::<i64, _>("total_time").and_then(|v| v.ok()).unwrap_or(0),
                sailing_time_ms: row.get_opt::<i64, _>("total_time_sailing").and_then(|v| v.ok()).unwrap_or(0),
                motoring_time_ms: row.get_opt::<i64, _>("total_time_motoring").and_then(|v| v.ok()).unwrap_or(0),
                moored_time_ms: row.get_opt::<i64, _>("total_time_moored").and_then(|v| v.ok()).unwrap_or(0),
                sailing_distance_nm: row.get_opt::<f64, _>("total_distance_sailed").and_then(|v| v.ok()).unwrap_or(0.0),
                motoring_distance_nm: row.get_opt::<f64, _>("total_distance_motoring").and_then(|v| v.ok()).unwrap_or(0.0),
            };
            Ok(Some(trip))
        } else {
            Ok(None)
        }
    }

    /// Fetch trips with optional filtering
    pub fn fetch_trips(&self, year: Option<i32>, last_months: Option<u32>) -> Result<Vec<TripSummary>, Box<dyn std::error::Error>> {
        let mut query = String::from(
            "SELECT id, 
                    description,
                    DATE_FORMAT(start_timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as start_ts,
                    DATE_FORMAT(end_timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as end_ts,
                    (total_distance_sailed + total_distance_motoring) as total_distance,
                    (total_time_sailing + total_time_motoring + total_time_moored) as total_time,
                    total_time_sailing as total_time_sailing,
                    total_time_motoring as total_time_motoring,
                    total_time_moored as total_time_moored,
                    total_distance_sailed as total_distance_sailed,
                    total_distance_motoring as total_distance_motoring
             FROM trips WHERE "
        );

        if let Some(year) = year {
            query.push_str(&format!(" YEAR(start_timestamp) = {}", year));
        } else if let Some(months) = last_months {
            query.push_str(&format!(" start_timestamp >= DATE_SUB(NOW(), INTERVAL {} MONTH)", months));
        } else {
            // If no filters specified, get all trips (to populate year filter with all available years)
            query.push_str(" 1=1");
        }

        query.push_str(" ORDER BY start_timestamp DESC");

        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        let results: Vec<mysql::Row> = conn.query(&query)
            .map_err(|e| format!("Database query error: {}", e))?;

        let trips = results
            .iter()
            .map(|row| TripSummary {
                id: row.get_opt("id").and_then(|v| v.ok()).unwrap_or(0),
                description: row.get_opt::<String, _>("description").and_then(|v| v.ok()).unwrap_or_default(),
                start_date: row.get_opt::<String, _>("start_ts").and_then(|v| v.ok()).unwrap_or_default(),
                end_date: row.get_opt::<String, _>("end_ts").and_then(|v| v.ok()).unwrap_or_default(),
                total_distance_nm: row.get_opt::<f64, _>("total_distance").and_then(|v| v.ok()).unwrap_or(0.0),
                total_time_ms: row.get_opt::<i64, _>("total_time").and_then(|v| v.ok()).unwrap_or(0),
                sailing_time_ms: row.get_opt::<i64, _>("total_time_sailing").and_then(|v| v.ok()).unwrap_or(0),
                motoring_time_ms: row.get_opt::<i64, _>("total_time_motoring").and_then(|v| v.ok()).unwrap_or(0),
                moored_time_ms: row.get_opt::<i64, _>("total_time_moored").and_then(|v| v.ok()).unwrap_or(0),
                sailing_distance_nm: row.get_opt::<f64, _>("total_distance_sailed").and_then(|v| v.ok()).unwrap_or(0.0),
                motoring_distance_nm: row.get_opt::<f64, _>("total_distance_motoring").and_then(|v| v.ok()).unwrap_or(0.0),
            })
            .collect();

        Ok(trips)
    }

    /// Fetch monthly statistics since January 2020
    /// Returns monthly sailed and motored nautical miles, including months with no activity
    pub fn fetch_monthly_statistics(&self) -> Result<MonthlyStatistics, Box<dyn std::error::Error>> {
        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        // Get all trip data grouped by year and month
        let results: Vec<mysql::Row> = conn.query(
            r"SELECT YEAR(start_timestamp) as year,
                     MONTH(start_timestamp) as month,
                     SUM(total_distance_sailed) as sailing_distance,
                     SUM(total_distance_motoring) as motoring_distance
              FROM trips
              WHERE start_timestamp >= '2020-01-01'
              GROUP BY YEAR(start_timestamp), MONTH(start_timestamp)
              ORDER BY year ASC, month ASC"
        )
            .map_err(|e| format!("Database query error: {}", e))?;

        // Build a map of (year, month) -> (sailing_distance, motoring_distance)
        let mut month_data: std::collections::HashMap<(i32, u32), (f64, f64)> = std::collections::HashMap::new();
        
        for row in results {
            let year: i32 = row.get_opt("year")
                .and_then(|v| v.ok())
                .ok_or("Missing year")?;
            let month: u32 = row.get_opt::<u32, _>("month")
                .and_then(|v| v.ok())
                .ok_or("Missing month")?;
            let sailing_distance: f64 = row.get_opt::<f64, _>("sailing_distance")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            let motoring_distance: f64 = row.get_opt::<f64, _>("motoring_distance")
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
            let start_month = if year == 2020 { 1 } else { 1 };
            let end_month = if year == current_year { current_month } else { 12 };
            
            for month in start_month..=end_month {
                let (sailing_dist, motoring_dist) = month_data
                    .get(&(year, month))
                    .copied()
                    .unwrap_or((0.0, 0.0));
                
                let date = format!("{:04}-{:02}", year, month);
                
                all_months.push(MonthlyStatistic {
                    year,
                    month: month as u32,
                    date,
                    sailing_distance_nm: sailing_dist,
                    motoring_distance_nm: motoring_dist,
                });
            }
        }

        Ok(MonthlyStatistics {
            months: all_months,
        })
    }

    /// Fetch vessel track data by trip_id or date range
    pub fn fetch_track(&self, trip_id: Option<u32>, start: Option<&str>, end: Option<&str>) -> Result<Vec<TrackPoint>, Box<dyn std::error::Error>> {
        let query = if let Some(trip_id) = trip_id {
            // Get trip date range and fetch vessel_status data for that period
            format!(
                "SELECT DATE_FORMAT(vs.timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as timestamp,
                        vs.latitude, vs.longitude, vs.average_speed_kn, vs.max_speed_kn, 
                        vs.is_moored, vs.engine_on, vs.total_distance_nm, vs.total_time_ms,
                        vs.average_wind_speed_kn, vs.average_wind_angle_deg,
                        vs.cog_deg, vs.average_heading_deg
                 FROM vessel_status vs
                 JOIN trips t ON vs.timestamp BETWEEN t.start_timestamp AND COALESCE(t.end_timestamp, NOW())
                 WHERE t.id = {}
                 ORDER BY vs.timestamp",
                trip_id
            )
        } else if let (Some(start), Some(end)) = (start, end) {
            format!(
                "SELECT DATE_FORMAT(timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as timestamp,
                        latitude, longitude, average_speed_kn, max_speed_kn, is_moored, engine_on,
                        total_distance_nm, total_time_ms,
                        average_wind_speed_kn, average_wind_angle_deg,
                        cog_deg, average_heading_deg
                 FROM vessel_status WHERE timestamp BETWEEN '{}' AND '{}' ORDER BY timestamp",
                start, end
            )
        } else {
            return Err("Either trip_id or both start and end timestamps are required".into());
        };

        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        let results: Vec<mysql::Row> = conn.query(&query)
            .map_err(|e| format!("Database query error: {}", e))?;

        let track = results
            .iter()
            .map(|row| TrackPoint {
                timestamp: row.get_opt::<String, _>("timestamp")
                    .and_then(|v| v.ok())
                    .unwrap_or_default(),
                latitude: row.get_opt::<f64, _>("latitude")
                    .and_then(|v| v.ok()),
                longitude: row.get_opt::<f64, _>("longitude")
                    .and_then(|v| v.ok()),
                avg_speed_kn: row.get_opt::<f64, _>("average_speed_kn")
                    .and_then(|v| v.ok()),
                max_speed_kn: row.get_opt::<f64, _>("max_speed_kn")
                    .and_then(|v| v.ok()),
                moored: row.get_opt::<i32, _>("is_moored")
                    .and_then(|v| v.ok())
                    .map(|v| v != 0)
                    .unwrap_or(false),
                engine_on: row.get_opt::<u8, _>("engine_on")
                    .and_then(|v| v.ok())
                    .unwrap_or(2),  // Default to unknown if not available
                total_distance_nm: row.get_opt::<f64, _>("total_distance_nm")
                    .and_then(|v| v.ok()),
                total_time_ms: row.get_opt::<u64, _>("total_time_ms")
                    .and_then(|v| v.ok())
                    .unwrap_or(0),
                average_wind_speed_kn: row.get_opt::<f64, _>("average_wind_speed_kn")
                    .and_then(|v| v.ok()),
                average_wind_angle_deg: row.get_opt::<f64, _>("average_wind_angle_deg")
                    .and_then(|v| v.ok()),
                cog_deg: row.get_opt::<f64, _>("cog_deg")
                    .and_then(|v| v.ok()),
                average_heading_deg: row.get_opt::<f64, _>("average_heading_deg")
                    .and_then(|v| v.ok()),
            })
            .collect();

        Ok(track)
    }

    /// Fetch environmental metrics by metric_id with optional trip_id or date range
    pub fn fetch_metrics(&self, metric: &str, trip_id: Option<u32>, start: Option<&str>, end: Option<&str>) -> Result<Vec<WebMetricData>, Box<dyn std::error::Error>> {
        let query = if let Some(trip_id) = trip_id {
            format!(
                "SELECT DATE_FORMAT(e.timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as timestamp,
                        e.metric_id, e.value_avg, e.value_max, e.value_min 
                 FROM environmental_data e 
                 WHERE e.timestamp >= (SELECT COALESCE(start_timestamp, NOW()) FROM trips WHERE id = {}) AND e.timestamp <= (SELECT COALESCE(end_timestamp, NOW()) FROM trips WHERE id = {})
                 AND e.metric_id = '{}' 
                 ORDER BY e.timestamp",
                trip_id, trip_id, metric
            )
        } else if let (Some(start), Some(end)) = (start, end) {
            format!(
                "SELECT DATE_FORMAT(timestamp, '%Y-%m-%dT%H:%i:%S.000Z') as timestamp,
                        metric_id, value_avg, value_max, value_min
                 FROM environmental_data 
                 WHERE metric_id = '{}' AND timestamp BETWEEN '{}' AND '{}' 
                 ORDER BY timestamp",
                metric, start, end
            )
        } else {
            return Err("Either trip_id or both start and end timestamps are required".into());
        };

        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        let results: Vec<mysql::Row> = conn.query(&query)
            .map_err(|e| format!("Database query error: {}", e))?;

        let metrics = results
            .iter()
            .map(|row| WebMetricData {
                timestamp: row.get_opt::<String, _>("timestamp")
                    .and_then(|v| v.ok())
                    .unwrap_or_default(),
                metric_id: row.get_opt::<String, _>("metric_id")
                    .and_then(|v| v.ok())
                    .unwrap_or_default(),
                avg_value: row.get_opt::<f64, _>("value_avg")
                    .and_then(|v| v.ok()),
                max_value: row.get_opt::<f64, _>("value_max")
                    .and_then(|v| v.ok()),
                min_value: row.get_opt::<f64, _>("value_min")
                    .and_then(|v| v.ok())
            })
            .collect();

        Ok(metrics)
    }

    /// Fetch speed distribution data for a trip
    pub fn fetch_speed_distribution(&self, trip_id: Option<u32>, start: Option<&str>, end: Option<&str>) -> Result<SpeedDistributionData, Box<dyn std::error::Error>> {
        // Create buckets for speeds from 0 to 10 knots in 0.5 knot increments
        let max_speed = 10.0;
        let bucket_size = 0.5;
        let num_buckets = ((max_speed / bucket_size) as f64).ceil() as usize;
        
        let mut sailing_buckets = vec![0.0; num_buckets];
        let mut motoring_buckets = vec![0.0; num_buckets];
        let mut labels = Vec::with_capacity(num_buckets);
        
        // Initialize labels
        for i in 0..num_buckets {
            let min_speed = i as f64 * bucket_size;
            let max_speed = (i + 1) as f64 * bucket_size;
            labels.push(format!("{:.1}-{:.1}", min_speed, max_speed));
        }
        
        // Build query based on parameters
        let query = if let Some(trip_id) = trip_id {
            format!(
                "SELECT vs.average_speed_kn, vs.total_distance_nm, vs.engine_on
                 FROM vessel_status vs
                 JOIN trips t ON vs.timestamp BETWEEN t.start_timestamp AND COALESCE(t.end_timestamp, NOW())
                 WHERE t.id = {}
                 ORDER BY vs.timestamp",
                trip_id
            )
        } else if let (Some(start), Some(end)) = (start, end) {
            format!(
                "SELECT vs.average_speed_kn, vs.total_distance_nm, vs.engine_on
                 FROM vessel_status vs
                 WHERE vs.timestamp BETWEEN '{}' AND '{}'
                 ORDER BY vs.timestamp",
                start, end
            )
        } else {
            return Err("Either trip_id or both start and end timestamps are required".into());
        };

        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        let results: Vec<mysql::Row> = conn.query(&query)
            .map_err(|e| format!("Database query error: {}", e))?;

        // Process each row and accumulate distances in buckets
        for row in results {
            let speed: Option<f64> = row.get_opt("average_speed_kn")
                .and_then(|v| v.ok());
            let distance: Option<f64> = row.get_opt("total_distance_nm")
                .and_then(|v| v.ok());
            let engine_on: u8 = row.get_opt("engine_on")
                .and_then(|v| v.ok())
                .unwrap_or(2);  // Default to unknown
            
            if let (Some(speed), Some(distance)) = (speed, distance) {
                let bucket_index = ((speed / bucket_size).floor() as usize).min(num_buckets - 1);
                
                // Only count as motoring if engine_on == 1, treat unknown (2) as sailing
                if engine_on == 1 {
                    motoring_buckets[bucket_index] += distance;
                } else {
                    sailing_buckets[bucket_index] += distance;
                }
            }
        }

        Ok(SpeedDistributionData {
            labels,
            sailing: sailing_buckets,
            motoring: motoring_buckets,
        })
    }

    /// Fetch wind statistics data for a trip or time range
    pub fn fetch_wind_statistics(&self, trip_id: Option<u32>, start: Option<&str>, end: Option<&str>) -> Result<WindStatisticsData, Box<dyn std::error::Error>> {
        // Create 72 buckets for wind directions (360 degrees / 5 degrees = 72 buckets)
        let bucket_size = 5.0;
        let num_buckets = 72;
        
        let mut wind_distances = vec![0.0; num_buckets];
        let mut max_wind_speeds = vec![0.0; num_buckets];
        let mut directions = Vec::with_capacity(num_buckets);
        
        // Initialize directions (0, 5, 10, ..., 355)
        for i in 0..num_buckets {
            directions.push(i as f64 * bucket_size);
        }
        
        // Build query based on parameters
        let query = if let Some(trip_id) = trip_id {
            format!(
                r"SELECT 
                    vs.average_wind_angle_deg, 
                    vs.average_wind_speed_kn,
                    vs.timestamp
                 FROM vessel_status vs
                 JOIN trips t ON vs.timestamp BETWEEN t.start_timestamp AND COALESCE(t.end_timestamp, NOW())
                 WHERE t.id = {}
                 AND vs.average_wind_angle_deg IS NOT NULL 
                 AND vs.average_wind_speed_kn IS NOT NULL
                 AND vs.is_moored = false
                 ORDER BY vs.timestamp",
                trip_id
            )
        } else if let (Some(start), Some(end)) = (start, end) {
            format!(
                r"SELECT 
                    vs.average_wind_angle_deg, 
                    vs.average_wind_speed_kn,
                    vs.timestamp
                 FROM vessel_status vs
                 WHERE vs.timestamp BETWEEN '{}' AND '{}'
                 AND vs.average_wind_angle_deg IS NOT NULL 
                 AND vs.average_wind_speed_kn IS NOT NULL
                 AND vs.is_moored = false
                 ORDER BY vs.timestamp",
                start, end
            )
        } else {
            return Err("Either trip_id or both start and end timestamps are required".into());
        };

        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        let results: Vec<mysql::Row> = conn.query(&query)
            .map_err(|e| format!("Database query error: {}", e))?;

        // Collect all data points first
        let mut data_points = Vec::new();
        for row in results {
            let wind_direction: Option<f64> = row.get_opt("average_wind_angle_deg")
                .and_then(|v| v.ok());
            let wind_speed: Option<f64> = row.get_opt("average_wind_speed_kn")
                .and_then(|v| v.ok());
            let timestamp: Option<String> = row.get_opt("timestamp")
                .and_then(|v| v.ok());
            
            if let (Some(direction), Some(speed), Some(ts)) = (wind_direction, wind_speed, timestamp) {
                let dt = chrono::NaiveDateTime::parse_from_str(&ts, "%Y-%m-%d %H:%M:%S%.f")
                    .or_else(|_| chrono::NaiveDateTime::parse_from_str(&ts, "%Y-%m-%d %H:%M:%S"))
                    .map_err(|e| format!("Timestamp parse error for '{}': {}", ts, e))?;
                data_points.push((direction, speed, dt));
            }
        }

        // Process consecutive data points to calculate time intervals
        for i in 0..data_points.len().saturating_sub(1) {
            let (direction, speed, curr_dt) = data_points[i];
            let (_, _, next_dt) = data_points[i + 1];
            
            let time_hours = (next_dt - curr_dt).num_seconds() as f64 / 3600.0;
            
            if time_hours > 0.0 {
                // Calculate bucket index (normalize direction to 0-359, then divide by 5)
                let normalized_direction = direction % 360.0;
                let bucket_index = ((normalized_direction / bucket_size).floor() as usize).min(num_buckets - 1);
                
                // Add wind distance (speed * time)
                let wind_distance = speed * time_hours;
                wind_distances[bucket_index] += wind_distance;
                
                // Update max wind speed for this bucket
                max_wind_speeds[bucket_index] = f64::max(max_wind_speeds[bucket_index], speed);
            }
        }

        Ok(WindStatisticsData {
            directions,
            wind_distances,
            max_wind_speeds,
        })
    }

    /// Fetch trip legs data - divides trip into legs between mooring periods
    pub fn fetch_trip_legs(&self, trip_id: u32) -> Result<TripLegsData, Box<dyn std::error::Error>> {
        let query = format!(
            r"SELECT 
                DATE_FORMAT(vs.timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as timestamp,
                vs.is_moored,
                vs.engine_on,
                vs.total_distance_nm,
                vs.total_time_ms
             FROM vessel_status vs
             JOIN trips t ON vs.timestamp BETWEEN t.start_timestamp AND COALESCE(t.end_timestamp, NOW())
             WHERE t.id = {}
             ORDER BY vs.timestamp",
            trip_id
        );

        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        let results: Vec<mysql::Row> = conn.query(&query)
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
            let timestamp: String = row.get("timestamp").unwrap_or_default();
            let is_moored: bool = row.get("is_moored").unwrap_or(false);
            let engine_on_u8: u8 = row.get("engine_on").unwrap_or(2); // 0=off, 1=on, 2=unknown
            let engine_on = engine_on_u8 == 1; // Only treat 1 (On) as true
            let interval_distance: f64 = row.get("total_distance_nm").unwrap_or(0.0);
            let interval_time: u64 = row.get("total_time_ms").unwrap_or(0);

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
            let last_timestamp = results.last()
                .and_then(|r| r.get::<String, _>("timestamp"))
                .unwrap_or_default();
                
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
    pub fn fetch_track_analytics(&self, start: &str, end: &str) -> Result<TrackAnalytics, Box<dyn std::error::Error>> {
        let query = format!(
            r"SELECT 
                DATE_FORMAT(vs.timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') as timestamp,
                vs.latitude,
                vs.longitude,
                vs.average_speed_kn,
                vs.engine_on
             FROM vessel_status vs
             WHERE vs.timestamp BETWEEN '{}' AND '{}'
             AND vs.average_speed_kn IS NOT NULL
             ORDER BY vs.timestamp",
            start, end
        );

        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        let results: Vec<mysql::Row> = conn.query(&query)
            .map_err(|e| format!("Database query error: {}", e))?;

        if results.is_empty() {
            return Ok(TrackAnalytics {
                max_speed_kn: None,
                max_speed_timestamp: None,
                fastest_1nm: None,
                fastest_5nm: None,
                fastest_10nm: None,
            });
        }

        // Collect track points
        let mut track_points = Vec::new();
        for row in &results {
            let timestamp: String = row.get_opt("timestamp")
                .and_then(|v| v.ok())
                .unwrap_or_default();
            let latitude: Option<f64> = row.get_opt("latitude")
                .and_then(|v| v.ok());
            let longitude: Option<f64> = row.get_opt("longitude")
                .and_then(|v| v.ok());
            let speed: Option<f64> = row.get_opt("average_speed_kn")
                .and_then(|v| v.ok());
            let engine_on: bool = row.get_opt("engine_on")
                .and_then(|v| v.ok())
                .map(|v: u8| v == 1) // Only treat 1 (On) as true
                .unwrap_or(false);

            if let (Some(lat), Some(lon), Some(spd)) = (latitude, longitude, speed) {
                track_points.push((timestamp, lat, lon, spd, engine_on));
            }
        }

        // Find max speed when sailing
        let mut max_speed = None;
        let mut max_speed_timestamp = None;
        for (timestamp, _, _, speed, engine_on) in &track_points {
            if !engine_on && (max_speed.is_none() || *speed > max_speed.unwrap()) {
                max_speed = Some(*speed);
                max_speed_timestamp = Some(timestamp.clone());
            }
        }

        // Calculate fastest segments for 1NM, 5NM, and 10NM
        let fastest_1nm = find_fastest_segment(&track_points, 1.0);
        let fastest_5nm = find_fastest_segment(&track_points, 5.0);
        let fastest_10nm = find_fastest_segment(&track_points, 10.0);

        Ok(TrackAnalytics {
            max_speed_kn: max_speed,
            max_speed_timestamp,
            fastest_1nm,
            fastest_5nm,
            fastest_10nm,
        })
    }

    /// Fetch heatmap data - distance traveled grouped by day for 365 days before the given date
    pub fn fetch_heatmap(&self, end_date: &str) -> Result<HeatmapData, Box<dyn std::error::Error>> {
        // Parse the end date and calculate start date (365 days before)
        let end_dt = chrono::NaiveDate::parse_from_str(end_date, "%Y-%m-%d")?;
        let start_dt = end_dt - chrono::Duration::days(365);
        
        let query = format!(
            r"SELECT DATE(vs.timestamp) as day, COALESCE(SUM(COALESCE(vs.total_distance_nm, 0)), 0) as total_distance
             FROM vessel_status vs
             WHERE DATE(vs.timestamp) BETWEEN '{}' AND '{}' AND vs.is_moored = 0
             GROUP BY DATE(vs.timestamp)
             ORDER BY vs.timestamp",
            start_dt, end_dt
        );

        let mut conn = self.pool.get_conn()
            .map_err(|e| format!("Database connection error: {}", e))?;
        
        let results: Vec<mysql::Row> = conn.query(&query)
            .map_err(|e| format!("Database query error: {}", e))?;

        let mut days = Vec::new();
        let mut min_distance: f64 = f64::MAX;
        let mut max_distance: f64 = 0.0;
        let mut total_distance: f64 = 0.0;

        for row in results {
            let date: String = row.get_opt("day")
                .and_then(|v| v.ok())
                .unwrap_or_default();
            let distance: f64 = row.get_opt("total_distance")
                .and_then(|v| v.ok())
                .unwrap_or(0.0);
            
            days.push(HeatmapDay {
                date,
                distance_nm: distance,
            });
            
            total_distance += distance;
            if distance > 0.0 {
                min_distance = min_distance.min(distance);
                max_distance = max_distance.max(distance);
            }
        }

        // If no days with distance data, set min_distance to 0
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

    /// Get system status (tracking and metrics enabled/disabled state)
    pub fn get_system_status(&self, key: &str) -> Result<bool, Box<dyn Error>> {
        let cache = self.system_status_cache.lock().unwrap();
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
        let mut cache = self.system_status_cache.lock().unwrap();
        cache.insert(key.to_string(), value);
        
        Ok(())
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
                    "%Y-%m-%dT%H:%M:%S%.f"
                ) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                let end_time = match chrono::NaiveDateTime::parse_from_str(
                    &end_ts.replace('Z', ""),
                    "%Y-%m-%dT%H:%M:%S%.f"
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
    use std::time::{SystemTime, Duration};
    use std::ops::Add;

    #[cfg(test)]
    use crate::config::Config;
    #[cfg(test)]
    use crate::db::test_helpers::{setup_test_db, reset_test_db, add_test_trip, add_test_vessel_status, assert_approx_equal};
    #[cfg(test)]
    use crate::utilities::EngineStatus;

    fn setup_db() -> VesselDatabase {
        let config = Config::load_for_context()
            .expect("Failed to load test config - ensure test_config.json exists");
        let db_url = config.database.connection.connection_url();
        let db = setup_test_db(&db_url)
            .expect("Failed to setup test database - ensure MySQL is running and test database exists");
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
        ).expect("Failed to insert test trip");

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
            ).expect("Failed to insert vessel status");

            current_time = current_time.add(Duration::from_secs(600)); // Every 10 minutes
            lat += 0.01;
            lon += 0.01;
        }

        // Fetch track by trip_id
        let points = db.fetch_track(Some(trip_id), None, None)
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
            "First point latitude"
        );
        assert_approx_equal(
            first.longitude.expect("First point should have longitude"),
            2.0,
            0.1,
            "First point longitude"
        );
        assert_approx_equal(
            last.latitude.expect("Last point should have latitude"),
            41.0 + 0.01 * 2.0,
            0.1,
            "Last point latitude"
        );
        assert_approx_equal(
            last.longitude.expect("Last point should have longitude"),
            2.0 + 0.01 * 2.0,
            0.1,
            "Last point longitude"
        );
    }

    #[test]
    #[ignore]
    fn test_system_status_set() {
        let db = setup_db();

        // Test setting to true
        db.set_system_status("tracking_enabled", true)
            .expect("Failed to set tracking_enabled to true");
        assert!(db.get_system_status("tracking_enabled")
            .expect("Failed to get tracking_enabled"),
            "tracking_enabled should be true");

        // Test setting to false
        db.set_system_status("tracking_enabled", false)
            .expect("Failed to set tracking_enabled to false");
        assert!(!db.get_system_status("tracking_enabled")
            .expect("Failed to get tracking_enabled"),
            "tracking_enabled should be false");

        // Test different key
        db.set_system_status("metrics_enabled", true)
            .expect("Failed to set metrics_enabled");
        assert!(db.get_system_status("metrics_enabled")
            .expect("Failed to get metrics_enabled"),
            "metrics_enabled should be true");
    }

    #[test]
    #[ignore]
    fn test_system_status_default() {
        let db = setup_db();

        // Test that non-existent keys default to true
        let result = db.get_system_status("a_key_that_does_not_exist")
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
        assert!(db.get_system_status("test_key")
            .expect("Failed to get test_key"),
            "test_key should be true after setting");

        // Create a new database instance (simulates application restart)
        let config = Config::load_for_context()
            .expect("Failed to load test config");
        let db_url = config.database.connection.connection_url();
        let db2 = VesselDatabase::new(&db_url)
            .expect("Failed to create second database instance");

        // Verify value persists
        assert!(db2.get_system_status("test_key")
            .expect("Failed to get test_key from new instance"),
            "test_key should persist across database instances");
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
        ).expect("Failed to insert test trip") as i64;

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
            ).expect("Failed to insert vessel status");

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
        let metadata = fs::metadata(&export_path)
            .expect("Failed to get export file metadata");
        assert!(metadata.len() > 0, "Export file should not be empty");

        // Verify JSON structure
        let contents = fs::read_to_string(&export_path)
            .expect("Failed to read export file");
        let json: serde_json::Value = serde_json::from_str(&contents)
            .expect("Export file should contain valid JSON");

        assert!(json["trip"].is_object(), "Should have trip object");
        assert_eq!(json["trip"]["id"], trip_id, "Trip ID should match");
        assert!(json["trip"]["description"].is_string(), "Trip should have description");
        assert_eq!(json["trip"]["description"], "Export Test Trip", "Trip description should match");
        assert!(json["trip"]["start_timestamp"].is_string(), "Trip should have start_timestamp");
        assert!(json["trip"]["end_timestamp"].is_string(), "Trip should have end_timestamp");

        // Verify arrays exist
        assert!(json["vessel_statuses"].is_array(), "Should have vessel_statuses array");
        assert!(json["environmental_metrics"].is_array(), "Should have environmental_metrics array");
        assert!(json["export_metadata"].is_object(), "Should have export_metadata object");

        // Clean up
        fs::remove_file(&export_path)
            .expect("Should be able to delete test file");
    }
}

