use core::f64;
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

    #[allow(dead_code)]
    pub fn is_stationary(&self, time_window: Duration, accuracy: f64, threshold_meters: f64, now: Instant) -> bool {
        let cutoff = now - time_window;
   
        // Get positions from the last X minutes        
        let mut iter = self.samples.iter().rev();
        let mut recent_positions: Vec<&PositionSample> = Vec::new();
        let mut sum_lat: f64 = f64::NAN;
        let mut sum_lon: f64 = f64::NAN;
        let mut count = 0;
        while let Some(sample) = iter.clone().next() {
            if sample.timestamp < cutoff {
                break;
            }
            recent_positions.push(sample);
            if sum_lat.is_nan() {
                sum_lat = sample.position.latitude;
            } else {
                sum_lat += sample.position.latitude;
            }
            if sum_lon.is_nan() {
                sum_lon = sample.position.longitude;
            } else {
                sum_lon += sample.position.longitude;
            }
            count += 1;
            iter.next();
        }

        if count < 2 {
            // not enough positions to determine stationary
            return false;
        }

        // Calculate the average position
        let avg_lat = sum_lat / count as f64;
        let avg_lon = sum_lon / count as f64;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_position_distance() {
        let pos1 = Position { latitude: 45.0, longitude: -122.0 };
        let pos2 = Position { latitude: 45.0, longitude: -122.0 };
        assert!(pos1.distance_to_nm(&pos2) < 0.001); // Same position

        let pos3 = Position { latitude: 46.0, longitude: -122.0 };
        assert!(pos1.distance_to_nm(&pos3) > 59.0 && pos1.distance_to_nm(&pos3) < 61.0); // ~60nm
    }

    #[test]
    fn test_position_queue_new() {
        let queue = PositionQueue::new(Duration::from_secs(60));
        assert_eq!(queue.len(), 0);
        assert_eq!(queue.max_duration, Duration::from_secs(60));
    }

    #[test]
    fn test_position_queue_add_sample() {
        let mut queue = PositionQueue::new(Duration::from_secs(60));
        let pos = Position { latitude: 45.0, longitude: -122.0 };
        let now = Instant::now();
        
        queue.add_sample(pos, now);
        assert_eq!(queue.len(), 1);
        
        let latest = queue.get_latest_position().unwrap();
        assert_eq!(latest.latitude, 45.0);
        assert_eq!(latest.longitude, -122.0);
    }

    #[test]
    fn test_position_queue_get_latest() {
        let mut queue = PositionQueue::new(Duration::from_secs(60));
        assert!(queue.get_latest_position().is_none());
        assert!(queue.get_latest_position_timestamp().is_none());
        
        let pos1 = Position { latitude: 45.0, longitude: -122.0 };
        let now1 = Instant::now();
        queue.add_sample(pos1, now1);
        
        std::thread::sleep(Duration::from_millis(10));
        
        let pos2 = Position { latitude: 46.0, longitude: -123.0 };
        let now2 = Instant::now();
        queue.add_sample(pos2, now2);
        
        let latest = queue.get_latest_position().unwrap();
        assert_eq!(latest.latitude, 46.0);
        assert_eq!(latest.longitude, -123.0);
        
        let latest_ts = queue.get_latest_position_timestamp().unwrap();
        assert!(latest_ts >= now2);
    }

    #[test]
    fn test_position_queue_rolling_median() {
        let mut queue = PositionQueue::new(Duration::from_secs(60));
        let now = Instant::now();
        
        // Add 5 positions with slight variations around a center point
        for i in 0..5 {
            let pos = Position {
                latitude: 45.0 + (i as f64) * 0.001,
                longitude: -122.0 + (i as f64) * 0.001,
            };
            queue.add_sample(pos, now);
        }
        
        let (count, median) = queue.get_rolling_median_position(Duration::from_secs(10), 3, now);
        assert_eq!(count, 5);
        assert!(median.is_some());
        
        let med_pos = median.unwrap();
        // Median position should be somewhere near the center of the distribution
        // The implementation finds the median by distance from mean, so exact value depends on that
        assert!(med_pos.latitude >= 45.0 && med_pos.latitude <= 45.004);
        assert!(med_pos.longitude >= -122.0 && med_pos.longitude <= -121.996);
    }

    #[test]
    fn test_position_queue_rolling_median_insufficient_samples() {
        let mut queue = PositionQueue::new(Duration::from_secs(60));
        let now = Instant::now();
        
        // Add only 2 positions
        queue.add_sample(Position { latitude: 45.0, longitude: -122.0 }, now);
        queue.add_sample(Position { latitude: 45.001, longitude: -122.001 }, now);
        
        let (count, median) = queue.get_rolling_median_position(Duration::from_secs(10), 5, now);
        assert_eq!(count, 2);
        assert!(median.is_none()); // Not enough samples
    }

    #[test]
    fn test_position_queue_is_stationary() {
        let mut queue = PositionQueue::new(Duration::from_secs(180));
        let now = Instant::now();
        
        // Add multiple positions at nearly the same location with small variations
        let variations = [0.00001, -0.00001, 0.00002, -0.00002, 0.00001, -0.00001, 0.00002, -0.00002, 0.0, 0.0];
        for i in 0..10 {
            let pos = Position {
                latitude: 45.0 + variations[i], // ~1m variation
                longitude: -122.0 + variations[i],
            };
            queue.add_sample(pos, now);
        }
        
        // Should detect as stationary with 30m threshold
        assert!(queue.is_stationary(Duration::from_secs(180), 0.90, 30.0, now));
    }

    #[test]
    fn test_position_queue_not_stationary() {
        let mut queue = PositionQueue::new(Duration::from_secs(180));
        let now = Instant::now();
        
        // Add positions with large variations (moving)
        for i in 0..10 {
            let pos = Position {
                latitude: 45.0 + (i as f64) * 0.001, // ~100m between each
                longitude: -122.0,
            };
            queue.add_sample(pos, now);
        }
        
        // Should not detect as stationary with 30m threshold
        assert!(!queue.is_stationary(Duration::from_secs(180), 0.90, 30.0, now));
    }

    #[test]
    fn test_position_queue_is_stationary_insufficient_samples() {
        let mut queue = PositionQueue::new(Duration::from_secs(60));
        let now = Instant::now();
        
        // Add only 1 position
        queue.add_sample(Position { latitude: 45.0, longitude: -122.0 }, now);
        
        // Should return false with < 2 samples
        assert!(!queue.is_stationary(Duration::from_secs(60), 0.90, 30.0, now));
    }

    #[test]
    fn test_position_queue_cleanup_old_samples() {
        let mut queue = PositionQueue::new(Duration::from_millis(100));
        let now = Instant::now();
        
        // Add a position
        queue.add_sample(Position { latitude: 45.0, longitude: -122.0 }, now);
        assert_eq!(queue.len(), 1);
        
        // Wait for it to become old
        std::thread::sleep(Duration::from_millis(150));
        
        // Add another position (should trigger cleanup)
        queue.add_sample(Position { latitude: 46.0, longitude: -123.0 }, Instant::now());
        
        // Old sample should be removed
        assert_eq!(queue.len(), 1);
        let latest = queue.get_latest_position().unwrap();
        assert_eq!(latest.latitude, 46.0);
    }
}