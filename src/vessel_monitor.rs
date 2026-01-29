use std::collections::VecDeque;
use std::time::{Duration, Instant};
use nmea2k::pgns::{PositionRapidUpdate, CogSogRapidUpdate};
use crate::config::VesselStatusConfig;
use crate::utilities::{angle_diff, average_angle, calculate_true_wind};

const EVENT_INTERVAL: Duration = Duration::from_secs(10);
const MOORING_DETECTION_WINDOW: Duration = Duration::from_secs(120); // 2 minutes
const MOORING_THRESHOLD_METERS: f64 = 30.0; // 30 meters radius
const MOORING_ACCURACY: f64 = 0.90; // 90% of positions within threshold
const MAX_VALID_SOG_KN: f64 = 25.0; // 25 knots (noise filter)
const MAX_POSITION_DEVIATION_METERS: f64 = 100.0; // Maximum distance from median (noise filter)
const POSITION_VALIDATION_WINDOW: Duration = Duration::from_secs(10); // Time window for median calculation
const MIN_SAMPLES_FOR_VALIDATION: usize = 10; // Minimum samples required for validation 

#[derive(Debug, Clone)]
pub struct VesselStatus {
    pub current_position: Position,
    pub average_position: Option<Position>,
    pub number_of_samples: usize,
    pub max_speed_kn: f64,       // Knots
    pub is_moored: bool,
    pub engine_on: bool,
    pub wind_speed_kn: Option<f64>,
    pub wind_speed_variance: Option<f64>,
    pub wind_angle_deg: Option<f64>,
    pub wind_angle_variance: Option<f64>,
    pub timestamp: Instant,
}

impl VesselStatus {
    pub fn is_valid(&self) -> bool {
        self.number_of_samples > 0
    }

    pub fn get_effective_position(&self) -> Position {
        if self.is_moored {
            if let Some(avg_pos) = self.average_position {
                return avg_pos;
            }
        }
        self.current_position
    }

    pub fn get_total_distance_and_time_from_last_report(&self, _last_status: &mut Option<VesselStatus>) -> (f64, u64) {
        if let previous = Some(VesselStatus)
        {
            let distance_nm = previous.get_effective_position().distance_to_nm(&self.get_effective_position());
            let time_secs = self.timestamp.duration_since(previous.timestamp).as_millis();
            (distance_nm, time_secs)
        }
        else 
            (0.0, 0)
    }
}

impl Position {
    /// Returns the distance to another position in nautical miles (using Haversine formula)
    pub fn distance_to_nm(&self, other: &Position) -> f64 {
        let r = 6371.0; // Earth radius in km
        let dlat = (other.latitude - self.latitude).to_radians();
        let dlon = (other.longitude - self.longitude).to_radians();
        let lat1 = self.latitude.to_radians();
        let lat2 = other.latitude.to_radians();
        let a = (dlat / 2.0).sin().powi(2)
            + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
        let distance_km = r * c;
        distance_km / 1.852 // convert km to nautical miles
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Position {
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct PositionSample {
    pub position: Position,
    pub timestamp: Instant,
}

#[derive(Debug, Clone, Copy)]
pub struct WindSample {
    wind_speed_kn: f64,
    wind_angle_deg: f64,
    timestamp: Instant,
}

#[derive(Debug, Clone, Copy)]
struct SpeedSample {
    speed_kn: f64,
    timestamp: Instant,
}

pub struct VesselMonitor {
    positions: VecDeque<PositionSample>,
    speeds: VecDeque<SpeedSample>,
    winds: VecDeque<WindSample>,
    last_event_time: Instant,
    engine_on: bool,
    rolling_median_position: Option<Position>,
}

impl VesselMonitor {
    pub fn new(_config: VesselStatusConfig) -> Self {
        let now = Instant::now();
        VesselMonitor {
            positions: VecDeque::new(),
            speeds: VecDeque::new(),
            winds: VecDeque::new(),
            last_event_time: now,
            engine_on: false,
            rolling_median_position: None,
        }
    }
    
    fn process_wind(&mut self, wind_msg: &nmea2k::pgns::WindData) {
                // For test diagnosis: print the decoded values
                    // Removed print statement for wind_msg
            // For test diagnosis: print the decoded values
                // Removed print statement for wind_msg
        let now = Instant::now();

        let wind_speed_kn = wind_msg.speed_knots(); // knots
        let wind_angle_deg = wind_msg.angle.to_degrees();
        // verify if the speed sample is recent enough
        let speed_sample = self.speeds.back();
        if let Some(speed_sample) = speed_sample {
            let speed_kn = speed_sample.speed_kn;
            if speed_sample.timestamp + Duration::from_secs(5) < now {
                // the speed sample is not recent enough - calculation of true wind not possible
                return;
            } else {
                let (true_wind_speed_kn, true_wind_angle_deg) = calculate_true_wind(wind_speed_kn, wind_angle_deg, speed_kn);
                self.winds.push_back(WindSample {
                    wind_speed_kn: true_wind_speed_kn,
                    wind_angle_deg: crate::utilities::normalize0_360(true_wind_angle_deg),
                    timestamp: now,
                });
            }

        }

        // Clean up old wind samples (keep only last 10 minutes + buffer)
        let cutoff = now - Duration::from_secs(600) - Duration::from_secs(30);
        while let Some(sample) = self.winds.front() {
            if sample.timestamp < cutoff {
                self.winds.pop_front();
            } else {
                break;
            }
        }
    }

    /// Process a position rapid update message
    pub fn process_position(&mut self, position_msg: &PositionRapidUpdate) {
        let now = Instant::now();
        let position = Position {
            latitude: position_msg.latitude,
            longitude: position_msg.longitude,
        };

        // Noise filter: Check distance from median of last samples
        if !self.is_valid_position(&position) {
            return; // Reject noisy position
        }
        self.positions.push_back(PositionSample {
            position,
            timestamp: now,
        });

        // Clean up old position samples (keep only last 2 minutes + 30s buffer)
        let cutoff = now - MOORING_DETECTION_WINDOW - Duration::from_secs(30);
        while let Some(sample) = self.positions.front() {
            if sample.timestamp < cutoff {
                self.positions.pop_front();
            } else {
                break;
            }
        }
    }

    /// Process a COG & SOG rapid update message
    pub fn process_cog_sog(&mut self, cog_sog_msg: &CogSogRapidUpdate) {
        let now = Instant::now();
        let sog_kn = cog_sog_msg.sog_knots();
        
        // Noise filter: Reject unrealistic SOG values (> 25 knots)
        if sog_kn > MAX_VALID_SOG_KN {
            return; // Reject noisy speed reading
        }

        self.speeds.push_back(SpeedSample {
            speed_kn: sog_kn,
            timestamp: now,
        });

        // Clean up old speed samples (keep only last 30s + buffer)
        let cutoff = now - EVENT_INTERVAL - Duration::from_secs(5);
        while let Some(sample) = self.speeds.front() {
            if sample.timestamp < cutoff {
                self.speeds.pop_front();
            } else {
                break;
            }
        }
    }

    /// Process engine rapid update to determine engine status
    pub fn process_engine(&mut self, engine_msg: &nmea2k::pgns::EngineRapidUpdate) {
        self.engine_on = engine_msg.is_engine_running();
    }

    /// Validate position against median of recent samples (noise filter)
    fn is_valid_position(&self, position: &Position) -> bool {
        let now = Instant::now();
        let cutoff = now - POSITION_VALIDATION_WINDOW;
        
        // Get samples from the last 10 seconds
        let recent_positions: Vec<&Position> = self.positions
            .iter()
            .rev()
            .take_while(|s| s.timestamp >= cutoff)
            .map(|s| &s.position)
            .collect();
        
        // If we don't have enough samples yet, accept to build up the buffer
        if recent_positions.len() < MIN_SAMPLES_FOR_VALIDATION {
            return true; // Accept during bootstrap phase
        }

        // Calculate median position
        let median = self.calculate_median_position(&recent_positions);
        
        // Check distance from median
        let distance = position.distance_to_nm(&median) * 1852.0; // Convert nm to meters
        distance <= MAX_POSITION_DEVIATION_METERS
    }

    /// Calculate median position from a set of positions
    fn calculate_median_position(&self, positions: &[&Position]) -> Position {
        if positions.is_empty() {
            return Position { latitude: 0.0, longitude: 0.0 };
        }

        let mut lats: Vec<f64> = positions.iter().map(|p| p.latitude).collect();
        let mut lons: Vec<f64> = positions.iter().map(|p| p.longitude).collect();
        
        lats.sort_by(|a, b| a.partial_cmp(b).unwrap());
        lons.sort_by(|a, b| a.partial_cmp(b).unwrap());
        
        let mid = lats.len() / 2;
        let median_lat = if lats.len() % 2 == 0 {
            (lats[mid - 1] + lats[mid]) / 2.0
        } else {
            lats[mid]
        };
        
        let median_lon = if lons.len() % 2 == 0 {
            (lons[mid - 1] + lons[mid]) / 2.0
        } else {
            lons[mid]
        };
        
        Position {
            latitude: median_lat,
            longitude: median_lon,
        }
    }

    /// Check if it's time to generate a status event
    pub fn should_generate_event(&self) -> bool {
        Instant::now().duration_since(self.last_event_time) >= EVENT_INTERVAL
    }

    /// Generate a vessel status event
    pub fn generate_status(&mut self) -> Option<VesselStatus> {
        if !self.should_generate_event() || self.positions.is_empty() {
            return None;
        }

        let current_position = self.positions.back().unwrap().position;
        let (sample_count, average_position) = self.calculate_average_position(EVENT_INTERVAL);
        let (_, _, max_speed) = self.calculate_average_and_max_speed(EVENT_INTERVAL);
        let is_moored = self.is_vessel_moored();
        let (wind_speed_kn, wind_speed_variance, wind_angle_deg, wind_angle_variance_deg) = self.calculate_wind_statistics(&self.winds, EVENT_INTERVAL);

        // Use the timestamp of the last position in the buffer, or current time if no positions
        let timestamp = self.positions.back()
            .map(|sample| sample.timestamp)
            .unwrap_or_else(|| Instant::now());
        
        self.last_event_time = Instant::now();

        Some(VesselStatus {
            current_position,
            average_position,
            number_of_samples: sample_count,
            max_speed_kn: max_speed,
            is_moored,
            engine_on: self.engine_on,
            timestamp,
            wind_speed_kn,
            wind_speed_variance,
            wind_angle_deg,
            wind_angle_variance: wind_angle_variance_deg,
        })
    }

    fn calculate_wind_statistics(&self, winds: &VecDeque<WindSample>, window: Duration) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
        let now = Instant::now();
        let cutoff = now - window;

        let relevant_winds: Vec<&WindSample> = winds.iter().rev()
            .take_while(|w| w.timestamp >= cutoff)
            .collect();

        if relevant_winds.is_empty() {
            return (None, None, None, None);
        }

        let count: f64 = relevant_winds.len() as f64;
        let mean_speed: f64 = relevant_winds.iter().map(|w| w.wind_speed_kn).sum::<f64>() / count;
        let mean_angle: f64 = average_angle(&relevant_winds.iter().map(|w| w.wind_angle_deg.to_radians()).collect::<Vec<f64>>());
        
        // Calculate variance of wind angle
        let mut variance_speed = 0.0;
        let mut variance_angle = 0.0;
        for w in relevant_winds.iter() {
            variance_angle += angle_diff(w.wind_angle_deg, mean_angle).powi(2);
            variance_speed += (w.wind_speed_kn - mean_speed).powi(2);
        }
        variance_speed = (variance_speed / count).sqrt();
        variance_angle = (variance_angle / count).sqrt();

        (Some(mean_speed), Some(variance_speed), Some(mean_angle), Some(variance_angle))
    }

    fn calculate_average_position(&mut self, window: Duration) -> (usize, Option<Position>) {
        let mut avg_latitude = 0.0;
        let mut avg_longitude = 0.0;
        let mut sample_count = 0;
        
        let iterator = self.positions.iter().rev();
        for p in iterator {
            if p.timestamp.duration_since(self.last_event_time) > window {
                break; // go back until last event time, then stop
            }
            avg_latitude += p.position.latitude;
            avg_longitude += p.position.longitude;
            sample_count += 1;  
        }
    
        let average_position = if sample_count > 0 {
            Some(Position {
                latitude: avg_latitude / sample_count as f64,
                longitude: avg_longitude / sample_count as f64,
            })
        } else {
            None
        };
        (sample_count, average_position)
    }
    
    fn calculate_average_and_max_speed(&self, window: Duration) -> (usize, f64, f64) {
        let now = Instant::now();
        let cutoff = now - window;

        let iterator = self.speeds.iter().rev();
        let mut total_speed_kn = 0.0;
        let mut count = 0;
        let mut max_speed_kn = 0.0;
        for s in iterator {
            if s.timestamp < cutoff {
                break;
            }
            total_speed_kn += s.speed_kn;
            if s.speed_kn > max_speed_kn {
                max_speed_kn = s.speed_kn;
            }
            count += 1;
        }
        let average_speed_kn = if count > 0 {
            total_speed_kn / count as f64
        } else {
            0.0
        };
        (count, average_speed_kn, max_speed_kn)
    }

    fn is_vessel_moored(&self) -> bool {
        if self.positions.len() < 2 {
            return false;
        }

        let now = Instant::now();
        let cutoff = now - MOORING_DETECTION_WINDOW;

        // Get positions from the last 2 minutes
        let recent_positions: Vec<&PositionSample> = self
            .positions
            .iter()
            .filter(|p| p.timestamp >= cutoff)
            .collect();

        if recent_positions.is_empty() {
            return false;
        }

        // Calculate the average position
        let avg_lat = recent_positions.iter().map(|p| p.position.latitude).sum::<f64>()
            / recent_positions.len() as f64;
        let avg_lon = recent_positions.iter().map(|p| p.position.longitude).sum::<f64>()
            / recent_positions.len() as f64;

        let avg_position = Position {
            latitude: avg_lat,
            longitude: avg_lon,
        };

        // Check if all positions are within threshold of average position
        recent_positions
            .iter()
            .filter(|p| (p.position.distance_to_nm(&avg_position) * 1852.0) <= MOORING_THRESHOLD_METERS)
            .count() >= (recent_positions.len() as f64 * MOORING_ACCURACY) as usize // At least 90% within threshold
    }
}

impl nmea2k::MessageHandler for VesselMonitor {
    fn handle_message(&mut self, message: &nmea2k::N2kMessage) {
        match message {
            nmea2k::pgns::N2kMessage::WindData(wind) => {
                self.process_wind(wind);
            }
            nmea2k::pgns::N2kMessage::PositionRapidUpdate(pos) => {
                self.process_position(pos);
            }
            nmea2k::pgns::N2kMessage::CogSogRapidUpdate(cog_sog) => {
                self.process_cog_sog(cog_sog);
            }
            nmea2k::pgns::N2kMessage::EngineRapidUpdate(engine) => {
                self.process_engine(engine);
            }
            _ => {} // Ignore messages we're not interested in
        }
    }
}

impl Default for VesselMonitor {
    fn default() -> Self {
        Self::new(VesselStatusConfig::default())
    }
}

#[cfg(test)]
mod tests {
        use nmea2k::pgns::WindData;

        fn make_speed_sample(monitor: &mut VesselMonitor, sog_kn: f64) {
            // Helper to inject a speed sample
            // SOG in knots, convert to m/s for the message (1 kn = 0.514444 m/s)
            let sog_ms = sog_kn * 0.514444;
            let mut data = [0u8; 8];
            // SOG is at bytes 4-5 as u16, in cm/s
            let sog_cmps = (sog_ms * 100.0) as u16;
            data[4] = (sog_cmps & 0xFF) as u8;
            data[5] = (sog_cmps >> 8) as u8;
            let cog_sog_msg = CogSogRapidUpdate::from_bytes(&data).unwrap();
            monitor.process_cog_sog(&cog_sog_msg);
        }

        fn make_wind_sample(monitor: &mut VesselMonitor, speed_kn: f64, angle_deg: f64) {
            // For test diagnosis: print the encoded and decoded values
            let speed_mps = speed_kn * 0.514444;
            let angle_rad = angle_deg.to_radians();
            println!("Encoding wind: {} kn ({:.3} m/s), angle {} deg ({:.3} rad)", speed_kn, speed_mps, angle_deg, angle_rad);
            let wind_msg = WindData::new_apparent(speed_mps, angle_rad);
            // Removed print statement for wind_msg
            monitor.process_wind(&wind_msg);
        }


        #[test]
        fn test_wind_sample_ignored_if_no_recent_speed() {
            let config = VesselStatusConfig::default();
            let mut monitor = VesselMonitor::new(config);
            // Add position samples
            for _ in 0..10 {
                let position_msg = PositionRapidUpdate {
                    pgn: 129025,
                    latitude: 45.0,
                    longitude: -122.0,
                };
                monitor.process_position(&position_msg);
                std::thread::sleep(Duration::from_millis(10));
            }
            // No speed sample yet
            make_wind_sample(&mut monitor, 10.0, 90.0);
            // Wind buffer should remain empty
            assert_eq!(monitor.winds.len(), 0);
        }

        #[test]
        fn test_wind_sample_ignored_if_speed_outdated() {
            let config = VesselStatusConfig::default();
            let mut monitor = VesselMonitor::new(config);
            // Add position samples
            for _ in 0..10 {
                let position_msg = PositionRapidUpdate {
                    pgn: 129025,
                    latitude: 45.0,
                    longitude: -122.0,
                };
                monitor.process_position(&position_msg);
                std::thread::sleep(Duration::from_millis(10));
            }
            // Add a speed sample, but wait so it becomes outdated
            make_speed_sample(&mut monitor, 5.0);
            std::thread::sleep(Duration::from_secs(6)); // >5s, so speed sample is outdated
            make_wind_sample(&mut monitor, 10.0, 90.0);
            // Wind buffer should remain empty
            assert_eq!(monitor.winds.len(), 0);
        }

        #[test]
        fn test_wind_rolling_window() {
            let config = VesselStatusConfig::default();
            let mut monitor = VesselMonitor::new(config);
            // Add position samples
            for _ in 0..10 {
                let position_msg = PositionRapidUpdate {
                    pgn: 129025,
                    latitude: 45.0,
                    longitude: -122.0,
                };
                monitor.process_position(&position_msg);
            }
            // Add a speed sample
            make_speed_sample(&mut monitor, 5.0);
            // Add wind samples, then manually set their timestamps to simulate old samples
            use std::time::Instant;
            let now = Instant::now();
            for i in 0..15 {
                make_wind_sample(&mut monitor, 10.0 + i as f64, 45.0);
                // Set the timestamp of the last wind sample to simulate it being old
                if let Some(last) = monitor.winds.back_mut() {
                    last.timestamp = now - Duration::from_secs(700 + i * 10); // spread over 700..840s ago
                }
            }
            // Add another wind sample with a current timestamp
            make_wind_sample(&mut monitor, 20.0, 90.0);
            // Only recent samples should remain (<= 10 min + 30s buffer)
            let cutoff = now - Duration::from_secs(600) - Duration::from_secs(30);
            let all_recent = monitor.winds.iter().all(|w| w.timestamp >= cutoff);
            assert!(all_recent);
        }
    use super::*;
    use nmea2k::pgns::{PositionRapidUpdate, CogSogRapidUpdate};

    #[test]
    fn test_vessel_status_creation() {
        let config = VesselStatusConfig::default();
        let mut monitor = VesselMonitor::new(config);
        // Add position samples to allow status generation
        for _ in 0..10 {
            let position_msg = PositionRapidUpdate {
                pgn: 129025,
                latitude: 45.0,
                longitude: -122.0,
            };
            monitor.process_position(&position_msg);
        }
        // Add a speed sample (required for true wind calculation)
        let boat_speed_kn = 5.0;
        make_speed_sample(&mut monitor, boat_speed_kn);

        // Add several wind samples
        let wind_samples = vec![(10.0, 45.0), (12.0, 50.0), (8.0, 40.0)];
        let mut expected_speeds = Vec::new();
        let mut expected_angles = Vec::new();
        for (ws, wa) in &wind_samples {
            let (tw_speed, tw_angle) = crate::utilities::calculate_true_wind(*ws, *wa, boat_speed_kn);
            expected_speeds.push(tw_speed);
            expected_angles.push(crate::utilities::normalize0_360(tw_angle));
            make_wind_sample(&mut monitor, *ws, *wa);
        }
        // Force last_event_time to the past to allow status generation
        monitor.last_event_time = std::time::Instant::now() - EVENT_INTERVAL - Duration::from_secs(1);
        let status = monitor.generate_status().unwrap();
        // Wind statistics should be present
        assert!(status.wind_speed_kn.is_some());
        assert!(status.wind_angle_deg.is_some());
        let speed = status.wind_speed_kn.unwrap();
        let angle = status.wind_angle_deg.unwrap();
        let expected_speed = expected_speeds.iter().sum::<f64>() / expected_speeds.len() as f64;
        let expected_angle = expected_angles.iter().sum::<f64>() / expected_angles.len() as f64;
        // Allow a small margin for floating point error
        println!("Buffer wind values:");
        for (i, w) in monitor.winds.iter().enumerate() {
            println!("  {}: speed {:.3} angle {:.3}", i, w.wind_speed_kn, w.wind_angle_deg);
        }
        println!("Expected avg speed: {:.3}, got: {:.3}", expected_speed, speed);
        println!("Expected avg angle: {:.3}, got: {:.3}", expected_angle, angle);
        assert!((speed - expected_speed).abs() < 0.01, "Expected {}, got {}", expected_speed, speed);
        assert!((angle - expected_angle).abs() < 0.1, "Expected {}, got {}", expected_angle, angle);
    }
    #[test]
    fn test_process_position() {
        let config = VesselStatusConfig::default();
        let mut monitor = VesselMonitor::new(config);
        
        // Add 10 positions to meet minimum requirement
        for _ in 0..10 {
            let position_msg = PositionRapidUpdate {
                pgn: 129025,
                latitude: 45.0,
                longitude: -122.0,
            };
            monitor.process_position(&position_msg);
            std::thread::sleep(Duration::from_millis(50));
        }
        
        // Add one more position which should be accepted
        let position_msg = PositionRapidUpdate {
            pgn: 129025,
            latitude: 45.0,
            longitude: -122.0,
        };
        monitor.process_position(&position_msg);
        
        assert_eq!(monitor.positions.len(), 11);
        let pos = monitor.positions.back().unwrap().position;
        assert_eq!(pos.latitude, 45.0);
        assert_eq!(pos.longitude, -122.0);
    }

    #[test]
    fn test_process_cog_sog() {
        let config = VesselStatusConfig::default();
        let mut monitor = VesselMonitor::new(config);
        
        // Create a valid COG/SOG message using from_bytes
        let data = vec![
            0x01, // SID
            0x00, // COG reference (true)
            0xB8, 0x22, // COG = 8888 * 0.0001 rad ≈ 50.9°
            0xF4, 0x01, // SOG = 500 * 0.01 = 5.0 m/s
            0x00, 0x00, // Reserved
        ];
        let cog_sog_msg = CogSogRapidUpdate::from_bytes(&data).unwrap();
        
        monitor.process_cog_sog(&cog_sog_msg);
        
        assert_eq!(monitor.speeds.len(), 1);
    }

    #[test]
    fn test_noise_filter_rejects_high_sog() {
        let config = VesselStatusConfig::default();
        let mut monitor = VesselMonitor::new(config);
        
        // Try to add a speed sample > 25 knots (should be rejected)
        let data_high = vec![
            0x01, 0x00,
            0xB8, 0x22, // COG
            0x10, 0x27, // SOG = 10000 * 0.01 = 100 m/s (~194 knots) - unrealistic
            0x00, 0x00,
        ];
        let cog_sog_msg = CogSogRapidUpdate::from_bytes(&data_high).unwrap();
        monitor.process_cog_sog(&cog_sog_msg);
        
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
        monitor.process_cog_sog(&cog_sog_msg_valid);
        
        // Speed buffer should have one sample
        assert_eq!(monitor.speeds.len(), 1);
    }

    #[test]
    fn test_noise_filter_rejects_distant_position() {
        let config = VesselStatusConfig::default();
        let mut monitor = VesselMonitor::new(config);
        
        // Add several positions at approximately the same location
        // Need at least 10 samples for validation to work
        for _ in 0..10 {
            let position_msg = PositionRapidUpdate {
                pgn: 129025,
                latitude: 45.0,
                longitude: -122.0,
            };
            monitor.process_position(&position_msg);
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
        monitor.process_position(&distant_position);
        
        // Should still have 10 positions (distant one rejected)
        assert_eq!(monitor.positions.len(), 10);
        
        // Add a position close to the median (< 100m)
        let close_position = PositionRapidUpdate {
            pgn: 129025,
            latitude: 45.0001, // ~11 meters away
            longitude: -122.0,
        };
        monitor.process_position(&close_position);
        
        // Should now have 11 positions (close one accepted)
        assert_eq!(monitor.positions.len(), 11);
    }

    #[test]
    fn test_noise_filter_requires_minimum_samples() {
        let config = VesselStatusConfig::default();
        let mut monitor = VesselMonitor::new(config);
        
        // Add only 5 positions (less than minimum required) - these should be accepted during bootstrap
        for _ in 0..5 {
            let position_msg = PositionRapidUpdate {
                pgn: 129025,
                latitude: 45.0,
                longitude: -122.0,
            };
            monitor.process_position(&position_msg);
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
            monitor.process_position(&position_msg);
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
        monitor.process_position(&distant_position);
        
        // Should still have 15 positions (distant one rejected)
        assert_eq!(monitor.positions.len(), 15);
    }

    #[test]
    fn test_mooring_detection_stationary() {
        let config = VesselStatusConfig::default();
        let mut monitor = VesselMonitor::new(config);
        
        // Add multiple positions at the same location over time
        let position_msg = PositionRapidUpdate {
            pgn: 129025,
            latitude: 45.0,
            longitude: -122.0,
        };
        
        // Add 15 positions with delays to ensure we have enough samples
        for _ in 0..15 {
            monitor.process_position(&position_msg);
            std::thread::sleep(Duration::from_millis(50));
        }
        
        let is_moored = monitor.is_vessel_moored();
        // Should detect mooring (all positions within small radius)
        assert!(is_moored);
        // Should have at least 10 samples accepted
        assert!(monitor.positions.len() >= 10);
    }

    #[test]
    fn test_vessel_status_generation() {
        let config = VesselStatusConfig::default();
        let mut monitor = VesselMonitor::new(config);
        
        // Add enough position samples to meet minimum requirement
        for _ in 0..10 {
            let position_msg = PositionRapidUpdate {
                pgn: 129025,
                latitude: 45.0,
                longitude: -122.0,
            };
            monitor.process_position(&position_msg);
            std::thread::sleep(Duration::from_millis(50));
        }
        
        let data = vec![
            0x01, 0x00,
            0xB8, 0x22, // COG
            0xC8, 0x00, // SOG = 200 * 0.01 = 2.0 m/s
            0x00, 0x00,
        ];
        let cog_sog_msg = CogSogRapidUpdate::from_bytes(&data).unwrap();
        monitor.process_cog_sog(&cog_sog_msg);
        
        // Wait for event interval (10 seconds)
        std::thread::sleep(EVENT_INTERVAL + Duration::from_millis(100));
        
        let status = monitor.generate_status();
        assert!(status.is_some());
    }
}
