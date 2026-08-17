-- MariaDB/MySQL Database Schema for NMEA2000 Router
-- 
-- Create database and user:
-- CREATE DATABASE nmea_router;
-- CREATE USER 'nmea'@'localhost' IDENTIFIED BY 'nmea';
-- GRANT ALL PRIVILEGES ON nmea_router.* TO 'nmea'@'localhost';
-- FLUSH PRIVILEGES;
--
-- Apply this file against the target database, e.g.:
--   mysql -u nmea -pnmea nmea_router < schema.sql
-- Do NOT hardcode a `USE <db>;` here: it would override the database selected on
-- the command line and silently apply the schema to the wrong database.

-- ============================================================================
-- SYSTEM STATUS TABLE
-- ============================================================================
-- Stores runtime application state that persists across restarts
-- Uses key-value pairs to track system toggles and settings
CREATE TABLE IF NOT EXISTS system_status (
    id INT AUTO_INCREMENT PRIMARY KEY,
    status_key VARCHAR(255) UNIQUE NOT NULL COMMENT 'Status key (e.g., "tracking_enabled", "metrics_enabled")',
    status_value VARCHAR(255) NOT NULL COMMENT 'Status value (e.g., "1", "0", or other values)',
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP COMMENT 'Last update time',
    INDEX idx_key (status_key)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='Stores persistent system status and runtime configuration';

-- Initialize default status values:
INSERT IGNORE INTO system_status (status_key, status_value) VALUES
('tracking_enabled', '1'),
('metrics_enabled', '1');

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
    engine_on TINYINT NOT NULL DEFAULT 2 COMMENT 'Engine status: 0=off, 1=on, 2=unknown',
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
    unit CHAR(3) COMMENT 'Unit of measurement (Pa, C, %, kn, deg)',
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
    uuid CHAR(36) NULL COMMENT 'UUID v4 for portable trip identification (used for import deduplication)',
    updated_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3) COMMENT 'Bumped by MariaDB on any UPDATE to this row; drives remote sync change-detection',
    INDEX idx_end_timestamp (end_timestamp),
    INDEX idx_start_timestamp (start_timestamp),
    INDEX idx_trips_time_range (start_timestamp, end_timestamp),
    UNIQUE INDEX idx_trips_uuid (uuid)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='Stores vessel trips with sailing vs motoring breakdown';

-- For existing databases, run:
-- ALTER TABLE trips ADD COLUMN uuid CHAR(36) NULL COMMENT 'UUID v4 for portable trip identification';
-- ALTER TABLE trips ADD UNIQUE INDEX idx_trips_uuid (uuid);
-- ALTER TABLE trips ADD INDEX idx_trips_time_range (start_timestamp, end_timestamp);
-- ALTER TABLE trips ADD COLUMN updated_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3) COMMENT 'Bumped by MariaDB on any UPDATE to this row; drives remote sync change-detection';
-- UPDATE trips SET updated_at = end_timestamp; -- backfill so only edits made after this migration trigger a re-sync

-- ============================================================================
-- HEATMAP CACHE TABLE
-- ============================================================================
-- Stores pre-computed per-day sailing distances for the heatmap UI.
-- Today is never cached (still being written); all past days are cached after first computation.
CREATE TABLE IF NOT EXISTS heatmap_cache (
    date DATE NOT NULL COMMENT 'UTC date of the aggregated sailing distance',
    distance_nm DOUBLE NOT NULL DEFAULT 0 COMMENT 'Total distance (sailing + motoring) in nautical miles',
    sailing_distance_nm DOUBLE NOT NULL DEFAULT 0 COMMENT 'Distance with engine off (engine_on=0) in nautical miles',
    motoring_distance_nm DOUBLE NOT NULL DEFAULT 0 COMMENT 'Distance with engine on (engine_on=1) in nautical miles',
    PRIMARY KEY (date)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='Per-day heatmap distance cache; recomputed only for missing past days and today';

-- For existing databases, run:
-- ALTER TABLE heatmap_cache
--     ADD COLUMN IF NOT EXISTS sailing_distance_nm DOUBLE NOT NULL DEFAULT 0,
--     ADD COLUMN IF NOT EXISTS motoring_distance_nm DOUBLE NOT NULL DEFAULT 0;

-- Pre-computed leg breakdown per trip. Invalidated on trip mutation (trim/delete) and repopulated
-- on the next fetch. Only closed trips (end_timestamp > 24h ago) are cached.
-- sailing_time_formatted/motoring_time_formatted are NOT stored; derived at read time.
CREATE TABLE IF NOT EXISTS trip_legs_cache (
    trip_id              INT UNSIGNED    NOT NULL COMMENT 'Trip this leg belongs to (mirrors trips.id, no FK)',
    leg_number           INT UNSIGNED    NOT NULL COMMENT 'Leg sequence within the trip, starting at 1',
    start_timestamp      VARCHAR(30)     NOT NULL COMMENT 'Leg start time as ISO-8601 string',
    end_timestamp        VARCHAR(30)     NOT NULL COMMENT 'Leg end time as ISO-8601 string',
    total_distance_nm    DOUBLE          NOT NULL DEFAULT 0,
    sailing_distance_nm  DOUBLE          NOT NULL DEFAULT 0,
    motoring_distance_nm DOUBLE          NOT NULL DEFAULT 0,
    sailing_time_ms      BIGINT UNSIGNED NOT NULL DEFAULT 0,
    motoring_time_ms     BIGINT UNSIGNED NOT NULL DEFAULT 0,
    start_lat            DOUBLE          NULL COMMENT 'Latitude at leg start in decimal degrees',
    start_lon            DOUBLE          NULL COMMENT 'Longitude at leg start in decimal degrees',
    end_lat              DOUBLE          NULL COMMENT 'Latitude at leg end in decimal degrees',
    end_lon              DOUBLE          NULL COMMENT 'Longitude at leg end in decimal degrees',
    nav_start_timestamp  VARCHAR(30)     NULL COMMENT 'Start of pure navigation window (engine off or speed >= 4 kn)',
    nav_end_timestamp    VARCHAR(30)     NULL COMMENT 'End of pure navigation window',
    nav_distance_nm      DOUBLE          NOT NULL DEFAULT 0 COMMENT 'Distance within the nav window',
    nav_time_ms          BIGINT UNSIGNED NOT NULL DEFAULT 0 COMMENT 'Duration of the nav window in ms',
    nav_detection_method VARCHAR(20)     NULL COMMENT 'engine_transition | speed_fallback',
    max_speed_kn                 DOUBLE          NULL COMMENT 'Fastest speed recorded while engine off',
    max_speed_timestamp          VARCHAR(30)     NULL COMMENT 'Timestamp of max_speed_kn',
    fastest_1nm_distance_nm      DOUBLE          NULL COMMENT 'Distance of fastest >=1nm segment',
    fastest_1nm_avg_speed_kn     DOUBLE          NULL COMMENT 'Average speed of fastest >=1nm segment',
    fastest_1nm_duration_ms      BIGINT UNSIGNED NULL COMMENT 'Duration of fastest >=1nm segment',
    fastest_1nm_start_timestamp  VARCHAR(30)     NULL COMMENT 'Start of fastest >=1nm segment',
    fastest_1nm_end_timestamp    VARCHAR(30)     NULL COMMENT 'End of fastest >=1nm segment',
    fastest_5nm_distance_nm      DOUBLE          NULL COMMENT 'Distance of fastest >=5nm segment',
    fastest_5nm_avg_speed_kn     DOUBLE          NULL COMMENT 'Average speed of fastest >=5nm segment',
    fastest_5nm_duration_ms      BIGINT UNSIGNED NULL COMMENT 'Duration of fastest >=5nm segment',
    fastest_5nm_start_timestamp  VARCHAR(30)     NULL COMMENT 'Start of fastest >=5nm segment',
    fastest_5nm_end_timestamp    VARCHAR(30)     NULL COMMENT 'End of fastest >=5nm segment',
    fastest_10nm_distance_nm     DOUBLE          NULL COMMENT 'Distance of fastest >=10nm segment',
    fastest_10nm_avg_speed_kn    DOUBLE          NULL COMMENT 'Average speed of fastest >=10nm segment',
    fastest_10nm_duration_ms     BIGINT UNSIGNED NULL COMMENT 'Duration of fastest >=10nm segment',
    fastest_10nm_start_timestamp VARCHAR(30)     NULL COMMENT 'Start of fastest >=10nm segment',
    fastest_10nm_end_timestamp   VARCHAR(30)     NULL COMMENT 'End of fastest >=10nm segment',
    fastest_25nm_distance_nm     DOUBLE          NULL COMMENT 'Distance of fastest >=25nm segment',
    fastest_25nm_avg_speed_kn    DOUBLE          NULL COMMENT 'Average speed of fastest >=25nm segment',
    fastest_25nm_duration_ms     BIGINT UNSIGNED NULL COMMENT 'Duration of fastest >=25nm segment',
    fastest_25nm_start_timestamp VARCHAR(30)     NULL COMMENT 'Start of fastest >=25nm segment',
    fastest_25nm_end_timestamp   VARCHAR(30)     NULL COMMENT 'End of fastest >=25nm segment',
    PRIMARY KEY (trip_id, leg_number)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='Cached trip leg analysis; invalidated on trim/delete, recomputed on next fetch';

-- For existing databases, run:
-- CREATE TABLE IF NOT EXISTS trip_legs_cache (trip_id INT UNSIGNED NOT NULL, leg_number INT UNSIGNED NOT NULL, start_timestamp VARCHAR(30) NOT NULL, end_timestamp VARCHAR(30) NOT NULL, total_distance_nm DOUBLE NOT NULL DEFAULT 0, sailing_distance_nm DOUBLE NOT NULL DEFAULT 0, motoring_distance_nm DOUBLE NOT NULL DEFAULT 0, sailing_time_ms BIGINT UNSIGNED NOT NULL DEFAULT 0, motoring_time_ms BIGINT UNSIGNED NOT NULL DEFAULT 0, PRIMARY KEY (trip_id, leg_number)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS start_lat DOUBLE NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS start_lon DOUBLE NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS end_lat DOUBLE NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS end_lon DOUBLE NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS nav_start_timestamp VARCHAR(30) NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS nav_end_timestamp VARCHAR(30) NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS nav_distance_nm DOUBLE NOT NULL DEFAULT 0;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS nav_time_ms BIGINT UNSIGNED NOT NULL DEFAULT 0;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS nav_detection_method VARCHAR(20) NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS max_speed_kn DOUBLE NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS max_speed_timestamp VARCHAR(30) NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS fastest_1nm_distance_nm DOUBLE NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS fastest_1nm_avg_speed_kn DOUBLE NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS fastest_1nm_duration_ms BIGINT UNSIGNED NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS fastest_1nm_start_timestamp VARCHAR(30) NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS fastest_1nm_end_timestamp VARCHAR(30) NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS fastest_5nm_distance_nm DOUBLE NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS fastest_5nm_avg_speed_kn DOUBLE NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS fastest_5nm_duration_ms BIGINT UNSIGNED NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS fastest_5nm_start_timestamp VARCHAR(30) NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS fastest_5nm_end_timestamp VARCHAR(30) NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS fastest_10nm_distance_nm DOUBLE NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS fastest_10nm_avg_speed_kn DOUBLE NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS fastest_10nm_duration_ms BIGINT UNSIGNED NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS fastest_10nm_start_timestamp VARCHAR(30) NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS fastest_10nm_end_timestamp VARCHAR(30) NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS fastest_25nm_distance_nm DOUBLE NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS fastest_25nm_avg_speed_kn DOUBLE NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS fastest_25nm_duration_ms BIGINT UNSIGNED NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS fastest_25nm_start_timestamp VARCHAR(30) NULL;
-- ALTER TABLE trip_legs_cache ADD COLUMN IF NOT EXISTS fastest_25nm_end_timestamp VARCHAR(30) NULL;

CREATE TABLE IF NOT EXISTS trip_legs_nav_overrides (
    trip_id          INT UNSIGNED NOT NULL COMMENT 'Trip ID (mirrors trips.id, no FK)',
    leg_number       INT UNSIGNED NOT NULL COMMENT 'Leg number within the trip',
    nav_start        VARCHAR(30)  NULL     COMMENT 'User-corrected nav start (ISO-8601)',
    nav_end          VARCHAR(30)  NULL     COMMENT 'User-corrected nav end (ISO-8601)',
    auto_nav_start   VARCHAR(30)  NULL     COMMENT 'Algorithm-detected nav start at time of override',
    auto_nav_end     VARCHAR(30)  NULL     COMMENT 'Algorithm-detected nav end at time of override',
    corrected_at     DATETIME(3)  NOT NULL COMMENT 'UTC timestamp of when the override was set',
    PRIMARY KEY (trip_id, leg_number)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='User corrections for nav windows; auto_nav_* preserved for calibration analysis';

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

-- ============================================================================
-- FORECAST AREA TABLE
-- ============================================================================
CREATE TABLE IF NOT EXISTS forecast_area (
    id         INT AUTO_INCREMENT PRIMARY KEY,
    lat_min    DECIMAL(9,6) NOT NULL,
    lat_max    DECIMAL(9,6) NOT NULL,
    lon_min    DECIMAL(9,6) NOT NULL,
    lon_max    DECIMAL(9,6) NOT NULL,
    created_at DATETIME NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='Bounding boxes defining global forecast areas';

-- ============================================================================
-- FORECAST FETCH TABLE
-- ============================================================================
CREATE TABLE IF NOT EXISTS forecast_fetch (
    id              INT AUTO_INCREMENT PRIMARY KEY,
    area_id         INT NOT NULL,
    model           VARCHAR(16) NOT NULL DEFAULT 'ecmwf',
    lat             DECIMAL(9,6) NOT NULL,
    lon             DECIMAL(9,6) NOT NULL,
    fetched_at      DATETIME NOT NULL,
    forecast_from   DATETIME NOT NULL,
    forecast_to     DATETIME NOT NULL,
    INDEX idx_area_id (area_id),
    INDEX idx_fetched_at (fetched_at),
    CONSTRAINT fk_forecast_fetch_area FOREIGN KEY (area_id) REFERENCES forecast_area(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='One record per grid point per fetch operation, tagged with area and model';

-- ============================================================================
-- FORECAST HOURLY TABLE
-- ============================================================================
CREATE TABLE IF NOT EXISTS forecast_hourly (
    id                  INT AUTO_INCREMENT PRIMARY KEY,
    fetch_id            INT NOT NULL,
    timestamp           DATETIME NOT NULL,
    wind_speed_kn       DECIMAL(6,2),
    wind_direction_deg  DECIMAL(5,1),
    wind_gust_kn        DECIMAL(6,2),
    wave_height_m       DECIMAL(5,2),
    wave_period_s       DECIMAL(5,2),
    wave_direction_deg  DECIMAL(5,1),
    cape_j_kg           DECIMAL(8,2),
    INDEX idx_fetch_ts (fetch_id, timestamp),
    CONSTRAINT fk_forecast_hourly_fetch FOREIGN KEY (fetch_id) REFERENCES forecast_fetch(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='One row per forecasted hour per fetch; all timestamps UTC';
