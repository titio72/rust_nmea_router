use std::time::{Duration, Instant};
use tracing::{info, warn, debug};

use crate::vessel_monitor::{VesselStatus};
use crate::db::{VesselDatabase, TripOperation, VesselStatusOperation};
use crate::trip::Trip;
use crate::config::VesselStatusConfig;

/// State for tracking vessel status between reports
pub struct VesselStatusState {
    last_vessel_status: Option<VesselStatus>,
    last_reported_max_speed: f64,
    current_trip: Option<Trip>,
    last_db_persist_time: Instant,
    config: VesselStatusConfig,
}

/// Handler for vessel status reporting and persistence
pub struct VesselStatusHandler {
    state: VesselStatusState,
}

impl VesselStatusHandler {
    pub fn new(config: VesselStatusConfig) -> Self {
        Self {
            state: VesselStatusState::new(config),
        }
    }

    /// Load the last trip from database if available
    pub fn load_last_trip(&mut self, vessel_db: &VesselDatabase) {
        self.state.load_last_trip(vessel_db);
    }

    /// Handle vessel status reporting and persistence
    /// Returns Ok(true) if a vessel status report was written to the database
    /// Returns Ok(false) if no write was needed
    /// Returns Err if there was a database error
    pub fn handle_vessel_status(
        &mut self,
        vessel_db: &Option<VesselDatabase>,
        status: VesselStatus,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let effective_position = status.get_effective_position();
        debug!("Vessel Status: latitude={:.6}, longitude={:.6}, m/s, max_speed={:.2} m/s, moored={}", 
            effective_position.latitude,
            effective_position.longitude,
            status.max_speed, status.is_moored);
    
        // Write to database if connected, time to persist, and time is synchronized
        if let Some(ref db) = *vessel_db && self.state.should_persist_to_db(status.is_moored) {
            let position = status.get_effective_position();
            let latitude = position.latitude;
            let longitude = position.longitude;
            let (total_distance_nm, total_time_ms) = status.get_total_distance_and_time_from_last_report(&mut self.state.last_vessel_status);
            let time: Instant = status.timestamp;
            let average_speed = if total_time_ms > 0 { total_distance_nm / (total_time_ms as f64 / 1000.0) } else { 0.0 };
            let max_speed = if self.state.last_reported_max_speed > status.max_speed { self.state.last_reported_max_speed } else { status.max_speed };
            self.state.last_reported_max_speed = max_speed;

            // Determine trip operation (create, update, or none)
            let trip_operation = Self::determine_trip_operation(&mut self.state.current_trip, &status, total_distance_nm, total_time_ms);
            
            // Create vessel status operation
            let status_operation = VesselStatusOperation {
                time,
                latitude,
                longitude,
                average_speed,
                max_speed,
                is_moored: status.is_moored,
                engine_on: status.engine_on,
                total_distance_nm,
                total_time_ms,
            };
            
            // Perform atomic insert of vessel status and trip operation
            match db.insert_status_and_trip(status_operation, trip_operation) {
                Ok(new_trip_id) => {
                    debug!("Vessel status written to database: lat={:.6}, lon={:.6}, avg_speed={:.2} m/s, distance={:.3} nm, time={} ms, moored={}", 
                        position.latitude, position.longitude, average_speed, total_distance_nm, total_time_ms, status.is_moored);
                    self.state.mark_db_persisted();
                    self.state.last_vessel_status = Some(status.clone());
                    self.state.last_reported_max_speed = 0.0;
                    
                    // Update trip ID if we created a new trip
                    if let Some(trip_id) = new_trip_id {
                        if let Some(ref mut trip) = self.state.current_trip {
                            trip.id = Some(trip_id);
                            info!("Created new trip: {} (ID: {})", trip.description, trip_id);
                        }
                    } else if let Some(ref trip) = self.state.current_trip {
                        debug!("Updated trip: {} (ID: {}), total_distance={:.3}nm, total_time={}ms", 
                            trip.description, trip.id.unwrap_or(0), trip.total_distance(), trip.total_time());
                    }
                    
                    return Ok(true);
                }
                Err(e) => {
                    warn!("Error writing vessel status to database: {}", e);
                    return Err(e);
                }
            }
        }
        Ok(false)
    }

    /// Determine the trip operation to perform
    fn determine_trip_operation(current_trip: &mut Option<Trip>, status: &VesselStatus, distance: f64, time_ms: u64) -> TripOperation {
        let report_time = status.timestamp;
        
        // Check if we need to create a new trip or update existing
        let should_create_new = if let Some(ref trip) = *current_trip {
            !trip.is_active(report_time)
        } else {
            true // No current trip, create new one
        };
        
        if should_create_new {
            // Create new trip
            let start_time = report_time;
            
            // Format description with date
            let delta = Instant::now().duration_since(start_time);
            let system_time = std::time::SystemTime::now().checked_sub(delta).unwrap_or(std::time::UNIX_EPOCH);
            let datetime = chrono::DateTime::<chrono::Utc>::from(system_time);
            let description = format!("Trip {}", datetime.format("%Y-%m-%d"));
            
            let mut new_trip = Trip::new(start_time, description);
            new_trip.update(report_time, distance, time_ms, status.engine_on, status.is_moored);
            
            *current_trip = Some(new_trip.clone());
            TripOperation::CreateTrip(new_trip)
        } else {
            // Update existing trip
            if let Some(ref mut trip) = *current_trip {
                trip.update(report_time, distance, time_ms, status.engine_on, status.is_moored);
                TripOperation::UpdateTrip(trip.clone())
            } else {
                TripOperation::None
            }
        }
    }
}

impl VesselStatusState {
    fn new(config: VesselStatusConfig) -> Self {
        let now = Instant::now();
        Self {
            last_vessel_status: None,
            last_reported_max_speed: 0.0,
            current_trip: None,
            // Initialize to far past to ensure first report is written immediately
            last_db_persist_time: now - Duration::from_secs(86400), // 24 hours ago
            config,
        }
    }

    /// Check if it's time to persist status to database (adaptive based on mooring state)
    fn should_persist_to_db(&self, is_moored: bool) -> bool {
        let now = Instant::now();
        let interval = if is_moored {
            self.config.interval_moored()
        } else {
            self.config.interval_underway()
        };
        now.duration_since(self.last_db_persist_time) >= interval
    }

    /// Mark that we've persisted to the database
    fn mark_db_persisted(&mut self) {
        self.last_db_persist_time = Instant::now();
    }

    /// Load the last trip from database if available
    fn load_last_trip(&mut self, vessel_db: &VesselDatabase) {
        match vessel_db.get_last_trip() {
            Ok(trip) => {
                if let Some(t) = trip {
                    info!("Loaded last trip from database: {} (ID: {})", t.description, t.id.unwrap_or(0));
                    self.current_trip = Some(t);
                } else {
                    info!("No existing trip found in database");
                }
            }
            Err(e) => {
                warn!("Failed to load last trip from database: {}", e);
            }
        }
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_should_persist_moored() {
        let config = VesselStatusConfig {
            interval_moored_seconds: 0, // Set to 0 so it always needs to persist
            interval_underway_seconds: 5,
        };
        let state = VesselStatusState::new(config);
        
        // Should persist immediately with 0-second interval
        assert!(state.should_persist_to_db(true));
    }

    #[test]
    fn test_should_persist_underway() {
        let config = VesselStatusConfig {
            interval_moored_seconds: 10,
            interval_underway_seconds: 0, // Set to 0 so it always needs to persist
        };
        let state = VesselStatusState::new(config);
        
        // Should persist immediately with 0-second interval
        assert!(state.should_persist_to_db(false));
    }

    #[test]
    fn test_mark_db_persisted() {
        let config = VesselStatusConfig::default();
        let mut state = VesselStatusState::new(config);
        
        let before = state.last_db_persist_time;
        std::thread::sleep(Duration::from_millis(10));
        state.mark_db_persisted();
        let after = state.last_db_persist_time;
        
        assert!(after > before);
    }

    #[test]
    fn test_first_report_persists_immediately() {
        let config = VesselStatusConfig {
            interval_moored_seconds: 600, // 10 minutes
            interval_underway_seconds: 30, // 30 seconds
        };
        let state = VesselStatusState::new(config);
        
        // First report should persist immediately (regardless of interval)
        assert!(state.should_persist_to_db(true));
        assert!(state.should_persist_to_db(false));
    }
}
