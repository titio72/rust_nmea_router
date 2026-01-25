use std::collections::VecDeque;
use std::time::{Duration, Instant};

use nmea2k::pgns::{PositionRapidUpdate, CogSogRapidUpdate};
use crate::config::VesselStatusConfig;

const EVENT_INTERVAL: Duration = Duration::from_secs(10);
const MOORING_DETECTION_WINDOW: Duration = Duration::from_secs(120); // 2 minutes
const MOORING_THRESHOLD_METERS: f64 = 30.0; // 30 meters radius
const MOORING_ACCURACY: f64 = 0.90; // 90% of positions within threshold
const MAX_VALID_SOG_MS: f64 = 12.861; // 25 knots in m/s (noise filter)
const MAX_POSITION_DEVIATION_METERS: f64 = 100.0; // Maximum distance from median (noise filter)
const POSITION_VALIDATION_WINDOW: Duration = Duration::from_secs(10); // Time window for median calculation
const MIN_SAMPLES_FOR_VALIDATION: usize = 10; // Minimum samples required for validation 

#[derive(Debug, Clone)]
pub struct VesselStatus {
    pub current_position: Position,
    pub average_position: Option<Position>,
    pub number_of_samples: usize,
    //pub average_speed: f64,  // m/s
    pub max_speed: f64,       // m/s
    pub is_moored: bool,
    pub engine_on: bool,
    pub timestamp: Instant,
}

impl VesselStatus {
    pub fn get_effective_position(&self) -> Position {
        if self.is_moored { self.current_position } else { self.average_position.unwrap_or(self.current_position) }
    }

    pub fn is_valid(&self) -> bool {
        self.number_of_samples > 0
    }

    pub fn get_total_distance_and_time_from_last_report(&self, last_vessel_status: &mut Option<VesselStatus>) -> (f64, u64) {
        let (total_distance_nm, total_time_ms) = if let Some(ref last_status) = *last_vessel_status {
            let last_sample = PositionSample::from_status(last_status);
            let current_sample = PositionSample::from_status(self);
            let distance = last_sample.distance_to(&current_sample);
            let time = last_sample.time_difference_ms(&current_sample);
            (distance, time)
        } else {
            (0.0, 0)
        };
        (total_distance_nm, total_time_ms)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Position {
    pub latitude: f64,
    pub longitude: f64,
}

impl Position {
    pub fn distance_to(&self, other: &Position) -> f64 {
        // Haversine formula to calculate distance in nautical miles
        let r = 6371000.0; // Earth radius in meters
        let lat1 = self.latitude.to_radians();
        let lat2 = other.latitude.to_radians();
        let delta_lat = (other.latitude - self.latitude).to_radians();
        let delta_lon = (other.longitude - self.longitude).to_radians();

        let a = (delta_lat / 2.0).sin().powi(2)
            + lat1.cos() * lat2.cos() * (delta_lon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

        (r * c) / 1852.0 // Convert meters to nautical miles
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PositionSample {
    pub position: Position,
    pub timestamp: Instant,
}

impl PositionSample {
    pub fn from_status(report: &VesselStatus) -> Self {
        Self {
            position: report.current_position,
            timestamp: report.timestamp,
        }
    }
    
    pub fn distance_to(&self, other: &PositionSample) -> f64 {
        self.position.distance_to(&other.position)
    }

    pub fn time_difference(&self, other: &PositionSample) -> Duration {
        if self.timestamp > other.timestamp {
            self.timestamp - other.timestamp
        } else {
            other.timestamp - self.timestamp
        }
    }

    pub fn time_difference_ms(&self, other: &PositionSample) -> u64 {
        self.time_difference(other).as_millis() as u64
    }
}

struct SpeedSample {
    speed: f64,
    timestamp: Instant,
}

pub struct VesselMonitor {
    positions: VecDeque<PositionSample>,
    speeds: VecDeque<SpeedSample>,
    last_event_time: Instant,
    current_position: Option<Position>,
    engine_on: bool,
}

impl VesselMonitor {
    pub fn new(_config: VesselStatusConfig) -> Self {
        let now = Instant::now();
        VesselMonitor {
            positions: VecDeque::new(),
            speeds: VecDeque::new(),
            last_event_time: now,
            current_position: None,
            engine_on: false,
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

        self.current_position = Some(position);
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
        let sog = cog_sog_msg.sog;
        
        // Noise filter: Reject unrealistic SOG values (> 25 knots)
        if sog > MAX_VALID_SOG_MS {
            return; // Reject noisy speed reading
        }
        
        self.speeds.push_back(SpeedSample {
            speed: sog,
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
        let distance = position.distance_to(&median) * 1852.0; // Convert nm to meters
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
        if !self.should_generate_event() || self.current_position.is_none() {
            return None;
        }

        let (sample_count, average_position) = self.calculate_average_position(EVENT_INTERVAL);
        let (_, _, max_speed) = self.calculate_average_and_max_speed(EVENT_INTERVAL);
        let is_moored = self.is_vessel_moored();

        // Use the timestamp of the last position in the buffer, or current time if no positions
        let timestamp = self.positions.back()
            .map(|sample| sample.timestamp)
            .unwrap_or_else(|| Instant::now());
        
        self.last_event_time = Instant::now();

        Some(VesselStatus {
            current_position: self.current_position.unwrap(),
            average_position,
            number_of_samples: sample_count,
            max_speed: max_speed,
            is_moored,
            engine_on: self.engine_on,
            timestamp,
        })
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
        let mut total_speed = 0.0;
        let mut count = 0;
        let mut max_speed = 0.0;
        for s in iterator {
            if s.timestamp < cutoff {
                break;
            }
            total_speed += s.speed;
            if s.speed > max_speed {
                max_speed = s.speed;
            }
            count += 1;
        }
        let average_speed = if count > 0 {
            total_speed / count as f64
        } else {
            0.0
        };
        (count, average_speed, max_speed)
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
            .filter(|p| p.position.distance_to(&avg_position) <= MOORING_THRESHOLD_METERS)
            .count() >= (recent_positions.len() as f64 * MOORING_ACCURACY) as usize // At least 90% within threshold
    }
}

impl nmea2k::MessageHandler for VesselMonitor {
    fn handle_message(&mut self, message: &nmea2k::N2kMessage) {
        match message {
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
    use super::*;
    use nmea2k::pgns::{PositionRapidUpdate, CogSogRapidUpdate};

    #[test]
    fn test_vessel_monitor_creation() {
        let config = VesselStatusConfig {
            interval_moored_seconds: 600,
            interval_underway_seconds: 30,
        };
        let monitor = VesselMonitor::new(config);
        assert_eq!(monitor.positions.len(), 0);
        assert_eq!(monitor.speeds.len(), 0);
        assert!(monitor.current_position.is_none());
    }

    #[test]
    fn test_position_distance_calculation() {
        // Test distance between two known positions
        let pos1 = Position {
            latitude: 0.0,
            longitude: 0.0,
        };
        let pos2 = Position {
            latitude: 0.0,
            longitude: 0.001, // ~111 meters at equator = ~0.06 nm
        };
        
        let distance = pos1.distance_to(&pos2);
        // Should be approximately 0.06 nautical miles (111 meters / 1852)
        assert!(distance > 0.05 && distance < 0.07);
    }

    #[test]
    fn test_position_same_location() {
        let pos = Position {
            latitude: 45.0,
            longitude: -122.0,
        };
        
        let distance = pos.distance_to(&pos);
        assert!(distance < 0.1); // Should be essentially zero
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
        
        assert!(monitor.current_position.is_some());
        let pos = monitor.current_position.unwrap();
        assert_eq!(pos.latitude, 45.0);
        assert_eq!(pos.longitude, -122.0);
        assert_eq!(monitor.positions.len(), 11);
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
