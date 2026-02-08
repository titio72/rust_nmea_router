/// Utility functions for NMEA2000 router

use std::{collections::VecDeque, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

use chrono::{DateTime, Datelike};
use time::Date;
use world_magnetic_model::{GeomagneticField, uom::si::{angle::degree, f32::{Angle, Length}, length::meter}};

/// Calculate true wind speed and angle from apparent wind and boat speed.
/// 
/// # Arguments
/// * `apparent_wind_speed_kn` - Apparent wind speed in knots
/// * `apparent_wind_angle_deg` - Apparent wind angle in degrees (relative to bow)
/// * `boat_speed_kn` - Boat speed in knots
/// 
/// # Returns
/// Tuple of (true wind speed in knots, true wind angle in degrees)
pub fn calculate_true_wind(
    apparent_wind_speed_kn: f64,
    apparent_wind_angle_deg: f64,
    boat_speed_kn: f64,
) -> (f64, f64) {

    if boat_speed_kn.abs() < 0.2 {
        // If boat speed is negligible, true wind = apparent wind
        return (apparent_wind_speed_kn, apparent_wind_angle_deg);
    }

    let awa_rad = apparent_wind_angle_deg.to_radians();
    let aws = apparent_wind_speed_kn;
    let bs = boat_speed_kn;

    // Resolve apparent wind into components
    let aw_x = aws * awa_rad.cos();
    let aw_y = aws * awa_rad.sin();

    // Subtract boat speed from the x component
    let tw_x = aw_x - bs;
    let tw_y = aw_y;

    // Calculate true wind speed and angle
    let tw_speed = (tw_x.powi(2) + tw_y.powi(2)).sqrt();
    let tw_angle_rad = tw_y.atan2(tw_x);
    let tw_angle_deg = tw_angle_rad.to_degrees();

    (tw_speed, tw_angle_deg)
}

pub fn dirty_instant_to_systemtime(instant: Instant) -> SystemTime {
    let now_instant = Instant::now();
    let now_systemtime = SystemTime::now();
    if instant <= now_instant {
        let duration_ago = now_instant.duration_since(instant);
        now_systemtime.checked_sub(duration_ago).unwrap_or(UNIX_EPOCH)
    } else {
        let duration_ahead = instant.duration_since(now_instant);
        now_systemtime.checked_add(duration_ahead).unwrap_or(SystemTime::UNIX_EPOCH + Duration::from_secs(u64::MAX))
    }
}

// given two anles in degrees, compute the smallest difference between a and b (i.e., a - b)
pub fn angle_diff(a: f64, b: f64) -> f64 {
    let mut xx = ((a - b) % 360.0 + 360.0) % 360.0;
    if xx > 180.0 {
        xx = xx - 360.0;
    } else if xx < -180.0 {
        xx = xx + 360.0;
    }
    xx
}

pub fn normalize0_360(angle: f64) -> f64 {
    (angle % 360.0 + 360.0) % 360.0
}

pub fn average_angle(angles_deg: &[f64]) -> f64 {
    let mut x = 0.0;
    let mut y = 0.0;
    for w in angles_deg {
        let radians = w.to_radians();
        x += radians.cos();
        y += radians.sin();
    }
    let avg_radians = y.atan2(x);
    (avg_radians.to_degrees() + 360.0) % 360.0
}

/// Calculate the initial heading (bearing) from position1 to position2 using the haversine formula.
/// All lat/lon values are in degrees. Returns heading in degrees (0 = North, 90 = East).
pub fn haversine_heading(lat1_deg: f64, lon1_deg: f64, lat2_deg: f64, lon2_deg: f64) -> f64 {
    let lat1_rad = lat1_deg.to_radians();
    let lat2_rad = lat2_deg.to_radians();
    let dlon_rad = (lon2_deg - lon1_deg).to_radians();

    let y = dlon_rad.sin() * lat2_rad.cos();
    let x = lat1_rad.cos() * lat2_rad.sin() - lat1_rad.sin() * lat2_rad.cos() * dlon_rad.cos();
    let initial_bearing = y.atan2(x).to_degrees();
    (initial_bearing + 360.0) % 360.0
}

pub fn haversine_distance_nm(lat1_deg: f64, lon1_deg: f64, lat2_deg: f64, lon2_deg: f64) -> f64 {
    let radius_earth_nm = 3440.065; // Earth's radius in nautical miles

    let dlat_rad = (lat2_deg - lat1_deg).to_radians();
    let dlon_rad = (lon2_deg - lon1_deg).to_radians();

    let a = (dlat_rad / 2.0).sin().powi(2)
        + lat1_deg.to_radians().cos() * lat2_deg.to_radians().cos() * (dlon_rad / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();

    radius_earth_nm * c
}

#[derive(Debug)]
pub enum VariationError {
    InvalidDate,
    MagneticFieldError,
}

pub fn get_variation_deg(lat_deg: f64, lon_deg: f64, timestamp: DateTime<chrono::Utc>) -> Result<f64, VariationError> {
    let date = Date::from_ordinal_date(timestamp.year(), timestamp.ordinal() as u16)
        .map_err(|_| VariationError::InvalidDate)?;

    let geomagnetic_field_result = GeomagneticField::new(
        Length::new::<meter>(0.0),
        Angle::new::<degree>(lat_deg as f32),
        Angle::new::<degree>(lon_deg as f32),
        date,
    );

    let declination = geomagnetic_field_result
        .map_err(|_| VariationError::MagneticFieldError)?
        .declination()
        .get::<degree>() as f64;

    Ok(declination)
}

#[derive(Debug, Clone)]
pub struct Sample<T> {
    pub value: T,
    pub timestamp: Instant,
}

#[derive(Debug, Clone)]
pub struct TimedQueue<T> {
    pub samples: VecDeque<Sample<T>>,
    pub max_duration: Duration,
}

impl<T> TimedQueue<T> {
    pub fn new(max_duration: Duration) -> Self {
        TimedQueue {
            samples: VecDeque::new(),
            max_duration,
        }
    }

    pub fn add_sample(&mut self, value: T, timestamp: Instant) {
        let sample = Sample { value, timestamp };
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
}

impl<> TimedQueue<f64>  {
    pub fn get_average(&self, time_window: Duration, now: Instant) -> Option<f64> {

        let recent_values: Vec<&f64> = self.samples
            .iter()
            .rev()
            .take_while(|s| s.timestamp >= now - time_window)
            .map(|s| &s.value)
            .collect();

        if recent_values.is_empty() {
            return None;
        }

        let sum: f64 = recent_values.iter().copied().sum();
        let count = recent_values.len() as f64;
        Some(sum / count)
    }

    pub fn get_latest_sample(&self) -> Option<&Sample<f64>> {
        self.samples.back()
    }
    
    pub fn get_latest(&self) -> Option<f64> {
        self.samples.back().map(|s| s.value)
    }

    pub fn get_latest_timestamp(&self) -> Option<Instant> {
        self.samples.back().map(|s| s.timestamp)
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    pub fn get_max(&self, time_window: Duration, now: Instant) -> Option<f64> {
        let recent_values: Vec<&f64> = self.samples
            .iter()
            .rev()
            .take_while(|s| s.timestamp >= now - time_window)
            .map(|s| &s.value)
            .collect();
        
        recent_values.into_iter().max_by(|a, b| a.partial_cmp(b).unwrap()).cloned()
    }

    pub fn get_min(&self, time_window: Duration, now: Instant) -> Option<f64> {
        let recent_values: Vec<&f64> = self.samples
            .iter()
            .rev()
            .take_while(|s| s.timestamp >= now - time_window)
            .map(|s| &s.value)
            .collect();
        
        recent_values.into_iter().min_by(|a, b| a.partial_cmp(b).unwrap()).cloned()
    }

    pub fn get_rolling_median(&self, time_window: Duration, min_num_samples: usize, now: Instant) -> (usize, Option<f64>) {
        let recent_values: Vec<&f64> = self.samples
            .iter()
            .rev()
            .take_while(|s| s.timestamp >= now - time_window)
            .map(|s| &s.value)
            .collect();
        
        if recent_values.len() < min_num_samples {
            return (recent_values.len(), None);
        }

        let mut sorted_values: Vec<f64> = recent_values.iter().map(|&&v| v).collect();
        sorted_values.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let median = if sorted_values.len() % 2 == 0 {
            let mid = sorted_values.len() / 2;
            (sorted_values[mid - 1] + sorted_values[mid]) / 2.0
        } else {
            sorted_values[sorted_values.len() / 2]
        };

        (recent_values.len(), Some(median))
    }

    pub fn get_average_as_angle_deg(&self, time_window: Duration, now: Instant) -> Option<f64> {
        let recent_angles: Vec<f64> = self.samples
            .iter()
            .rev()
            .take_while(|s| s.timestamp >= now - time_window)
            .map(|s| s.value)
            .collect();
        
        if recent_angles.is_empty() {
            return None;
        }

        Some(average_angle(&recent_angles))
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;
    
    #[test]
    fn test_true_wind_zero_boat_speed() {
        // If boat speed is zero, true wind = apparent wind
        let (tw_speed, tw_angle) = calculate_true_wind(10.0, 45.0, 0.0);
        assert!((tw_speed - 10.0).abs() < 1e-6);
        assert!((tw_angle - 45.0).abs() < 1e-6);
    }

    #[test]
    fn test_true_wind_headwind() {
        // Apparent wind directly ahead, boat moving forward
        let (tw_speed, tw_angle) = calculate_true_wind(15.0, 0.0, 5.0);
        // True wind should be less than apparent wind, still from ahead
        assert!((tw_speed - 10.0).abs() < 1e-6);
        assert!((tw_angle - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_true_wind_beam_reach() {
        // Apparent wind from starboard (90 deg), boat moving forward
        let (tw_speed, tw_angle) = calculate_true_wind(12.0, 90.0, 6.0);
        // True wind should be greater than apparent wind, angle > 90
        assert!(tw_speed > 12.0);
        assert!(tw_angle > 90.0); // because the wind move towards bow, so the true angle is wider than 90
    }

    #[test]
    fn test_true_wind_apparent_behind() {
        // Apparent wind from behind (180 deg), boat moving forward
        let (tw_speed, tw_angle) = calculate_true_wind(8.0, 180.0, 4.0);
        // True wind should be greater than apparent wind, angle near 180
        assert!(tw_speed > 8.0);
        assert!((tw_angle - 180.0).abs() < 1e-6);
    }

    #[test]
    fn test_true_wind_negative_angle() {
        // Apparent wind from port (-45 deg), boat moving forward
        let (tw_speed, tw_angle) = calculate_true_wind(10.0, -45.0, 5.0);
        // True wind angle should be negative, speed should be positive
        assert!(tw_speed > 0.0);
        assert!(tw_angle < 0.0);
    }

    #[test]
    fn test_angle_diff() {
        assert_abs_diff_eq!(angle_diff(0.0, 0.0), 0.0);
        assert_abs_diff_eq!(angle_diff(10.0, 20.0), -10.0);
        assert_abs_diff_eq!(angle_diff(350.0, 340.0), 10.0);
        assert_abs_diff_eq!(angle_diff(10.0, 350.0), 20.0);
        assert_abs_diff_eq!(angle_diff(350.0, 10.0), -20.0);
        assert_abs_diff_eq!(angle_diff(90.0, 270.0), 180.0);
        assert_abs_diff_eq!(angle_diff(271.0, 90.0), -179.0);
    }

    #[test]
    fn test_normalize0_360() {
        assert!((normalize0_360(370.0) - 10.0).abs() < 1e-6);
        assert!((normalize0_360(-10.0) - 350.0).abs() < 1e-6);
        assert!((normalize0_360(720.0) - 0.0).abs() < 1e-6);
    }   

    #[test]
    fn test_average_angle() {
        let angles = vec![90.0_f64, 180.0_f64];
        let avg_angle = average_angle(&angles);
        assert!((avg_angle - 135.0).abs() < 1e-6);
    }
    
    #[test]
    fn test_average_angle_cross_north() {
        let angles = vec![5.1_f64, 355.1_f64, 10.1_f64, 350.1_f64];
        let avg_angle = average_angle(&angles);
        assert!((avg_angle - 0.1).abs() < 1e-6);
    }

    // TimedQueue tests
    #[test]
    fn test_timed_queue_new() {
        let queue: TimedQueue<f64> = TimedQueue::new(Duration::from_secs(60));
        assert_eq!(queue.len(), 0);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_timed_queue_add_sample() {
        let mut queue: TimedQueue<f64> = TimedQueue::new(Duration::from_secs(60));
        let now = Instant::now();
        
        queue.add_sample(10.0, now);
        assert_eq!(queue.len(), 1);
        assert!(!queue.is_empty());
        
        queue.add_sample(20.0, now);
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn test_timed_queue_get_latest() {
        let mut queue: TimedQueue<f64> = TimedQueue::new(Duration::from_secs(60));
        assert!(queue.get_latest().is_none());
        
        let now = Instant::now();
        queue.add_sample(10.0, now);
        assert_eq!(queue.get_latest().unwrap(), 10.0);
        
        queue.add_sample(20.0, now);
        assert_eq!(queue.get_latest().unwrap(), 20.0);
    }

    #[test]
    fn test_timed_queue_get_latest_timestamp() {
        let mut queue: TimedQueue<f64> = TimedQueue::new(Duration::from_secs(60));
        let now1 = Instant::now();
        queue.add_sample(10.0, now1);
        
        std::thread::sleep(Duration::from_millis(10));
        let now2 = Instant::now();
        queue.add_sample(20.0, now2);
        
        let ts = queue.get_latest_timestamp().unwrap();
        assert!(ts >= now2);
    }

    #[test]
    fn test_timed_queue_clear() {
        let mut queue: TimedQueue<f64> = TimedQueue::new(Duration::from_secs(60));
        let now = Instant::now();
        
        queue.add_sample(10.0, now);
        queue.add_sample(20.0, now);
        assert_eq!(queue.len(), 2);
        
        queue.clear();
        assert_eq!(queue.len(), 0);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_timed_queue_get_average() {
        let mut queue: TimedQueue<f64> = TimedQueue::new(Duration::from_secs(60));
        let now = Instant::now();
        
        queue.add_sample(10.0, now);
        queue.add_sample(20.0, now);
        queue.add_sample(30.0, now);
        
        let avg = queue.get_average(Duration::from_secs(10), now).unwrap();
        assert!((avg - 20.0).abs() < 0.001);
    }

    #[test]
    fn test_timed_queue_get_average_empty() {
        let queue: TimedQueue<f64> = TimedQueue::new(Duration::from_secs(60));
        let now = Instant::now();
        assert!(queue.get_average(Duration::from_secs(10), now).is_none());
    }

    #[test]
    fn test_timed_queue_get_max() {
        let mut queue: TimedQueue<f64> = TimedQueue::new(Duration::from_secs(60));
        let now = Instant::now();
        
        queue.add_sample(10.0, now);
        queue.add_sample(25.0, now);
        queue.add_sample(15.0, now);
        
        let max = queue.get_max(Duration::from_secs(10), now).unwrap();
        assert_eq!(max, 25.0);
    }

    #[test]
    fn test_timed_queue_get_min() {
        let mut queue: TimedQueue<f64> = TimedQueue::new(Duration::from_secs(60));
        let now = Instant::now();
        
        queue.add_sample(10.0, now);
        queue.add_sample(5.0, now);
        queue.add_sample(15.0, now);
        
        let min = queue.get_min(Duration::from_secs(10), now).unwrap();
        assert_eq!(min, 5.0);
    }

    #[test]
    fn test_timed_queue_get_rolling_median_odd() {
        let mut queue: TimedQueue<f64> = TimedQueue::new(Duration::from_secs(60));
        let now = Instant::now();
        
        queue.add_sample(10.0, now);
        queue.add_sample(20.0, now);
        queue.add_sample(30.0, now);
        queue.add_sample(40.0, now);
        queue.add_sample(50.0, now);
        
        let (count, median) = queue.get_rolling_median(Duration::from_secs(10), 3, now);
        assert_eq!(count, 5);
        assert_eq!(median.unwrap(), 30.0);
    }

    #[test]
    fn test_timed_queue_get_rolling_median_even() {
        let mut queue: TimedQueue<f64> = TimedQueue::new(Duration::from_secs(60));
        let now = Instant::now();
        
        queue.add_sample(10.0, now);
        queue.add_sample(20.0, now);
        queue.add_sample(30.0, now);
        queue.add_sample(40.0, now);
        
        let (count, median) = queue.get_rolling_median(Duration::from_secs(10), 3, now);
        assert_eq!(count, 4);
        assert_eq!(median.unwrap(), 25.0); // (20 + 30) / 2
    }

    #[test]
    fn test_timed_queue_get_rolling_median_insufficient_samples() {
        let mut queue: TimedQueue<f64> = TimedQueue::new(Duration::from_secs(60));
        let now = Instant::now();
        
        queue.add_sample(10.0, now);
        queue.add_sample(20.0, now);
        
        let (count, median) = queue.get_rolling_median(Duration::from_secs(10), 5, now);
        assert_eq!(count, 2);
        assert!(median.is_none());
    }

    #[test]
    fn test_timed_queue_get_average_as_angle() {
        let mut queue: TimedQueue<f64> = TimedQueue::new(Duration::from_secs(60));
        let now = Instant::now();
        
        // Test averaging angles around 0 degrees
        queue.add_sample(350.0, now);
        queue.add_sample(10.0, now);
        
        let avg = queue.get_average_as_angle_deg(Duration::from_secs(10), now).unwrap();
        // Should be close to 0 or 360
        assert!(avg < 5.0 || avg > 355.0, "Expected avg near 0/360, got {}", avg);
    }

    #[test]
    fn test_timed_queue_cleanup_old_samples() {
        let mut queue: TimedQueue<f64> = TimedQueue::new(Duration::from_millis(100));
        let now = Instant::now();
        
        queue.add_sample(10.0, now);
        assert_eq!(queue.len(), 1);
        
        // Wait for sample to become old
        std::thread::sleep(Duration::from_millis(150));
        
        // Add new sample (should trigger cleanup)
        queue.add_sample(20.0, Instant::now());
        
        // Old sample should be removed
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.get_latest().unwrap(), 20.0);
    }

    #[test]
    fn test_timed_queue_time_window_filtering() {
        let mut queue: TimedQueue<f64> = TimedQueue::new(Duration::from_secs(60));
        let now = Instant::now();
        
        // Add samples at different times
        queue.add_sample(10.0, now - Duration::from_secs(20));
        queue.add_sample(20.0, now - Duration::from_secs(10));
        queue.add_sample(30.0, now);
        
        // Get average for last 15 seconds (should only include last 2 samples)
        let avg = queue.get_average(Duration::from_secs(15), now).unwrap();
        assert!((avg - 25.0).abs() < 0.001); // (20 + 30) / 2
        
        // Get average for last 5 seconds (should only include last sample)
        let avg = queue.get_average(Duration::from_secs(5), now).unwrap();
        assert_eq!(avg, 30.0);
    }


}

/// Cleanup task to remove exported trip files older than 7 days
/// This function runs as a background task and checks every 24 hours
pub async fn cleanup_old_exports() {
    use std::path::Path;
    use std::fs;
    use tracing::{info, warn, error};

    let mut interval = tokio::time::interval(Duration::from_secs(24 * 60 * 60)); // 24 hours
    let seven_days = Duration::from_secs(7 * 24 * 60 * 60); // 7 days
    
    loop {
        interval.tick().await;
        
        let export_dir = Path::new("static/exports");
        
        // Skip cleanup if directory doesn't exist
        if !export_dir.exists() {
            continue;
        }
        
        let now = SystemTime::now();
        let mut deleted_count = 0;
        let mut deleted_size = 0u64;
        
        match fs::read_dir(export_dir) {
            Ok(entries) => {
                for entry in entries {
                    if let Ok(entry) = entry {
                        let path = entry.path();
                        
                        // Only process JSON files
                        if path.is_file() && path.extension().map(|ext| ext == "json").unwrap_or(false) {
                            if let Ok(metadata) = path.metadata() {
                                if let Ok(modified) = metadata.modified() {
                                    if let Ok(age) = now.duration_since(modified) {
                                        if age > seven_days {
                                            let file_size = metadata.len();
                                            
                                            match fs::remove_file(&path) {
                                                Ok(_) => {
                                                    info!(
                                                        path = %path.display(),
                                                        age_days = age.as_secs() / (24 * 60 * 60),
                                                        size_bytes = file_size,
                                                        "Deleted expired export file"
                                                    );
                                                    deleted_count += 1;
                                                    deleted_size += file_size;
                                                }
                                                Err(e) => {
                                                    warn!(
                                                        path = %path.display(),
                                                        error = %e,
                                                        "Failed to delete export file"
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                
                if deleted_count > 0 {
                    info!(
                        deleted_files = deleted_count,
                        freed_bytes = deleted_size,
                        "Cleanup completed: removed old export files"
                    );
                }
            }
            Err(e) => {
                error!(
                    path = %export_dir.display(),
                    error = %e,
                    "Failed to read exports directory during cleanup"
                );
            }
        }
    }
}