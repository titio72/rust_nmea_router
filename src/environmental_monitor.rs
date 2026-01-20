use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::pgns::{WindData, Temperature, Humidity, ActualPressure, Attitude};
use crate::config::EnvironmentalConfig;

const SAMPLE_INTERVAL: Duration = Duration::from_secs(60); // 1 minute

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
}

#[derive(Debug, Clone)]
pub struct MetricData {
    pub avg: Option<f64>,
    pub max: Option<f64>,
    pub min: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct EnvironmentalReport {
    #[allow(dead_code)]
    pub timestamp: Instant,
    pub pressure: MetricData,        // Pascals
    pub cabin_temp: MetricData,      // Celsius
    pub water_temp: MetricData,     // Celsius
    pub humidity: MetricData,        // Percent
    pub wind_speed: MetricData,      // m/s
    pub wind_dir: MetricData,        // degrees
    pub roll: MetricData,            // degrees
}

impl std::fmt::Display for EnvironmentalReport {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "╔═══════════════════════════════════════════════════════════════╗")?;
        writeln!(f, "║         ENVIRONMENTAL DATA REPORT (1-minute average)          ║")?;
        writeln!(f, "╠═══════════════════════════════════════════════════════════════╣")?;
        
        if let (Some(avg), Some(max), Some(min)) = (self.pressure.avg, self.pressure.max, self.pressure.min) {
            writeln!(f, "║  Pressure:   Avg: {:.0} Pa  Max: {:.0} Pa  Min: {:.0} Pa", avg, max, min)?;
        } else {
            writeln!(f, "║  Pressure:   No data")?;
        }
        
        if let (Some(avg), Some(max), Some(min)) = (self.cabin_temp.avg, self.cabin_temp.max, self.cabin_temp.min) {
            writeln!(f, "║  Cabin Temp: Avg: {:.1}°C  Max: {:.1}°C  Min: {:.1}°C", avg, max, min)?;
        } else {
            writeln!(f, "║  Cabin Temp: No data")?;
        }
        
        if let (Some(avg), Some(max), Some(min)) = (self.water_temp.avg, self.water_temp.max, self.water_temp.min) {
            writeln!(f, "║  Water Temp: Avg: {:.1}°C  Max: {:.1}°C  Min: {:.1}°C", avg, max, min)?;
        } else {
            writeln!(f, "║  Water Temp: No data")?;
        }
        
        if let (Some(avg), Some(max), Some(min)) = (self.humidity.avg, self.humidity.max, self.humidity.min) {
            writeln!(f, "║  Humidity:   Avg: {:.1}%  Max: {:.1}%  Min: {:.1}%", avg, max, min)?;
        } else {
            writeln!(f, "║  Humidity:   No data")?;
        }
        
        if let (Some(avg), Some(max), Some(min)) = (self.wind_speed.avg, self.wind_speed.max, self.wind_speed.min) {
            writeln!(f, "║  Wind Speed: Avg: {:.1} m/s  Max: {:.1} m/s  Min: {:.1} m/s", avg, max, min)?;
            writeln!(f, "║              Avg: {:.1} kt   Max: {:.1} kt   Min: {:.1} kt", 
                avg * 1.94384, max * 1.94384, min * 1.94384)?;
        } else {
            writeln!(f, "║  Wind Speed: No data")?;
        }
        
        if let (Some(avg), Some(max), Some(min)) = (self.wind_dir.avg, self.wind_dir.max, self.wind_dir.min) {
            writeln!(f, "║  Wind Dir:   Avg: {:.0}°  Max: {:.0}°  Min: {:.0}°", avg, max, min)?;
        } else {
            writeln!(f, "║  Wind Dir:   No data")?;
        }
        
        if let (Some(avg), Some(max), Some(min)) = (self.roll.avg, self.roll.max, self.roll.min) {
            writeln!(f, "║  Roll:       Avg: {:.1}°  Max: {:.1}°  Min: {:.1}°", avg, max, min)?;
        } else {
            writeln!(f, "║  Roll:       No data")?;
        }
        
        writeln!(f, "╚═══════════════════════════════════════════════════════════════╝")
    }
}

struct Sample<T> {
    value: T,
    timestamp: Instant,
}

pub struct EnvironmentalMonitor {
    pressure_samples: VecDeque<Sample<f64>>,
    cabin_temp_samples: VecDeque<Sample<f64>>,
    water_temp_samples: VecDeque<Sample<f64>>,
    humidity_samples: VecDeque<Sample<f64>>,
    wind_speed_samples: VecDeque<Sample<f64>>,
    wind_dir_samples: VecDeque<Sample<f64>>,
    roll_samples: VecDeque<Sample<f64>>,
    last_report_time: Instant,
    last_db_persist: std::collections::HashMap<MetricId, Instant>,
    config: EnvironmentalConfig,
}

impl EnvironmentalMonitor {
    pub fn new(config: EnvironmentalConfig) -> Self {
        Self {
            pressure_samples: VecDeque::new(),
            cabin_temp_samples: VecDeque::new(),
            water_temp_samples: VecDeque::new(),
            humidity_samples: VecDeque::new(),
            wind_speed_samples: VecDeque::new(),
            wind_dir_samples: VecDeque::new(),
            roll_samples: VecDeque::new(),
            last_report_time: Instant::now(),
            last_db_persist: std::collections::HashMap::new(),
            config,
        }
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
                self.cabin_temp_samples.push_back(Sample {
                    value: celsius,
                    timestamp: now,
                });
                
                self.cleanup_samples();
            } else if source==0 && instance==0 {
                // Source 0 is water temperature
                self.water_temp_samples.push_back(Sample {
                    value: celsius,
                    timestamp: now,
                });
                
                self.cleanup_samples();
            }

        }
    }

    /// Process wind data message (PGN 130306)
    pub fn process_wind(&mut self, wind: &WindData) {
        let now = Instant::now();
        
        // Store wind speed
        self.wind_speed_samples.push_back(Sample {
            value: wind.speed,
            timestamp: now,
        });
        
        // Store wind direction (convert radians to degrees)
        let degrees = wind.angle.to_degrees();
        self.wind_dir_samples.push_back(Sample {
            value: degrees,
            timestamp: now,
        });
        
        self.cleanup_samples();
    }
    
    /// Process a humidity message (PGN 130313)
    /// Standalone humidity sensor reading
    pub fn process_humidity(&mut self, hum: &Humidity) {
        let now = Instant::now();
        
        self.humidity_samples.push_back(Sample {
            value: hum.actual_humidity,
            timestamp: now,
        });
        
        self.cleanup_samples();
    }
    
    /// Process an actual pressure message (PGN 130314)
    /// Standalone pressure sensor reading
    pub fn process_actual_pressure(&mut self, pressure: &ActualPressure) {
        let now = Instant::now();
        let instance = pressure.instance;
        let source = pressure.source;

        if instance == 0 && source == 0 {
            // Primary atmospheric pressure sensor
            self.pressure_samples.push_back(Sample {
                value: pressure.pressure,
                timestamp: now,
            });
            
            self.cleanup_samples();
        }
    }
    
    /// Process an attitude message (PGN 127257)
    /// Extract roll angle in degrees
    pub fn process_attitude(&mut self, attitude: &Attitude) {
        if let Some(roll_deg) = attitude.roll_degrees() {
            let now = Instant::now();
            
            self.roll_samples.push_back(Sample {
                value: roll_deg,
                timestamp: now,
            });
            
            self.cleanup_samples();
        }
    }

    /// Generate a report if the sampling interval has elapsed
    pub fn generate_report(&mut self) -> Option<EnvironmentalReport> {
        let now = Instant::now();
        if now.duration_since(self.last_report_time) < SAMPLE_INTERVAL {
            return None;
        }
        
        self.last_report_time = now;
        
        Some(EnvironmentalReport {
            timestamp: now,
            pressure: MetricData {
                avg: self.calculate_avg(&self.pressure_samples),
                max: self.calculate_max(&self.pressure_samples),
                min: self.calculate_min(&self.pressure_samples),
            },
            cabin_temp: MetricData {
                avg: self.calculate_avg(&self.cabin_temp_samples),
                max: self.calculate_max(&self.cabin_temp_samples),
                min: self.calculate_min(&self.cabin_temp_samples),
            },
            water_temp: MetricData {
                avg: self.calculate_avg(&self.water_temp_samples),
                max: self.calculate_max(&self.water_temp_samples),
                min: self.calculate_min(&self.water_temp_samples),
            },
            humidity: MetricData {
                avg: self.calculate_avg(&self.humidity_samples),
                max: self.calculate_max(&self.humidity_samples),
                min: self.calculate_min(&self.humidity_samples),
            },
            wind_speed: MetricData {
                avg: self.calculate_avg(&self.wind_speed_samples),
                max: self.calculate_max(&self.wind_speed_samples),
                min: self.calculate_min(&self.wind_speed_samples),
            },
            wind_dir: MetricData {
                avg: self.calculate_avg(&self.wind_dir_samples),
                max: self.calculate_max(&self.wind_dir_samples),
                min: self.calculate_min(&self.wind_dir_samples),
            },
            roll: MetricData {
                avg: self.calculate_avg(&self.roll_samples),
                max: self.calculate_max(&self.roll_samples),
                min: self.calculate_min(&self.roll_samples),
            },
        })
    }

    fn cleanup_samples(&mut self) {
        let now = Instant::now();
        let cutoff = now - SAMPLE_INTERVAL - Duration::from_secs(10);
        
        Self::remove_old_samples(&mut self.pressure_samples, cutoff);
        Self::remove_old_samples(&mut self.cabin_temp_samples, cutoff);
        Self::remove_old_samples(&mut self.water_temp_samples, cutoff);
        Self::remove_old_samples(&mut self.humidity_samples, cutoff);
        Self::remove_old_samples(&mut self.wind_speed_samples, cutoff);
        Self::remove_old_samples(&mut self.wind_dir_samples, cutoff);
    }

    fn remove_old_samples<T>(samples: &mut VecDeque<Sample<T>>, cutoff: Instant) {
        while let Some(sample) = samples.front() {
            if sample.timestamp < cutoff {
                samples.pop_front();
            } else {
                break;
            }
        }
    }

    fn calculate_avg(&self, samples: &VecDeque<Sample<f64>>) -> Option<f64> {
        if samples.is_empty() {
            return None;
        }
        let sum: f64 = samples.iter().map(|s| s.value).sum();
        Some(sum / samples.len() as f64)
    }

    fn calculate_max(&self, samples: &VecDeque<Sample<f64>>) -> Option<f64> {
        samples.iter().map(|s| s.value).max_by(|a, b| a.partial_cmp(b).unwrap())
    }

    fn calculate_min(&self, samples: &VecDeque<Sample<f64>>) -> Option<f64> {
        samples.iter().map(|s| s.value).min_by(|a, b| a.partial_cmp(b).unwrap())
    }
    
    /// Get the list of metrics that should be persisted to the database now
    pub fn get_metrics_to_persist(&self) -> Vec<MetricId> {
        let now = Instant::now();
        let mut metrics_to_persist = Vec::new();
        
        let metrics = [
            (MetricId::WindSpeed, self.config.wind_speed_interval()),
            (MetricId::WindDir, self.config.wind_direction_interval()),
            (MetricId::Roll, self.config.roll_interval()),
            (MetricId::Pressure, self.config.pressure_interval()),
            (MetricId::CabinTemp, self.config.cabin_temp_interval()),
            (MetricId::WaterTemp, self.config.water_temp_interval()),
            (MetricId::Humidity, self.config.humidity_interval()),
        ];
        
        for (metric_id, interval) in metrics.iter() {
            if let Some(last_persist) = self.last_db_persist.get(metric_id) {
                if now.duration_since(*last_persist) >= *interval {
                    metrics_to_persist.push(*metric_id);
                }
            } else {
                // Never persisted before, should persist now
                metrics_to_persist.push(*metric_id);
            }
        }
        
        metrics_to_persist
    }
    
    /// Mark specific metrics as persisted to the database
    pub fn mark_metrics_persisted(&mut self, metrics: &[MetricId]) {
        let now = Instant::now();
        for metric_id in metrics {
            self.last_db_persist.insert(*metric_id, now);
        }
    }
}

impl Default for EnvironmentalMonitor {
    fn default() -> Self {
        Self::new(crate::config::EnvironmentalConfig::default())
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
        assert_eq!(monitor.pressure_samples.len(), 0);
        assert_eq!(monitor.cabin_temp_samples.len(), 0);
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
        assert_eq!(monitor.pressure_samples.len(), 1);
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
        assert_eq!(monitor.cabin_temp_samples.len(), 1);
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
        assert_eq!(monitor.water_temp_samples.len(), 1);
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
        assert_eq!(monitor.humidity_samples.len(), 1);
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
        assert_eq!(monitor.wind_speed_samples.len(), 1);
        assert_eq!(monitor.wind_dir_samples.len(), 1);
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
        assert_eq!(monitor.roll_samples.len(), 1);
    }

    #[test]
    fn test_mark_metrics_persisted() {
        let config = EnvironmentalConfig::default();
        let mut monitor = EnvironmentalMonitor::new(config);
        
        let metrics = vec![MetricId::Pressure, MetricId::CabinTemp];
        monitor.mark_metrics_persisted(&metrics);
        
        assert!(monitor.last_db_persist.contains_key(&MetricId::Pressure));
        assert!(monitor.last_db_persist.contains_key(&MetricId::CabinTemp));
        assert!(!monitor.last_db_persist.contains_key(&MetricId::WindSpeed));
    }

    #[test]
    fn test_get_metrics_to_persist_initial() {
        let config = EnvironmentalConfig {
            wind_speed_seconds: 10,
            wind_direction_seconds: 10,
            roll_seconds: 10,
            pressure_seconds: 10,
            cabin_temp_seconds: 10,
            water_temp_seconds: 10,
            humidity_seconds: 10,
        };
        let monitor = EnvironmentalMonitor::new(config);
        
        // Initially, all metrics should be ready to persist
        let metrics = monitor.get_metrics_to_persist();
        assert_eq!(metrics.len(), 7);
    }

    #[test]
    fn test_metric_data_all_none() {
        let data = MetricData {
            avg: None,
            max: None,
            min: None,
        };
        
        assert!(data.avg.is_none());
        assert!(data.max.is_none());
        assert!(data.min.is_none());
    }

    #[test]
    fn test_metric_data_with_values() {
        let data = MetricData {
            avg: Some(20.5),
            max: Some(25.0),
            min: Some(18.0),
        };
        
        assert_eq!(data.avg.unwrap(), 20.5);
        assert_eq!(data.max.unwrap(), 25.0);
        assert_eq!(data.min.unwrap(), 18.0);
    }
}
