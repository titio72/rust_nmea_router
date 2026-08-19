use crate::utilities::EngineStatus;
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct Trip {
    pub id: Option<i64>,
    pub uuid: String,
    pub description: String,
    pub start_timestamp: SystemTime,
    pub end_timestamp: SystemTime,
    pub total_distance_sailed: f64,   // nautical miles
    pub total_distance_motoring: f64, // nautical miles
    pub total_time_sailing: u64,      // milliseconds
    pub total_time_motoring: u64,     // milliseconds
    pub total_time_moored: u64,       // milliseconds
    pub total_distance_upwind: f64,   // nautical miles, subset of total_distance_sailed
    pub total_distance_reaching: f64, // nautical miles, subset of total_distance_sailed
    pub total_distance_running: f64,  // nautical miles, subset of total_distance_sailed
    pub total_time_upwind: u64,       // milliseconds, subset of total_time_sailing
    pub total_time_reaching: u64,     // milliseconds, subset of total_time_sailing
    pub total_time_running: u64,      // milliseconds, subset of total_time_sailing
}

impl Trip {
    /// Create a new trip with the given start time
    pub fn new(start_timestamp: SystemTime, description: String) -> Self {
        Self {
            id: None,
            uuid: uuid::Uuid::new_v4().to_string(),
            description,
            start_timestamp,
            end_timestamp: start_timestamp,
            total_distance_sailed: 0.0,
            total_distance_motoring: 0.0,
            total_time_sailing: 0,
            total_time_motoring: 0,
            total_time_moored: 0,
            total_distance_upwind: 0.0,
            total_distance_reaching: 0.0,
            total_distance_running: 0.0,
            total_time_upwind: 0,
            total_time_reaching: 0,
            total_time_running: 0,
        }
    }

    /// Update the trip with new vessel status data
    /// Unknown engine status is treated as sailing (conservative approach)
    pub fn update(
        &mut self,
        end_timestamp: SystemTime,
        distance: f64,
        time_ms: u64,
        engine_on: EngineStatus,
        is_moored: bool,
        wind_angle_deg: Option<f64>,
    ) {
        self.end_timestamp = end_timestamp;

        if is_moored {
            self.total_time_moored += time_ms;
        } else if engine_on.is_on() {
            self.total_distance_motoring += distance;
            self.total_time_motoring += time_ms;
        } else {
            // Both Off and Unknown are treated as sailing
            self.total_distance_sailed += distance;
            self.total_time_sailing += time_ms;

            if let Some(angle) = wind_angle_deg {
                use crate::utilities::{point_of_sail_from_twa, PointOfSail};
                match point_of_sail_from_twa(angle) {
                    PointOfSail::Upwind => {
                        self.total_distance_upwind += distance;
                        self.total_time_upwind += time_ms;
                    }
                    PointOfSail::Reaching => {
                        self.total_distance_reaching += distance;
                        self.total_time_reaching += time_ms;
                    }
                    PointOfSail::Running => {
                        self.total_distance_running += distance;
                        self.total_time_running += time_ms;
                    }
                }
            }
        }
    }

    /// Check if the trip is still active (end timestamp is within 24 hours of the given time)
    pub fn is_active(&self, current_time: SystemTime) -> bool {
        let duration = if current_time > self.end_timestamp {
            current_time.duration_since(self.end_timestamp)
        } else {
            self.end_timestamp.duration_since(current_time)
        };
        duration.is_ok_and(|d| d.as_secs() <= 24 * 60 * 60)
    }

    /// Get total distance (sailing + motoring)
    pub fn total_distance(&self) -> f64 {
        self.total_distance_sailed + self.total_distance_motoring
    }

    /// Get total time (sailing + motoring + moored)
    pub fn total_time(&self) -> u64 {
        self.total_time_sailing + self.total_time_motoring + self.total_time_moored
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_new_trip() {
        let now = SystemTime::now();
        let trip = Trip::new(now, "Test Trip".to_string());

        assert_eq!(trip.description, "Test Trip");
        assert_eq!(trip.total_distance_sailed, 0.0);
        assert_eq!(trip.total_distance_motoring, 0.0);
        assert_eq!(trip.total_time_sailing, 0);
        assert_eq!(trip.total_time_motoring, 0);
        assert_eq!(trip.total_time_moored, 0);
        assert_eq!(trip.total_distance_upwind, 0.0);
        assert_eq!(trip.total_distance_reaching, 0.0);
        assert_eq!(trip.total_distance_running, 0.0);
        assert_eq!(trip.total_time_upwind, 0);
        assert_eq!(trip.total_time_reaching, 0);
        assert_eq!(trip.total_time_running, 0);
    }

    #[test]
    fn test_uuid_generation() {
        let now = SystemTime::now();
        let trip = Trip::new(now, "UUID Test".to_string());

        // UUID must be non-empty and exactly 36 chars (8-4-4-4-12 with dashes)
        assert!(!trip.uuid.is_empty(), "UUID must not be empty");
        assert_eq!(trip.uuid.len(), 36, "UUID must be 36 characters");
        assert!(
            uuid::Uuid::parse_str(&trip.uuid).is_ok(),
            "UUID must be parseable"
        );

        // Two trips must have different UUIDs
        let trip2 = Trip::new(now, "UUID Test 2".to_string());
        assert_ne!(trip.uuid, trip2.uuid, "Each trip must get a unique UUID");
    }

    #[test]
    fn test_update_sailing() {
        let now = SystemTime::now();
        let mut trip = Trip::new(now, "Test Trip".to_string());

        let later = now + Duration::from_secs(100);
        trip.update(later, 1000.0, 100000, EngineStatus::Off, false, None);

        assert_eq!(trip.total_distance_sailed, 1000.0);
        assert_eq!(trip.total_time_sailing, 100000);
        assert_eq!(trip.total_distance_motoring, 0.0);
        assert_eq!(trip.total_time_motoring, 0);
        assert_eq!(trip.total_time_moored, 0);
    }

    #[test]
    fn test_update_motoring() {
        let now = SystemTime::now();
        let mut trip = Trip::new(now, "Test Trip".to_string());

        let later = now + Duration::from_secs(100);
        trip.update(later, 2000.0, 100000, EngineStatus::On, false, Some(30.0));

        assert_eq!(trip.total_distance_motoring, 2000.0);
        assert_eq!(trip.total_time_motoring, 100000);
        assert_eq!(trip.total_distance_sailed, 0.0);
        assert_eq!(trip.total_time_sailing, 0);
        assert_eq!(trip.total_time_moored, 0);
        // Motoring never contributes to point-of-sail buckets, even with a wind angle present
        assert_eq!(trip.total_distance_upwind, 0.0);
        assert_eq!(trip.total_time_upwind, 0);
    }

    #[test]
    fn test_update_moored() {
        let now = SystemTime::now();
        let mut trip = Trip::new(now, "Test Trip".to_string());

        let later = now + Duration::from_secs(100);
        trip.update(later, 0.0, 100000, EngineStatus::Off, true, Some(30.0));

        assert_eq!(trip.total_time_moored, 100000);
        assert_eq!(trip.total_distance_sailed, 0.0);
        assert_eq!(trip.total_distance_motoring, 0.0);
        assert_eq!(trip.total_time_sailing, 0);
        assert_eq!(trip.total_time_motoring, 0);
        assert_eq!(trip.total_distance_upwind, 0.0);
    }

    #[test]
    fn test_update_sailing_upwind() {
        let now = SystemTime::now();
        let mut trip = Trip::new(now, "Test Trip".to_string());

        let later = now + Duration::from_secs(100);
        trip.update(later, 5.0, 100000, EngineStatus::Off, false, Some(30.0));

        assert_eq!(trip.total_distance_sailed, 5.0);
        assert_eq!(trip.total_distance_upwind, 5.0);
        assert_eq!(trip.total_time_upwind, 100000);
        assert_eq!(trip.total_distance_reaching, 0.0);
        assert_eq!(trip.total_distance_running, 0.0);
    }

    #[test]
    fn test_update_sailing_reaching() {
        let now = SystemTime::now();
        let mut trip = Trip::new(now, "Test Trip".to_string());

        let later = now + Duration::from_secs(100);
        trip.update(later, 5.0, 100000, EngineStatus::Off, false, Some(90.0));

        assert_eq!(trip.total_distance_reaching, 5.0);
        assert_eq!(trip.total_time_reaching, 100000);
        assert_eq!(trip.total_distance_upwind, 0.0);
        assert_eq!(trip.total_distance_running, 0.0);
    }

    #[test]
    fn test_update_sailing_running() {
        let now = SystemTime::now();
        let mut trip = Trip::new(now, "Test Trip".to_string());

        let later = now + Duration::from_secs(100);
        trip.update(later, 5.0, 100000, EngineStatus::Off, false, Some(160.0));

        assert_eq!(trip.total_distance_running, 5.0);
        assert_eq!(trip.total_time_running, 100000);
        assert_eq!(trip.total_distance_upwind, 0.0);
        assert_eq!(trip.total_distance_reaching, 0.0);
    }

    #[test]
    fn test_update_sailing_no_wind_data() {
        let now = SystemTime::now();
        let mut trip = Trip::new(now, "Test Trip".to_string());

        let later = now + Duration::from_secs(100);
        trip.update(later, 5.0, 100000, EngineStatus::Off, false, None);

        // Counted in sailing totals, but none of the three buckets
        assert_eq!(trip.total_distance_sailed, 5.0);
        assert_eq!(trip.total_distance_upwind, 0.0);
        assert_eq!(trip.total_distance_reaching, 0.0);
        assert_eq!(trip.total_distance_running, 0.0);
    }

    #[test]
    fn test_is_active_within_24h() {
        let now = SystemTime::now();
        let trip = Trip::new(now, "Test Trip".to_string());

        let later = now + Duration::from_secs(23 * 60 * 60); // 23 hours later
        assert!(trip.is_active(later));
    }

    #[test]
    fn test_is_active_after_24h() {
        let now = SystemTime::now();
        let trip = Trip::new(now, "Test Trip".to_string());

        let later = now + Duration::from_secs(25 * 60 * 60); // 25 hours later
        assert!(!trip.is_active(later));
    }

    #[test]
    fn test_total_distance() {
        let now = SystemTime::now();
        let mut trip = Trip::new(now, "Test Trip".to_string());

        let later = now + Duration::from_secs(100);
        trip.update(later, 1000.0, 50000, EngineStatus::Off, false, None); // sailing
        trip.update(later, 500.0, 50000, EngineStatus::On, false, None); // motoring

        assert_eq!(trip.total_distance(), 1500.0);
    }

    #[test]
    fn test_total_time() {
        let now = SystemTime::now();
        let mut trip = Trip::new(now, "Test Trip".to_string());

        let later = now + Duration::from_secs(100);
        trip.update(later, 1000.0, 30000, EngineStatus::Off, false, None); // sailing
        trip.update(later, 500.0, 40000, EngineStatus::On, false, None); // motoring
        trip.update(later, 0.0, 50000, EngineStatus::Off, true, None); // moored

        assert_eq!(trip.total_time(), 120000);
    }
}
