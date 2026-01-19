use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub can_interface: String,
    pub time: TimeConfig,
    pub database: DatabaseConfig,
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
