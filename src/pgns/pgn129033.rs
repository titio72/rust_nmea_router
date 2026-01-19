use std::fmt;

#[derive(Debug, Clone)]
pub struct TimeDate {
    pub date: u16, // days since 1970-01-01
    pub time: f64, // seconds since midnight
}

impl TimeDate {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        Some(Self {
            date: u16::from_le_bytes([data[0], data[1]]),
            time: u32::from_le_bytes([data[2], data[3], data[4], data[5]]) as f64 * 0.0001,
        })
    }
}

impl fmt::Display for TimeDate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let hours = (self.time / 3600.0) as u32;
        let minutes = ((self.time % 3600.0) / 60.0) as u32;
        let seconds = (self.time % 60.0) as u32;
        write!(
            f,
            "      Days since epoch: {} | Time: {:02}:{:02}:{:02}",
            self.date, hours, minutes, seconds
        )
    }
}
