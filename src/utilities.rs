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
    samples: VecDeque<Sample<T>>,
    max_duration: Duration,
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


}