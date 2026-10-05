use core::f64;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use crate::utilities::haversine_distance_nm;

#[derive(Debug, Clone, Copy)]
pub struct Position {
    pub latitude: f64,
    pub longitude: f64,
}

impl Position {
    /// Returns the distance to another position in nautical miles (using Haversine formula)
    pub fn distance_to_nm(&self, other: &Position) -> f64 {
        haversine_distance_nm(
            self.latitude,
            self.longitude,
            other.latitude,
            other.longitude,
        )
    }

    pub fn course_from_deg(&self, other: &Position) -> f64 {
        crate::utilities::haversine_heading(
            self.latitude,
            self.longitude,
            other.latitude,
            other.longitude,
        )
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

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn add_sample(&mut self, position: Position, timestamp: Instant) {
        let sample = PositionSample {
            position,
            timestamp,
        };
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

    pub fn get_earliest_position_timestamp(&self) -> Option<Instant> {
        self.samples.front().map(|s| s.timestamp)
    }

    pub fn get_rolling_median_position(
        &self,
        time_window: Duration,
        min_num_samples: usize,
        now: Instant,
    ) -> (usize, Option<Position>) {
        let recent_iter = self
            .samples
            .iter()
            .rev()
            .take_while(|s| s.timestamp >= now - time_window);

        let hint = self.samples.len();
        let mut lats: Vec<f64> = Vec::with_capacity(hint);
        let mut lons: Vec<f64> = Vec::with_capacity(hint);
        for s in recent_iter {
            lats.push(s.position.latitude);
            lons.push(s.position.longitude);
        }

        if lats.is_empty() {
            return (0, None);
        }

        lats.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        lons.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        if lats.len() < min_num_samples {
            return (lats.len(), None);
        }

        let mid = lats.len() / 2;
        let median_lat = if lats.len().is_multiple_of(2) {
            (lats[mid - 1] + lats[mid]) / 2.0
        } else {
            lats[mid]
        };

        let median_lon = if lons.len().is_multiple_of(2) {
            (lons[mid - 1] + lons[mid]) / 2.0
        } else {
            lons[mid]
        };

        (
            lats.len(),
            Some(Position {
                latitude: median_lat,
                longitude: median_lon,
            }),
        )
    }

    /// Checks whether the vessel has stayed near a stable reference point.
    ///
    /// The reference point is the median position over `reference_window` (long,
    /// e.g. 10 min) rather than the mean of `check_window` itself: a swing whose
    /// period exceeds `check_window` (e.g. a slow current-driven anchor swing) only
    /// shows part of its arc within a short window, biasing a same-window mean
    /// toward whichever side of the arc it caught and making the boat look like
    /// it's moving away from "itself". A longer, independently-computed reference
    /// isn't affected by that partial-arc bias.
    pub fn is_stationary(
        &self,
        check_window: Duration,
        reference_window: Duration,
        min_reference_samples: usize,
        accuracy: f64,
        threshold_meters: f64,
        now: Instant,
    ) -> bool {
        let (_, reference) =
            self.get_rolling_median_position(reference_window, min_reference_samples, now);
        let Some(reference) = reference else {
            return false; // not enough data yet to establish a reference point
        };

        let cutoff = now - check_window;
        let recent: Vec<&PositionSample> = self
            .samples
            .iter()
            .rev()
            .take_while(|s| s.timestamp >= cutoff)
            .collect();

        if recent.len() < 2 {
            // not enough positions to determine stationary
            return false;
        }

        // Check how many recent positions are within threshold of the reference point
        recent
            .iter()
            .filter(|p| (p.position.distance_to_nm(&reference) * 1852.0) <= threshold_meters)
            .count()
            >= (recent.len() as f64 * accuracy) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_position_distance() {
        let pos1 = Position {
            latitude: 45.0,
            longitude: -122.0,
        };
        let pos2 = Position {
            latitude: 45.0,
            longitude: -122.0,
        };
        assert!(pos1.distance_to_nm(&pos2) < 0.001); // Same position

        let pos3 = Position {
            latitude: 46.0,
            longitude: -122.0,
        };
        assert!(pos1.distance_to_nm(&pos3) > 59.0 && pos1.distance_to_nm(&pos3) < 61.0);
        // ~60nm
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
        let pos = Position {
            latitude: 45.0,
            longitude: -122.0,
        };
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

        let pos1 = Position {
            latitude: 45.0,
            longitude: -122.0,
        };
        let now1 = Instant::now();
        queue.add_sample(pos1, now1);

        std::thread::sleep(Duration::from_millis(10));

        let pos2 = Position {
            latitude: 46.0,
            longitude: -123.0,
        };
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
        queue.add_sample(
            Position {
                latitude: 45.0,
                longitude: -122.0,
            },
            now,
        );
        queue.add_sample(
            Position {
                latitude: 45.001,
                longitude: -122.001,
            },
            now,
        );

        let (count, median) = queue.get_rolling_median_position(Duration::from_secs(10), 5, now);
        assert_eq!(count, 2);
        assert!(median.is_none()); // Not enough samples
    }

    #[test]
    fn test_position_queue_is_stationary() {
        let mut queue = PositionQueue::new(Duration::from_secs(180));
        let now = Instant::now();

        // Add multiple positions at nearly the same location with small variations
        let variations = [
            0.00001, -0.00001, 0.00002, -0.00002, 0.00001, -0.00001, 0.00002, -0.00002, 0.0, 0.0,
        ];
        for variation in variations {
            let pos = Position {
                latitude: 45.0 + variation, // ~1m variation
                longitude: -122.0 + variation,
            };
            queue.add_sample(pos, now);
        }

        // Should detect as stationary with 30m threshold
        assert!(queue.is_stationary(
            Duration::from_secs(180),
            Duration::from_secs(180),
            5,
            0.90,
            30.0,
            now
        ));
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
        assert!(!queue.is_stationary(
            Duration::from_secs(180),
            Duration::from_secs(180),
            5,
            0.90,
            30.0,
            now
        ));
    }

    #[test]
    fn test_position_queue_is_stationary_insufficient_samples() {
        let mut queue = PositionQueue::new(Duration::from_secs(60));
        let now = Instant::now();

        // Add only 1 position
        queue.add_sample(
            Position {
                latitude: 45.0,
                longitude: -122.0,
            },
            now,
        );

        // Should return false with < 2 samples
        assert!(!queue.is_stationary(
            Duration::from_secs(60),
            Duration::from_secs(60),
            2,
            0.90,
            30.0,
            now
        ));
    }

    #[test]
    fn test_position_queue_cleanup_old_samples() {
        let mut queue = PositionQueue::new(Duration::from_millis(100));
        let now = Instant::now();

        // Add a position
        queue.add_sample(
            Position {
                latitude: 45.0,
                longitude: -122.0,
            },
            now,
        );
        assert_eq!(queue.len(), 1);

        // Wait for it to become old
        std::thread::sleep(Duration::from_millis(150));

        // Add another position (should trigger cleanup)
        queue.add_sample(
            Position {
                latitude: 46.0,
                longitude: -123.0,
            },
            Instant::now(),
        );

        // Old sample should be removed
        assert_eq!(queue.len(), 1);
        let latest = queue.get_latest_position().unwrap();
        assert_eq!(latest.latitude, 46.0);
    }
}
