use crate::db::types::VesselDatabase;
use std::error::Error;
use std::time::Duration;
use tracing::{info, warn};
use mysql::prelude::Queryable;

impl VesselDatabase {
    /// Create a new database connection
    /// 
    /// Example connection string: "mysql://user:password@localhost:3306/nmea_router"
    /// 
    /// Required table schema:
    /// ```sql
    /// CREATE TABLE vessel_status (
    ///     id BIGINT AUTO_INCREMENT PRIMARY KEY,
    ///     timestamp DATETIME(3) NOT NULL COMMENT 'UTC timezone',
    ///     latitude DOUBLE,
    ///     longitude DOUBLE,
    ///     average_speed_kn DECIMAL(6,3) NOT NULL,
    ///     max_speed_kn DECIMAL(6,3) NOT NULL,
    ///     is_moored BOOLEAN NOT NULL,
    ///     engine_on TINYINT(1) NOT NULL DEFAULT 2, -- 0 = off, 1 = on, 2 = unknown
    ///     total_distance_nm DOUBLE NOT NULL DEFAULT 0,
    ///     total_time_ms BIGINT NOT NULL DEFAULT 0,
    ///     average_wind_speed_kn DECIMAL(6,3),
    ///     average_wind_angle_deg DECIMAL(6,3),
    ///     cog_deg DECIMAL(6,3),
    ///     average_heading_deg DECIMAL(6,3),
    ///     INDEX idx_timestamp (timestamp)
    /// );
    /// ```
    pub fn new(connection_url: &str) -> Result<Self, Box<dyn Error>> {
        use mysql::{Opts, OptsBuilder, Pool, PoolOpts, PoolConstraints};

        let opts = Opts::from_url(connection_url)?;

        // Configure connection pool with limits
        let pool_opts = PoolOpts::new()
            .with_constraints(PoolConstraints::new(2, 10).unwrap());  // min 2, max 10 connections

        // Set session timezone to UTC to ensure all timestamps are handled consistently
        // Configure timeouts to prevent hanging on unresponsive DB
        let opts_builder = OptsBuilder::from_opts(opts)
            .pool_opts(pool_opts)
            .tcp_connect_timeout(Some(Duration::from_secs(5)))  // 5s to establish connection
            .read_timeout(Some(Duration::from_secs(30)))        // 30s for query reads
            .write_timeout(Some(Duration::from_secs(30)))       // 30s for query writes
            .init(vec!["SET time_zone = '+00:00'"]);
        let pool = Pool::new(opts_builder)?;
        
        let db = VesselDatabase { 
            pool, 
            system_status_cache: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        };
        
        // Ensure system_status table exists
        if let Ok(mut conn) = db.pool.get_conn() {
            // Create table if it doesn't exist
            let _ = conn.query_drop(
                "CREATE TABLE IF NOT EXISTS system_status (
                    status_key VARCHAR(255) PRIMARY KEY,
                    status_value VARCHAR(255) NOT NULL,
                    INDEX idx_status_key (status_key)
                )"
            );
            
            // Load cache from database
            if let Ok(rows) = conn.query::<(String, String), _>("SELECT status_key, status_value FROM system_status") {
                let mut cache = db.system_status_cache.lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                for (key, value) in rows {
                    let enabled = value == "1" || value.to_lowercase() == "true";
                    cache.insert(key, enabled);
                }
            }
        }

        Ok(db)
    }
    
    /// Check database connection health using a simple query
    /// Returns Ok(()) if the connection is healthy, Err otherwise
    pub fn health_check(&self) -> Result<(), Box<dyn Error>> {
        let mut conn = self.pool.get_conn()?;
        conn.query_drop("SELECT 1")?;
        Ok(())
    }
    
    /// Attempt to reconnect to the database with exponential backoff
    /// Returns Some(VesselDatabase) if successful, None if all retries fail
    pub fn reconnect_with_retry(db_url: &str, max_retries: u32) -> Option<Self> {
        for attempt in 1..=max_retries {
            warn!("Attempting to reconnect to database (attempt {}/{})...", attempt, max_retries);
            match Self::new(db_url) {
                Ok(db) => {
                    info!("Database reconnection successful");
                    return Some(db);
                }
                Err(e) => {
                    warn!("Database reconnection attempt {} failed: {}", attempt, e);
                    if attempt < max_retries {
                        let wait_time = std::cmp::min(2_u64.pow(attempt - 1), 30); // Exponential backoff, max 30s
                        warn!("Waiting {} seconds before retry...", wait_time);
                        std::thread::sleep(Duration::from_secs(wait_time));
                    }
                }
            }
        }
        warn!("Failed to reconnect to database after {} attempts", max_retries);
        None
    }
}
