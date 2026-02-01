/// Utility functions for NMEA2000 router

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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