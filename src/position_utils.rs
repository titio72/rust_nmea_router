use std::{collections::VecDeque, time::{Duration, Instant}};

use crate::utilities::haversine_distance_nm;


#[derive(Debug, Clone, Copy)]
pub struct Position {
    pub latitude: f64,
    pub longitude: f64,
}

impl Position {
    /// Returns the distance to another position in nautical miles (using Haversine formula)
    pub fn distance_to_nm(&self, other: &Position) -> f64 {
        haversine_distance_nm(self.latitude, self.longitude, other.latitude, other.longitude)
    }

    pub fn course_from_deg(&self, other: &Position) -> f64 {
        crate::utilities::haversine_heading(self.latitude, self.longitude, other.latitude, other.longitude)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PositionSample {
    pub position: Position,
    pub timestamp: Instant,
}

#[derive(Debug)]
pub struct PositionQueue {
    pub samples: VecDeque<PositionSample>,
    pub max_duration: Duration,
}

impl PositionQueue {

    pub fn new(max_duration: Duration) -> Self {
        PositionQueue {
            samples: VecDeque::new(),
            max_duration,
        }
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn add_sample(&mut self, position: Position, timestamp: Instant) {
        let sample = PositionSample { position, timestamp };
        self.samples.push_back(sample);
        self.cleanup_old_samples();
    }

    fn cleanup_old_samples(&mut self) {
        let now = Instant::now();
        while let Some(front) = self.samples.front() {
            if now.duration_since(front.timestamp) > self.max_duration {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn get_latest_position(&self) -> Option<Position> {
        self.samples.back().map(|s| s.position)
    }

    pub fn get_latest_position_timestamp(&self) -> Option<Instant> {
        self.samples.back().map(|s| s.timestamp)
    }

    pub fn get_rolling_median_position(&self, time_window: Duration, min_num_samples: usize, now: Instant) -> (usize, Option<Position>) {
        let recent_positions: Vec<&Position> = self.samples
            .iter()
            .rev()
            .take_while(|s| s.timestamp >= now - time_window)
            .map(|s| &s.position)
            .collect();
        
        if recent_positions.is_empty() {
            return (0, None);
        }

        let mut lats: Vec<f64> = recent_positions.iter().map(|p| p.latitude).collect();
        let mut lons: Vec<f64> = recent_positions.iter().map(|p| p.longitude).collect();
        
        lats.sort_by(|a, b| a.partial_cmp(b).unwrap());
        lons.sort_by(|a, b| a.partial_cmp(b).unwrap());
        
        if lats.len() < min_num_samples {
            return (lats.len(), None);
        }

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
        
        (lats.len(), Some(Position {
            latitude: median_lat,
            longitude: median_lon,
        }))
    }

    pub fn is_stationary(&self, time_window: Duration, accuracy: f64, threshold_meters: f64, now: Instant) -> bool {
        if self.samples.len() < 2 {
            return false;
        }

        let cutoff = now - time_window;

        // Get positions from the last 2 minutes
        let recent_positions: Vec<&PositionSample> = self
            .samples
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
            .filter(|p| (p.position.distance_to_nm(&avg_position) * 1852.0) <= threshold_meters)
            .count() >= (recent_positions.len() as f64 * accuracy) as usize // At least 90% within threshold
    }
}