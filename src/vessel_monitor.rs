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
        let all_within_threshold = recent_positions
            .iter()
            .all(|p| p.position.distance_to(&avg_position) <= MOORING_THRESHOLD_METERS);

        all_within_threshold
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
            writeln!(f, "║ Position:     {:.6}° N, {:.6}° E", pos.latitude, pos.longitude)?;
        } else {
            writeln!(f, "║ Position:     Unknown                              ║")?;
        }
        
        writeln!(f, "║ Avg Speed:    {:.2} m/s ({:.2} knots)            ║", 
                 self.average_speed_30s, self.average_speed_30s * 1.94384)?;
        writeln!(f, "║ Max Speed:    {:.2} m/s ({:.2} knots)            ║", 
                 self.max_speed_30s, self.max_speed_30s * 1.94384)?;
        writeln!(f, "║ Status:       {}                              ║", 
                 if self.is_moored { "⚓ MOORED  " } else { "⛵ UNDERWAY" })?;
        writeln!(f, "╚════════════════════════════════════════════════════╝")
    }
}
