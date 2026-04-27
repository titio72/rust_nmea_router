// Vessel Status Handler - Persistence & Reporting Layer
// Responsible for generating boat status reports and managing trip state transitions.
// Database patterns: Use parameterized queries, transactions for multi-statement operations.
// Testing: Use test infrastructure from db::test_helpers for consistent test data.
// See: AGENTS.md for database patterns and testing strategy
//
use tracing::{debug, info, warn};

use crate::db::{TripOperation, VesselDatabase, VesselStatusOperation};
use crate::trip::Trip;
use crate::utilities::dirty_instant_to_systemtime;
use crate::vessel_monitor::VesselStatus;

/// State for tracking vessel status between reports
pub struct VesselStatusState {
    last_persisted_status: Option<VesselStatusOperation>,
    current_trip: Option<Trip>,
}

/// Handler for vessel status reporting and persistence
pub struct VesselStatusHandler {
    state: VesselStatusState,
}

const MAXIMUM_AGE_VALID_PREVIOUS_REPORT_SECS: u64 = 6 * 3600; // 6 hours - If the last persisted status is older than this, we won't use it for vector calculations

impl VesselStatusHandler {
    pub fn new() -> Self {
        Self {
            state: VesselStatusState::new(),
        }
    }

    /// Load the last trip from database if available
    pub fn load_last_trip(&mut self, vessel_db: &VesselDatabase) {
        self.state.load_last_trip(vessel_db);
    }

    pub fn load_last_vessel_status(&mut self, vessel_db: &VesselDatabase) {
        self.state.load_last_vessel_status(vessel_db);
    }

    fn generate_vessel_status_operation(&mut self, status: &VesselStatus) -> VesselStatusOperation {
        if !self.shall_accept_last_status(&self.state.last_persisted_status, status) {
            debug!("Previous vessel status is too old or too far from last persisted status, ignoring for vector calculations");
            self.state.reset_last_persisted_status(); // Clear last persisted status to avoid future comparisons until we get a valid one
        }
        let vessel_vector = if let Some(previous) = &self.state.last_persisted_status {
            status.get_vector_from(previous.position, previous.timestamp)
        } else {
            None
        };
        let position = status.get_effective_position();
        let total_distance_nm = if let Some(ref vessel_vector) = vessel_vector {
            vessel_vector.distance_nm
        } else {
            0.0
        };
        let total_time_ms = if let Some(ref vessel_vector) = vessel_vector {
            vessel_vector.delta_time_ms
        } else {
            status.period.as_millis() as u64
        };
        let average_speed_kn = if let Some(ref vessel_vector) = vessel_vector {
            vessel_vector.average_speed_kn()
        } else {
            0.0
        };
        let cog_deg: Option<f64> = vessel_vector
            .as_ref()
            .map(|vessel_vector| vessel_vector.course_deg);
        let average_heading_deg: Option<f64> = status.average_heading_deg;

        VesselStatusOperation {
            timestamp: status.timestamp,
            position,
            average_speed_kn,
            max_speed_kn: status.max_speed_kn,
            is_moored: status.is_moored,
            engine_on: status.engine_on,
            total_distance_nm,
            total_time_ms,
            wind_speed_kn: status.wind_speed_kn,
            wind_speed_variance: status.wind_speed_variance,
            wind_angle_deg: status.wind_angle_deg,
            wind_angle_variance: status.wind_angle_variance,
            cog_deg,
            average_heading_deg,
        }
    }

    fn shall_accept_last_status(
        &self,
        last_status: &Option<VesselStatusOperation>,
        new_status: &VesselStatus,
    ) -> bool {
        if let Some(last_status) = last_status {
            new_status.timestamp.duration_since(last_status.timestamp)
                < std::time::Duration::from_secs(MAXIMUM_AGE_VALID_PREVIOUS_REPORT_SECS)
                && new_status
                    .get_effective_position()
                    .distance_to_nm(&last_status.position)
                    < 8.0
        } else {
            false
        }
    }

    /// Handle vessel status reporting and persistence
    /// Returns Ok(true) if a vessel status report was written to the database
    /// Returns Ok(false) if no write was needed
    /// Returns Err if there was a database error
    pub fn handle_vessel_status(
        &mut self,
        vessel_db: &VesselDatabase,
        status: &VesselStatus,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let effective_position = status.get_effective_position();
        debug!("Vessel Status: latitude={:.6}, longitude={:.6}, max_speed={:.2}, avg_wind={:.2?}, avg_wind_direction={:.2?} knots, Head {:.2?} moored={}",
            effective_position.latitude,
            effective_position.longitude,
            status.max_speed_kn, status.wind_speed_kn, status.wind_angle_deg,
            status.average_heading_deg,
            status.is_moored);

        if status.is_valid() {
            let status_operation = self.generate_vessel_status_operation(status);
            self.state.set_last_persisted_status(&status_operation);

            // Determine trip operation (create, update, or none)
            let trip_operation = Self::determine_trip_operation(
                &mut self.state.current_trip,
                status,
                status_operation.total_distance_nm,
                status_operation.total_time_ms,
            );

            // Perform atomic insert of vessel status and trip operation
            match vessel_db.insert_status_and_trip(&status_operation, &trip_operation) {
                Ok(new_trip_id) => {
                    debug!("Vessel status written to database: latitude={:.6}, longitude={:.6}, is_moored={}", status_operation.position.latitude, status_operation.position.longitude, status_operation.is_moored);

                    // Update trip ID if we created a new trip
                    if let Some(trip_id) = new_trip_id {
                        if let Some(ref mut trip) = self.state.current_trip {
                            trip.id = Some(trip_id);
                            info!("Created new trip: {} (ID: {})", trip.description, trip_id);
                        }
                    } else if let Some(ref trip) = self.state.current_trip {
                        debug!(
                            "Updated trip: {} (ID: {}), total_distance={:.3}nm, total_time={}ms",
                            trip.description,
                            trip.id.unwrap_or(0),
                            trip.total_distance(),
                            trip.total_time()
                        );
                    }

                    // Note: Realtime data is now broadcast directly from NMEA message processing in main loop,
                    // not from the aggregated vessel status handler, to ensure complete data is sent

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
    fn determine_trip_operation(
        current_trip: &mut Option<Trip>,
        status: &VesselStatus,
        distance: f64,
        delta_time_ms: u64,
    ) -> TripOperation {
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
            new_trip.update(
                report_systemtime,
                effective_distance,
                delta_time_ms,
                status.engine_on,
                status.is_moored,
            );

            *current_trip = Some(new_trip.clone());
            TripOperation::CreateTrip(new_trip)
        } else {
            // Update existing trip
            if let Some(ref mut trip) = *current_trip {
                trip.update(
                    report_systemtime,
                    effective_distance,
                    delta_time_ms,
                    status.engine_on,
                    status.is_moored,
                );
                TripOperation::UpdateTrip(trip.clone())
            } else {
                TripOperation::None
            }
        }
    }
}

impl VesselStatusState {
    fn new() -> Self {
        Self {
            last_persisted_status: None,
            current_trip: None,
        }
    }

    fn set_last_persisted_status(&mut self, status: &VesselStatusOperation) {
        self.last_persisted_status = Some(status.clone());
    }

    fn reset_last_persisted_status(&mut self) {
        self.last_persisted_status = None;
    }

    /// Load the last vessel status from database if available
    fn load_last_vessel_status(&mut self, vessel_db: &VesselDatabase) {
        match vessel_db.get_last_vessel_status() {
            Ok(status) => {
                if let Some(s) = status {
                    info!("Loaded last vessel status from database: latitude={:.6}, longitude={:.6}, is_moored={}", s.position.latitude, s.position.longitude, s.is_moored);
                    self.last_persisted_status = Some(s);
                } else {
                    info!("No existing vessel status found in database");
                }
            }
            Err(e) => {
                warn!("Failed to load last vessel status from database: {}", e);
            }
        }
    }

    /// Load the last trip from database if available
    fn load_last_trip(&mut self, vessel_db: &VesselDatabase) {
        match vessel_db.get_last_trip() {
            Ok(trip) => {
                if let Some(t) = trip {
                    info!(
                        "Loaded last trip from database: {} (ID: {})",
                        t.description,
                        t.id.unwrap_or(0)
                    );
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
    use crate::utilities::EngineStatus;

    #[test]
    fn test_set_last_persisted_status() {
        use crate::position_utils::Position;

        let mut state = VesselStatusState::new();

        assert!(state.last_persisted_status.is_none());

        let status = VesselStatusOperation {
            timestamp: std::time::Instant::now(),
            position: Position {
                latitude: 0.0,
                longitude: 0.0,
            },
            average_speed_kn: 0.0,
            max_speed_kn: 0.0,
            is_moored: true,
            engine_on: EngineStatus::Off,
            total_distance_nm: 0.0,
            total_time_ms: 0,
            wind_speed_kn: None,
            wind_speed_variance: None,
            wind_angle_deg: None,
            wind_angle_variance: None,
            cog_deg: None,
            average_heading_deg: None,
        };

        state.set_last_persisted_status(&status);

        assert!(state.last_persisted_status.is_some());
        assert_eq!(
            state
                .last_persisted_status
                .as_ref()
                .unwrap()
                .position
                .latitude,
            0.0
        );
        assert_eq!(
            state
                .last_persisted_status
                .as_ref()
                .unwrap()
                .position
                .longitude,
            0.0
        );
    }

    #[test]
    fn test_reset_last_persisted_status() {
        use crate::position_utils::Position;

        let mut state = VesselStatusState::new();

        let status = VesselStatusOperation {
            timestamp: std::time::Instant::now(),
            position: Position {
                latitude: 45.0,
                longitude: -120.0,
            },
            average_speed_kn: 5.0,
            max_speed_kn: 10.0,
            is_moored: false,
            engine_on: EngineStatus::On,
            total_distance_nm: 1.5,
            total_time_ms: 1000,
            wind_speed_kn: Some(15.0),
            wind_speed_variance: None,
            wind_angle_deg: Some(180.0),
            wind_angle_variance: None,
            cog_deg: Some(090.0),
            average_heading_deg: Some(090.0),
        };

        state.set_last_persisted_status(&status);
        assert!(state.last_persisted_status.is_some());

        state.reset_last_persisted_status();
        assert!(state.last_persisted_status.is_none());
    }
}
