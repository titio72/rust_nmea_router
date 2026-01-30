-- MariaDB/MySQL Database Schema for NMEA2000 Router
-- 
-- Create database and user:
-- CREATE DATABASE nmea_router;
-- CREATE USER 'nmea'@'localhost' IDENTIFIED BY 'nmea';
-- GRANT ALL PRIVILEGES ON nmea_router.* TO 'nmea'@'localhost';
-- FLUSH PRIVILEGES;

USE nmea_router;

-- ============================================================================
-- VESSEL STATUS TABLE
-- ============================================================================
-- Stores vessel navigation status reports
-- Reports generated every 30 seconds while underway, 10 minutes while moored
CREATE TABLE IF NOT EXISTS vessel_status (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    timestamp DATETIME(3) NOT NULL COMMENT 'Report generation time in UTC with millisecond precision',
    latitude DOUBLE COMMENT 'Vessel latitude in decimal degrees (NULL if no position fix)',
    longitude DOUBLE COMMENT 'Vessel longitude in decimal degrees (NULL if no position fix)',
    average_speed_kn DECIMAL(6,3) NOT NULL COMMENT 'Average speed over reporting period in knots',
    max_speed_kn DECIMAL(6,3) NOT NULL COMMENT 'Maximum speed over reporting period in knots',
    average_wind_speed_kn DECIMAL(6,3) COMMENT 'Average wind speed over reporting period in knots (NULL if no wind data)',
    average_wind_angle_deg DECIMAL(6,3) COMMENT 'Average wind direction over reporting period in degrees (NULL if no wind data)',
    is_moored BOOLEAN NOT NULL COMMENT 'TRUE if vessel is moored (position stable for 2+ minutes within 30m radius)',
    engine_on BOOLEAN NOT NULL DEFAULT FALSE COMMENT 'TRUE if engine is running',
    total_distance_nm DOUBLE NOT NULL DEFAULT 0 COMMENT 'Distance traveled since last report in nautical miles (straight-line Haversine)',
    total_time_ms BIGINT NOT NULL DEFAULT 0 COMMENT 'Time elapsed since last report in milliseconds',
    cog_deg DECIMAL(6,3) COMMENT 'Course over ground over reporting period in degrees (NULL if no position fix)',
    average_heading_deg DECIMAL(6,3) COMMENT 'Average heading over reporting period in degrees (NULL if no heading data)',
    INDEX idx_timestamp (timestamp),
    INDEX idx_moored (is_moored, timestamp)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='Stores vessel navigation status reports';

-- ============================================================================
-- ENVIRONMENTAL DATA TABLE
-- ============================================================================
-- Stores environmental sensor readings with metric-based persistence
-- Each metric has its own configurable persistence interval
CREATE TABLE IF NOT EXISTS environmental_data (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    timestamp DATETIME(3) NOT NULL COMMENT 'Reading timestamp in UTC with millisecond precision',
    metric_id TINYINT UNSIGNED NOT NULL COMMENT '1=Pressure, 2=CabinTemp, 3=WaterTemp, 4=Humidity, 5=WindSpeed, 6=WindDir, 7=Roll',
    value_avg FLOAT COMMENT 'Average value over collection period',
    value_max FLOAT COMMENT 'Maximum value over collection period',
    value_min FLOAT COMMENT 'Minimum value over collection period',
    unit CHAR(10) COMMENT 'Unit of measurement (Pa, C, %, m/s, deg)',
    UNIQUE KEY unique_metric_time (timestamp, metric_id),
    INDEX idx_timestamp (timestamp),
    INDEX idx_metric (metric_id, timestamp)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='Stores environmental sensor data with min/max/avg aggregation';

-- ============================================================================
-- TRIPS TABLE
-- ============================================================================
-- Stores vessel trips with automatic boundary detection (24-hour inactivity)
-- Separates sailing, motoring, and moored time/distance
CREATE TABLE IF NOT EXISTS trips (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    description VARCHAR(255) NOT NULL COMMENT 'Trip name, auto-generated as "Trip YYYY-MM-DD"',
    start_timestamp DATETIME(3) NOT NULL COMMENT 'Trip start time in UTC',
    end_timestamp DATETIME(3) NOT NULL COMMENT 'Trip end time in UTC (updated with each status report)',
    total_distance_sailed DOUBLE NOT NULL DEFAULT 0 COMMENT 'Distance traveled under sail in nautical miles',
    total_distance_motoring DOUBLE NOT NULL DEFAULT 0 COMMENT 'Distance traveled with engine in nautical miles',
    total_time_sailing BIGINT NOT NULL DEFAULT 0 COMMENT 'Time spent sailing in milliseconds',
    total_time_motoring BIGINT NOT NULL DEFAULT 0 COMMENT 'Time spent motoring in milliseconds',
    total_time_moored BIGINT NOT NULL DEFAULT 0 COMMENT 'Time spent moored in milliseconds',
    INDEX idx_end_timestamp (end_timestamp),
    INDEX idx_start_timestamp (start_timestamp)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='Stores vessel trips with sailing vs motoring breakdown';

-- ============================================================================
-- EXAMPLE QUERIES
-- ============================================================================

-- Get latest vessel status:
-- SELECT * FROM vessel_status ORDER BY timestamp DESC LIMIT 1;

-- Get average speed over last hour:
-- SELECT AVG(average_speed_kn) as avg_speed_knots 
-- FROM vessel_status 
-- WHERE timestamp >= NOW() - INTERVAL 1 HOUR;

-- Get current trip summary:
-- SELECT 
--     description,
--     start_timestamp,
--     end_timestamp,
--     ROUND(total_distance_sailed + total_distance_motoring, 2) as total_nm,
--     CONCAT(FLOOR(total_time_sailing / 3600000), 'h ', 
--            FLOOR((total_time_sailing % 3600000) / 60000), 'm') as time_sailing,
--     ROUND(total_distance_sailed / (total_distance_sailed + total_distance_motoring) * 100, 1) as sail_percentage
-- FROM trips 
-- ORDER BY end_timestamp DESC 
-- LIMIT 1;

-- Get latest environmental readings:
-- SELECT 
--     timestamp,
--     MAX(CASE WHEN metric_id = 1 THEN value_avg END) as pressure_pa,
--     MAX(CASE WHEN metric_id = 2 THEN value_avg END) as cabin_temp_c,
--     MAX(CASE WHEN metric_id = 3 THEN value_avg END) as water_temp_c,
--     MAX(CASE WHEN metric_id = 4 THEN value_avg END) as humidity_pct,
--     MAX(CASE WHEN metric_id = 5 THEN value_avg END) as wind_speed_ms,
--     MAX(CASE WHEN metric_id = 6 THEN value_avg END) as wind_dir_deg,
--     MAX(CASE WHEN metric_id = 7 THEN value_avg END) as roll_deg
-- FROM environmental_data
-- WHERE timestamp >= NOW() - INTERVAL 1 HOUR
-- GROUP BY timestamp
-- ORDER BY timestamp DESC
-- LIMIT 10;

-- Get mooring events (transitions from moving to moored):
-- SELECT 
--     timestamp,
--     latitude,
--     longitude,
--     'Moored' as event
-- FROM vessel_status v1
-- WHERE is_moored = TRUE
--   AND NOT EXISTS (
--       SELECT 1 FROM vessel_status v2 
--       WHERE v2.timestamp < v1.timestamp 
--         AND v2.timestamp >= v1.timestamp - INTERVAL 5 MINUTE
--         AND v2.is_moored = TRUE
--   )
-- ORDER BY timestamp DESC;
