-- Migration: Change vessel_status distance from meters to nautical miles
-- Description: Updates the total_distance column to store nautical miles instead of meters
-- Date: 2026-01-24

-- Rename column and convert existing data from meters to nautical miles
ALTER TABLE vessel_status 
    CHANGE COLUMN total_distance_m total_distance_nm DOUBLE NOT NULL DEFAULT 0 COMMENT 'Total distance in nautical miles';

-- Update existing data: 1 nautical mile = 1852 meters
UPDATE vessel_status 
    SET total_distance_nm = total_distance_nm / 1852.0;
