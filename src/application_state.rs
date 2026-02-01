use std::time::Instant;

use chrono::{DateTime, Utc};

use crate::{config::Config, vessel_monitor::Position};

#[derive(Debug)]
pub struct ApplicationState {
    pub last_gnss_timestamp: Option<DateTime<Utc>>,
    pub last_position: Option<Position>,
    pub last_median_position: Option<Position>,
    pub last_position_timestamp: Option<Instant>,
    pub last_heading_deg: Option<f64>, // in degrees
    pub last_heading_timestamp: Option<Instant>,
    pub config: Config
}

impl ApplicationState {
    pub fn new(config: Config) -> Self {
        ApplicationState {
            last_gnss_timestamp: None,
            last_position: None,
            last_median_position: None,
            last_position_timestamp: None,
            last_heading_deg: None, // in degrees
            last_heading_timestamp: None,
            config,
        }
    }

    pub fn update_gnss_timestamp(&mut self, timestamp: DateTime<Utc>) {
        self.last_gnss_timestamp = Some(timestamp);
    }

    pub fn update_position(&mut self, position: Position, median_position: Position, timestamp: Instant) {
        self.last_position = Some(position);
        self.last_median_position = Some(median_position);
        self.last_position_timestamp = Some(timestamp);
    }

    pub fn update_heading(&mut self, heading_deg: f64, timestamp: Instant) {
        self.last_heading_deg = Some(heading_deg);
        self.last_heading_timestamp = Some(timestamp);
    }
}
