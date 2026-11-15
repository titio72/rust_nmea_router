-- Shrink vessel_status angle and speed columns
--   angles (average_wind_angle_deg, cog_deg, average_heading_deg): DECIMAL(6,3) -> DECIMAL(4,1)  (4 -> 3 bytes)
--   speeds (average_speed_kn, max_speed_kn, average_wind_speed_kn): DECIMAL(6,3) -> DECIMAL(4,2)  (4 -> 2 bytes)
--
-- The ALTER rebuilds the whole table: stop nmea_router and take a backup
-- (scripts/backup.sh) before running it.

-- 1. Speeds must fit DECIMAL(4,2) (max 99.99) or the ALTER fails under strict
--    SQL mode. Check for out-of-range rows (GPS glitch spikes) first:
SELECT COUNT(*) AS out_of_range_rows FROM vessel_status
WHERE ABS(average_speed_kn) > 99.99
   OR ABS(max_speed_kn) > 99.99
   OR ABS(average_wind_speed_kn) > 99.99;

-- 2. Clamp them (the app clamps the same way on insert from now on).
--    If step 1 returned rows, trip_legs_cache entries covering them still hold
--    the old max speed: invalidate them afterwards (see DB_ANALYST.md).
UPDATE vessel_status SET average_speed_kn      = SIGN(average_speed_kn)      * 99.99 WHERE ABS(average_speed_kn)      > 99.99;
UPDATE vessel_status SET max_speed_kn          = SIGN(max_speed_kn)          * 99.99 WHERE ABS(max_speed_kn)          > 99.99;
UPDATE vessel_status SET average_wind_speed_kn = SIGN(average_wind_speed_kn) * 99.99 WHERE ABS(average_wind_speed_kn) > 99.99;

-- 3. Change the column types (values are rounded to the new scale).
ALTER TABLE vessel_status
  MODIFY average_speed_kn       DECIMAL(4,2) NOT NULL COMMENT 'Average speed over reporting period in knots',
  MODIFY max_speed_kn           DECIMAL(4,2) NOT NULL COMMENT 'Maximum speed over reporting period in knots',
  MODIFY average_wind_speed_kn  DECIMAL(4,2)          COMMENT 'Average wind speed over reporting period in knots (NULL if no wind data)',
  MODIFY average_wind_angle_deg DECIMAL(4,1)          COMMENT 'Average wind direction over reporting period in degrees (NULL if no wind data)',
  MODIFY cog_deg                DECIMAL(4,1)          COMMENT 'Course over ground over reporting period in degrees (NULL if no position fix)',
  MODIFY average_heading_deg    DECIMAL(4,1)          COMMENT 'Average heading over reporting period in degrees (NULL if no heading data)';
