// This module is used exclusively by the gap_filler binary; suppress dead_code
// warnings produced when the nmea_router main binary compiles it without calling anything.
#![allow(dead_code)]

/// Gap detection and repair database operations.
use crate::db::types::{VesselDatabase, VesselStatusOperation};
use crate::position_utils::Position;
use crate::trip::Trip;
use crate::utilities::{dirty_instant_to_systemtime, EngineStatus};
use chrono::NaiveDateTime;
use mysql::params;
use mysql::prelude::Queryable;
use std::error::Error;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A detected gap between two consecutive vessel_status records.
#[derive(Debug)]
pub struct Gap {
    /// The record just before the gap
    pub before_id: i64,
    pub before_timestamp: SystemTime,
    pub before_position: Position,
    pub before_heading: Option<f64>,

    /// The record just after the gap (the "restart" record to be corrected)
    pub after_id: i64,
    pub after_timestamp: SystemTime,
    pub after_record: VesselStatusOperation,
}

// ---- Row parsing helpers --------------------------------------------------

fn parse_systemtime(s: &str) -> Result<SystemTime, Box<dyn Error>> {
    let clean = s.trim_end_matches('Z');
    // Accept both "YYYY-MM-DDTHH:MM:SS.fff" and "YYYY-MM-DD HH:MM:SS.fff"
    let dt = NaiveDateTime::parse_from_str(clean, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(clean, "%Y-%m-%d %H:%M:%S%.f"))?;
    let utc = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc);
    Ok(SystemTime::from(utc))
}

fn systemtime_to_instant(st: SystemTime) -> Result<std::time::Instant, Box<dyn std::error::Error>> {
    let elapsed = st
        .elapsed()
        .map_err(|e| format!("Timestamp is in the future (clock skew?): {}", e))?;
    Ok(std::time::Instant::now() - elapsed)
}

fn systemtime_to_mysql_str(st: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Utc> = st.into();
    dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
}

fn duration_between(earlier: SystemTime, later: SystemTime) -> Duration {
    later.duration_since(earlier).unwrap_or(Duration::ZERO)
}

// ---- VesselDatabase impl --------------------------------------------------

impl VesselDatabase {
    /// Scan vessel_status records in `[from, to]` and return every consecutive pair
    /// where the timestamp difference exceeds `threshold`.
    pub fn find_gaps(
        &self,
        from: SystemTime,
        to: SystemTime,
        threshold: Duration,
    ) -> Result<Vec<Gap>, Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;

        let from_str = systemtime_to_mysql_str(from);
        let to_str = systemtime_to_mysql_str(to);

        let rows: Vec<mysql::Row> = conn.exec(
            r"SELECT id,
                     DATE_FORMAT(timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') AS ts,
                     latitude,
                     longitude,
                     average_speed_kn,
                     max_speed_kn,
                     is_moored,
                     engine_on,
                     total_distance_nm,
                     total_time_ms,
                     average_wind_speed_kn,
                     average_wind_angle_deg,
                     cog_deg,
                     average_heading_deg
              FROM vessel_status
              WHERE timestamp BETWEEN :from AND :to
              ORDER BY timestamp ASC",
            params! { "from" => &from_str, "to" => &to_str },
        )?;

        let mut gaps = Vec::new();

        // We keep the previous row's data for comparison
        struct PrevRow {
            id: i64,
            ts: SystemTime,
            lat: f64,
            lon: f64,
            heading_deg: Option<f64>,
        }

        let mut prev: Option<PrevRow> = None;

        for row in rows {
            let id: i64 = row.get("id").ok_or("missing id")?;
            let ts_str: String = row.get("ts").ok_or("missing ts")?;
            let ts = parse_systemtime(&ts_str)?;
            let lat: Option<f64> = row.get("latitude");
            let lon: Option<f64> = row.get("longitude");
            let lat = lat.unwrap_or(0.0);
            let lon = lon.unwrap_or(0.0);
            let heading_deg: Option<f64> = row.get_opt("average_heading_deg").and_then(|r| r.ok());

            if let Some(ref p) = prev {
                let gap_duration = duration_between(p.ts, ts);
                if gap_duration > threshold {
                    // Build the full VesselStatusOperation for the "after" record
                    let avg_speed: Option<f64> =
                        row.get_opt("average_speed_kn").and_then(|r| r.ok());
                    let max_speed: Option<f64> = row.get_opt("max_speed_kn").and_then(|r| r.ok());
                    let is_moored: bool = row.get("is_moored").unwrap_or(false);
                    let engine_on_u8: u8 = row.get("engine_on").unwrap_or(2);
                    let total_distance: Option<f64> =
                        row.get_opt("total_distance_nm").and_then(|r| r.ok());
                    let total_time: u64 = row.get("total_time_ms").unwrap_or(0);
                    let wind_speed: Option<f64> =
                        row.get_opt("average_wind_speed_kn").and_then(|r| r.ok());
                    let wind_angle: Option<f64> =
                        row.get_opt("average_wind_angle_deg").and_then(|r| r.ok());
                    let cog_deg: Option<f64> = row.get_opt("cog_deg").and_then(|r| r.ok());

                    let after_record = VesselStatusOperation {
                        timestamp: systemtime_to_instant(ts)?,
                        position: Position {
                            latitude: lat,
                            longitude: lon,
                        },
                        average_speed_kn: avg_speed.unwrap_or(0.0),
                        max_speed_kn: max_speed.unwrap_or(0.0),
                        is_moored,
                        engine_on: EngineStatus::from_u8(engine_on_u8),
                        total_distance_nm: total_distance.unwrap_or(0.0),
                        total_time_ms: total_time,
                        wind_speed_kn: wind_speed,
                        wind_speed_variance: None,
                        wind_angle_deg: wind_angle,
                        wind_angle_variance: None,
                        cog_deg,
                        average_heading_deg: heading_deg,
                    };

                    gaps.push(Gap {
                        before_id: p.id,
                        before_timestamp: p.ts,
                        before_position: Position {
                            latitude: p.lat,
                            longitude: p.lon,
                        },
                        before_heading: p.heading_deg,
                        after_id: id,
                        after_timestamp: ts,
                        after_record,
                    });
                }
            }

            prev = Some(PrevRow {
                id,
                ts,
                lat,
                lon,
                heading_deg,
            });
        }

        Ok(gaps)
    }

    /// Delete a single vessel_status record by id.
    pub fn delete_vessel_status_by_id(&self, id: i64) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        conn.exec_drop(
            "DELETE FROM vessel_status WHERE id = :id",
            params! { "id" => id },
        )?;
        Ok(())
    }

    /// Insert one synthetic vessel_status record. No trip side-effects.
    pub fn insert_synthetic_vessel_status(
        &self,
        op: &VesselStatusOperation,
    ) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;

        let timestamp =
            chrono::DateTime::<chrono::Utc>::from(dirty_instant_to_systemtime(op.timestamp));

        conn.exec_drop(
            r"INSERT INTO vessel_status
                (timestamp, latitude, longitude, average_speed_kn, max_speed_kn,
                 is_moored, engine_on, total_distance_nm, total_time_ms,
                 average_wind_speed_kn, average_wind_angle_deg, cog_deg, average_heading_deg)
              VALUES
                (:timestamp, :latitude, :longitude, :avg_speed, :max_speed,
                 :is_moored, :engine_on, :total_distance, :total_time,
                 :avg_wind_speed, :avg_wind_angle, :cog_deg, :avg_heading_deg)",
            params! {
                "timestamp" => timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
                "latitude" => op.position.latitude,
                "longitude" => op.position.longitude,
                "avg_speed" => op.average_speed_kn,
                "max_speed" => op.max_speed_kn,
                "is_moored" => op.is_moored,
                "engine_on" => op.engine_on.as_u8(),
                "total_distance" => op.total_distance_nm,
                "total_time" => op.total_time_ms,
                "avg_wind_speed" => op.wind_speed_kn,
                "avg_wind_angle" => op.wind_angle_deg,
                "cog_deg" => op.cog_deg,
                "avg_heading_deg" => op.average_heading_deg,
            },
        )?;

        Ok(())
    }

    /// Find the trip that contains the given time window, if any.
    pub fn find_trip_for_gap(
        &self,
        gap_start: SystemTime,
        gap_end: SystemTime,
    ) -> Result<Option<Trip>, Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;

        let start_str = systemtime_to_mysql_str(gap_start);
        let end_str = systemtime_to_mysql_str(gap_end);

        let row: Option<mysql::Row> = conn.exec_first(
            r"SELECT id, description,
                     DATE_FORMAT(start_timestamp, '%Y-%m-%dT%H:%i:%S.%fZ') AS start_ts,
                     DATE_FORMAT(end_timestamp,   '%Y-%m-%dT%H:%i:%S.%fZ') AS end_ts,
                     total_distance_sailed, total_distance_motoring,
                     total_time_sailing, total_time_motoring, total_time_moored, uuid
              FROM trips
              WHERE start_timestamp <= :gap_end AND end_timestamp >= :gap_start
              ORDER BY id DESC
              LIMIT 1",
            params! { "gap_start" => &start_str, "gap_end" => &end_str },
        )?;

        if let Some(mut row) = row {
            let id: i64 = row.take("id").ok_or("missing id")?;
            let description: String = row.take("description").ok_or("missing description")?;
            let start_ts: String = row.take("start_ts").ok_or("missing start_ts")?;
            let end_ts: String = row.take("end_ts").ok_or("missing end_ts")?;
            let total_distance_sailed: f64 = row
                .take("total_distance_sailed")
                .ok_or("missing total_distance_sailed")?;
            let total_distance_motoring: f64 = row
                .take("total_distance_motoring")
                .ok_or("missing total_distance_motoring")?;
            let total_time_sailing: u64 = row
                .take("total_time_sailing")
                .ok_or("missing total_time_sailing")?;
            let total_time_motoring: u64 = row
                .take("total_time_motoring")
                .ok_or("missing total_time_motoring")?;
            let total_time_moored: u64 = row
                .take("total_time_moored")
                .ok_or("missing total_time_moored")?;
            let uuid: Option<String> = row.take("uuid").unwrap_or(None);

            let start_ts_clean = start_ts.trim_end_matches('Z');
            let end_ts_clean = end_ts.trim_end_matches('Z');
            let start_dt = NaiveDateTime::parse_from_str(start_ts_clean, "%Y-%m-%dT%H:%M:%S%.f")?;
            let end_dt = NaiveDateTime::parse_from_str(end_ts_clean, "%Y-%m-%dT%H:%M:%S%.f")?;
            let start_utc =
                chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(start_dt, chrono::Utc);
            let end_utc =
                chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(end_dt, chrono::Utc);

            Ok(Some(Trip {
                id: Some(id),
                uuid: uuid.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                description,
                start_timestamp: SystemTime::from(start_utc),
                end_timestamp: SystemTime::from(end_utc),
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

    /// Recalculate trip totals from vessel_status records in the trip's time range
    /// and update the trips table.
    pub fn recalculate_and_update_trip(
        &self,
        trip_id: i64,
        trip_start: SystemTime,
        trip_end: SystemTime,
    ) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        let mut tx = conn.start_transaction(mysql::TxOpts::default())?;

        let start_str = systemtime_to_mysql_str(trip_start);
        let end_str = systemtime_to_mysql_str(trip_end);

        let row: Option<mysql::Row> = tx.exec_first(
            r"SELECT
                  SUM(CASE WHEN is_moored = 1 THEN total_time_ms  ELSE 0 END) AS time_moored,
                  SUM(CASE WHEN is_moored = 0 AND engine_on = 1 THEN total_time_ms  ELSE 0 END) AS time_motoring,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 THEN total_time_ms ELSE 0 END) AS time_sailing,
                  SUM(CASE WHEN is_moored = 0 AND engine_on = 1 THEN total_distance_nm ELSE 0 END) AS dist_motoring,
                  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 THEN total_distance_nm ELSE 0 END) AS dist_sailed,
                  MAX(timestamp) AS last_ts
              FROM vessel_status
              WHERE timestamp BETWEEN :start AND :end",
            params! { "start" => &start_str, "end" => &end_str },
        )?;

        if let Some(row) = row {
            let time_moored: u64 = row.get("time_moored").unwrap_or(0);
            let time_motoring: u64 = row.get("time_motoring").unwrap_or(0);
            let time_sailing: u64 = row.get("time_sailing").unwrap_or(0);
            let dist_motoring: f64 = row.get("dist_motoring").unwrap_or(0.0);
            let dist_sailed: f64 = row.get("dist_sailed").unwrap_or(0.0);

            // MariaDB may return MAX(timestamp) as Value::Date or Value::Bytes depending
            // on context (aggregate vs plain column). Handle both to avoid a panic.
            let last_ts_val: mysql::Value = row.get("last_ts").unwrap_or(mysql::Value::NULL);
            let last_ts_str: Option<String> = match last_ts_val {
                mysql::Value::Date(y, mo, d, h, m, s, us) => Some(format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:06}",
                    y, mo, d, h, m, s, us
                )),
                mysql::Value::Bytes(b) => String::from_utf8(b).ok(),
                _ => None,
            };

            // Update trip end timestamp to the last vessel_status record's time
            let end_ts_str = if let Some(ts) = last_ts_str {
                ts
            } else {
                end_str.clone()
            };

            tx.exec_drop(
                r"UPDATE trips
                  SET total_time_moored      = :time_moored,
                      total_time_motoring    = :time_motoring,
                      total_time_sailing     = :time_sailing,
                      total_distance_motoring = :dist_motoring,
                      total_distance_sailed   = :dist_sailed,
                      end_timestamp          = :end_ts
                  WHERE id = :trip_id",
                params! {
                    "time_moored"    => time_moored,
                    "time_motoring"  => time_motoring,
                    "time_sailing"   => time_sailing,
                    "dist_motoring"  => dist_motoring,
                    "dist_sailed"    => dist_sailed,
                    "end_ts"         => &end_ts_str,
                    "trip_id"        => trip_id,
                },
            )?;
        }

        tx.commit()?;
        Ok(())
    }
}

// Convert SystemTime → millis since UNIX_EPOCH (for arithmetic)
pub(crate) fn systemtime_to_millis(st: SystemTime) -> u64 {
    st.duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

pub(crate) fn millis_to_systemtime(ms: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_helpers::{
        add_test_trip, add_test_vessel_status, assert_approx_equal, setup_db,
    };
    use crate::utilities::EngineStatus;
    use mysql::prelude::Queryable;

    // T0: 12 hours in the past so all test timestamps remain well in the past
    fn t0() -> SystemTime {
        SystemTime::now() - Duration::from_secs(43200)
    }

    #[test]
    #[ignore]
    fn test_find_gaps_empty_range() {
        let db = setup_db();
        let base = t0();
        let gaps = db
            .find_gaps(
                base,
                base + Duration::from_secs(3600),
                Duration::from_secs(300),
            )
            .unwrap();
        assert!(gaps.is_empty(), "No gaps expected in empty DB");
    }

    #[test]
    #[ignore]
    fn test_find_gaps_no_gap_below_threshold() {
        let db = setup_db();
        let base = t0();
        // 5 records at 5-min intervals; threshold = 10 min → no gap
        for i in 0..5u64 {
            add_test_vessel_status(
                &db,
                base + Duration::from_secs(i * 300),
                51.0,
                -1.0,
                5.0,
                6.0,
                None,
                None,
                false,
                EngineStatus::Off,
                1.0,
                300_000,
                None,
                None,
            )
            .unwrap();
        }
        let gaps = db
            .find_gaps(
                base - Duration::from_secs(1),
                base + Duration::from_secs(1300),
                Duration::from_secs(600),
            )
            .unwrap();
        assert!(
            gaps.is_empty(),
            "No gap expected when all intervals are below threshold"
        );
    }

    #[test]
    #[ignore]
    fn test_find_gaps_detects_single_gap() {
        let db = setup_db();
        let base = t0();
        // Records at T0, T0+45min, T0+50min; threshold=30min → one gap between T0 and T0+45min
        add_test_vessel_status(
            &db,
            base,
            51.0,
            -1.0,
            5.0,
            6.0,
            None,
            None,
            false,
            EngineStatus::Off,
            0.5,
            300_000,
            None,
            None,
        )
        .unwrap();
        add_test_vessel_status(
            &db,
            base + Duration::from_secs(2700),
            51.1,
            -1.0,
            5.0,
            6.0,
            None,
            None,
            false,
            EngineStatus::Off,
            0.5,
            300_000,
            None,
            None,
        )
        .unwrap();
        add_test_vessel_status(
            &db,
            base + Duration::from_secs(3000),
            51.2,
            -1.0,
            5.0,
            6.0,
            None,
            None,
            false,
            EngineStatus::Off,
            0.5,
            300_000,
            None,
            None,
        )
        .unwrap();
        let gaps = db
            .find_gaps(
                base - Duration::from_secs(1),
                base + Duration::from_secs(3100),
                Duration::from_secs(1800),
            )
            .unwrap();
        assert_eq!(gaps.len(), 1, "Expected exactly 1 gap");
        let gap = &gaps[0];
        // MySQL stores timestamps with millisecond precision so compare with 1s tolerance
        let before_ms = systemtime_to_millis(gap.before_timestamp) as i64;
        let base_ms = systemtime_to_millis(base) as i64;
        assert!(
            (before_ms - base_ms).abs() < 1000,
            "Gap start should be approximately T0"
        );
        let gap_secs = gap
            .after_timestamp
            .duration_since(gap.before_timestamp)
            .unwrap()
            .as_secs();
        assert_eq!(gap_secs, 2700, "Gap should span 45 minutes");
    }

    #[test]
    #[ignore]
    fn test_find_gaps_multiple_gaps() {
        let db = setup_db();
        let base = t0();
        // Records at T0, T0+45m, T0+90m, T0+135m; 30-min threshold → 3 gaps
        for i in 0..4u64 {
            add_test_vessel_status(
                &db,
                base + Duration::from_secs(i * 2700),
                51.0 + i as f64 * 0.1,
                -1.0,
                5.0,
                6.0,
                None,
                None,
                false,
                EngineStatus::Off,
                0.5,
                300_000,
                None,
                None,
            )
            .unwrap();
        }
        let gaps = db
            .find_gaps(
                base - Duration::from_secs(1),
                base + Duration::from_secs(10000),
                Duration::from_secs(1800),
            )
            .unwrap();
        assert_eq!(gaps.len(), 3, "Expected 3 gaps");
    }

    #[test]
    #[ignore]
    fn test_find_trip_for_gap_overlapping() {
        let db = setup_db();
        let base = t0();
        let trip_id = add_test_trip(
            &db,
            "Gap Trip".to_string(),
            base,
            base + Duration::from_secs(7200),
            5.0,
            0.0,
            7_200_000,
            0,
            0,
        )
        .unwrap();
        // Gap window T0+30min to T0+60min falls inside the trip
        let trip = db
            .find_trip_for_gap(
                base + Duration::from_secs(1800),
                base + Duration::from_secs(3600),
            )
            .unwrap();
        assert!(trip.is_some(), "Should find trip for overlapping gap");
        assert_eq!(trip.unwrap().id.unwrap(), trip_id as i64);
    }

    #[test]
    #[ignore]
    fn test_find_trip_for_gap_no_overlap() {
        let db = setup_db();
        let base = t0();
        add_test_trip(
            &db,
            "Trip Far".to_string(),
            base,
            base + Duration::from_secs(3600),
            5.0,
            0.0,
            3_600_000,
            0,
            0,
        )
        .unwrap();
        // Gap window after the trip ends
        let trip = db
            .find_trip_for_gap(
                base + Duration::from_secs(7200),
                base + Duration::from_secs(10800),
            )
            .unwrap();
        assert!(
            trip.is_none(),
            "Should not find trip outside its time range"
        );
    }

    #[test]
    #[ignore]
    fn test_delete_vessel_status_by_id() {
        let db = setup_db();
        let base = t0();
        add_test_vessel_status(
            &db,
            base,
            51.0,
            -1.0,
            5.0,
            6.0,
            None,
            None,
            false,
            EngineStatus::Off,
            1.0,
            600_000,
            None,
            None,
        )
        .unwrap();
        let mut conn = db.pool.get_conn().unwrap();
        let id: i64 = conn
            .query_first("SELECT id FROM vessel_status LIMIT 1")
            .unwrap()
            .unwrap();
        drop(conn);
        db.delete_vessel_status_by_id(id).unwrap();
        let mut conn = db.pool.get_conn().unwrap();
        let count: u64 = conn
            .query_first("SELECT COUNT(*) FROM vessel_status")
            .unwrap()
            .unwrap();
        assert_eq!(count, 0, "vessel_status should be empty after deletion");
    }

    #[test]
    #[ignore]
    fn test_insert_synthetic_vessel_status() {
        use crate::db::types::VesselStatusOperation;
        use std::time::Instant;
        let db = setup_db();
        let ts_instant = Instant::now() - Duration::from_secs(3600);
        let op = VesselStatusOperation {
            timestamp: ts_instant,
            position: Position {
                latitude: 48.0,
                longitude: 2.0,
            },
            average_speed_kn: 4.0,
            max_speed_kn: 5.0,
            is_moored: true,
            engine_on: EngineStatus::Unknown,
            total_distance_nm: 0.0,
            total_time_ms: 300_000,
            wind_speed_kn: None,
            wind_speed_variance: None,
            wind_angle_deg: None,
            wind_angle_variance: None,
            cog_deg: None,
            average_heading_deg: None,
        };
        db.insert_synthetic_vessel_status(&op).unwrap();
        let mut conn = db.pool.get_conn().unwrap();
        let count: u64 = conn
            .query_first("SELECT COUNT(*) FROM vessel_status")
            .unwrap()
            .unwrap();
        assert_eq!(count, 1, "One synthetic row should exist");
    }

    #[test]
    #[ignore]
    fn test_recalculate_and_update_trip() {
        let db = setup_db();
        let base = t0();
        // Trip with zeroed totals — recalculate from status rows
        let trip_id = add_test_trip(
            &db,
            "Recalc".to_string(),
            base,
            base + Duration::from_secs(3600),
            0.0,
            0.0,
            0,
            0,
            0,
        )
        .unwrap();
        // 2 sailing rows (engine off, not moored), 1 motoring row (engine on)
        add_test_vessel_status(
            &db,
            base + Duration::from_secs(300),
            51.0,
            -1.0,
            5.0,
            6.0,
            None,
            None,
            false,
            EngineStatus::Off,
            2.0,
            300_000,
            None,
            None,
        )
        .unwrap();
        add_test_vessel_status(
            &db,
            base + Duration::from_secs(600),
            51.1,
            -1.0,
            5.0,
            6.0,
            None,
            None,
            false,
            EngineStatus::Off,
            2.0,
            300_000,
            None,
            None,
        )
        .unwrap();
        add_test_vessel_status(
            &db,
            base + Duration::from_secs(900),
            51.2,
            -1.0,
            5.0,
            6.0,
            None,
            None,
            false,
            EngineStatus::On,
            1.5,
            300_000,
            None,
            None,
        )
        .unwrap();
        db.recalculate_and_update_trip(trip_id as i64, base, base + Duration::from_secs(3600))
            .unwrap();
        let mut conn = db.pool.get_conn().unwrap();
        let row: mysql::Row = conn.query_first(format!("SELECT total_distance_sailed, total_distance_motoring, total_time_sailing, total_time_motoring FROM trips WHERE id = {}", trip_id)).unwrap().unwrap();
        let dist_sailed: f64 = row.get("total_distance_sailed").unwrap();
        let dist_motor: f64 = row.get("total_distance_motoring").unwrap();
        let time_sail: u64 = row.get("total_time_sailing").unwrap();
        let time_motor: u64 = row.get("total_time_motoring").unwrap();
        assert_approx_equal(
            dist_sailed,
            4.0,
            0.01,
            "sailing distance should sum to 4.0 NM",
        );
        assert_approx_equal(dist_motor, 1.5, 0.01, "motoring distance should be 1.5 NM");
        assert_eq!(time_sail, 600_000, "sailing time should be 600s");
        assert_eq!(time_motor, 300_000, "motoring time should be 300s");
    }
}
