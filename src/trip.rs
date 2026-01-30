use std::{time::{SystemTime}};

#[derive(Debug, Clone)]
pub struct Trip {
    pub id: Option<i64>,
    pub description: String,
    pub start_timestamp: SystemTime,
    pub end_timestamp: SystemTime,
    pub total_distance_sailed: f64,  // nautical miles
    pub total_distance_motoring: f64, // nautical miles
    pub total_time_sailing: u64,     // milliseconds
    pub total_time_motoring: u64,    // milliseconds
    pub total_time_moored: u64,      // milliseconds
}

impl Trip {
    /// Create a new trip with the given start time
    pub fn new(start_timestamp: SystemTime, description: String) -> Self {
        Self {
            id: None,
            description,
            start_timestamp,
            end_timestamp: start_timestamp,
            total_distance_sailed: 0.0,
            total_distance_motoring: 0.0,
            total_time_sailing: 0,
            total_time_motoring: 0,
            total_time_moored: 0,
        }
    }
    
    /// Update the trip with new vessel status data
    pub fn update(&mut self, 
        end_timestamp: SystemTime,
        distance: f64, 
        time_ms: u64, 
        engine_on: bool, 
        is_moored: bool) {
        self.end_timestamp = end_timestamp;
        
        if is_moored {
            self.total_time_moored += time_ms;
        } else if engine_on {
            self.total_distance_motoring += distance;
            self.total_time_motoring += time_ms;
        } else {
            self.total_distance_sailed += distance;
            self.total_time_sailing += time_ms;
        }
    }
    
    /// Check if the trip is still active (end timestamp is within 24 hours of the given time)
    pub fn is_active(&self, current_time: SystemTime) -> bool {
        let duration = if current_time > self.end_timestamp {
            current_time.duration_since(self.end_timestamp)
        } else {
            self.end_timestamp.duration_since(current_time)
        };
        
        match duration {
            Err(_) => return false, // SystemTime error
            Ok(d) => return d.as_secs() <= 24 * 60 * 60 // 24 hours
        };
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
    }

    #[test]
    fn test_update_sailing() {
        let now = SystemTime::now();
        let mut trip = Trip::new(now, "Test Trip".to_string());
        
        let later = now + Duration::from_secs(100);
        trip.update(later, 1000.0, 100000, false, false);
        
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
        trip.update(later, 2000.0, 100000, true, false);
        
        assert_eq!(trip.total_distance_motoring, 2000.0);
        assert_eq!(trip.total_time_motoring, 100000);
        assert_eq!(trip.total_distance_sailed, 0.0);
        assert_eq!(trip.total_time_sailing, 0);
        assert_eq!(trip.total_time_moored, 0);
    }

    #[test]
    fn test_update_moored() {
        let now = SystemTime::now();
        let mut trip = Trip::new(now, "Test Trip".to_string());
        
        let later = now + Duration::from_secs(100);
        trip.update(later, 0.0, 100000, false, true);
        
        assert_eq!(trip.total_time_moored, 100000);
        assert_eq!(trip.total_distance_sailed, 0.0);
        assert_eq!(trip.total_distance_motoring, 0.0);
        assert_eq!(trip.total_time_sailing, 0);
        assert_eq!(trip.total_time_motoring, 0);
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
        trip.update(later, 1000.0, 50000, false, false); // sailing
        trip.update(later, 500.0, 50000, true, false);   // motoring
        
        assert_eq!(trip.total_distance(), 1500.0);
    }

    #[test]
    fn test_total_time() {
        let now = SystemTime::now();
        let mut trip = Trip::new(now, "Test Trip".to_string());
        
        let later = now + Duration::from_secs(100);
        trip.update(later, 1000.0, 30000, false, false); // sailing
        trip.update(later, 500.0, 40000, true, false);   // motoring
        trip.update(later, 0.0, 50000, false, true);     // moored
        
        assert_eq!(trip.total_time(), 120000);
    }
}
