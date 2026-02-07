use std::{collections::VecDeque, time::{Duration, Instant}};

use crate::utilities::Sample;

const MINUM_NUMBER_OF_SAMPLES_FOR_MOORING_DETECTION: usize = 100;

#[derive(Debug, Clone)]
pub struct MooringDetectionQueue {
    pub samples: VecDeque<Sample<f64>>,
    pub max_duration: Duration,
    samples_below_threshold: usize,
    samples_total: usize,
    accuracy: f64,
    vmg_threshold_knots: f64,

}

impl MooringDetectionQueue {
    pub fn new(max_duration: Duration, accuracy: f64, vmg_threshold_knots: f64) -> Self {
        MooringDetectionQueue {
            samples: VecDeque::new(),
            max_duration,
            samples_below_threshold: 0,
            samples_total: 0,
            accuracy,
            vmg_threshold_knots,
        }
    }

    pub fn add_sample(&mut self, value: f64, timestamp: Instant) {
        let sample = Sample { value, timestamp };
        self.samples.push_back(sample);
        if value.abs() < self.vmg_threshold_knots {
            self.samples_below_threshold += 1;
        }
        self.samples_total += 1;
        self.cleanup_old_samples();
    }

    fn cleanup_old_samples(&mut self) {
        let now = Instant::now();
        while let Some(front) = self.samples.front() {
            if now.duration_since(front.timestamp) > self.max_duration {
                let old_sample = self.samples.pop_front().unwrap();
                if old_sample.value.abs() < self.vmg_threshold_knots {
                    self.samples_below_threshold -= 1;
                }
                self.samples_total -= 1;
            } else {
                break;
            }
        }
    }

    pub fn is_stationary(&self, _now: Instant) -> bool {
        if self.samples_total < MINUM_NUMBER_OF_SAMPLES_FOR_MOORING_DETECTION {
            return false;
        }
        let ratio = self.samples_below_threshold as f64 / self.samples_total as f64;
        ratio >= self.accuracy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_new() {
        let queue = MooringDetectionQueue::new(
            Duration::from_secs(300),
            0.90,
            0.5,
        );
        assert_eq!(queue.samples.len(), 0);
        assert_eq!(queue.samples_below_threshold, 0);
        assert_eq!(queue.samples_total, 0);
        assert_eq!(queue.accuracy, 0.90);
        assert_eq!(queue.vmg_threshold_knots, 0.5);
        assert_eq!(queue.max_duration, Duration::from_secs(300));
    }

    #[test]
    fn test_add_sample_below_threshold() {
        let mut queue = MooringDetectionQueue::new(
            Duration::from_secs(60),
            0.90,
            0.5,
        );
        let now = Instant::now();
        
        queue.add_sample(0.3, now); // Below threshold
        assert_eq!(queue.samples_total, 1);
        assert_eq!(queue.samples_below_threshold, 1);
        assert_eq!(queue.samples.len(), 1);
    }

    #[test]
    fn test_add_sample_above_threshold() {
        let mut queue = MooringDetectionQueue::new(
            Duration::from_secs(60),
            0.90,
            0.5,
        );
        let now = Instant::now();
        
        queue.add_sample(1.0, now); // Above threshold
        assert_eq!(queue.samples_total, 1);
        assert_eq!(queue.samples_below_threshold, 0);
        assert_eq!(queue.samples.len(), 1);
    }

    #[test]
    fn test_add_sample_negative_below_threshold() {
        let mut queue = MooringDetectionQueue::new(
            Duration::from_secs(60),
            0.90,
            0.5,
        );
        let now = Instant::now();
        
        queue.add_sample(-0.3, now); // Negative but abs below threshold
        assert_eq!(queue.samples_total, 1);
        assert_eq!(queue.samples_below_threshold, 1);
    }

    #[test]
    fn test_add_multiple_samples() {
        let mut queue = MooringDetectionQueue::new(
            Duration::from_secs(60),
            0.90,
            0.5,
        );
        let now = Instant::now();
        
        queue.add_sample(0.2, now);
        queue.add_sample(0.8, now);
        queue.add_sample(0.1, now);
        queue.add_sample(1.2, now);
        queue.add_sample(0.4, now);
        
        assert_eq!(queue.samples_total, 5);
        assert_eq!(queue.samples_below_threshold, 3); // 0.2, 0.1, 0.4
        assert_eq!(queue.samples.len(), 5);
    }

    #[test]
    fn test_is_stationary_insufficient_samples() {
        let mut queue = MooringDetectionQueue::new(
            Duration::from_secs(60),
            0.90,
            0.5,
        );
        let now = Instant::now();
        
        // Add fewer than MINUM_NUMBER_OF_SAMPLES_FOR_MOORING_DETECTION (100) samples
        for _ in 0..50 {
            queue.add_sample(0.2, now);
        }
        
        assert_eq!(queue.samples_total, 50);
        assert!(!queue.is_stationary(now)); // Should return false
    }

    #[test]
    fn test_is_stationary_sufficient_samples_stationary() {
        let mut queue = MooringDetectionQueue::new(
            Duration::from_secs(600),
            0.90,
            0.5,
        );
        let now = Instant::now();
        
        // Add 100 samples, 95 below threshold (95% accuracy)
        for i in 0..100 {
            if i < 95 {
                queue.add_sample(0.2, now); // Below threshold
            } else {
                queue.add_sample(1.0, now); // Above threshold
            }
        }
        
        assert_eq!(queue.samples_total, 100);
        assert_eq!(queue.samples_below_threshold, 95);
        assert!(queue.is_stationary(now)); // 95% >= 90% accuracy
    }

    #[test]
    fn test_is_stationary_sufficient_samples_not_stationary() {
        let mut queue = MooringDetectionQueue::new(
            Duration::from_secs(600),
            0.90,
            0.5,
        );
        let now = Instant::now();
        
        // Add 100 samples, only 85 below threshold (85% accuracy)
        for i in 0..100 {
            if i < 85 {
                queue.add_sample(0.2, now); // Below threshold
            } else {
                queue.add_sample(1.0, now); // Above threshold
            }
        }
        
        assert_eq!(queue.samples_total, 100);
        assert_eq!(queue.samples_below_threshold, 85);
        assert!(!queue.is_stationary(now)); // 85% < 90% accuracy
    }

    #[test]
    fn test_is_stationary_exact_threshold() {
        let mut queue = MooringDetectionQueue::new(
            Duration::from_secs(600),
            0.90,
            0.5,
        );
        let now = Instant::now();
        
        // Add 100 samples, exactly 90 below threshold (90% accuracy)
        for i in 0..100 {
            if i < 90 {
                queue.add_sample(0.2, now); // Below threshold
            } else {
                queue.add_sample(1.0, now); // Above threshold
            }
        }
        
        assert_eq!(queue.samples_total, 100);
        assert_eq!(queue.samples_below_threshold, 90);
        assert!(queue.is_stationary(now)); // 90% >= 90% accuracy
    }

    #[test]
    fn test_cleanup_old_samples() {
        let mut queue = MooringDetectionQueue::new(
            Duration::from_millis(100),
            0.90,
            0.5,
        );
        let now = Instant::now();
        
        // Add samples that will become old
        for _ in 0..5 {
            queue.add_sample(0.2, now); // Below threshold
        }
        for _ in 0..3 {
            queue.add_sample(1.0, now); // Above threshold
        }
        
        assert_eq!(queue.samples_total, 8);
        assert_eq!(queue.samples_below_threshold, 5);
        
        // Wait for samples to become old
        std::thread::sleep(Duration::from_millis(150));
        
        // Add new sample to trigger cleanup
        queue.add_sample(0.3, Instant::now());
        
        // Old samples should be removed, only the new one remains
        assert_eq!(queue.samples_total, 1);
        assert_eq!(queue.samples_below_threshold, 1);
        assert_eq!(queue.samples.len(), 1);
    }

    #[test]
    fn test_cleanup_mixed_samples() {
        let mut queue = MooringDetectionQueue::new(
            Duration::from_millis(100),
            0.90,
            0.5,
        );
        let start = Instant::now();
        
        // Add some old samples
        queue.add_sample(0.2, start);
        queue.add_sample(1.0, start);
        queue.add_sample(0.3, start);
        
        // Wait a bit
        std::thread::sleep(Duration::from_millis(60));
        
        // Add recent samples
        let recent = Instant::now();
        queue.add_sample(0.1, recent);
        queue.add_sample(1.5, recent);
        
        assert_eq!(queue.samples_total, 5);
        assert_eq!(queue.samples_below_threshold, 3); // 0.2, 0.3, 0.1
        
        // Wait for old samples to expire
        std::thread::sleep(Duration::from_millis(60));
        
        // Add another sample to trigger cleanup
        queue.add_sample(0.4, Instant::now());
        
        // Only recent samples should remain (plus the new one)
        assert_eq!(queue.samples_total, 3); // 0.1, 1.5, 0.4
        assert_eq!(queue.samples_below_threshold, 2); // 0.1, 0.4
    }

    #[test]
    fn test_zero_vmg_is_below_threshold() {
        let mut queue = MooringDetectionQueue::new(
            Duration::from_secs(60),
            0.90,
            0.5,
        );
        let now = Instant::now();
        
        queue.add_sample(0.0, now);
        assert_eq!(queue.samples_below_threshold, 1);
    }

    #[test]
    fn test_boundary_vmg_values() {
        let mut queue = MooringDetectionQueue::new(
            Duration::from_secs(60),
            0.90,
            0.5,
        );
        let now = Instant::now();
        
        // Exactly at threshold (0.5)
        queue.add_sample(0.5, now);
        assert_eq!(queue.samples_below_threshold, 0); // abs(0.5) is not < 0.5
        
        // Just below threshold
        queue.add_sample(0.49, now);
        assert_eq!(queue.samples_below_threshold, 1);
        
        // Just above threshold
        queue.add_sample(0.51, now);
        assert_eq!(queue.samples_below_threshold, 1); // Still 1
    }
}
