// Vessel Monitoring & Status Tracking
// Core business logic for processing NMEA2000 messages and maintaining vessel state.
// Unit standards: Speed in knots, distance in nautical miles, positions in decimal degrees.
// Important: Never call now() in this module except in event handlers. Timestamps are parameters.
// See: AGENTS.md for conventions and calculation methods (Haversine, angle averaging, etc.)
//
use std::time::{Duration, Instant};
use nmea2k::pgns::{CogSogRapidUpdate, HeadingReference, PositionRapidUpdate};
use crate::mooring_detection::MooringDetectionQueue;
use crate::position_utils::{Position, PositionQueue};
use crate::utilities::{TimedQueue, calculate_true_wind, EngineStatus};

const MOORING_DETECTION_WINDOW: Duration = Duration::from_secs(180); // 3 minutes
#[allow(dead_code)] // Used in mooring detection logic based on position (but we switched to VMG-based detection, so this is now unused)
const MOORING_THRESHOLD_METERS: f64 = 30.0; // 30 meters radius
const MOORING_THRESHOLD_VMG_KNOTS: f64 = 0.25; // 0.25 knots
const MOORING_ACCURACY: f64 = 0.85; // 85% of positions within threshold
const MAX_VALID_SOG_KN: f64 = 25.0; // 25 knots (noise filter)
const MAX_POSITION_DEVIATION_METERS: f64 = 100.0; // Maximum distance from median (noise filter)
const MIN_SAMPLES_FOR_VALIDATION: usize = 10; // Minimum samples required for validation 
const TIME_WINDOW_POSITION_NOISE_FILTER: Duration = Duration::from_secs(10); // 10 seconds

#[derive(Debug, Clone)]
pub struct VesselStatus {
    pub current_position: Position,
    pub median_position: Option<Position>,
    pub number_of_samples: usize,
    pub max_speed_kn: f64,       // Knots
    pub is_moored: bool,
    pub engine_on: EngineStatus,
    pub wind_speed_kn: Option<f64>,
    #[allow(dead_code)] // Used in database writes but not in internal logic
    pub max_wind_speed_kn: Option<f64>,
    pub wind_speed_variance: Option<f64>,
    pub wind_angle_deg: Option<f64>,
    pub wind_angle_variance: Option<f64>,
    pub average_heading_deg: Option<f64>,
    pub timestamp: Instant,
    pub period: Duration,
}

pub struct VesselVector {
    #[allow(dead_code)]
    pub position_1: Position,
    #[allow(dead_code)]
    pub position_2: Position,
    pub delta_time_ms: u64,
    pub distance_nm: f64,
    pub course_deg: f64,
}

impl VesselVector {

    pub fn average_speed_kn(&self) -> f64 {
        if self.delta_time_ms > 0 {
            self.distance_nm / (self.delta_time_ms as f64 / 3600000.0)
        } else {
            0.0
        }
    }  
}

impl VesselStatus {
    pub fn is_valid(&self) -> bool {
        self.number_of_samples > 0
    }

    pub fn get_effective_position(&self) -> Position {
        if self.is_moored {
            if let Some(median_pos) = self.median_position {
                return median_pos;
            }
        }
        self.current_position
    }

    pub fn get_vector_from(&self, position: Position, timestamp: Instant) -> Option<VesselVector> {
        let position_1 = position;
        let position_2 = self.get_effective_position();
        let distance_nm = position_1.distance_to_nm(&position_2);
        let course_from_deg = position_1.course_from_deg(&position_2);
        let time_msecs = self.timestamp.duration_since(timestamp).as_millis() as u64;
        Some(VesselVector {
            position_1,
            position_2,
            delta_time_ms: time_msecs,
            distance_nm,
            course_deg: course_from_deg,
        })
    }
}

#[derive(Debug)]
pub struct VesselMonitor {
    status_report_period: Duration,
    status_report_moored_period: Duration,
    positions: PositionQueue,
    speeds: TimedQueue<f64>,
    vmg_for_mooring: MooringDetectionQueue,
    wind_speeds: TimedQueue<f64>,
    wind_angles: TimedQueue<f64>,
    headings: TimedQueue<f64>,
    last_event_time: Instant,
    engine_on: EngineStatus,
    engine_pending_status: Option<EngineStatus>,
    engine_pending_since: Option<Instant>,
    status_report_ready: bool,
    first_report: bool,
    mooring_status: Option<bool>, // None = unknown, Some(true) = moored, Some(false) = underway
}

impl VesselMonitor {
    pub fn new(status_report_period: Duration, status_report_moored_period: Duration) -> Self {
        let now = Instant::now();
        let time_window_secs = std::cmp::max(status_report_moored_period.as_secs(), status_report_period.as_secs()) + 30;
        let sample_age_window = Duration::from_secs(time_window_secs);
        VesselMonitor {
            status_report_period,
            status_report_moored_period,
            positions: PositionQueue::new(sample_age_window),
            speeds: TimedQueue::new(sample_age_window),
            vmg_for_mooring: MooringDetectionQueue::new(MOORING_DETECTION_WINDOW, MOORING_ACCURACY, MOORING_THRESHOLD_VMG_KNOTS),
            wind_speeds: TimedQueue::new(sample_age_window),
            wind_angles: TimedQueue::new(sample_age_window),
            headings: TimedQueue::new(sample_age_window),
            last_event_time: now,
            engine_on: EngineStatus::Unknown,
            engine_pending_status: None,
            engine_pending_since: None,
            status_report_ready: false,
            first_report: true,
            mooring_status: None,
        }
    }

    /// Process a position rapid update message
    /// Expected maximum rate is 10 Hz
    pub fn process_position(&mut self, position_msg: &PositionRapidUpdate, timestamp: Instant) {
        let position = Position {
            latitude: position_msg.latitude,
            longitude: position_msg.longitude,
        };

        let median_position = self.positions.get_rolling_median_position(TIME_WINDOW_POSITION_NOISE_FILTER, MIN_SAMPLES_FOR_VALIDATION, timestamp);

        // if we have enough samples, validate against median and reject if too far
        if let Some(median) = median_position.1 {
            let distance = position.distance_to_nm(&median) * 1852.0; // Convert nm to meters
            if distance > MAX_POSITION_DEVIATION_METERS {
                return; // Reject noisy position
            }
        }

        self.positions.add_sample(position, timestamp);

        if self.should_generate_event(timestamp) {
            self.status_report_ready = true;
        }
    }

    /// Process a COG & SOG rapid update message
    /// Expected maximum rate is 10 Hz
    pub fn process_cog_sog(&mut self, cog_sog_msg: &CogSogRapidUpdate, timestamp: Instant) {
        let sog_kn = cog_sog_msg.sog_knots();
        
        // Noise filter: Reject unrealistic SOG values (> 25 knots)
        if sog_kn > MAX_VALID_SOG_KN {
            return; // Reject noisy speed reading
        }

        // calculate VMG for mooring detection
        // we use the vmg because the vessel moves sideways at anchor or moored (in the latter case, it does not move at all)
        if let Some(last_heading) = self.headings.get_latest() {
            let cog_deg = cog_sog_msg.cog.to_degrees();
            let vmg_kn = sog_kn * (cog_deg - last_heading).to_radians().cos();
            self.vmg_for_mooring.add_sample(vmg_kn, timestamp);
        } else {
            // No valid heading available, cannot calculate VMG for mooring detection, but the raw sog is almost as good as an approximation for mooring detection, so we can still add the speed sample to the mooring detection queue
            self.vmg_for_mooring.add_sample(sog_kn, timestamp);
        }

        // Update mooring status based on VMG
        let new_status = Some(self.vmg_for_mooring.is_stationary(timestamp));
        if new_status != self.mooring_status {
            self.mooring_status = new_status;
            tracing::info!("Mooring status changed: {:?}", self.mooring_status);
        }

        self.speeds.add_sample(sog_kn, timestamp);
    }

    /// Process a wind data message
    /// Expected maximum rate is 10 Hz
    fn process_wind(&mut self, wind_msg: &nmea2k::pgns::WindData, timestamp: Instant) {
        let wind_speed_kn = wind_msg.speed_knots(); // knots
        let wind_angle_deg = wind_msg.angle.to_degrees();
        // verify if the speed sample is recent enough
        let speed_sample = self.speeds.get_latest_sample();
        if let Some(speed) = speed_sample {
            if speed.timestamp + Duration::from_secs(5) < timestamp {
                // the speed sample is not recent enough - calculation of true wind not possible
                return;
            } else {
                let (true_wind_speed_kn, true_wind_angle_deg) = calculate_true_wind(wind_speed_kn, wind_angle_deg, speed.value);
                //println!("Calculated true wind: speed {:.3} kn, angle {:.3} deg (apparent speed {:.3} kn, angle {:.3} deg, boat speed {:.3} kn)", true_wind_speed_kn, true_wind_angle_deg, wind_speed_kn, wind_angle_deg, speed.value);
                self.wind_speeds.add_sample(true_wind_speed_kn, timestamp);
                self.wind_angles.add_sample(crate::utilities::normalize0_360(true_wind_angle_deg), timestamp);
            }
        }
    }

    /// Process engine rapid update to determine engine status
    /// Expected maximum rate is 1 Hz
    pub fn process_engine(&mut self, engine_msg: &nmea2k::pgns::EngineRapidUpdate, timestamp: Instant) {
        // Determine engine status from RPM: > 50 = on, <= 50 = off, missing/invalid = unknown
        let new_status = match engine_msg.engine_speed {
            Some(rpm) if rpm > 50.0 => EngineStatus::On,
            Some(rpm) if rpm <= 50.0 => EngineStatus::Off,
            _ => EngineStatus::Unknown,
        };
        if new_status == self.engine_on {
            // Stable — clear any pending transition
            self.engine_pending_status = None;
            self.engine_pending_since = None;
        } else {
            // Status differs from current — apply 5-second hysteresis
            if self.engine_pending_status == Some(new_status) {
                // Pending transition has been consistent long enough
                if self.engine_pending_since
                    .map_or(false, |t| timestamp.duration_since(t) >= Duration::from_secs(5))
                {
                    self.engine_on = new_status;
                    self.engine_pending_status = None;
                    self.engine_pending_since = None;
                }
            } else {
                // Start tracking a new candidate transition
                self.engine_pending_status = Some(new_status);
                self.engine_pending_since = Some(timestamp);
            }
        }
    }

    /// Process vessel heading message
    /// Expected maximum rate is 10 Hz
    pub fn process_heading(&mut self, heading_msg: &nmea2k::pgns::VesselHeading, timestamp: Instant) {
        if heading_msg.reference == HeadingReference::Magnetic {
            // For magnetic heading, we would need to apply variation correction
            // For simplicity, we skip magnetic headings in this implementation
            if let Some(pos) = self.positions.get_latest_position() {
                let heading_deg = heading_msg.heading.to_degrees();
                let var = match crate::utilities::get_variation_deg(pos.latitude, pos.longitude, chrono::Utc::now()) {
                    Ok(v) => v,
                    Err(_) => 0.0, // Unable to get variation, revert to magnetic - better than nothing
                };
                let true_heading_deg = crate::utilities::normalize0_360(heading_deg + var);
                self.headings.add_sample( true_heading_deg, timestamp);
            } else {
                // No position available to calculate variation, but better magnetic than nothing
                self.headings.add_sample(heading_msg.heading.to_degrees(), timestamp);
            }
        }
    }

    /// Check if it's time to generate a status event
    fn should_generate_event(&self, now: Instant) -> bool {
        let period = if !self.is_moored(now) || self.first_report {
            // in case we are moored, but it's the first report, use the regular reporting period so we have a status asap
            self.status_report_period
        } else {
            self.status_report_moored_period
        };
        now.duration_since(self.last_event_time) >= period && self.positions.len() >= MIN_SAMPLES_FOR_VALIDATION
    }

    pub fn is_moored(&self, now: Instant) -> bool {
        //self.positions.is_stationary(MOORING_DETECTION_WINDOW, MOORING_ACCURACY, MOORING_THRESHOLD_METERS, now)
        self.vmg_for_mooring.is_stationary(now)
    }

    /// Generate a vessel status event
    pub fn generate_status(&mut self, now: Instant) -> Option<VesselStatus> {
        if !self.status_report_ready {
            return None;
        }

        self.status_report_ready = false;

        self.last_event_time = now;

        let current_position = self.positions.get_latest_position().unwrap();
        let start_sampling_time = self.positions.get_earliest_position_timestamp().unwrap_or(now);
        let (number_of_samples, median_position) = self.positions.get_rolling_median_position(self.status_report_period, MIN_SAMPLES_FOR_VALIDATION, now);
        let max_speed_kn = self.speeds.get_max(self.status_report_period, now).unwrap_or(0.0);
        let is_moored = self.is_moored(now);
        let wind_speed_kn = self.wind_speeds.get_average(self.status_report_period, now);
        let wind_angle_deg = self.wind_angles.get_average_as_angle_deg(self.status_report_period, now);
        let max_wind_speed_kn = self.wind_speeds.get_max(self.status_report_period, now);
        
        let average_heading = self.headings.get_average_as_angle_deg(self.status_report_period, now);
        // Use the timestamp of the last position in the buffer, or current time if no positions
        let timestamp = self.positions.get_latest_position_timestamp().unwrap();
        
        self.first_report = false;

        Some(VesselStatus {
            current_position,
            median_position,
            number_of_samples,
            max_speed_kn,
            is_moored,
            engine_on: self.engine_on.clone(),
            timestamp,
            period: now.duration_since(start_sampling_time),
            wind_speed_kn,
            max_wind_speed_kn,
            wind_speed_variance: None, // Variance calculation not implemented
            wind_angle_deg,
            wind_angle_variance: None, // Variance calculation not implemented
            average_heading_deg: average_heading,
        })
    }
}

impl nmea2k::MessageHandler for VesselMonitor {
    fn handle_message(&mut self, frame: &nmea2k::N2kFrame, timestamp: std::time::Instant) {
        match &frame.message {
            nmea2k::pgns::N2kMessage::WindData(wind) => {
                self.process_wind(wind, timestamp);
            }
            nmea2k::pgns::N2kMessage::PositionRapidUpdate(pos) => {
                self.process_position(pos, timestamp);
            }
            nmea2k::pgns::N2kMessage::CogSogRapidUpdate(cog_sog) => {
                self.process_cog_sog(cog_sog, timestamp);
            }
            nmea2k::pgns::N2kMessage::EngineRapidUpdate(engine) => {
                self.process_engine(engine, timestamp);
            }
            nmea2k::pgns::N2kMessage::VesselHeading(heading) => {
                self.process_heading(heading, timestamp);
            }
            _ => {} // Ignore messages we're not interested in
        }
    }
}

impl Default for VesselMonitor {
    fn default() -> Self {
        Self::new(Duration::from_secs(30), Duration::from_secs(300))
    }
}

#[cfg(test)]
mod tests {
        use nmea2k::pgns::WindData;

        fn make_speed_sample(monitor: &mut VesselMonitor, sog_kn: f64, now: std::time::Instant) {
            // Helper to inject a speed sample
            // SOG in knots, convert to m/s for the message (1 kn = 0.514444 m/s)
            let sog_ms = sog_kn * 0.514444;
            let mut data = [0u8; 8];
            // SOG is at bytes 4-5 as u16, in cm/s
            let sog_cmps = (sog_ms * 100.0) as u16;
            data[4] = (sog_cmps & 0xFF) as u8;
            data[5] = (sog_cmps >> 8) as u8;
            let cog_sog_msg = CogSogRapidUpdate::from_bytes(&data).unwrap();
            monitor.process_cog_sog(&cog_sog_msg, now);
        }

        fn make_wind_sample(monitor: &mut VesselMonitor, speed_kn: f64, angle_deg: f64, now: std::time::Instant) {
            // For test diagnosis: print the encoded and decoded values
            let speed_mps = speed_kn * 0.514444;
            let angle_rad = angle_deg.to_radians();
            println!("Encoding wind: {} kn ({:.3} m/s), angle {} deg ({:.3} rad)", speed_kn, speed_mps, angle_deg, angle_rad);
            let wind_msg = WindData::new_apparent(speed_mps, angle_rad);
            // Removed print statement for wind_msg
            monitor.process_wind(&wind_msg, now);
        }


        #[test]
        fn test_wind_sample_ignored_if_no_recent_speed() {
            let mut monitor = VesselMonitor::default();
            let base_time = Instant::now();
            
            // Add position samples using simulated time
            for i in 0..10 {
                let position_msg = PositionRapidUpdate {
                    pgn: 129025,
                    latitude: 45.0,
                    longitude: -122.0,
                };
                let timestamp = base_time + Duration::from_millis(i * 10);
                monitor.process_position(&position_msg, timestamp);
            }
            // No speed sample yet
            make_wind_sample(&mut monitor, 10.0, 90.0, base_time + Duration::from_millis(100));
            // Wind buffer should remain empty
            assert_eq!(monitor.wind_speeds.len(), 0);
        }

        #[test]
        fn test_wind_sample_ignored_if_speed_outdated() {
            let mut monitor = VesselMonitor::default();
            let base_time = Instant::now();
            
            // Add position samples using simulated time
            for i in 0..10 {
                let position_msg = PositionRapidUpdate {
                    pgn: 129025,
                    latitude: 45.0,
                    longitude: -122.0,
                };
                let timestamp = base_time + Duration::from_millis(i * 10);
                monitor.process_position(&position_msg, timestamp);
            }
            
            // Add a speed sample at base_time
            make_speed_sample(&mut monitor, 5.0, base_time);
            
            // Try to add wind sample >5s later (speed is now outdated)
            let wind_time = base_time + Duration::from_secs(6);
            make_wind_sample(&mut monitor, 10.0, 90.0, wind_time);
            
            // Wind buffer should remain empty because speed sample is outdated
            assert_eq!(monitor.wind_speeds.len(), 0);
        }

        #[test]
        fn test_wind_rolling_window() {
            let mut monitor = VesselMonitor::default();
            // Add position samples
            for _ in 0..10 {
                let position_msg = PositionRapidUpdate {
                    pgn: 129025,
                    latitude: 45.0,
                    longitude: -122.0,
                };
                monitor.process_position(&position_msg, Instant::now());
            }
            // Add a speed sample
            make_speed_sample(&mut monitor, 5.0, Instant::now());
            // Add wind samples over time
            use std::time::Instant;
            let now = Instant::now();
            for i in 0..15 {
                let timestamp = now - Duration::from_secs(700 + i * 10); // Old timestamps
                make_wind_sample(&mut monitor, 10.0 + i as f64, 45.0, timestamp);
            }
            // Add another wind sample with a current timestamp
            make_wind_sample(&mut monitor, 20.0, 90.0, now);
            
            // Check that we have some wind samples
            assert!(monitor.wind_speeds.len() > 0, "Expected wind speed samples");
            
            // TimedQueue automatically cleans old samples, so only recent ones remain
            let cutoff = now - Duration::from_secs(10);
            let latest_timestamp = monitor.wind_speeds.get_latest_timestamp().unwrap();
            assert!(latest_timestamp >= cutoff, "Expected recent wind samples");
        }
    use super::*;
    use nmea2k::pgns::{PositionRapidUpdate, CogSogRapidUpdate};

    #[test]
    fn test_vessel_status_creation() {
        let mut monitor = VesselMonitor::default();
        // Add position samples to allow status generation
        for _ in 0..10 {
            let position_msg = PositionRapidUpdate {
                pgn: 129025,
                latitude: 45.0,
                longitude: -122.0,
            };
            monitor.process_position(&position_msg, Instant::now());
        }
        // Add a speed sample (required for true wind calculation)
        let boat_speed_kn = 5.0;
        make_speed_sample(&mut monitor, boat_speed_kn, Instant::now());

        // Add several wind samples
        let wind_samples = vec![(10.0, 45.0), (12.0, 50.0), (8.0, 40.0)];
        let mut expected_speeds = Vec::new();
        let mut expected_angles = Vec::new();
        for (ws, wa) in &wind_samples {
            let (tw_speed, tw_angle) = crate::utilities::calculate_true_wind(*ws, *wa, boat_speed_kn);
            expected_speeds.push(tw_speed);
            expected_angles.push(crate::utilities::normalize0_360(tw_angle));
            make_wind_sample(&mut monitor, *ws, *wa, Instant::now());
        }
        // Force last_event_time to the past to allow status generation
        monitor.last_event_time = std::time::Instant::now() - monitor.status_report_period - Duration::from_secs(1);
        monitor.status_report_ready = true; // Mark as ready for generation
        
        let status = monitor.generate_status(Instant::now()).unwrap();
        // Wind statistics should be present
        assert!(status.wind_speed_kn.is_some());
        assert!(status.wind_angle_deg.is_some());
        let speed = status.wind_speed_kn.unwrap();
        let angle = status.wind_angle_deg.unwrap();
        let expected_speed = expected_speeds.iter().sum::<f64>() / expected_speeds.len() as f64;
        let expected_angle = expected_angles.iter().sum::<f64>() / expected_angles.len() as f64;
        // Allow a small margin for floating point error
        println!("Expected avg speed: {:.3}, got: {:.3}", expected_speed, speed);
        println!("Expected avg angle: {:.3}, got: {:.3}", expected_angle, angle);
        assert!((speed - expected_speed).abs() < 0.01, "Expected {}, got {}", expected_speed, speed);
        assert!((angle - expected_angle).abs() < 0.1, "Expected {}, got {}", expected_angle, angle);
    }
    #[test]
    fn test_process_position() {
        let mut monitor = VesselMonitor::default();
        
        // Add 10 positions to meet minimum requirement
        for _ in 0..10 {
            let position_msg = PositionRapidUpdate {
                pgn: 129025,
                latitude: 45.0,
                longitude: -122.0,
            };
            monitor.process_position(&position_msg, Instant::now());
            std::thread::sleep(Duration::from_millis(50));
        }
        
        // Add one more position which should be accepted
        let position_msg = PositionRapidUpdate {
            pgn: 129025,
            latitude: 45.0,
            longitude: -122.0,
        };
        monitor.process_position(&position_msg, Instant::now());
        
        assert_eq!(monitor.positions.len(), 11);
        let pos = monitor.positions.get_latest_position().unwrap();
        assert_eq!(pos.latitude, 45.0);
        assert_eq!(pos.longitude, -122.0);
    }

    #[test]
    fn test_process_cog_sog() {
        let mut monitor = VesselMonitor::default();
        
        // Create a valid COG/SOG message using from_bytes
        let data = vec![
            0x01, // SID
            0x00, // COG reference (true)
            0xB8, 0x22, // COG = 8888 * 0.0001 rad ≈ 50.9°
            0xF4, 0x01, // SOG = 500 * 0.01 = 5.0 m/s
            0x00, 0x00, // Reserved
        ];
        let cog_sog_msg = CogSogRapidUpdate::from_bytes(&data).unwrap();
        
        monitor.process_cog_sog(&cog_sog_msg, Instant::now());
        
        assert_eq!(monitor.speeds.len(), 1);
    }

    #[test]
    fn test_noise_filter_rejects_high_sog() {
        let mut monitor = VesselMonitor::default();
        
        // Try to add a speed sample > 25 knots (should be rejected)
        let data_high = vec![
            0x01, 0x00,
            0xB8, 0x22, // COG
            0x10, 0x27, // SOG = 10000 * 0.01 = 100 m/s (~194 knots) - unrealistic
            0x00, 0x00,
        ];
        let cog_sog_msg = CogSogRapidUpdate::from_bytes(&data_high).unwrap();
        monitor.process_cog_sog(&cog_sog_msg, Instant::now());
        
        // Speed buffer should be empty (rejected)
        assert_eq!(monitor.speeds.len(), 0);
        
        // Try to add a valid speed sample < 25 knots (should be accepted)
        let data_valid = vec![
            0x01, 0x00,
            0xB8, 0x22, // COG
            0xC8, 0x00, // SOG = 200 * 0.01 = 2.0 m/s (~3.9 knots) - realistic
            0x00, 0x00,
        ];
        let cog_sog_msg_valid = CogSogRapidUpdate::from_bytes(&data_valid).unwrap();
        monitor.process_cog_sog(&cog_sog_msg_valid, Instant::now());
        
        // Speed buffer should have one sample
        assert_eq!(monitor.speeds.len(), 1);
    }

    #[test]
    fn test_noise_filter_rejects_distant_position() {
        let mut monitor = VesselMonitor::default();
        
        // Add several positions at approximately the same location
        // Need at least 10 samples for validation to work
        for _ in 0..10 {
            let position_msg = PositionRapidUpdate {
                pgn: 129025,
                latitude: 45.0,
                longitude: -122.0,
            };
            monitor.process_position(&position_msg, Instant::now());
            std::thread::sleep(Duration::from_millis(50)); // Small delay to ensure timestamps differ
        }
        
        assert_eq!(monitor.positions.len(), 10);
        
        // Try to add a position very far away (> 100m from median)
        // ~0.01 degrees latitude ≈ 1.1 km
        let distant_position = PositionRapidUpdate {
            pgn: 129025,
            latitude: 45.01, // ~1.1 km away
            longitude: -122.0,
        };
        monitor.process_position(&distant_position, Instant::now());
        
        // Should still have 10 positions (distant one rejected)
        assert_eq!(monitor.positions.len(), 10);
        
        // Add a position close to the median (< 100m)
        let close_position = PositionRapidUpdate {
            pgn: 129025,
            latitude: 45.0001, // ~11 meters away
            longitude: -122.0,
        };
        monitor.process_position(&close_position, Instant::now());
        
        // Should now have 11 positions (close one accepted)
        assert_eq!(monitor.positions.len(), 11);
    }

    #[test]
    fn test_noise_filter_requires_minimum_samples() {
        let mut monitor = VesselMonitor::default();
        
        // Add only 5 positions (less than minimum required) - these should be accepted during bootstrap
        for _ in 0..5 {
            let position_msg = PositionRapidUpdate {
                pgn: 129025,
                latitude: 45.0,
                longitude: -122.0,
            };
            monitor.process_position(&position_msg, Instant::now());
            std::thread::sleep(Duration::from_millis(50));
        }
        
        // Should have 5 positions (accepted during bootstrap phase)
        assert_eq!(monitor.positions.len(), 5);
        
        // Add more positions to reach the minimum (total 15)
        for _ in 0..10 {
            let position_msg = PositionRapidUpdate {
                pgn: 129025,
                latitude: 45.0,
                longitude: -122.0,
            };
            monitor.process_position(&position_msg, Instant::now());
            std::thread::sleep(Duration::from_millis(50));
        }
        
        // Now should have 15 positions
        assert_eq!(monitor.positions.len(), 15);
        
        // Now that we have enough samples, a distant position should be rejected
        let distant_position = PositionRapidUpdate {
            pgn: 129025,
            latitude: 45.01, // ~1.1 km away
            longitude: -122.0,
        };
        monitor.process_position(&distant_position, Instant::now());
        
        // Should still have 15 positions (distant one rejected)
        assert_eq!(monitor.positions.len(), 15);
    }

    #[test]
    fn test_mooring_detection_stationary() {
        let mut monitor = VesselMonitor::default();
        
        // Add multiple positions at the same location over time
        let position_msg = PositionRapidUpdate {
            pgn: 129025,
            latitude: 45.0,
            longitude: -122.0,
        };
        
        // Add 15 positions with delays to ensure we have enough samples
        for _ in 0..15 {
            monitor.process_position(&position_msg, Instant::now());
            std::thread::sleep(Duration::from_millis(50));
        }
        
        // Check mooring detection using the PositionQueue method
        let is_moored = monitor.positions.is_stationary(
            Duration::from_secs(180), // MOORING_DETECTION_WINDOW
            0.90,                      // MOORING_ACCURACY
            30.0,                      // MOORING_THRESHOLD_METERS
            Instant::now()
        );
        // Should detect mooring (all positions within small radius)
        assert!(is_moored);
        // Should have at least 10 samples accepted
        assert!(monitor.positions.len() >= 10);
    }

    #[test]
    fn test_vessel_status_generation() {
        let mut monitor = VesselMonitor::default();
        let base_time = Instant::now();
        
        // Add enough position samples to meet minimum requirement using simulated time
        for i in 0..10 {
            let position_msg = PositionRapidUpdate {
                pgn: 129025,
                latitude: 45.0,
                longitude: -122.0,
            };
            let timestamp = base_time + Duration::from_millis(i * 5);
            monitor.process_position(&position_msg, timestamp);
        }
        
        let data = vec![
            0x01, 0x00,
            0xB8, 0x22, // COG
            0xC8, 0x00, // SOG = 200 * 0.01 = 2.0 m/s
            0x00, 0x00,
        ];
        let cog_sog_msg = CogSogRapidUpdate::from_bytes(&data).unwrap();
        monitor.process_cog_sog(&cog_sog_msg, base_time + Duration::from_millis(100));
        
        // Simulate status report period elapsed
        let status_time = base_time + monitor.status_report_period + Duration::from_millis(100);
        
        // Mark status as ready for generation
        monitor.status_report_ready = true;
        
        let status = monitor.generate_status(status_time);
        assert!(status.is_some());
    }
}
