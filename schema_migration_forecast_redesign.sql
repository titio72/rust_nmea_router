-- Migration: forecast redesign
-- Run once against the production database

-- NOTE: Run this migration ONLY after deploying the updated Rust binary.
-- The application code must not reference forecast_poi or insert into
-- forecast_fetch without trip_id/area_id before this migration is applied.
SET FOREIGN_KEY_CHECKS = 0;

-- 1. Remove incompatible old data
TRUNCATE TABLE forecast_hourly;
TRUNCATE TABLE forecast_fetch;
DROP TABLE IF EXISTS forecast_poi;

-- 2. Add new columns to forecast_fetch
-- Split into two statements: MariaDB evaluates AFTER references against the
-- existing table state, so area_id AFTER trip_id would fail if both columns
-- are added in the same ALTER TABLE.
ALTER TABLE forecast_fetch
  ADD COLUMN trip_id INT NOT NULL AFTER lon;

ALTER TABLE forecast_fetch
  ADD COLUMN area_id INT NOT NULL AFTER trip_id,
  ADD INDEX idx_trip_id (trip_id),
  ADD INDEX idx_area_id (area_id);

-- 3. Create trip_forecast_area
CREATE TABLE IF NOT EXISTS trip_forecast_area (
    id         INT AUTO_INCREMENT PRIMARY KEY,
    trip_id    INT NOT NULL,
    lat_min    DECIMAL(9,6) NOT NULL,
    lat_max    DECIMAL(9,6) NOT NULL,
    lon_min    DECIMAL(9,6) NOT NULL,
    lon_max    DECIMAL(9,6) NOT NULL,
    created_at DATETIME NOT NULL,
    INDEX idx_trip_id (trip_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- 4. Add FK from forecast_fetch to trip_forecast_area (cascade delete)
ALTER TABLE forecast_fetch
  ADD CONSTRAINT fk_forecast_fetch_area
    FOREIGN KEY (area_id) REFERENCES trip_forecast_area(id) ON DELETE CASCADE;

SET FOREIGN_KEY_CHECKS = 1;
