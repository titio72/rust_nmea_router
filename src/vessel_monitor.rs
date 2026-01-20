use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::pgns::{PositionRapidUpdate, CogSogRapidUpdate};
use crate::config::VesselStatusConfig;

const EVENT_INTERVAL: Duration = Duration::from_secs(30);
const MOORING_DETECTION_WINDOW: Duration = Duration::from_secs(120); // 2 minutes
const MOORING_THRESHOLD_METERS: f64 = 10.0; // 10 meters radius

#[derive(Debug, Clone)]
pub struct VesselStatus {
    pub current_position: Option<Position>,
    pub average_speed_30s: f64,  // m/s
    pub max_speed_30s: f64,       // m/s
    pub is_moored: bool,
    pub engine_on: bool,
    #[allow(dead_code)]
    pub timestamp: Instant,
}

#[derive(Debug, Clone, Copy)]
pub struct Position {
    pub latitude: f64,
    pub longitude: f64,
}

impl Position {
    fn distance_to(&self, other: &Position) -> f64 {
        // Haversine formula to calculate distance in meters
        let r = 6371000.0; // Earth radius in meters
        let lat1 = self.latitude.to_radians();
        let lat2 = other.latitude.to_radians();
        let delta_lat = (other.latitude - self.latitude).to_radians();
        let delta_lon = (other.longitude - self.longitude).to_radians();

        let a = (delta_lat / 2.0).sin().powi(2)
            + lat1.cos() * lat2.cos() * (delta_lon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

        r * c
    }
}

struct PositionSample {
    position: Position,
    timestamp: Instant,
}

struct SpeedSample {
    speed: f64,
    timestamp: Instant,
}

pub struct VesselMonitor {
    positions: VecDeque<PositionSample>,
    speeds: VecDeque<SpeedSample>,
    last_event_time: Instant,
    last_db_persist_time: Instant,
    current_position: Option<Position>,
    engine_on: bool,
    config: VesselStatusConfig,
}

impl VesselMonitor {
    pub fn new(config: VesselStatusConfig) -> Self {
        Self {
            positions: VecDeque::new(),
            speeds: VecDeque::new(),
            last_event_time: Instant::now(),
            last_db_persist_time: Instant::now(),
            current_position: None,
            engine_on: false,
            config,
        }
    }

    /// Process a position rapid update message
    pub fn process_position(&mut self, position_msg: &PositionRapidUpdate) {
        let now = Instant::now();
        let position = Position {
            latitude: position_msg.latitude,
            longitude: position_msg.longitude,
        };

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
        
        self.speeds.push_back(SpeedSample {
            speed: cog_sog_msg.sog,
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
    pub fn process_engine(&mut self, engine_msg: &crate::pgns::EngineRapidUpdate) {
        self.engine_on = engine_msg.is_engine_running();
    }

    /// Check if it's time to generate a status event
    pub fn should_generate_event(&self) -> bool {
        Instant::now().duration_since(self.last_event_time) >= EVENT_INTERVAL
    }

    /// Check if it's time to persist status to database (adaptive based on mooring state)
    pub fn should_persist_to_db(&self, is_moored: bool) -> bool {
        let now = Instant::now();
        let interval = if is_moored {
            self.config.interval_moored()
        } else {
            self.config.interval_underway()
        };
        now.duration_since(self.last_db_persist_time) >= interval
    }

    /// Mark that we've persisted to the database
    pub fn mark_db_persisted(&mut self) {
        self.last_db_persist_time = Instant::now();
    }

    /// Generate a vessel status event
    pub fn generate_status(&mut self) -> Option<VesselStatus> {
        if !self.should_generate_event() {
            return None;
        }

        let now = Instant::now();
        self.last_event_time = now;

        let average_speed = self.calculate_average_speed(EVENT_INTERVAL);
        let max_speed = self.calculate_max_speed(EVENT_INTERVAL);
        let is_moored = self.is_vessel_moored();

        Some(VesselStatus {
            current_position: self.current_position,
            average_speed_30s: average_speed,
            max_speed_30s: max_speed,
            is_moored,
            engine_on: self.engine_on,
            timestamp: now,
        })
    }

    fn calculate_average_speed(&self, window: Duration) -> f64 {
        let now = Instant::now();
        let cutoff = now - window;

        let speeds: Vec<f64> = self
            .speeds
            .iter()
            .filter(|s| s.timestamp >= cutoff)
            .map(|s| s.speed)
            .collect();

        if speeds.is_empty() {
            0.0
        } else {
            speeds.iter().sum::<f64>() / speeds.len() as f64
        }
    }

    fn calculate_max_speed(&self, window: Duration) -> f64 {
        let now = Instant::now();
        let cutoff = now - window;

        self.speeds
            .iter()
            .filter(|s| s.timestamp >= cutoff)
            .map(|s| s.speed)
            .fold(0.0, f64::max)
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
            .all(|p| p.position.distance_to(&avg_position) <= MOORING_THRESHOLD_METERS)
    }
}

impl Default for VesselMonitor {
    fn default() -> Self {
        Self::new(VesselStatusConfig::default())
    }
}

impl std::fmt::Display for VesselStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "╔════════════════════════════════════════════════════╗")?;
        writeln!(f, "║         VESSEL STATUS REPORT                       ║")?;
        writeln!(f, "╠════════════════════════════════════════════════════╣")?;
        
        if let Some(pos) = self.current_position {
            writeln!(f, "║ Position:     {:+010.6}°, {:+010.6}°             ║", pos.latitude, pos.longitude)?;
        } else {
            writeln!(f, "║ Position:     Unknown                              ║")?;
        }
        
        writeln!(f, "║ Avg Speed:    {:5.2} m/s ({:5.2} knots)              ║", 
                 self.average_speed_30s, self.average_speed_30s * 1.94384)?;
        writeln!(f, "║ Max Speed:    {:5.2} m/s ({:5.2} knots)              ║", 
                 self.max_speed_30s, self.max_speed_30s * 1.94384)?;
        writeln!(f, "║ Status:       {}                          ║", 
                 if self.is_moored { "⚓ MOORED  " } else { "⛵ UNDERWAY" })?;
        writeln!(f, "║ Engine:       {}                              ║", 
                 if self.engine_on { "🟢 ON  " } else { "⚫ OFF " })?;
        writeln!(f, "╚════════════════════════════════════════════════════╝")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pgns::{PositionRapidUpdate, CogSogRapidUpdate};

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
            longitude: 0.001, // ~111 meters at equator
        };
        
        let distance = pos1.distance_to(&pos2);
        // Should be approximately 111 meters
        assert!(distance > 100.0 && distance < 120.0);
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
        assert_eq!(monitor.positions.len(), 1);
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
    fn test_should_persist_moored() {
        let config = VesselStatusConfig {
            interval_moored_seconds: 0, // Set to 0 so it always needs to persist
            interval_underway_seconds: 5,
        };
        let monitor = VesselMonitor::new(config);
        
        // Should persist immediately with 0-second interval
        assert!(monitor.should_persist_to_db(true));
    }

    #[test]
    fn test_should_persist_underway() {
        let config = VesselStatusConfig {
            interval_moored_seconds: 10,
            interval_underway_seconds: 0, // Set to 0 so it always needs to persist
        };
        let monitor = VesselMonitor::new(config);
        
        // Should persist immediately with 0-second interval
        assert!(monitor.should_persist_to_db(false));
    }

    #[test]
    fn test_mark_db_persisted() {
        let config = VesselStatusConfig::default();
        let mut monitor = VesselMonitor::new(config);
        
        let before = monitor.last_db_persist_time;
        std::thread::sleep(Duration::from_millis(10));
        monitor.mark_db_persisted();
        let after = monitor.last_db_persist_time;
        
        assert!(after > before);
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
        
        for _ in 0..10 {
            monitor.process_position(&position_msg);
            std::thread::sleep(Duration::from_millis(10));
        }
        
        let is_moored = monitor.is_vessel_moored();
        // Should detect mooring (all positions within small radius)
        assert!(is_moored);
    }

    #[test]
    fn test_vessel_status_generation() {
        let config = VesselStatusConfig::default();
        let mut monitor = VesselMonitor::new(config);
        
        // Add some data
        let position_msg = PositionRapidUpdate {
            pgn: 129025,
            latitude: 45.0,
            longitude: -122.0,
        };
        monitor.process_position(&position_msg);
        
        let data = vec![
            0x01, 0x00,
            0xB8, 0x22, // COG
            0xC8, 0x00, // SOG = 200 * 0.01 = 2.0 m/s
            0x00, 0x00,
        ];
        let cog_sog_msg = CogSogRapidUpdate::from_bytes(&data).unwrap();
        monitor.process_cog_sog(&cog_sog_msg);
        
        // Wait for event interval
        std::thread::sleep(Duration::from_millis(100));
        
        let status = monitor.generate_status();
        if let Some(status) = status {
            assert!(status.current_position.is_some());
        }
    }
}
