-- Forecast areas become global (no trip relationship)

-- 1. Rename trip_forecast_area → forecast_area, drop trip_id
RENAME TABLE trip_forecast_area TO forecast_area;
ALTER TABLE forecast_area DROP INDEX idx_trip_id;
ALTER TABLE forecast_area DROP COLUMN trip_id;

-- 2. Drop trip_id from forecast_fetch (keep area_id FK intact)
ALTER TABLE forecast_fetch DROP INDEX idx_trip_id;
ALTER TABLE forecast_fetch DROP COLUMN trip_id;

-- 3. Truncate stale forecast data (rows carry trip context that no longer applies)
SET FOREIGN_KEY_CHECKS = 0;
TRUNCATE TABLE forecast_hourly;
TRUNCATE TABLE forecast_fetch;
SET FOREIGN_KEY_CHECKS = 1;
