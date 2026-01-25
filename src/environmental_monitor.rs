use std::collections::VecDeque;
use std::time::{Duration, Instant};

use nmea2k::pgns::{WindData, Temperature, Humidity, ActualPressure, Attitude};
use crate::config::EnvironmentalConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MetricId {
    Pressure = 1,
    CabinTemp = 2,
    WaterTemp = 3,
    Humidity = 4,
    WindSpeed = 5,
    WindDir = 6,
    Roll = 7,
}

impl MetricId {
    pub fn as_u8(&self) -> u8 {
        *self as u8
    }

    pub fn as_index(&self) -> usize {
        match self {
            MetricId::Pressure => 0,
            MetricId::CabinTemp => 1,
            MetricId::WaterTemp => 2,
            MetricId::Humidity => 3,
            MetricId::WindSpeed => 4,
            MetricId::WindDir => 5,
            MetricId::Roll => 6,
        }
    }
    
    pub fn unit(&self) -> &'static str {
        match self {
            MetricId::Pressure => "Pa",
            MetricId::CabinTemp => "C",
            MetricId::WaterTemp => "C",
            MetricId::Humidity => "%",
            MetricId::WindSpeed => "m/s",
            MetricId::WindDir => "deg",
            MetricId::Roll => "deg",
        }
    }
    
    #[allow(dead_code)]
    pub fn name(&self) -> &'static str {
        match self {
            MetricId::Pressure => "pressure",
            MetricId::CabinTemp => "cabin_temp",
            MetricId::WaterTemp => "water_temp",
            MetricId::Humidity => "humidity",
            MetricId::WindSpeed => "wind_speed",
            MetricId::WindDir => "wind_dir",
            MetricId::Roll => "roll",
        }
    }

    pub const ALL_METRICS: [MetricId; 7] = [
        MetricId::Pressure,
        MetricId::CabinTemp,
        MetricId::WaterTemp,
        MetricId::Humidity,
        MetricId::WindSpeed,
        MetricId::WindDir,
        MetricId::Roll,
    ];
}

#[derive(Debug, Clone)]
pub struct MetricData {
    pub avg: Option<f64>,
    pub max: Option<f64>,
    pub min: Option<f64>,
    pub count: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Sample<T> {
    pub value: T,
    #[allow(dead_code)]
    pub timestamp: Instant,
}

pub struct EnvironmentalMonitor {
    db_periods: [Duration; 7],
    pub data_samples: [VecDeque<Sample<f64>>; 7],
}

impl EnvironmentalMonitor {
    pub fn new(config: EnvironmentalConfig) -> Self {
        let res = Self {
            data_samples: [
                VecDeque::new(), // Pressure    
                VecDeque::new(), // CabinTemp
                VecDeque::new(), // WaterTemp
                VecDeque::new(), // Humidity
                VecDeque::new(), // WindSpeed
                VecDeque::new(), // WindDir
                VecDeque::new(), // Roll
            ],
            db_periods: [
                config.wind_speed_interval(),
                config.wind_direction_interval(),
                config.roll_interval(),
                config.pressure_interval(),
                config.cabin_temp_interval(),
                config.water_temp_interval(),
                config.humidity_interval(),
            ],
        };
        res
    }

    /// Process a temperature message (PGN 130312)
    /// Instance 0 is typically the cabin temperature (and source 4 is "Inside Ambient")
    pub fn process_temperature(&mut self, temp: &Temperature) {
        if temp.instance == 0 { // Cabin temperature
            let now = Instant::now();
            let celsius = temp.temperature - 273.15;
            let source = temp.source;
            let instance = temp.instance;
            
            if source==4 && instance==0 {
                // Source 4 is "Inside Ambient"
                self.data_samples[MetricId::CabinTemp.as_index()].push_back(Sample {
                    value: celsius,
                    timestamp: now,
                });
            } else if source==0 && instance==0 {
                // Source 0 is water temperature
                self.data_samples[MetricId::WaterTemp.as_index()].push_back(Sample {
                    value: celsius,
                    timestamp: now,
                });
            }
        }
    }

    /// Process wind data message (PGN 130306)
    pub fn process_wind(&mut self, wind: &WindData) {
        let now = Instant::now();
        
        // Store wind speed
        self.data_samples[MetricId::WindSpeed.as_index()].push_back(Sample {
            value: wind.speed,
            timestamp: now,
        });
        
        // Store wind direction (convert radians to degrees)
        let degrees = wind.angle.to_degrees();
        self.data_samples[MetricId::WindDir.as_index()].push_back(Sample {
            value: degrees,
            timestamp: now,
        });
    }
    
    /// Process a humidity message (PGN 130313)
    /// Standalone humidity sensor reading
    pub fn process_humidity(&mut self, hum: &Humidity) {
        let now = Instant::now();
        
        self.data_samples[MetricId::Humidity.as_index()].push_back(Sample {
            value: hum.actual_humidity,
            timestamp: now,
        });
    }
    
    /// Process an actual pressure message (PGN 130314)
    /// Standalone pressure sensor reading
    pub fn process_actual_pressure(&mut self, pressure: &ActualPressure) {
        let now = Instant::now();
        let instance = pressure.instance;
        let source = pressure.source;

        if instance == 0 && source == 0 {
            // Primary atmospheric pressure sensor
            self.data_samples[MetricId::Pressure.as_index()].push_back(Sample {
                value: pressure.pressure,
                timestamp: now,
            });
        }
    }
    
    /// Process an attitude message (PGN 127257)
    /// Extract roll angle in degrees
    pub fn process_attitude(&mut self, attitude: &Attitude) {
        if let Some(roll_deg) = attitude.roll_degrees() {
            let now = Instant::now();
            
            self.data_samples[MetricId::Roll.as_index()].push_back(Sample {
                value: roll_deg,
                timestamp: now,
            });
        }
    }

    pub fn cleanup_all_samples(&mut self, metric_id: MetricId) {
        self.data_samples[metric_id.as_index()].clear();
    }

    pub fn calculate_metric_data(&self, metric_id: MetricId) -> Option<MetricData> {
        let samples = &self.data_samples[metric_id.as_index()];
        self.calculate(samples)
    }

    fn calculate(&self, samples: &VecDeque<Sample<f64>>) -> Option<MetricData> {
        if samples.is_empty() {
            return None;
        }
        let mut avg: f64 = 0.0;
        // Initialize with the first value to handle negative numbers or non-zero baselines correctly
        let first_val = samples[0].value;
        let mut max: f64 = first_val;
        let mut min: f64 = first_val;
        
        let count = samples.len() as f64;
        for sample in samples.iter() {
            avg += sample.value;
            if sample.value > max {
                max = sample.value;
            }
            if sample.value < min {
                min = sample.value;
            }
        }
        Some(MetricData {
            avg: Some(avg / count),
            max: Some(max),
            min: Some(min),
            count: Some(samples.len()),
        })
    }
    
    /// Check if there are samples for a specific metric
    pub fn has_samples(&self, metric: MetricId) -> bool {
        !self.data_samples[metric.as_index()].is_empty()
    }
    
    /// Get the database persistence periods for all metrics
    pub fn db_periods(&self) -> [Duration; 7] {
        self.db_periods
    }
}

impl Default for EnvironmentalMonitor {
    fn default() -> Self {
        Self::new(crate::config::EnvironmentalConfig::default())
    }
}

impl nmea2k::MessageHandler for EnvironmentalMonitor {
    fn handle_message(&mut self, message: &nmea2k::N2kMessage) {
        match message {
            nmea2k::pgns::N2kMessage::Temperature(temp) => {
                self.process_temperature(temp);
            }
            nmea2k::pgns::N2kMessage::WindData(wind) => {
                self.process_wind(wind);
            }
            nmea2k::pgns::N2kMessage::Humidity(hum) => {
                self.process_humidity(hum);
            }
            nmea2k::pgns::N2kMessage::ActualPressure(pressure) => {
                self.process_actual_pressure(pressure);
            }
            nmea2k::pgns::N2kMessage::Attitude(attitude) => {
                self.process_attitude(attitude);
            }
            _ => {} // Ignore messages we're not interested in
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_id_as_u8() {
        assert_eq!(MetricId::Pressure.as_u8(), 1);
        assert_eq!(MetricId::CabinTemp.as_u8(), 2);
        assert_eq!(MetricId::WaterTemp.as_u8(), 3);
        assert_eq!(MetricId::Humidity.as_u8(), 4);
        assert_eq!(MetricId::WindSpeed.as_u8(), 5);
        assert_eq!(MetricId::WindDir.as_u8(), 6);
        assert_eq!(MetricId::Roll.as_u8(), 7);
    }

    #[test]
    fn test_metric_id_unit() {
        assert_eq!(MetricId::Pressure.unit(), "Pa");
        assert_eq!(MetricId::CabinTemp.unit(), "C");
        assert_eq!(MetricId::WaterTemp.unit(), "C");
        assert_eq!(MetricId::Humidity.unit(), "%");
        assert_eq!(MetricId::WindSpeed.unit(), "m/s");
        assert_eq!(MetricId::WindDir.unit(), "deg");
        assert_eq!(MetricId::Roll.unit(), "deg");
    }

    #[test]
    fn test_metric_id_name() {
        assert_eq!(MetricId::Pressure.name(), "pressure");
        assert_eq!(MetricId::CabinTemp.name(), "cabin_temp");
        assert_eq!(MetricId::WaterTemp.name(), "water_temp");
        assert_eq!(MetricId::Humidity.name(), "humidity");
        assert_eq!(MetricId::WindSpeed.name(), "wind_speed");
        assert_eq!(MetricId::WindDir.name(), "wind_dir");
        assert_eq!(MetricId::Roll.name(), "roll");
    }

    #[test]
    fn test_environmental_monitor_creation() {
        let config = EnvironmentalConfig::default();
        let monitor = EnvironmentalMonitor::new(config);
        assert_eq!(monitor.data_samples[MetricId::Pressure.as_index()].len(), 0);
        assert_eq!(monitor.data_samples[MetricId::CabinTemp.as_index()].len(), 0);
    }

    #[test]
    fn test_process_pressure() {
        let config = EnvironmentalConfig::default();
        let mut monitor = EnvironmentalMonitor::new(config);
        
        // Create pressure message using from_bytes: 101325 Pa (1 atm)
        let data = vec![
            0x01, // SID
            0x00, // Instance
            0x00, // Source
            0x0D, 0x8B, 0x01, 0x00, // Pressure = 101325 Pa
        ];
        let pressure_msg = ActualPressure::from_bytes(&data).unwrap();
        
        monitor.process_actual_pressure(&pressure_msg);
        assert_eq!(monitor.data_samples[MetricId::Pressure.as_index()].len(), 1);
    }

    #[test]
    fn test_process_temperature_cabin() {
        let config = EnvironmentalConfig::default();
        let mut monitor = EnvironmentalMonitor::new(config);
        
        // Create temperature message: 20.5°C = 293.65 K
        // Source must be 4 (Inside Ambient) for cabin temp
        let data = vec![
            0x01, // SID
            0x00, // Instance (cabin)
            0x04, // Source = 4 (Inside Ambient)
            0x25, 0x72, // Temperature = 29285 * 0.01 = 292.85 K ≈ 19.7°C
            0x00, // Padding to reach 6 bytes
        ];
        let temp_msg = Temperature::from_bytes(&data).unwrap();
        
        monitor.process_temperature(&temp_msg);
        assert_eq!(monitor.data_samples[MetricId::CabinTemp.as_index()].len(), 1);
    }

    #[test]
    fn test_process_temperature_water() {
        let config = EnvironmentalConfig::default();
        let mut monitor = EnvironmentalMonitor::new(config);
        
        // Create temperature message: 15.5°C = 288.65 K
        // Source must be 0 (Water) and instance=0 for water temp
        let data = vec![
            0x01, // SID
            0x00, // Instance = 0
            0x00, // Source = 0 (Water)
            0xD9, 0x70, // Temperature = 28889 * 0.01 = 288.89 K ≈ 15.74°C
            0x00, // Padding to reach 6 bytes
        ];
        let temp_msg = Temperature::from_bytes(&data).unwrap();
        
        monitor.process_temperature(&temp_msg);
        assert_eq!(monitor.data_samples[MetricId::WaterTemp.as_index()].len(), 1);
    }

    #[test]
    fn test_process_humidity() {
        let config = EnvironmentalConfig::default();
        let mut monitor = EnvironmentalMonitor::new(config);
        
        // Create humidity message: 65.0%
        // Need at least 6 bytes for Humidity::from_bytes
        let data = vec![
            0x01, // SID
            0x00, // Instance
            0x00, // Source
            0x22, 0x40, // Humidity = 16418 * 0.004 = 65.672% ≈ 65%
            0x00, 0x00, // Padding to reach 6+ bytes
        ];
        let humidity_msg = Humidity::from_bytes(&data).unwrap();
        
        monitor.process_humidity(&humidity_msg);
        assert_eq!(monitor.data_samples[MetricId::Humidity.as_index()].len(), 1);
    }

    #[test]
    fn test_process_wind() {
        let config = EnvironmentalConfig::default();
        let mut monitor = EnvironmentalMonitor::new(config);
        
        // Create wind message: 5.5 m/s, 180° (pi radians)
        let data = vec![
            0x01, // SID
            0x26, 0x02, // Speed = 550 * 0.01 = 5.5 m/s
            0x54, 0x7B, // Angle = 31572 * 0.0001 rad ≈ 180.9°
            0x02, // Reference (Apparent)
        ];
        let wind_msg = WindData::from_bytes(&data).unwrap();
        
        monitor.process_wind(&wind_msg);
        assert_eq!(monitor.data_samples[MetricId::WindSpeed.as_index()].len(), 1);
        assert_eq!(monitor.data_samples[MetricId::WindDir.as_index()].len(), 1);
    }

    #[test]
    fn test_process_attitude_roll() {
        let config = EnvironmentalConfig::default();
        let mut monitor = EnvironmentalMonitor::new(config);
        
        let attitude_msg = Attitude::from_bytes(&vec![
            0x01,
            0x00, 0x00,
            0x00, 0x00,
            0xE8, 0x03, // Roll = 1000 * 0.0001 = 0.1 rad ≈ 5.73°
        ]).unwrap();
        
        monitor.process_attitude(&attitude_msg);
        assert_eq!(monitor.data_samples[MetricId::Roll.as_index()].len(), 1);
    }

    #[test]
    fn test_metric_data_all_none() {
        let data = MetricData {
            avg: None,
            max: None,
            min: None,
            count: None,
        };
        
        assert!(data.avg.is_none());
        assert!(data.max.is_none());
        assert!(data.min.is_none());
        assert!(data.count.is_none());
    }

    #[test]
    fn test_metric_data_with_values() {
        let data = MetricData {
            avg: Some(20.5),
            max: Some(25.0),
            min: Some(18.0),
            count: Some(10),
        };
        
        assert_eq!(data.avg.unwrap(), 20.5);
        assert_eq!(data.max.unwrap(), 25.0);
        assert_eq!(data.min.unwrap(), 18.0);
        assert_eq!(data.count.unwrap(), 10);
    }
}
