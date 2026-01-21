-- MariaDB/MySQL Database Schema for NMEA2000 Router
-- 
-- Create database and user:
-- CREATE DATABASE nmea_router;
-- CREATE USER 'nmea'@'localhost' IDENTIFIED BY 'nmea';
-- GRANT ALL PRIVILEGES ON nmea_router.* TO 'nmea'@'localhost';
-- FLUSH PRIVILEGES;

USE nmea_router;

-- Table to store vessel status reports
CREATE TABLE IF NOT EXISTS vessel_status (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    timestamp DATETIME(3) NOT NULL COMMENT 'Report generation time in UTC with millisecond precision',
    latitude DOUBLE COMMENT 'Vessel latitude in decimal degrees (NULL if no position fix)',
    longitude DOUBLE COMMENT 'Vessel longitude in decimal degrees (NULL if no position fix)',
    average_speed_ms DOUBLE NOT NULL COMMENT 'Average speed over last 30 seconds in meters/second',
    max_speed_ms DOUBLE NOT NULL COMMENT 'Maximum speed over last 30 seconds in meters/second',
    is_moored BOOLEAN NOT NULL COMMENT 'TRUE if vessel is moored (position stable for 2+ minutes within 10m radius)',
    engine_on BOOLEAN NOT NULL DEFAULT FALSE COMMENT 'TRUE if engine is running',
    total_distance_m DOUBLE NOT NULL DEFAULT 0 COMMENT 'Distance traveled since last report in meters (straight-line Haversine)',
    total_time_ms BIGINT NOT NULL DEFAULT 0 COMMENT 'Time elapsed since last report in milliseconds',
    INDEX idx_timestamp (timestamp),
    INDEX idx_moored (is_moored, timestamp)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='Stores vessel navigation status reports generated every 30 seconds';

-- Example queries:
-- 
-- Get latest vessel status:
-- SELECT * FROM vessel_status ORDER BY timestamp DESC LIMIT 1;
--
-- Get average speed over last hour:
-- SELECT AVG(average_speed_ms) * 1.94384 as avg_speed_knots 
-- FROM vessel_status 
-- WHERE timestamp >= NOW() - INTERVAL 1 HOUR;
--
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
--
-- Get time series data for the last 24 hours:
-- SELECT 
--     timestamp,
--     latitude,
--     longitude,
--     average_speed_ms * 1.94384 as speed_knots,
--     is_moored
-- FROM vessel_status
-- WHERE timestamp >= NOW() - INTERVAL 24 HOUR
-- ORDER BY timestamp ASC;
