// Configuration Management
// Guidelines: Configuration fields are read-only. Mutable application state belongs in the database.
// For conventions on field naming and defaults, see: AGENTS.md
//
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Duration;
use tracing::warn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanConfig {
    #[serde(default = "CanConfig::default_interface")]
    pub interface: String,
    #[serde(default = "CanConfig::default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub source_filter: SourceFilterConfig,
}

impl CanConfig {
    fn default_interface() -> String {
        "can0".to_string()
    }
    fn default_enabled() -> bool {
        true
    }
}

impl Default for CanConfig {
    fn default() -> Self {
        Self {
            interface: Self::default_interface(),
            enabled: Self::default_enabled(),
            source_filter: SourceFilterConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub can: CanConfig,
    pub time: TimeConfig,
    pub database: DatabaseConfig,
    #[serde(default)]
    pub logging: LogConfig,
    #[serde(default)]
    pub web: WebConfig,
    #[serde(default)]
    pub udp: UdpConfig,
    #[serde(default)]
    pub signalk: SignalKConfig,
    #[serde(default)]
    pub sync: SyncConfig,
    /// Path to polar diagram CSV. Optional — when absent the route planner uses a fixed speed.
    #[serde(default)]
    pub polars_file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebConfig {
    /// Enable or disable the web interface
    #[serde(default = "default_web_enabled")]
    pub enabled: bool,
    /// Port for the web server to listen on
    #[serde(default = "default_web_port")]
    pub port: u16,
    /// Shared password for web UI access (plaintext)
    pub auth_password: Option<String>,
    /// Session lifetime in seconds (default: 7 days)
    #[serde(default = "default_session_duration_secs")]
    pub session_duration_secs: u64,
    /// Set Secure flag on session cookies. Disable for local HTTP dev.
    #[serde(default = "default_secure_cookies")]
    pub secure_cookies: bool,
    /// Restrict UI and API to read-only trip/stats views (for public internet deployment).
    #[serde(default)]
    pub read_only: bool,
}

fn default_web_enabled() -> bool {
    true
}

fn default_web_port() -> u16 {
    8080
}

fn default_session_duration_secs() -> u64 {
    7 * 24 * 3600
}

fn default_secure_cookies() -> bool {
    true
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 8080,
            auth_password: None,
            session_duration_secs: default_session_duration_secs(),
            secure_cookies: default_secure_cookies(),
            read_only: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UdpConfig {
    /// Enable or disable UDP broadcasting
    #[serde(default = "default_udp_enabled")]
    pub enabled: bool,
    /// UDP destination address (e.g., "192.168.1.255:10110" or "224.0.0.1:10110" for multicast)
    #[serde(default = "default_udp_address")]
    pub address: String,
    /// Local interface/address to bind the UDP socket to (e.g., "192.168.1.10:0" or "0.0.0.0:0")
    #[serde(default = "default_udp_bind_address")]
    pub bind_address: String,
}

fn default_udp_enabled() -> bool {
    false
}

fn default_udp_address() -> String {
    "192.168.1.255:10110".to_string()
}

fn default_udp_bind_address() -> String {
    "0.0.0.0:0".to_string()
}

impl Default for UdpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            address: "192.168.1.255:10110".to_string(),
            bind_address: "0.0.0.0:0".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalKConfig {
    /// Enable or disable SignalK broadcasting
    #[serde(default = "default_signalk_enabled")]
    pub enabled: bool,
    /// Rate limit in milliseconds for each SignalK path
    #[serde(default = "default_signalk_rate_limit_ms")]
    pub rate_limit_ms: u64,
    /// Vessel UUID for SignalK (defaults to hardcoded value if not specified)
    #[serde(default = "default_vessel_uuid")]
    pub vessel_uuid: String,
    /// Vessel name for SignalK (defaults to "Itaca" if not specified)
    #[serde(default = "default_vessel_name")]
    pub vessel_name: String,
}

fn default_signalk_enabled() -> bool {
    false
}

fn default_signalk_rate_limit_ms() -> u64 {
    100 // 10Hz default
}

fn default_vessel_uuid() -> String {
    "e8bf264e-0321-46e5-8dfb-c1394ff974f8".to_string()
}

fn default_vessel_name() -> String {
    "Itaca".to_string()
}

impl Default for SignalKConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            rate_limit_ms: 100,
            vessel_uuid: "e8bf264e-0321-46e5-8dfb-c1394ff974f8".to_string(),
            vessel_name: "Itaca".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    /// Enable push sync from this node to trips_viewer
    #[serde(default)]
    pub enabled: bool,
    /// Base URL of the trips_viewer instance, e.g. "https://trips.example.com"
    #[serde(default)]
    pub target_url: String,
    /// Shared secret used as Bearer token; None means receive endpoint rejects all requests
    pub api_key: Option<String>,
    /// HTTP timeout for the push call in seconds
    #[serde(default = "default_sync_timeout_secs")]
    pub timeout_secs: u64,
    /// Skip TLS certificate verification — use only for local/self-signed setups, never in production
    #[serde(default)]
    pub accept_invalid_certs: bool,
}

fn default_sync_timeout_secs() -> u64 {
    120
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            target_url: String::new(),
            api_key: None,
            timeout_secs: default_sync_timeout_secs(),
            accept_invalid_certs: false,
        }
    }
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
            formatter.write_str(
                "a boolean value (true/false), string (\"true\"/\"false\"), or number (1/0)",
            )
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
    #[serde(default = "DatabaseConnectionConfig::default_pool_min")]
    pub pool_min: usize,
    #[serde(default = "DatabaseConnectionConfig::default_pool_max")]
    pub pool_max: usize,
}

impl DatabaseConnectionConfig {
    fn default_pool_min() -> usize {
        2
    }
    fn default_pool_max() -> usize {
        10
    }
}

impl Default for DatabaseConnectionConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 3306,
            username: "nmea".to_string(),
            password: "nmea".to_string(),
            database_name: "nmea_router".to_string(),
            pool_min: Self::default_pool_min(),
            pool_max: Self::default_pool_max(),
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
            interval_moored_seconds: 1800, // 30 minutes
            interval_underway_seconds: 30, // 30 seconds
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
    /// In test mode, loads test_config.json instead of config.json if no path specified
    fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, crate::error::AppError> {
        let contents = fs::read_to_string(path)?;
        let mut config: Config = serde_json::from_str(&contents)?;
        config.validate_and_fix()?;
        Ok(config)
    }

    #[cfg(not(test))]
    pub fn load_for_context(config_path: Option<&str>) -> Result<Self, crate::error::AppError> {
        let resolved = if let Some(path) = config_path {
            path.to_string()
        } else if std::path::Path::new("./config.json").exists() {
            "./config.json".to_string()
        } else {
            "/etc/nmea_router/config.json".to_string()
        };
        let mut config = Self::from_file(&resolved)?;
        config.apply_env_overrides();
        Ok(config)
    }

    /// Load configuration with automatic test mode detection
    /// When cfg(test) is active, loads test_config.json by default
    #[cfg(test)]
    pub fn load_for_context(_config_path: Option<&str>) -> Result<Self, crate::error::AppError> {
        Self::from_file("test_config.json")
    }

    /// Validate configuration and fix invalid values by reverting to defaults
    /// Returns an error if CAN interface is invalid (unrecoverable)
    fn validate_and_fix(&mut self) -> Result<(), crate::error::AppError> {
        // Validate CAN interface only when CAN is enabled
        if self.can.enabled {
            if self.can.interface.is_empty() {
                return Err(crate::error::AppError::Configuration(
                    "CAN interface cannot be empty".to_string(),
                ));
            }
            if !self
                .can
                .interface
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
            {
                return Err(crate::error::AppError::Configuration(format!(
                    "Invalid CAN interface name '{}'. Must contain only alphanumeric characters, underscores, or hyphens.",
                    self.can.interface
                )));
            }
        }

        // Validate time skew threshold (must be >= 100 ms)
        if self.time.skew_threshold_ms < 100 {
            warn!("Configuration warning: skew_threshold_ms ({}) is below minimum 100ms. Reverting to default 500ms.", self.time.skew_threshold_ms);
            self.time.skew_threshold_ms = TimeConfig::default().skew_threshold_ms;
        }

        // Validate PGN source filter
        let mut invalid_pgns = Vec::new();
        let mut invalid_sources = Vec::new();

        for (pgn, source) in &self.can.source_filter.pgn_source_map {
            // Check PGN range (50000-200000)
            if *pgn < 50000 || *pgn > 200000 {
                invalid_pgns.push(*pgn);
            }
            // Check source range (0-254)
            // Note: 255 is not legal as a source address, 254 is used for addresses not claimed yet, and 251-253 are reserved. Use 255 to indicate that the pgn must always be rejected
            if *source >= 251 && *source <= 253 {
                invalid_sources.push((*pgn, *source));
            }
        }

        // Remove invalid entries and warn
        for pgn in invalid_pgns {
            warn!("Configuration warning: Invalid PGN {} in source filter (must be 50000-200000). Removing entry.", pgn);
            self.can.source_filter.pgn_source_map.remove(&pgn);
        }

        for (pgn, source) in invalid_sources {
            warn!("Configuration warning: Invalid source {} for PGN {} (must be 0-255, excluding 251-253). Removing entry.", source, pgn);
            self.can.source_filter.pgn_source_map.remove(&pgn);
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
        if self.database.vessel_status.interval_moored_seconds < 30
            || self.database.vessel_status.interval_moored_seconds > 600
        {
            warn!("Configuration warning: interval_moored_seconds ({}) is out of range (30-600). Reverting to default {}.",
                self.database.vessel_status.interval_moored_seconds, defaults.interval_moored_seconds);
            self.database.vessel_status.interval_moored_seconds = defaults.interval_moored_seconds;
        }

        // Validate underway interval (10 seconds - 10 minutes)
        if self.database.vessel_status.interval_underway_seconds < 10
            || self.database.vessel_status.interval_underway_seconds > 600
        {
            warn!("Configuration warning: interval_underway_seconds ({}) is out of range (10-600). Reverting to default {}.",
                self.database.vessel_status.interval_underway_seconds, defaults.interval_underway_seconds);
            self.database.vessel_status.interval_underway_seconds =
                defaults.interval_underway_seconds;
        }
    }

    fn validate_environmental_intervals(&mut self) {
        let defaults = EnvironmentalConfig::default();

        // Validate each environmental interval (30 seconds - 10 minutes = 30-600 seconds)
        if self.database.environmental.wind_speed_seconds < 30
            || self.database.environmental.wind_speed_seconds > 600
        {
            warn!("Configuration warning: wind_speed_seconds ({}) is out of range (30-600). Reverting to default {}.",
                self.database.environmental.wind_speed_seconds, defaults.wind_speed_seconds);
            self.database.environmental.wind_speed_seconds = defaults.wind_speed_seconds;
        }

        if self.database.environmental.wind_direction_seconds < 30
            || self.database.environmental.wind_direction_seconds > 600
        {
            warn!("Configuration warning: wind_direction_seconds ({}) is out of range (30-600). Reverting to default {}.",
                self.database.environmental.wind_direction_seconds, defaults.wind_direction_seconds);
            self.database.environmental.wind_direction_seconds = defaults.wind_direction_seconds;
        }

        if self.database.environmental.roll_seconds < 30
            || self.database.environmental.roll_seconds > 600
        {
            warn!("Configuration warning: roll_seconds ({}) is out of range (30-600). Reverting to default {}.",
                self.database.environmental.roll_seconds, defaults.roll_seconds);
            self.database.environmental.roll_seconds = defaults.roll_seconds;
        }

        if self.database.environmental.pressure_seconds < 30
            || self.database.environmental.pressure_seconds > 600
        {
            warn!("Configuration warning: pressure_seconds ({}) is out of range (30-600). Reverting to default {}.",
                self.database.environmental.pressure_seconds, defaults.pressure_seconds);
            self.database.environmental.pressure_seconds = defaults.pressure_seconds;
        }

        if self.database.environmental.cabin_temp_seconds < 30
            || self.database.environmental.cabin_temp_seconds > 600
        {
            warn!("Configuration warning: cabin_temp_seconds ({}) is out of range (30-600). Reverting to default {}.",
                self.database.environmental.cabin_temp_seconds, defaults.cabin_temp_seconds);
            self.database.environmental.cabin_temp_seconds = defaults.cabin_temp_seconds;
        }

        if self.database.environmental.water_temp_seconds < 30
            || self.database.environmental.water_temp_seconds > 600
        {
            warn!("Configuration warning: water_temp_seconds ({}) is out of range (30-600). Reverting to default {}.",
                self.database.environmental.water_temp_seconds, defaults.water_temp_seconds);
            self.database.environmental.water_temp_seconds = defaults.water_temp_seconds;
        }

        if self.database.environmental.humidity_seconds < 30
            || self.database.environmental.humidity_seconds > 600
        {
            warn!("Configuration warning: humidity_seconds ({}) is out of range (30-600). Reverting to default {}.",
                self.database.environmental.humidity_seconds, defaults.humidity_seconds);
            self.database.environmental.humidity_seconds = defaults.humidity_seconds;
        }
    }

    /// Apply environment variable overrides after JSON loading.
    /// Supports: DATABASE_URL, MYSQL_URL, MYSQL_PUBLIC_URL, MYSQLHOST/PORT/USER/PASSWORD/DATABASE,
    ///           PORT, AUTH_PASSWORD, SECURE_COOKIES, LOG_LEVEL
    pub fn apply_env_overrides(&mut self) {
        let db_url = std::env::var("DATABASE_URL")
            .or_else(|_| std::env::var("MYSQL_URL"))
            .or_else(|_| std::env::var("MYSQL_PUBLIC_URL"))
            .ok();
        if let Some(url) = db_url {
            if let Some(conn) = Self::parse_database_url(&url) {
                eprintln!("[config] DB host from URL env var: {}:{}", conn.host, conn.port);
                self.database.connection = conn;
            } else {
                eprintln!("[config] WARNING: DB URL env var set but could not be parsed: {}", url);
            }
        } else if let (Ok(host), Ok(port_str), Ok(user), Ok(pass), Ok(db)) = (
            std::env::var("MYSQLHOST"),
            std::env::var("MYSQLPORT"),
            std::env::var("MYSQLUSER"),
            std::env::var("MYSQLPASSWORD"),
            std::env::var("MYSQLDATABASE"),
        ) {
            match port_str.parse::<u16>() {
                Ok(port) => {
                    eprintln!("[config] DB host from MYSQL* env vars: {}:{}", host, port);
                    self.database.connection.host = host;
                    self.database.connection.port = port;
                    self.database.connection.username = user;
                    self.database.connection.password = pass;
                    self.database.connection.database_name = db;
                }
                Err(_) => eprintln!("[config] WARNING: MYSQLPORT '{}' is not a valid port, ignoring MYSQL* vars", port_str),
            }
        } else {
            eprintln!("[config] WARNING: no DB env vars found (DATABASE_URL / MYSQL_URL / MYSQLHOST); using config file values ({}:{})", self.database.connection.host, self.database.connection.port);
        }
        if let Ok(port_str) = std::env::var("PORT") {
            match port_str.parse::<u16>() {
                Ok(port) => self.web.port = port,
                Err(_) => warn!("PORT env var '{}' is not a valid u16, ignoring", port_str),
            }
        }
        if let Ok(password) = std::env::var("AUTH_PASSWORD") {
            self.web.auth_password = if password.is_empty() { None } else { Some(password) };
        }
        if let Ok(val) = std::env::var("SECURE_COOKIES") {
            self.web.secure_cookies = matches!(val.to_lowercase().as_str(), "true" | "1" | "yes");
        }
        if let Ok(level) = std::env::var("LOG_LEVEL") {
            if !level.is_empty() {
                self.logging.level = level;
            }
        }
        if let Ok(key) = std::env::var("SYNC_API_KEY") {
            self.sync.api_key = if key.is_empty() { None } else { Some(key) };
        }
        if let Ok(val) = std::env::var("SYNC_ENABLED") {
            self.sync.enabled = matches!(val.to_lowercase().as_str(), "true" | "1" | "yes");
        }
        if let Ok(url) = std::env::var("SYNC_TARGET_URL") {
            if !url.is_empty() {
                self.sync.target_url = url;
            }
        }
    }

    /// Parse a mysql://user:pass@host:port/dbname[?params] URL into a DatabaseConnectionConfig.
    /// Accepts mysql://, mysql2://, and mysqls:// prefixes. Query parameters are ignored.
    fn parse_database_url(url: &str) -> Option<DatabaseConnectionConfig> {
        let rest = url
            .strip_prefix("mysql2://")
            .or_else(|| url.strip_prefix("mysqls://"))
            .or_else(|| url.strip_prefix("mysql://"))?;
        let (userinfo, hostinfo) = rest.split_once('@')?;
        let (username, password) = userinfo.split_once(':')?;
        let (hostport, rest) = hostinfo.split_once('/')?;
        let (host, port_str) = hostport.split_once(':')?;
        let port = port_str.parse::<u16>().ok()?;
        // Strip query parameters (e.g. ?sslaccept=strict appended by Railway)
        let database_name = rest.split('?').next().unwrap_or(rest).to_string();
        Some(DatabaseConnectionConfig {
            host: host.to_string(),
            port,
            username: username.to_string(),
            password: password.to_string(),
            database_name,
            pool_min: DatabaseConnectionConfig::default_pool_min(),
            pool_max: DatabaseConnectionConfig::default_pool_max(),
        })
    }

    /// Create default configuration
    #[allow(dead_code)]
    pub fn new_default_instance() -> Self {
        Config {
            can: CanConfig { interface: "vcan0".to_string(), enabled: true, source_filter: SourceFilterConfig::default() },
            time: TimeConfig::default(),
            database: DatabaseConfig {
                connection: DatabaseConnectionConfig::default(),
                vessel_status: VesselStatusConfig::default(),
                environmental: EnvironmentalConfig::default(),
            },
            logging: LogConfig::default(),
            web: WebConfig::default(),
            udp: UdpConfig::default(),
            signalk: SignalKConfig::default(),
            sync: SyncConfig::default(),
            polars_file_path: None,
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
            pool_min: DatabaseConnectionConfig::default_pool_min(),
            pool_max: DatabaseConnectionConfig::default_pool_max(),
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
        let config = Config::new_default_instance();
        assert_eq!(config.can.interface, "vcan0");
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
        let config = Config::new_default_instance();
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("vcan0"));
        assert!(json.contains("localhost"));
    }

    #[test]
    fn test_config_deserialization() {
        let json = r#"{
            "can": {"interface": "can0", "enabled": true},
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
        assert_eq!(config.can.interface, "can0");
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
            "can": {"interface": "", "enabled": true},
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
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("CAN interface cannot be empty"));
    }

    #[test]
    fn test_validation_invalid_can_interface() {
        let json = r#"{
            "can": {"interface": "can@#$%", "enabled": true},
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
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid CAN interface name"));
    }

    #[test]
    fn test_validation_skew_threshold_too_low() {
        let json = r#"{
            "can": {"interface": "vcan0", "enabled": true},
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
            "can": {"interface": "vcan0", "enabled": true},
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
            "can": {"interface": "vcan0", "enabled": true, "source_filter": {
                "pgn_source_map": {
                    "129025": 22,
                    "30000": 10,
                    "250000": 5
                }
            }},
            "time": {"skew_threshold_ms": 500},
            "database": {
                "connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"},
                "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30},
                "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}
            }
        }"#;

        let mut config: Config = serde_json::from_str(json).unwrap();
        config.validate_and_fix().unwrap();

        // Valid PGN should remain
        assert_eq!(config.can.source_filter.pgn_source_map.get(&129025), Some(&22));
        // Invalid PGNs should be removed
        assert_eq!(config.can.source_filter.pgn_source_map.get(&30000), None);
        assert_eq!(config.can.source_filter.pgn_source_map.get(&250000), None);
    }

    #[test]
    fn test_validation_source_out_of_range() {
        let json = r#"{
            "can": {"interface": "vcan0", "enabled": true, "source_filter": {
                "pgn_source_map": {
                    "129025": 22,
                    "129026": 0,
                    "129027": 255,
                    "129028": 251,
                    "129029": 252,
                    "129030": 253
                }
            }},
            "time": {"skew_threshold_ms": 500},
            "database": {
                "connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"},
                "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30},
                "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}
            }
        }"#;

        let mut config: Config = serde_json::from_str(json).unwrap();
        config.validate_and_fix().unwrap();

        // Valid sources should remain (0, 22, 255 are valid)
        assert_eq!(config.can.source_filter.pgn_source_map.get(&129025), Some(&22));
        assert_eq!(config.can.source_filter.pgn_source_map.get(&129026), Some(&0));
        assert_eq!(config.can.source_filter.pgn_source_map.get(&129027), Some(&255));
        // Invalid sources (251-253 are reserved) should be removed
        assert_eq!(config.can.source_filter.pgn_source_map.get(&129028), None);
        assert_eq!(config.can.source_filter.pgn_source_map.get(&129029), None);
        assert_eq!(config.can.source_filter.pgn_source_map.get(&129030), None);
    }

    #[test]
    fn test_set_system_time_safe_deserialization_bool() {
        // Test normal boolean values
        let json = r#"{"can": {"interface": "vcan0", "enabled": true}, "time": {"skew_threshold_ms": 500, "set_system_time": true}, "database": {"connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"}, "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30}, "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert!(config.time.set_system_time);

        let json = r#"{"can": {"interface": "vcan0", "enabled": true}, "time": {"skew_threshold_ms": 500, "set_system_time": false}, "database": {"connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"}, "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30}, "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert!(!config.time.set_system_time);
    }

    #[test]
    fn test_set_system_time_safe_deserialization_string() {
        // Test string values
        let json = r#"{"can": {"interface": "vcan0", "enabled": true}, "time": {"skew_threshold_ms": 500, "set_system_time": "true"}, "database": {"connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"}, "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30}, "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert!(config.time.set_system_time);

        let json = r#"{"can": {"interface": "vcan0", "enabled": true}, "time": {"skew_threshold_ms": 500, "set_system_time": "yes"}, "database": {"connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"}, "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30}, "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert!(config.time.set_system_time);

        let json = r#"{"can": {"interface": "vcan0", "enabled": true}, "time": {"skew_threshold_ms": 500, "set_system_time": "no"}, "database": {"connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"}, "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30}, "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert!(!config.time.set_system_time);
    }

    #[test]
    fn test_set_system_time_safe_deserialization_number() {
        // Test number values
        let json = r#"{"can": {"interface": "vcan0", "enabled": true}, "time": {"skew_threshold_ms": 500, "set_system_time": 1}, "database": {"connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"}, "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30}, "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert!(config.time.set_system_time);

        let json = r#"{"can": {"interface": "vcan0", "enabled": true}, "time": {"skew_threshold_ms": 500, "set_system_time": 0}, "database": {"connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"}, "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30}, "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert!(!config.time.set_system_time);
    }

    #[test]
    fn test_set_system_time_safe_deserialization_malformed() {
        // Test malformed values - should default to false
        let json = r#"{"can": {"interface": "vcan0", "enabled": true}, "time": {"skew_threshold_ms": 500, "set_system_time": "invalid"}, "database": {"connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"}, "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30}, "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert!(!config.time.set_system_time);
    }

    #[test]
    fn test_set_system_time_safe_deserialization_missing() {
        // Test missing value - should default to false
        let json = r#"{"can": {"interface": "vcan0", "enabled": true}, "time": {"skew_threshold_ms": 500}, "database": {"connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"}, "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30}, "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert!(!config.time.set_system_time);
    }

    #[test]
    fn test_web_config_default() {
        let config = WebConfig::default();
        assert!(config.enabled);
        assert_eq!(config.port, 8080);
    }

    #[test]
    fn test_web_config_custom() {
        let config = WebConfig {
            enabled: false,
            port: 9000,
            auth_password: None,
            session_duration_secs: default_session_duration_secs(),
            secure_cookies: false,
            read_only: false,
        };
        assert!(!config.enabled);
        assert_eq!(config.port, 9000);
    }

    #[test]
    fn test_web_config_serialization() {
        let config = WebConfig {
            enabled: true,
            port: 3000,
            auth_password: None,
            session_duration_secs: default_session_duration_secs(),
            secure_cookies: true,
            read_only: false,
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("3000"));
        assert!(json.contains("true"));

        let deserialized: WebConfig = serde_json::from_str(&json).unwrap();
        assert!(deserialized.enabled);
        assert_eq!(deserialized.port, 3000);
    }

    #[test]
    fn test_web_config_deserialization_with_defaults() {
        // Test with missing fields - should use defaults
        let json = r#"{}"#;
        let config: WebConfig = serde_json::from_str(json).unwrap();
        assert!(config.enabled);
        assert_eq!(config.port, 8080);
    }

    #[test]
    fn test_web_config_in_full_config() {
        let json = r#"{
            "can": {"interface": "vcan0", "enabled": true},
            "time": {"skew_threshold_ms": 500},
            "web": {
                "enabled": false,
                "port": 3000
            },
            "database": {
                "connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"},
                "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30},
                "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}
            }
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert!(!config.web.enabled);
        assert_eq!(config.web.port, 3000);
    }

    #[test]
    fn test_web_config_missing_in_full_config() {
        // Test with missing web config - should use defaults
        let json = r#"{
            "can": {"interface": "vcan0", "enabled": true},
            "time": {"skew_threshold_ms": 500},
            "database": {
                "connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"},
                "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30},
                "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}
            }
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert!(config.web.enabled);
        assert_eq!(config.web.port, 8080);
    }

    #[test]
    fn test_udp_config_default() {
        let config = UdpConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.address, "192.168.1.255:10110");
    }

    #[test]
    fn test_udp_config_custom() {
        let config = UdpConfig {
            enabled: true,
            address: "224.0.0.1:5555".to_string(),
            bind_address: "0.0.0.0:0".to_string(),
        };
        assert!(config.enabled);
        assert_eq!(config.address, "224.0.0.1:5555");
    }

    #[test]
    fn test_udp_config_serialization() {
        let config = UdpConfig {
            enabled: true,
            address: "192.168.100.255:12345".to_string(),
            bind_address: "0.0.0.0:0".to_string(),
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("192.168.100.255:12345"));
        assert!(json.contains("true"));

        let deserialized: UdpConfig = serde_json::from_str(&json).unwrap();
        assert!(deserialized.enabled);
        assert_eq!(deserialized.address, "192.168.100.255:12345");
    }

    #[test]
    fn test_udp_config_deserialization_with_defaults() {
        // Test with missing fields - should use defaults
        let json = r#"{}"#;
        let config: UdpConfig = serde_json::from_str(json).unwrap();
        assert!(!config.enabled);
        assert_eq!(config.address, "192.168.1.255:10110");
    }

    #[test]
    fn test_udp_config_multicast_address() {
        let config = UdpConfig {
            enabled: true,
            address: "224.0.0.1:10110".to_string(),
            bind_address: "0.0.0.0:0".to_string(),
        };
        assert_eq!(config.address, "224.0.0.1:10110");
    }

    #[test]
    fn test_udp_config_broadcast_address() {
        let config = UdpConfig {
            enabled: true,
            address: "255.255.255.255:10110".to_string(),
            bind_address: "0.0.0.0:0".to_string(),
        };
        assert_eq!(config.address, "255.255.255.255:10110");
    }

    #[test]
    fn test_udp_config_in_full_config() {
        let json = r#"{
            "can": {"interface": "vcan0", "enabled": true},
            "time": {"skew_threshold_ms": 500},
            "udp": {
                "enabled": true,
                "address": "224.0.0.1:5555"
            },
            "database": {
                "connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"},
                "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30},
                "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}
            }
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert!(config.udp.enabled);
        assert_eq!(config.udp.address, "224.0.0.1:5555");
    }

    #[test]
    fn test_udp_config_missing_in_full_config() {
        // Test with missing UDP config - should use defaults
        let json = r#"{
            "can": {"interface": "vcan0", "enabled": true},
            "time": {"skew_threshold_ms": 500},
            "database": {
                "connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"},
                "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30},
                "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}
            }
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert!(!config.udp.enabled);
        assert_eq!(config.udp.address, "192.168.1.255:10110");
    }

    #[test]
    fn test_full_config_with_web_and_udp() {
        let json = r#"{
            "can": {"interface": "can0", "enabled": true},
            "time": {"skew_threshold_ms": 1000, "set_system_time": false},
            "web": {
                "enabled": true,
                "port": 8888
            },
            "udp": {
                "enabled": true,
                "address": "192.168.1.100:9999"
            },
            "logging": {
                "directory": "/var/log/nmea",
                "file_prefix": "router",
                "level": "debug"
            },
            "database": {
                "connection": {"host": "dbhost", "port": 3307, "username": "user", "password": "pass", "database_name": "db"},
                "vessel_status": {"interval_moored_seconds": 600, "interval_underway_seconds": 20},
                "environmental": {"wind_speed_seconds": 45, "wind_direction_seconds": 45, "roll_seconds": 45, "pressure_seconds": 150, "cabin_temp_seconds": 350, "water_temp_seconds": 350, "humidity_seconds": 350}
            }
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();

        // Web config
        assert!(config.web.enabled);
        assert_eq!(config.web.port, 8888);

        // UDP config
        assert!(config.udp.enabled);
        assert_eq!(config.udp.address, "192.168.1.100:9999");

        // Logging config
        assert_eq!(config.logging.directory, "/var/log/nmea");
        assert_eq!(config.logging.file_prefix, "router");
        assert_eq!(config.logging.level, "debug");

        // Other configs
        assert_eq!(config.can.interface, "can0");
        assert_eq!(config.time.skew_threshold_ms, 1000);
    }

    #[test]
    fn test_parse_database_url_valid() {
        let conn = Config::parse_database_url("mysql://user:pass@myhost:3307/mydb").unwrap();
        assert_eq!(conn.username, "user");
        assert_eq!(conn.password, "pass");
        assert_eq!(conn.host, "myhost");
        assert_eq!(conn.port, 3307);
        assert_eq!(conn.database_name, "mydb");
    }

    #[test]
    fn test_parse_database_url_invalid() {
        assert!(Config::parse_database_url("not-a-url").is_none());
        assert!(Config::parse_database_url("mysql://nocolon@host:3306/db").is_none());
    }

    #[test]
    fn test_can_config_default() {
        let can = CanConfig::default();
        assert!(can.enabled);
        assert_eq!(can.interface, "can0");
    }

    #[test]
    fn test_can_disabled_skips_interface_validation() {
        let json = r#"{
            "can": {"interface": "", "enabled": false},
            "time": {"skew_threshold_ms": 500},
            "database": {
                "connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"},
                "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30},
                "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}
            }
        }"#;
        let mut config: Config = serde_json::from_str(json).unwrap();
        assert!(config.validate_and_fix().is_ok(), "disabled CAN with empty interface should be valid");
    }

    #[test]
    fn test_sync_config_defaults() {
        let config = SyncConfig::default();
        assert!(!config.enabled);
        assert!(config.target_url.is_empty());
        assert!(config.api_key.is_none());
        assert_eq!(config.timeout_secs, 120);
        assert!(!config.accept_invalid_certs);
    }

    #[test]
    fn test_sync_config_deserialize_full() {
        let json = r#"{
            "enabled": true,
            "target_url": "https://trips.example.com",
            "api_key": "my-secret-key",
            "timeout_secs": 60,
            "accept_invalid_certs": true
        }"#;
        let config: SyncConfig = serde_json::from_str(json).unwrap();
        assert!(config.enabled);
        assert_eq!(config.target_url, "https://trips.example.com");
        assert_eq!(config.api_key.as_deref(), Some("my-secret-key"));
        assert_eq!(config.timeout_secs, 60);
        assert!(config.accept_invalid_certs);
    }

    #[test]
    fn test_sync_config_deserialize_missing_fields_use_defaults() {
        let json = r#"{}"#;
        let config: SyncConfig = serde_json::from_str(json).unwrap();
        assert!(!config.enabled);
        assert!(config.target_url.is_empty());
        assert!(config.api_key.is_none());
        assert_eq!(config.timeout_secs, 120);
    }

    #[test]
    fn test_sync_config_missing_from_full_config_uses_defaults() {
        // sync section absent → defaults applied
        let json = r#"{
            "can": {"interface": "vcan0", "enabled": false},
            "time": {"skew_threshold_ms": 500},
            "database": {
                "connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"},
                "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30},
                "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}
            }
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert!(!config.sync.enabled);
        assert!(config.sync.api_key.is_none());
    }

    #[test]
    fn test_sync_config_in_full_config() {
        let json = r#"{
            "can": {"interface": "vcan0", "enabled": false},
            "time": {"skew_threshold_ms": 500},
            "database": {
                "connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"},
                "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30},
                "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}
            },
            "sync": {
                "enabled": true,
                "target_url": "https://trips.example.com",
                "api_key": "secret",
                "timeout_secs": 90
            }
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert!(config.sync.enabled);
        assert_eq!(config.sync.target_url, "https://trips.example.com");
        assert_eq!(config.sync.api_key.as_deref(), Some("secret"));
        assert_eq!(config.sync.timeout_secs, 90);
    }
}
