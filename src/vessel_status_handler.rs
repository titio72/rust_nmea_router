use std::time::{Duration, Instant};
use tracing::{info, warn, debug};

use crate::utilities::dirty_instant_to_systemtime;
use crate::vessel_monitor::{VesselStatus};
use crate::db::{VesselDatabase, TripOperation, VesselStatusOperation};
use crate::trip::Trip;
use crate::config::VesselStatusConfig;

/// State for tracking vessel status between reports
pub struct VesselStatusState {
    last_persisted_status: Option<VesselStatus>,
    last_reported_max_speed: f64,
    current_trip: Option<Trip>,
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
        debug!("Vessel Status: latitude={:.6}, longitude={:.6}, max_speed={:.2}, avg_wind={:.2?}, avg_wind_direction={:.2?} knots, Head {:.2?} moored={}", 
            effective_position.latitude,
            effective_position.longitude,
            status.max_speed_kn, status.wind_speed_kn, status.wind_angle_deg, 
            status.average_heading_deg,
            status.is_moored);
    
        // Write to database if connected, time to persist, and time is synchronized
        if let Some(ref db) = *vessel_db && status.is_valid() && self.state.should_persist_to_db(&status) {
            let time: Instant = status.timestamp;
            let position = status.get_effective_position();
            let latitude = position.latitude;
            let longitude = position.longitude;
            let vessel_vector = status.get_vector_from(&mut self.state.last_persisted_status);
            let total_distance_nm = if let Some(ref vessel_vector) = vessel_vector { vessel_vector.distance_nm } else { 0.0 };
            let total_time_ms = if let Some(ref vessel_vector) = vessel_vector { vessel_vector.delta_time_ms } else { 0 };
            let average_speed_kn = if let Some(ref vessel_vector) = vessel_vector { vessel_vector.average_speed_kn() } else { 0.0 };
            let cog_deg: Option<f64> = if let Some(ref vessel_vector) = vessel_vector { Some(vessel_vector.course_deg) } else { None };
            let average_heading_deg: Option<f64> = status.average_heading_deg;
            self.state.last_reported_max_speed = self.state.last_reported_max_speed.max(status.max_speed_kn);

            // Determine trip operation (create, update, or none)
            let trip_operation = Self::determine_trip_operation(&mut self.state.current_trip, &status, total_distance_nm, total_time_ms);
            
            // Create vessel status operation
            let status_operation = VesselStatusOperation {
                time,
                latitude,
                longitude,
                average_speed_kn: average_speed_kn,
                max_speed_kn: self.state.last_reported_max_speed,
                is_moored: status.is_moored,
                engine_on: status.engine_on,
                total_distance_nm,
                total_time_ms,
                average_wind_speed_kn: status.wind_speed_kn,
                wind_speed_variance: status.wind_speed_variance,
                average_wind_angle_deg: status.wind_angle_deg,
                wind_angle_variance: status.wind_angle_variance,
                cog_deg,
                average_heading_deg,
            };
            
            self.state.set_last_persisted_status(&status);
            self.state.last_reported_max_speed = 0.0;
                    
            // Perform atomic insert of vessel status and trip operation
            match db.insert_status_and_trip(status_operation, trip_operation) {
                Ok(new_trip_id) => {
                    debug!("Vessel status written to database: lat={:.6}, lon={:.6}, avg_speed={:.2} knots, distance={:.3} nm, time={} ms, moored={}", 
                        position.latitude, position.longitude, average_speed_kn, total_distance_nm, total_time_ms, status.is_moored);
                    
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
    fn determine_trip_operation(current_trip: &mut Option<Trip>, status: &VesselStatus, distance: f64, delta_time_ms: u64) -> TripOperation {
        let report_time = status.timestamp;
        let report_systemtime = dirty_instant_to_systemtime(report_time);
        // Check if we need to create a new trip or update existing
        let should_create_new = if let Some(ref trip) = *current_trip {
            !trip.is_active(report_systemtime)
        } else {
            true // No current trip, create new one
        };
        
        let effective_distance = if status.is_moored { 0.0 } else { distance };

        if should_create_new {
            // Create new trip
            let start_time = report_systemtime;
            
            // Format description with date
            let datetime = chrono::DateTime::<chrono::Utc>::from(start_time);
            let description = format!("Trip {}", datetime.format("%Y-%m-%d"));
            
            let mut new_trip = Trip::new(start_time, description);
            new_trip.update(report_systemtime, effective_distance, delta_time_ms, status.engine_on, status.is_moored);
            
            *current_trip = Some(new_trip.clone());
            TripOperation::CreateTrip(new_trip)
        } else {
            // Update existing trip
            if let Some(ref mut trip) = *current_trip {
                trip.update(report_systemtime, effective_distance, delta_time_ms, status.engine_on, status.is_moored);
                TripOperation::UpdateTrip(trip.clone())
            } else {
                TripOperation::None
            }
        }
    }
}

impl VesselStatusState {
    fn new(config: VesselStatusConfig) -> Self {
        //let now = Instant::now();
        Self {
            last_persisted_status: None,
            last_reported_max_speed: 0.0,
            current_trip: None,
            // Initialize to far past to ensure first report is written immediately
            //last_db_persist_time: now - Duration::from_secs(86400), // 24 hours ago
            config,
        }
    }

    /// Check if it's time to persist status to database (adaptive based on mooring state)
    fn should_persist_to_db(&self, new_status: &VesselStatus) -> bool {
        let interval = if new_status.is_moored {
            self.config.interval_moored()
        } else {
            Duration::from_secs(1) // self.config.interval_underway()
        };
        //println!("Checking if should persist to DB (moored={}) interval : {:?}", new_status.is_moored, interval);
        if let Some(previous_status) = &self.last_persisted_status {
            let elapsed = new_status.timestamp.duration_since(previous_status.timestamp);
            //println!(" elapsed: {:?}", elapsed.as_secs());
            elapsed >= interval
        } else {
            // Always persist first report
            true
        }
    }

    fn set_last_persisted_status(&mut self, status: &VesselStatus) {
        self.last_persisted_status = Some(status.clone());
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

    #[test]
    fn test_should_persist_moored() {
        use crate::vessel_monitor::{VesselStatus};
        use crate::position_utils::Position;
        
        let config = VesselStatusConfig {
            interval_moored_seconds: 0, // Set to 0 so it always needs to persist
            interval_underway_seconds: 5,
        };
        let state = VesselStatusState::new(config);
        
        let status = VesselStatus {
            current_position: Position { latitude: 0.0, longitude: 0.0 },
            median_position: None,
            number_of_samples: 1,
            max_speed_kn: 0.0,
            is_moored: true,
            engine_on: false,
            wind_speed_kn: None,
            max_wind_speed_kn: None,
            wind_speed_variance: None,
            wind_angle_deg: None,
            wind_angle_variance: None,
            timestamp: std::time::Instant::now(),
            average_heading_deg: None,
        };
        
        // Should persist immediately with 0-second interval
        assert!(state.should_persist_to_db(&status));
    }

    #[test]
    fn test_should_persist_underway() {
        use crate::vessel_monitor::{VesselStatus};
        use crate::position_utils::Position;
        
        let config = VesselStatusConfig {
            interval_moored_seconds: 60,
            interval_underway_seconds: 0,
        };
        let state = VesselStatusState::new(config);
        
        let status = VesselStatus {
            current_position: Position { latitude: 0.0, longitude: 0.0 },
            median_position: None,
            number_of_samples: 1,
            max_speed_kn: 5.0,
            is_moored: false,
            engine_on: true,
            wind_speed_kn: None,
            max_wind_speed_kn: None,
            wind_speed_variance: None,
            wind_angle_deg: None,
            wind_angle_variance: None,
            timestamp: std::time::Instant::now(),
            average_heading_deg: None,
        };
        
        // Should persist immediately with 0-second interval
        assert!(state.should_persist_to_db(&status));
    }

    #[test]
    fn test_set_last_persisted_status() {
        use crate::vessel_monitor::{VesselStatus};
        use crate::position_utils::Position;
        
        let config = VesselStatusConfig::default();
        let mut state = VesselStatusState::new(config);
        
        assert!(state.last_persisted_status.is_none());
        
        let status = VesselStatus {
            current_position: Position { latitude: 0.0, longitude: 0.0 },
            median_position: None,
            number_of_samples: 1,
            max_speed_kn: 0.0,
            is_moored: true,
            engine_on: false,
            wind_speed_kn: None,
            max_wind_speed_kn: None,
            wind_speed_variance: None,
            wind_angle_deg: None,
            wind_angle_variance: None,
            timestamp: std::time::Instant::now(),
            average_heading_deg: None,
        };
        
        state.set_last_persisted_status(&status);
        
        assert!(state.last_persisted_status.is_some());
    }

    #[test]
    fn test_first_report_persists_immediately() {
        use crate::vessel_monitor::{VesselStatus};
        use crate::position_utils::Position;
        
        let config = VesselStatusConfig {
            interval_moored_seconds: 60,
            interval_underway_seconds: 30,
        };
        let state = VesselStatusState::new(config);
        
        let moored_status = VesselStatus {
            current_position: Position { latitude: 0.0, longitude: 0.0 },
            median_position: None,
            number_of_samples: 1,
            max_speed_kn: 0.0,
            is_moored: true,
            engine_on: false,
            wind_speed_kn: None,
            max_wind_speed_kn: None,
            wind_speed_variance: None,
            wind_angle_deg: None,
            wind_angle_variance: None,
            timestamp: std::time::Instant::now(),
            average_heading_deg: None,
        };
        
        let underway_status = VesselStatus {
            is_moored: false,
            max_speed_kn: 5.0,
            engine_on: true,
            ..moored_status.clone()
        };
        
        // First report should persist immediately (regardless of interval)
        assert!(state.should_persist_to_db(&moored_status));
        assert!(state.should_persist_to_db(&underway_status));
    }
}
