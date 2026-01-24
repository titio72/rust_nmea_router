-- Migration: Create trips table
-- Description: Adds trip tracking functionality to record vessel journeys
-- Date: 2026-01-24

CREATE TABLE IF NOT EXISTS trips (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    description VARCHAR(255) NOT NULL,
    start_timestamp DATETIME(3) NOT NULL COMMENT 'UTC timezone',
    end_timestamp DATETIME(3) NOT NULL COMMENT 'UTC timezone',
    total_distance_sailed DOUBLE NOT NULL DEFAULT 0 COMMENT 'Distance traveled under sail in nautical miles',
    total_distance_motoring DOUBLE NOT NULL DEFAULT 0 COMMENT 'Distance traveled with engine in nautical miles',
    total_time_sailing BIGINT NOT NULL DEFAULT 0 COMMENT 'Time spent sailing in milliseconds',
    total_time_motoring BIGINT NOT NULL DEFAULT 0 COMMENT 'Time spent motoring in milliseconds',
    total_time_moored BIGINT NOT NULL DEFAULT 0 COMMENT 'Time spent moored in milliseconds',
    INDEX idx_end_timestamp (end_timestamp),
    INDEX idx_start_timestamp (start_timestamp)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
