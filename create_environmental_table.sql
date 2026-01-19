-- Create environmental_data table for storing 1-minute averaged environmental metrics
-- All timestamps are in UTC timezone
-- This is a time-series table with one row per metric per time interval

CREATE TABLE IF NOT EXISTS environmental_data (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    timestamp DATETIME(3) NOT NULL COMMENT 'UTC timezone',
    metric_id TINYINT UNSIGNED NOT NULL COMMENT 'Metric identifier: 1=pressure, 2=cabin_temp, 3=water_temp, 4=humidity, 5=wind_speed, 6=wind_dir, 7=roll',
    value_avg DOUBLE COMMENT 'Average value over the 1-minute interval',
    value_max DOUBLE COMMENT 'Maximum value over the 1-minute interval',
    value_min DOUBLE COMMENT 'Minimum value over the 1-minute interval',
    unit VARCHAR(10) COMMENT 'Unit of measurement: Pa, C, %, m/s, deg',
    
    INDEX idx_timestamp (timestamp),
    INDEX idx_metric_timestamp (metric_id, timestamp),
    INDEX idx_timestamp_metric (timestamp, metric_id),
    UNIQUE KEY unique_metric_time (timestamp, metric_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='Environmental data collected from NMEA2000 bus at 1-minute intervals';

-- Metric IDs (enumeration):
-- 1 = 'pressure'    - Atmospheric pressure in Pascals (Pa)
-- 2 = 'cabin_temp'  - Cabin temperature in Celsius (°C)
-- 3 = 'water_temp'  - Water temperature in Celsius (°C)
-- 4 = 'humidity'    - Relative humidity in percent (%)
-- 5 = 'wind_speed'  - Wind speed in meters per second (m/s)
-- 6 = 'wind_dir'    - Wind direction in degrees (°)
-- 7 = 'roll'        - Roll angle in degrees (°)

-- Example query to retrieve last 24 hours of data (pivot format)
-- SELECT 
--     timestamp,
--     MAX(CASE WHEN metric_id = 1 THEN ROUND(value_avg, 0) END) as pressure_pa,
--     MAX(CASE WHEN metric_id = 2 THEN ROUND(value_avg, 1) END) as cabin_temp_c,
--     MAX(CASE WHEN metric_id = 3 THEN ROUND(value_avg, 1) END) as water_temp_c,
--     MAX(CASE WHEN metric_id = 4 THEN ROUND(value_avg, 1) END) as humidity_pct,
--     MAX(CASE WHEN metric_id = 5 THEN ROUND(value_avg * 1.94384, 1) END) as wind_speed_kt,
--     MAX(CASE WHEN metric_id = 6 THEN ROUND(value_avg, 0) END) as wind_dir_deg
-- FROM environmental_data
-- WHERE timestamp >= NOW() - INTERVAL 24 HOUR
-- GROUP BY timestamp
-- ORDER BY timestamp DESC;

-- Example query for a specific metric time series (cabin temperature)
-- SELECT 
--     timestamp,
--     ROUND(value_avg, 1) as avg,
--     ROUND(value_max, 1) as max,
--     ROUND(value_min, 1) as min
-- FROM environmental_data
-- WHERE metric_id = 2
--   AND timestamp >= NOW() - INTERVAL 24 HOUR
-- ORDER BY timestamp;
