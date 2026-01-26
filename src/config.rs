use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Duration;
use tracing::warn;

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

fn deserialize_bool_safe<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct BoolVisitor;

    impl<'de> Visitor<'de> for BoolVisitor {
        type Value = bool;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a boolean value (true/false), string (\"true\"/\"false\"), or number (1/0)")
        }

        fn visit_bool<E>(self, value: bool) -> Result<bool, E>
        where
            E: de::Error,
        {
            Ok(value)
        }

        fn visit_str<E>(self, value: &str) -> Result<bool, E>
        where
            E: de::Error,
        {
            match value.to_lowercase().as_str() {
                "true" | "yes" | "1" | "on" | "enabled" => Ok(true),
                "false" | "no" | "0" | "off" | "disabled" => Ok(false),
                _ => {
                    warn!("Invalid boolean value '{}', defaulting to false", value);
                    Ok(false)
                }
            }
        }

        fn visit_i64<E>(self, value: i64) -> Result<bool, E>
        where
            E: de::Error,
        {
            Ok(value != 0)
        }

        fn visit_u64<E>(self, value: u64) -> Result<bool, E>
        where
            E: de::Error,
        {
            Ok(value != 0)
        }

        fn visit_none<E>(self) -> Result<bool, E>
        where
            E: de::Error,
        {
            Ok(false)
        }

        fn visit_unit<E>(self) -> Result<bool, E>
        where
            E: de::Error,
        {
            Ok(false)
        }
    }

    deserializer.deserialize_any(BoolVisitor)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeConfig {
    pub skew_threshold_ms: i64,
    /// Whether to attempt to set system time when NMEA time is available
    /// This is useful on systems without NTP/time synchronization
    /// Requires appropriate permissions (typically root/sudo)
    /// Accepts: true/false, "true"/"false", 1/0, or various string representations
    /// Defaults to false on any error or malformed value
    #[serde(default, deserialize_with = "deserialize_bool_safe")]
    pub set_system_time: bool,
}

impl Default for TimeConfig {
    fn default() -> Self {
        Self {
            skew_threshold_ms: 500,
            set_system_time: false,
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
        let mut config: Config = serde_json::from_str(&contents)?;
        config.validate_and_fix()?;
        Ok(config)
    }
    
    /// Validate configuration and fix invalid values by reverting to defaults
    /// Returns an error if CAN interface is invalid (unrecoverable)
    fn validate_and_fix(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Validate CAN interface - must not be empty
        if self.can_interface.is_empty() {
            return Err("Configuration error: CAN interface cannot be empty".into());
        }
        
        // Validate CAN interface is a valid device name (basic check)
        if !self.can_interface.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
            return Err(format!("Configuration error: Invalid CAN interface name '{}'. Must contain only alphanumeric characters, underscores, or hyphens.", self.can_interface).into());
        }
        
        // Validate time skew threshold (must be >= 100 ms)
        if self.time.skew_threshold_ms < 100 {
            warn!("Configuration warning: skew_threshold_ms ({}) is below minimum 100ms. Reverting to default 500ms.", self.time.skew_threshold_ms);
            self.time.skew_threshold_ms = TimeConfig::default().skew_threshold_ms;
        }
        
        // Validate PGN source filter
        let mut invalid_pgns = Vec::new();
        let mut invalid_sources = Vec::new();
        
        for (pgn, source) in &self.source_filter.pgn_source_map {
            // Check PGN range (50000-200000)
            if *pgn < 50000 || *pgn > 200000 {
                invalid_pgns.push(*pgn);
            }
            // Check source range (1-254)
            if *source < 1 || *source > 254 {
                invalid_sources.push((*pgn, *source));
            }
        }
        
        // Remove invalid entries and warn
        for pgn in invalid_pgns {
            warn!("Configuration warning: Invalid PGN {} in source filter (must be 50000-200000). Removing entry.", pgn);
            self.source_filter.pgn_source_map.remove(&pgn);
        }
        
        for (pgn, source) in invalid_sources {
            warn!("Configuration warning: Invalid source {} for PGN {} (must be 1-254). Removing entry.", source, pgn);
            self.source_filter.pgn_source_map.remove(&pgn);
        }
        
        // Validate vessel status intervals
        self.validate_vessel_status_intervals();
        
        // Validate environmental intervals (30 seconds - 10 minutes = 30-600 seconds)
        self.validate_environmental_intervals();
        
        Ok(())
    }
    
    fn validate_vessel_status_intervals(&mut self) {
        let defaults = VesselStatusConfig::default();
        
        // Validate moored interval (30 seconds - 10 minutes)
        if self.database.vessel_status.interval_moored_seconds < 30 || self.database.vessel_status.interval_moored_seconds > 600 {
            warn!("Configuration warning: interval_moored_seconds ({}) is out of range (30-600). Reverting to default {}.", 
                self.database.vessel_status.interval_moored_seconds, defaults.interval_moored_seconds);
            self.database.vessel_status.interval_moored_seconds = defaults.interval_moored_seconds;
        }
        
        // Validate underway interval (30 seconds - 10 minutes)
        if self.database.vessel_status.interval_underway_seconds < 30 || self.database.vessel_status.interval_underway_seconds > 600 {
            warn!("Configuration warning: interval_underway_seconds ({}) is out of range (30-600). Reverting to default {}.", 
                self.database.vessel_status.interval_underway_seconds, defaults.interval_underway_seconds);
            self.database.vessel_status.interval_underway_seconds = defaults.interval_underway_seconds;
        }
    }
    
    fn validate_environmental_intervals(&mut self) {
        let defaults = EnvironmentalConfig::default();
        
        // Validate each environmental interval (30 seconds - 10 minutes = 30-600 seconds)
        if self.database.environmental.wind_speed_seconds < 30 || self.database.environmental.wind_speed_seconds > 600 {
            warn!("Configuration warning: wind_speed_seconds ({}) is out of range (30-600). Reverting to default {}.", 
                self.database.environmental.wind_speed_seconds, defaults.wind_speed_seconds);
            self.database.environmental.wind_speed_seconds = defaults.wind_speed_seconds;
        }
        
        if self.database.environmental.wind_direction_seconds < 30 || self.database.environmental.wind_direction_seconds > 600 {
            warn!("Configuration warning: wind_direction_seconds ({}) is out of range (30-600). Reverting to default {}.", 
                self.database.environmental.wind_direction_seconds, defaults.wind_direction_seconds);
            self.database.environmental.wind_direction_seconds = defaults.wind_direction_seconds;
        }
        
        if self.database.environmental.roll_seconds < 30 || self.database.environmental.roll_seconds > 600 {
            warn!("Configuration warning: roll_seconds ({}) is out of range (30-600). Reverting to default {}.", 
                self.database.environmental.roll_seconds, defaults.roll_seconds);
            self.database.environmental.roll_seconds = defaults.roll_seconds;
        }
        
        if self.database.environmental.pressure_seconds < 30 || self.database.environmental.pressure_seconds > 600 {
            warn!("Configuration warning: pressure_seconds ({}) is out of range (30-600). Reverting to default {}.", 
                self.database.environmental.pressure_seconds, defaults.pressure_seconds);
            self.database.environmental.pressure_seconds = defaults.pressure_seconds;
        }
        
        if self.database.environmental.cabin_temp_seconds < 30 || self.database.environmental.cabin_temp_seconds > 600 {
            warn!("Configuration warning: cabin_temp_seconds ({}) is out of range (30-600). Reverting to default {}.", 
                self.database.environmental.cabin_temp_seconds, defaults.cabin_temp_seconds);
            self.database.environmental.cabin_temp_seconds = defaults.cabin_temp_seconds;
        }
        
        if self.database.environmental.water_temp_seconds < 30 || self.database.environmental.water_temp_seconds > 600 {
            warn!("Configuration warning: water_temp_seconds ({}) is out of range (30-600). Reverting to default {}.", 
                self.database.environmental.water_temp_seconds, defaults.water_temp_seconds);
            self.database.environmental.water_temp_seconds = defaults.water_temp_seconds;
        }
        
        if self.database.environmental.humidity_seconds < 30 || self.database.environmental.humidity_seconds > 600 {
            warn!("Configuration warning: humidity_seconds ({}) is out of range (30-600). Reverting to default {}.", 
                self.database.environmental.humidity_seconds, defaults.humidity_seconds);
            self.database.environmental.humidity_seconds = defaults.humidity_seconds;
        }
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

    #[test]
    fn test_validation_empty_can_interface() {
        let json = r#"{
            "can_interface": "",
            "time": {"skew_threshold_ms": 500},
            "database": {
                "connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"},
                "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30},
                "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}
            }
        }"#;
        
        let mut config: Config = serde_json::from_str(json).unwrap();
        let result = config.validate_and_fix();
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("CAN interface cannot be empty"));
    }

    #[test]
    fn test_validation_invalid_can_interface() {
        let json = r#"{
            "can_interface": "can@#$%",
            "time": {"skew_threshold_ms": 500},
            "database": {
                "connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"},
                "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30},
                "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}
            }
        }"#;
        
        let mut config: Config = serde_json::from_str(json).unwrap();
        let result = config.validate_and_fix();
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid CAN interface name"));
    }

    #[test]
    fn test_validation_skew_threshold_too_low() {
        let json = r#"{
            "can_interface": "vcan0",
            "time": {"skew_threshold_ms": 50},
            "database": {
                "connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"},
                "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30},
                "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}
            }
        }"#;
        
        let mut config: Config = serde_json::from_str(json).unwrap();
        config.validate_and_fix().unwrap();
        
        // Should be reverted to default
        assert_eq!(config.time.skew_threshold_ms, 500);
    }

    #[test]
    fn test_validation_environmental_period_out_of_range() {
        let json = r#"{
            "can_interface": "vcan0",
            "time": {"skew_threshold_ms": 500},
            "database": {
                "connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"},
                "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30},
                "environmental": {"wind_speed_seconds": 10, "wind_direction_seconds": 700, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}
            }
        }"#;
        
        let mut config: Config = serde_json::from_str(json).unwrap();
        config.validate_and_fix().unwrap();
        
        // wind_speed_seconds too low (10 < 30), should be reverted to default
        assert_eq!(config.database.environmental.wind_speed_seconds, 30);
        // wind_direction_seconds too high (700 > 600), should be reverted to default
        assert_eq!(config.database.environmental.wind_direction_seconds, 30);
    }

    #[test]
    fn test_validation_pgn_out_of_range() {
        let json = r#"{
            "can_interface": "vcan0",
            "time": {"skew_threshold_ms": 500},
            "source_filter": {
                "pgn_source_map": {
                    "129025": 22,
                    "30000": 10,
                    "250000": 5
                }
            },
            "database": {
                "connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"},
                "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30},
                "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}
            }
        }"#;
        
        let mut config: Config = serde_json::from_str(json).unwrap();
        config.validate_and_fix().unwrap();
        
        // Valid PGN should remain
        assert_eq!(config.source_filter.pgn_source_map.get(&129025), Some(&22));
        // Invalid PGNs should be removed
        assert_eq!(config.source_filter.pgn_source_map.get(&30000), None);
        assert_eq!(config.source_filter.pgn_source_map.get(&250000), None);
    }

    #[test]
    fn test_validation_source_out_of_range() {
        let json = r#"{
            "can_interface": "vcan0",
            "time": {"skew_threshold_ms": 500},
            "source_filter": {
                "pgn_source_map": {
                    "129025": 22,
                    "129026": 0,
                    "129029": 255
                }
            },
            "database": {
                "connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"},
                "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30},
                "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}
            }
        }"#;
        
        let mut config: Config = serde_json::from_str(json).unwrap();
        config.validate_and_fix().unwrap();
        
        // Valid source should remain
        assert_eq!(config.source_filter.pgn_source_map.get(&129025), Some(&22));
        // Invalid sources (0, 255) should be removed
        assert_eq!(config.source_filter.pgn_source_map.get(&129026), None);
        assert_eq!(config.source_filter.pgn_source_map.get(&129029), None);
    }

    #[test]
    fn test_set_system_time_safe_deserialization_bool() {
        // Test normal boolean values
        let json = r#"{"can_interface": "vcan0", "time": {"skew_threshold_ms": 500, "set_system_time": true}, "database": {"connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"}, "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30}, "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.time.set_system_time, true);

        let json = r#"{"can_interface": "vcan0", "time": {"skew_threshold_ms": 500, "set_system_time": false}, "database": {"connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"}, "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30}, "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.time.set_system_time, false);
    }

    #[test]
    fn test_set_system_time_safe_deserialization_string() {
        // Test string values
        let json = r#"{"can_interface": "vcan0", "time": {"skew_threshold_ms": 500, "set_system_time": "true"}, "database": {"connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"}, "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30}, "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.time.set_system_time, true);

        let json = r#"{"can_interface": "vcan0", "time": {"skew_threshold_ms": 500, "set_system_time": "yes"}, "database": {"connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"}, "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30}, "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.time.set_system_time, true);

        let json = r#"{"can_interface": "vcan0", "time": {"skew_threshold_ms": 500, "set_system_time": "no"}, "database": {"connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"}, "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30}, "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.time.set_system_time, false);
    }

    #[test]
    fn test_set_system_time_safe_deserialization_number() {
        // Test number values
        let json = r#"{"can_interface": "vcan0", "time": {"skew_threshold_ms": 500, "set_system_time": 1}, "database": {"connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"}, "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30}, "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.time.set_system_time, true);

        let json = r#"{"can_interface": "vcan0", "time": {"skew_threshold_ms": 500, "set_system_time": 0}, "database": {"connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"}, "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30}, "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.time.set_system_time, false);
    }

    #[test]
    fn test_set_system_time_safe_deserialization_malformed() {
        // Test malformed values - should default to false
        let json = r#"{"can_interface": "vcan0", "time": {"skew_threshold_ms": 500, "set_system_time": "invalid"}, "database": {"connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"}, "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30}, "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.time.set_system_time, false);
    }

    #[test]
    fn test_set_system_time_safe_deserialization_missing() {
        // Test missing value - should default to false
        let json = r#"{"can_interface": "vcan0", "time": {"skew_threshold_ms": 500}, "database": {"connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"}, "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30}, "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.time.set_system_time, false);
    }
}
