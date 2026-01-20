use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub can_interface: String,
    pub time: TimeConfig,
    pub database: DatabaseConfig,
    #[serde(default)]
    pub source_filter: SourceFilterConfig,
    #[serde(default)]
    pub logging: LogConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    /// Directory where log files will be stored
    pub directory: String,
    /// Log file name prefix (date will be appended)
    pub file_prefix: String,
    /// Log level (trace, debug, info, warn, error)
    pub level: String,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            directory: "./logs".to_string(),
            file_prefix: "nmea_router".to_string(),
            level: "info".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SourceFilterConfig {
    /// Map of PGN to allowed source address
    /// If a PGN is present in this map, only messages from the specified source will be accepted
    /// If a PGN is not in the map, all sources are accepted
    #[serde(default)]
    pub pgn_source_map: std::collections::HashMap<u32, u8>,
}

impl SourceFilterConfig {
    /// Check if a message should be accepted based on its PGN and source
    /// Returns true if:
    /// - No filter is configured for this PGN (accept all sources)
    /// - A filter is configured and the source matches
    pub fn should_accept(&self, pgn: u32, source: u8) -> bool {
        match self.pgn_source_map.get(&pgn) {
            Some(&allowed_source) => source == allowed_source,
            None => true, // No filter for this PGN, accept all sources
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeConfig {
    pub skew_threshold_ms: i64,
}

impl Default for TimeConfig {
    fn default() -> Self {
        Self {
            skew_threshold_ms: 500,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub connection: DatabaseConnectionConfig,
    pub vessel_status: VesselStatusConfig,
    pub environmental: EnvironmentalConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConnectionConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database_name: String,
}

impl Default for DatabaseConnectionConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 3306,
            username: "nmea".to_string(),
            password: "nmea".to_string(),
            database_name: "nmea_router".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VesselStatusConfig {
    pub interval_moored_seconds: u64,
    pub interval_underway_seconds: u64,
}

impl Default for VesselStatusConfig {
    fn default() -> Self {
        Self {
            interval_moored_seconds: 1800,  // 30 minutes
            interval_underway_seconds: 30,   // 30 seconds
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentalConfig {
    pub wind_speed_seconds: u64,
    pub wind_direction_seconds: u64,
    pub roll_seconds: u64,
    pub pressure_seconds: u64,
    pub cabin_temp_seconds: u64,
    pub water_temp_seconds: u64,
    pub humidity_seconds: u64,
}

impl Default for EnvironmentalConfig {
    fn default() -> Self {
        Self {
            wind_speed_seconds: 30,
            wind_direction_seconds: 30,
            roll_seconds: 30,
            pressure_seconds: 120,
            cabin_temp_seconds: 300,
            water_temp_seconds: 300,
            humidity_seconds: 300,
        }
    }
}

impl Config {
    /// Load configuration from a JSON file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let contents = fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&contents)?;
        Ok(config)
    }
    
    /// Create default configuration
    pub fn default() -> Self {
        Config {
            can_interface: "vcan0".to_string(),
            time: TimeConfig::default(),
            database: DatabaseConfig {
                connection: DatabaseConnectionConfig::default(),
                vessel_status: VesselStatusConfig::default(),
                environmental: EnvironmentalConfig::default(),
            },
            source_filter: SourceFilterConfig::default(),
            logging: LogConfig::default(),
        }
    }
}

impl DatabaseConnectionConfig {
    /// Build MySQL connection URL from config
    pub fn connection_url(&self) -> String {
        format!(
            "mysql://{}:{}@{}:{}/{}",
            self.username, self.password, self.host, self.port, self.database_name
        )
    }
}

impl VesselStatusConfig {
    pub fn interval_moored(&self) -> Duration {
        Duration::from_secs(self.interval_moored_seconds)
    }
    
    pub fn interval_underway(&self) -> Duration {
        Duration::from_secs(self.interval_underway_seconds)
    }
}

impl EnvironmentalConfig {
    pub fn wind_speed_interval(&self) -> Duration {
        Duration::from_secs(self.wind_speed_seconds)
    }
    
    pub fn wind_direction_interval(&self) -> Duration {
        Duration::from_secs(self.wind_direction_seconds)
    }
    
    pub fn roll_interval(&self) -> Duration {
        Duration::from_secs(self.roll_seconds)
    }
    
    pub fn pressure_interval(&self) -> Duration {
        Duration::from_secs(self.pressure_seconds)
    }
    
    pub fn cabin_temp_interval(&self) -> Duration {
        Duration::from_secs(self.cabin_temp_seconds)
    }
    
    pub fn water_temp_interval(&self) -> Duration {
        Duration::from_secs(self.water_temp_seconds)
    }
    
    pub fn humidity_interval(&self) -> Duration {
        Duration::from_secs(self.humidity_seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_config_default() {
        let config = TimeConfig::default();
        assert_eq!(config.skew_threshold_ms, 500);
    }

    #[test]
    fn test_database_connection_config_default() {
        let config = DatabaseConnectionConfig::default();
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 3306);
        assert_eq!(config.username, "nmea");
        assert_eq!(config.password, "nmea");
        assert_eq!(config.database_name, "nmea_router");
    }

    #[test]
    fn test_database_connection_url() {
        let config = DatabaseConnectionConfig {
            host: "testhost".to_string(),
            port: 3307,
            username: "testuser".to_string(),
            password: "testpass".to_string(),
            database_name: "testdb".to_string(),
        };
        let url = config.connection_url();
        assert_eq!(url, "mysql://testuser:testpass@testhost:3307/testdb");
    }

    #[test]
    fn test_vessel_status_config_default() {
        let config = VesselStatusConfig::default();
        assert_eq!(config.interval_moored_seconds, 1800);
        assert_eq!(config.interval_underway_seconds, 30);
    }

    #[test]
    fn test_vessel_status_config_intervals() {
        let config = VesselStatusConfig {
            interval_moored_seconds: 120,
            interval_underway_seconds: 10,
        };
        assert_eq!(config.interval_moored(), Duration::from_secs(120));
        assert_eq!(config.interval_underway(), Duration::from_secs(10));
    }

    #[test]
    fn test_environmental_config_default() {
        let config = EnvironmentalConfig::default();
        assert_eq!(config.wind_speed_seconds, 30);
        assert_eq!(config.wind_direction_seconds, 30);
        assert_eq!(config.roll_seconds, 30);
        assert_eq!(config.pressure_seconds, 120);
        assert_eq!(config.cabin_temp_seconds, 300);
        assert_eq!(config.water_temp_seconds, 300);
        assert_eq!(config.humidity_seconds, 300);
    }

    #[test]
    fn test_environmental_config_intervals() {
        let config = EnvironmentalConfig {
            wind_speed_seconds: 10,
            wind_direction_seconds: 20,
            roll_seconds: 30,
            pressure_seconds: 40,
            cabin_temp_seconds: 50,
            water_temp_seconds: 60,
            humidity_seconds: 70,
        };
        assert_eq!(config.wind_speed_interval(), Duration::from_secs(10));
        assert_eq!(config.wind_direction_interval(), Duration::from_secs(20));
        assert_eq!(config.roll_interval(), Duration::from_secs(30));
        assert_eq!(config.pressure_interval(), Duration::from_secs(40));
        assert_eq!(config.cabin_temp_interval(), Duration::from_secs(50));
        assert_eq!(config.water_temp_interval(), Duration::from_secs(60));
        assert_eq!(config.humidity_interval(), Duration::from_secs(70));
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.can_interface, "vcan0");
        assert_eq!(config.time.skew_threshold_ms, 500);
        assert_eq!(config.database.connection.host, "localhost");
        assert_eq!(config.database.vessel_status.interval_moored_seconds, 1800);
        assert_eq!(config.database.environmental.wind_speed_seconds, 30);
    }

    #[test]
    fn test_source_filter_no_filter() {
        let filter = SourceFilterConfig::default();
        // No filters configured, should accept all sources
        assert!(filter.should_accept(129025, 10));
        assert!(filter.should_accept(129025, 22));
        assert!(filter.should_accept(127488, 5));
    }

    #[test]
    fn test_source_filter_with_filter() {
        let mut filter = SourceFilterConfig::default();
        filter.pgn_source_map.insert(129025, 22);
        filter.pgn_source_map.insert(127488, 5);
        
        // PGN 129025 should only accept source 22
        assert!(filter.should_accept(129025, 22));
        assert!(!filter.should_accept(129025, 10));
        assert!(!filter.should_accept(129025, 5));
        
        // PGN 127488 should only accept source 5
        assert!(filter.should_accept(127488, 5));
        assert!(!filter.should_accept(127488, 22));
        
        // PGN 130312 has no filter, should accept all sources
        assert!(filter.should_accept(130312, 10));
        assert!(filter.should_accept(130312, 22));
    }

    #[test]
    fn test_source_filter_serialization() {
        let mut filter = SourceFilterConfig::default();
        filter.pgn_source_map.insert(129025, 22);
        filter.pgn_source_map.insert(127488, 5);
        
        let json = serde_json::to_string(&filter).unwrap();
        assert!(json.contains("129025"));
        assert!(json.contains("127488"));
        
        let deserialized: SourceFilterConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.pgn_source_map.get(&129025), Some(&22));
        assert_eq!(deserialized.pgn_source_map.get(&127488), Some(&5));
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("vcan0"));
        assert!(json.contains("localhost"));
    }

    #[test]
    fn test_config_deserialization() {
        let json = r#"{
            "can_interface": "can0",
            "time": {
                "skew_threshold_ms": 1000
            },
            "database": {
                "connection": {
                    "host": "myhost",
                    "port": 3306,
                    "username": "user",
                    "password": "pass",
                    "database_name": "mydb"
                },
                "vessel_status": {
                    "interval_moored_seconds": 600,
                    "interval_underway_seconds": 15
                },
                "environmental": {
                    "wind_speed_seconds": 20,
                    "wind_direction_seconds": 20,
                    "roll_seconds": 20,
                    "pressure_seconds": 100,
                    "cabin_temp_seconds": 200,
                    "water_temp_seconds": 200,
                    "humidity_seconds": 200
                }
            }
        }"#;
        
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.can_interface, "can0");
        assert_eq!(config.time.skew_threshold_ms, 1000);
        assert_eq!(config.database.connection.host, "myhost");
        assert_eq!(config.database.vessel_status.interval_moored_seconds, 600);
        assert_eq!(config.database.environmental.wind_speed_seconds, 20);
    }

    #[test]
    fn test_log_config_default() {
        let log_config = LogConfig::default();
        assert_eq!(log_config.directory, "./logs");
        assert_eq!(log_config.file_prefix, "nmea_router");
        assert_eq!(log_config.level, "info");
    }

    #[test]
    fn test_log_config_serialization() {
        let log_config = LogConfig {
            directory: "/var/log/nmea".to_string(),
            file_prefix: "router".to_string(),
            level: "debug".to_string(),
        };
        
        let json = serde_json::to_string(&log_config).unwrap();
        assert!(json.contains("/var/log/nmea"));
        assert!(json.contains("router"));
        assert!(json.contains("debug"));
        
        let deserialized: LogConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.directory, "/var/log/nmea");
        assert_eq!(deserialized.file_prefix, "router");
        assert_eq!(deserialized.level, "debug");
    }
}
