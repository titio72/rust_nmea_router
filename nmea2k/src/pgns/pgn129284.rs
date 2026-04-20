use std::fmt;

use super::nmea2000_date_time::N2kDateTime;

/// Course/bearing reference for navigation calculations.
#[derive(Debug, Clone, PartialEq)]
pub enum CourseBearingReference {
    True,
    Magnetic,
    Error,
    Unknown(u8),
}

impl fmt::Display for CourseBearingReference {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", match self {
            CourseBearingReference::True => "True",
            CourseBearingReference::Magnetic => "Magnetic",
            CourseBearingReference::Error => "Error",
            CourseBearingReference::Unknown(_) => "Unknown",
        })
    }
}

/// Whether the vessel has crossed the perpendicular to the destination waypoint.
#[derive(Debug, Clone, PartialEq)]
pub enum PerpendicularCrossed {
    No,
    Yes,
    Unknown(u8),
}

/// Whether the vessel has entered the arrival circle around the destination waypoint.
#[derive(Debug, Clone, PartialEq)]
pub enum ArrivalCircleEntered {
    No,
    Yes,
    Unknown(u8),
}

/// Route calculation method.
#[derive(Debug, Clone, PartialEq)]
pub enum CalculationType {
    GreatCircle,
    Rhumbline,
    Unknown(u8),
}

/// NMEA2000 PGN 129284 — Navigation Data.
///
/// Provides active leg routing data: distance and bearing to destination waypoint,
/// ETA, closing velocity, and destination coordinates.
///
/// Bearings are in radians (SI). Distance is in metres. Closing velocity is in m/s.
/// Lat/lon are in decimal degrees. Convert at call site as needed.
#[derive(Debug, Clone)]
pub struct NavigationData {
    #[allow(dead_code)]
    pub pgn: u32,
    #[allow(dead_code)]
    pub sid: u8,
    /// Distance to destination waypoint, metres.
    pub distance_to_waypoint: f64,
    pub course_bearing_reference: CourseBearingReference,
    pub perpendicular_crossed: PerpendicularCrossed,
    pub arrival_circle_entered: ArrivalCircleEntered,
    pub calculation_type: CalculationType,
    /// Estimated time of arrival (date + seconds-since-midnight in 0.0001 s units).
    pub eta: N2kDateTime,
    /// Bearing from origin waypoint to destination waypoint, radians.
    pub bearing_origin_to_destination: f64,
    /// Bearing from current position to destination waypoint, radians.
    pub bearing_position_to_destination: f64,
    pub origin_waypoint_number: u32,
    pub destination_waypoint_number: u32,
    /// Destination latitude, decimal degrees (positive = North).
    pub destination_latitude: f64,
    /// Destination longitude, decimal degrees (positive = East).
    pub destination_longitude: f64,
    /// Closing velocity toward destination, m/s (negative = moving away).
    pub waypoint_closing_velocity: f64,
}

impl NavigationData {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 34 {
            return None;
        }

        let sid = data[0];
        let dist_raw = u32::from_le_bytes([data[1], data[2], data[3], data[4]]);
        let flags = data[5];
        let eta_time_raw = u32::from_le_bytes([data[6], data[7], data[8], data[9]]);
        let eta_date = u16::from_le_bytes([data[10], data[11]]);
        let bearing_orig_raw = u16::from_le_bytes([data[12], data[13]]);
        let bearing_pos_raw = u16::from_le_bytes([data[14], data[15]]);
        let origin_wp = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
        let dest_wp = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
        let dest_lat_raw = i32::from_le_bytes([data[24], data[25], data[26], data[27]]);
        let dest_lon_raw = i32::from_le_bytes([data[28], data[29], data[30], data[31]]);
        let closing_vel_raw = i16::from_le_bytes([data[32], data[33]]);

        Some(Self {
            pgn: 129284,
            sid,
            distance_to_waypoint: dist_raw as f64 * 0.01,
            course_bearing_reference: match flags & 0x03 {
                0 => CourseBearingReference::True,
                1 => CourseBearingReference::Magnetic,
                2 => CourseBearingReference::Error,
                v => CourseBearingReference::Unknown(v),
            },
            perpendicular_crossed: match (flags >> 2) & 0x03 {
                0 => PerpendicularCrossed::No,
                1 => PerpendicularCrossed::Yes,
                v => PerpendicularCrossed::Unknown(v),
            },
            arrival_circle_entered: match (flags >> 4) & 0x03 {
                0 => ArrivalCircleEntered::No,
                1 => ArrivalCircleEntered::Yes,
                v => ArrivalCircleEntered::Unknown(v),
            },
            calculation_type: match (flags >> 6) & 0x03 {
                0 => CalculationType::GreatCircle,
                1 => CalculationType::Rhumbline,
                v => CalculationType::Unknown(v),
            },
            eta: N2kDateTime {
                date: eta_date,
                time: eta_time_raw as f64,
            },
            bearing_origin_to_destination: bearing_orig_raw as f64 * 0.0001,
            bearing_position_to_destination: bearing_pos_raw as f64 * 0.0001,
            origin_waypoint_number: origin_wp,
            destination_waypoint_number: dest_wp,
            destination_latitude: dest_lat_raw as f64 * 1e-7,
            destination_longitude: dest_lon_raw as f64 * 1e-7,
            waypoint_closing_velocity: closing_vel_raw as f64 * 0.01,
        })
    }
}

impl fmt::Display for NavigationData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Nav Data: DTW={:.1}m brg={:.4}rad dest={:.6},{:.6} ETA day={} vel={:.2}m/s",
            self.distance_to_waypoint,
            self.bearing_position_to_destination,
            self.destination_latitude,
            self.destination_longitude,
            self.eta.date,
            self.waypoint_closing_velocity,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_test_frame() -> Vec<u8> {
        let mut data = vec![0u8; 34];
        // [0] sid = 1
        data[0] = 0x01;
        // [1..4] distance_to_waypoint: 100.0 m → raw = 10000 = 0x00002710
        data[1..5].copy_from_slice(&10000u32.to_le_bytes());
        // [5] flags: all zero (True, No, No, GreatCircle)
        data[5] = 0x00;
        // [6..9] etaTime raw = 100000 (= 10.0 s at 0.0001 s/unit)
        data[6..10].copy_from_slice(&100000u32.to_le_bytes());
        // [10..11] etaDate = 100 days
        data[10..12].copy_from_slice(&100u16.to_le_bytes());
        // [12..13] bearingOrigin raw = 15708 (≈ 1.5708 rad)
        data[12..14].copy_from_slice(&15708u16.to_le_bytes());
        // [14..15] bearingPosition raw = 10000 (= 1.0000 rad)
        data[14..16].copy_from_slice(&10000u16.to_le_bytes());
        // [16..19] originWaypointNumber = 42
        data[16..20].copy_from_slice(&42u32.to_le_bytes());
        // [20..23] destinationWaypointNumber = 43
        data[20..24].copy_from_slice(&43u32.to_le_bytes());
        // [24..27] destinationLatitude: 45.0° → raw = 450000000
        data[24..28].copy_from_slice(&450000000i32.to_le_bytes());
        // [28..31] destinationLongitude: 9.0° → raw = 90000000
        data[28..32].copy_from_slice(&90000000i32.to_le_bytes());
        // [32..33] waypointClosingVelocity: 5.0 m/s → raw = 500
        data[32..34].copy_from_slice(&500i16.to_le_bytes());
        data
    }

    #[test]
    fn test_navigation_data_from_bytes_known_values() {
        let data = build_test_frame();
        let msg = NavigationData::from_bytes(&data).unwrap();

        assert_eq!(msg.pgn, 129284);
        assert_eq!(msg.sid, 1);
        assert!((msg.distance_to_waypoint - 100.0).abs() < 1e-9);
        assert_eq!(msg.course_bearing_reference, CourseBearingReference::True);
        assert_eq!(msg.perpendicular_crossed, PerpendicularCrossed::No);
        assert_eq!(msg.arrival_circle_entered, ArrivalCircleEntered::No);
        assert_eq!(msg.calculation_type, CalculationType::GreatCircle);
        assert_eq!(msg.eta.date, 100);
        assert!((msg.eta.time - 100000.0).abs() < 1e-9);
        assert!((msg.bearing_origin_to_destination - 1.5708).abs() < 1e-6);
        assert!((msg.bearing_position_to_destination - 1.0).abs() < 1e-9);
        assert_eq!(msg.origin_waypoint_number, 42);
        assert_eq!(msg.destination_waypoint_number, 43);
        assert!((msg.destination_latitude - 45.0).abs() < 1e-6);
        assert!((msg.destination_longitude - 9.0).abs() < 1e-6);
        assert!((msg.waypoint_closing_velocity - 5.0).abs() < 1e-9);
    }

    #[test]
    fn test_navigation_data_flags_magnetic_rhumbline() {
        let mut data = build_test_frame();
        // courseBearingReference=Magnetic(1), perpendicularCrossed=Yes(1),
        // arrivalCircleEntered=Yes(1), calculationType=Rhumbline(1)
        // flags = 0b01_01_01_01 = 0x55
        data[5] = 0x55;
        let msg = NavigationData::from_bytes(&data).unwrap();
        assert_eq!(msg.course_bearing_reference, CourseBearingReference::Magnetic);
        assert_eq!(msg.perpendicular_crossed, PerpendicularCrossed::Yes);
        assert_eq!(msg.arrival_circle_entered, ArrivalCircleEntered::Yes);
        assert_eq!(msg.calculation_type, CalculationType::Rhumbline);
    }

    #[test]
    fn test_navigation_data_negative_lat_lon() {
        let mut data = build_test_frame();
        // destinationLatitude: -33.9° → raw = -339000000
        data[24..28].copy_from_slice(&(-339000000i32).to_le_bytes());
        // destinationLongitude: -70.6° → raw = -706000000
        data[28..32].copy_from_slice(&(-706000000i32).to_le_bytes());
        let msg = NavigationData::from_bytes(&data).unwrap();
        assert!((msg.destination_latitude - (-33.9)).abs() < 1e-5);
        assert!((msg.destination_longitude - (-70.6)).abs() < 1e-5);
    }

    #[test]
    fn test_navigation_data_from_bytes_too_short() {
        let data = vec![0u8; 33];
        assert!(NavigationData::from_bytes(&data).is_none());
    }
}
