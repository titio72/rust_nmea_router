use crate::db::test_helpers::{add_test_trip, add_test_vessel_status};
use crate::db::types::VesselDatabase;
use crate::position_utils::Position;
use crate::utilities::EngineStatus;
use std::error::Error;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Batch operations for test data insertion
/// Provides efficient methods to populate test database with realistic data sets
///
/// Insert a complete simulated sailing trip with realistic vessel status records
///
/// # Arguments
/// * `db` - Database connection
/// * `start_position` - Starting position
/// * `end_position` - Ending position
/// * `start_time` - Trip start time
/// * `speed_kn` - Average sailing speed in knots
/// * `interval_s` - Interval between status records in seconds
///
/// # Returns
/// Trip ID of the inserted trip
pub fn insert_simulated_sailing_trip(
    db: &VesselDatabase,
    start_position: Position,
    end_position: Position,
    start_time: SystemTime,
    speed_kn: f64,
    interval_s: u64,
) -> Result<u32, Box<dyn Error>> {
    use crate::db::test_helpers::generate_track;

    // Generate track points
    let track = generate_track(
        start_position,
        end_position,
        speed_kn,
        interval_s,
        start_time,
    );

    if track.is_empty() {
        return Err("Generated track is empty".into());
    }

    // Calculate trip totals
    let mut total_distance = 0.0;
    let mut total_time_ms = 0u64;

    for i in 1..track.len() {
        let dist = track[i].0.distance_to_nm(&track[i - 1].0);
        total_distance += dist;

        let time_diff = track[i]
            .1
            .duration_since(track[i - 1].1)
            .unwrap_or(Duration::from_secs(0))
            .as_millis() as u64;
        total_time_ms += time_diff;
    }

    let end_time = track.last().unwrap().1;

    // Insert trip
    let trip_id = add_test_trip(
        db,
        format!(
            "Simulated sailing trip from ({:.3}, {:.3}) to ({:.3}, {:.3})",
            start_position.latitude,
            start_position.longitude,
            end_position.latitude,
            end_position.longitude
        ),
        start_time,
        end_time,
        total_distance, // All sailed
        0.0,            // No motoring
        total_time_ms,
        0,
        0,
    )?;

    // Insert vessel status records
    let mut _cumulative_distance = 0.0;
    let mut _cumulative_time = 0u64;

    for i in 0..track.len() {
        let (pos, timestamp) = track[i];

        let segment_distance = if i > 0 {
            pos.distance_to_nm(&track[i - 1].0)
        } else {
            0.0
        };

        let segment_time = if i > 0 {
            timestamp
                .duration_since(track[i - 1].1)
                .unwrap_or(Duration::from_secs(0))
                .as_millis() as u64
        } else {
            0
        };

        _cumulative_distance += segment_distance;
        _cumulative_time += segment_time;

        let cog = if i > 0 {
            Some(track[i - 1].0.course_from_deg(&pos))
        } else {
            None
        };

        add_test_vessel_status(
            db,
            timestamp,
            pos.latitude,
            pos.longitude,
            speed_kn,
            speed_kn * 1.1, // max speed slightly higher
            Some(12.0),     // 12 knots wind
            Some(45.0),     // 45 degrees TWA
            false,          // not moored
            EngineStatus::Off,
            segment_distance,
            segment_time,
            cog,
            cog, // heading same as COG for simplicity
        )?;
    }

    Ok(trip_id)
}

/// Insert a complete simulated motoring trip with realistic vessel status records
pub fn insert_simulated_motoring_trip(
    db: &VesselDatabase,
    start_position: Position,
    end_position: Position,
    start_time: SystemTime,
    speed_kn: f64,
    interval_s: u64,
) -> Result<u32, Box<dyn Error>> {
    use crate::db::test_helpers::generate_track;

    let track = generate_track(
        start_position,
        end_position,
        speed_kn,
        interval_s,
        start_time,
    );

    if track.is_empty() {
        return Err("Generated track is empty".into());
    }

    let mut total_distance = 0.0;
    let mut total_time_ms = 0u64;

    for i in 1..track.len() {
        let dist = track[i].0.distance_to_nm(&track[i - 1].0);
        total_distance += dist;

        let time_diff = track[i]
            .1
            .duration_since(track[i - 1].1)
            .unwrap_or(Duration::from_secs(0))
            .as_millis() as u64;
        total_time_ms += time_diff;
    }

    let end_time = track.last().unwrap().1;

    // Insert trip
    let trip_id = add_test_trip(
        db,
        format!(
            "Simulated motoring trip from ({:.3}, {:.3}) to ({:.3}, {:.3})",
            start_position.latitude,
            start_position.longitude,
            end_position.latitude,
            end_position.longitude
        ),
        start_time,
        end_time,
        0.0,            // No sailing
        total_distance, // All motoring
        0,
        total_time_ms,
        0,
    )?;

    // Insert vessel status records
    for i in 0..track.len() {
        let (pos, timestamp) = track[i];

        let segment_distance = if i > 0 {
            pos.distance_to_nm(&track[i - 1].0)
        } else {
            0.0
        };

        let segment_time = if i > 0 {
            timestamp
                .duration_since(track[i - 1].1)
                .unwrap_or(Duration::from_secs(0))
                .as_millis() as u64
        } else {
            0
        };

        let cog = if i > 0 {
            Some(track[i - 1].0.course_from_deg(&pos))
        } else {
            None
        };

        add_test_vessel_status(
            db,
            timestamp,
            pos.latitude,
            pos.longitude,
            speed_kn,
            speed_kn * 1.05,
            None, // No wind data when motoring
            None,
            false,
            EngineStatus::On,
            segment_distance,
            segment_time,
            cog,
            cog,
        )?;
    }

    Ok(trip_id)
}

/// Insert a moored status record (vessel at anchor/dock)
pub fn insert_moored_status(
    db: &VesselDatabase,
    position: Position,
    timestamp: SystemTime,
    duration_ms: u64,
) -> Result<u32, Box<dyn Error>> {
    add_test_vessel_status(
        db,
        timestamp,
        position.latitude,
        position.longitude,
        0.0, // No speed
        0.0,
        None,
        None,
        true, // moored
        EngineStatus::Off,
        0.0,
        duration_ms,
        None,
        None,
    )
}

/// Populate database with a predefined set of realistic trips
/// This is useful for testing queries and analytics
pub fn populate_sample_trips(db: &VesselDatabase) -> Result<Vec<u32>, Box<dyn Error>> {
    let mut trip_ids = Vec::new();

    // Trip 1: Short sailing trip in Mediterranean
    let start1 = UNIX_EPOCH + Duration::from_secs(1609459200); // 2021-01-01 00:00:00 UTC
    let p1_start = Position {
        latitude: 41.0,
        longitude: 2.0,
    };
    let p1_end = Position {
        latitude: 41.2,
        longitude: 2.3,
    };
    trip_ids.push(insert_simulated_sailing_trip(
        db, p1_start, p1_end, start1, 5.5, 600,
    )?);

    // Trip 2: Motoring trip - 48 hours later
    let start2 = start1 + Duration::from_secs(48 * 3600);
    let p2_start = Position {
        latitude: 41.2,
        longitude: 2.3,
    };
    let p2_end = Position {
        latitude: 41.5,
        longitude: 2.1,
    };
    trip_ids.push(insert_simulated_motoring_trip(
        db, p2_start, p2_end, start2, 6.5, 300,
    )?);

    // Trip 3: Longer sailing trip - week later
    let start3 = start2 + Duration::from_secs(7 * 24 * 3600);
    let p3_start = Position {
        latitude: 41.5,
        longitude: 2.1,
    };
    let p3_end = Position {
        latitude: 42.0,
        longitude: 3.0,
    };
    trip_ids.push(insert_simulated_sailing_trip(
        db, p3_start, p3_end, start3, 6.0, 1800,
    )?);

    Ok(trip_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::test_helpers::{reset_test_db, setup_test_db};

    #[test]
    #[ignore] // Requires test database to be set up
    fn test_insert_simulated_trip() {
        let config = Config::load_for_context(None).expect("Failed to load test config");
        let db_url = config.database.connection.connection_url();
        let db = setup_test_db(&db_url).expect("Failed to setup test db");
        reset_test_db(&db).expect("Failed to reset test db");

        let start_pos = Position {
            latitude: 40.0,
            longitude: -70.0,
        };
        let end_pos = Position {
            latitude: 40.5,
            longitude: -69.5,
        };
        let start_time = UNIX_EPOCH + Duration::from_secs(1609459200);

        let trip_id = insert_simulated_sailing_trip(&db, start_pos, end_pos, start_time, 6.0, 600)
            .expect("Failed to insert simulated trip");

        assert!(trip_id > 0);
    }
}
