-- Point-of-sail (upwind/reaching/running) backfill for a Railway-hosted trips_viewer
-- (or any remote instance) already running the point-of-sail-aware binary.
--
-- Run this manually against the remote database, e.g.:
--   mysql -h <host> -P <port> -u <user> -p<password> <database> < scripts/backfill_point_of_sail.sql
--
-- Follows this project's write protocol (see DB_ANALYST.md): preview, transact, verify.
-- Safe to re-run: the ALTER block is idempotent (errors on already-existing columns are
-- ignorable), and the UPDATE recomputes deterministically from vessel_status each time.

-- ---------------------------------------------------------------------------
-- Step 0 (optional safety net): ensure the 6 trips columns exist.
-- The app already self-migrates these at every startup (VesselDatabase::new(),
-- src/db/connection.rs) if the currently-deployed binary includes that fix — so on an
-- already-deployed Railway instance this should be a no-op. Included defensively in
-- case the app process hasn't restarted since deploy. Run each line separately if your
-- client aborts the whole script on the first "Duplicate column" error.
-- ---------------------------------------------------------------------------
ALTER TABLE trips ADD COLUMN total_distance_upwind DOUBLE NOT NULL DEFAULT 0;
ALTER TABLE trips ADD COLUMN total_distance_reaching DOUBLE NOT NULL DEFAULT 0;
ALTER TABLE trips ADD COLUMN total_distance_running DOUBLE NOT NULL DEFAULT 0;
ALTER TABLE trips ADD COLUMN total_time_upwind BIGINT NOT NULL DEFAULT 0;
ALTER TABLE trips ADD COLUMN total_time_reaching BIGINT NOT NULL DEFAULT 0;
ALTER TABLE trips ADD COLUMN total_time_running BIGINT NOT NULL DEFAULT 0;

-- ---------------------------------------------------------------------------
-- Step 1: Preview before backfilling.
-- ---------------------------------------------------------------------------
SELECT COUNT(*) AS total_trips,
       SUM(CASE WHEN total_distance_sailed > 0 THEN 1 ELSE 0 END) AS trips_with_sailing,
       SUM(CASE WHEN total_distance_upwind + total_distance_reaching + total_distance_running > 0
                THEN 1 ELSE 0 END) AS trips_already_backfilled
FROM trips;

-- ---------------------------------------------------------------------------
-- Step 2: Backfill every trip's point-of-sail totals from its own vessel_status window.
-- Same folding/bucketing as the app: fold TWA to 0-180 via LEAST(x, 360-x), then
-- upwind <= 60, reaching 60-120 (exclusive), running >= 120. Sailing-only
-- (is_moored = 0 AND engine_on != 1), and only rows with a recorded wind angle.
-- ---------------------------------------------------------------------------
START TRANSACTION;

UPDATE trips t SET
  total_distance_upwind = (SELECT COALESCE(SUM(CASE WHEN vs.is_moored=0 AND vs.engine_on!=1
    AND vs.average_wind_angle_deg IS NOT NULL
    AND LEAST(vs.average_wind_angle_deg, 360-vs.average_wind_angle_deg) <= 60
    THEN vs.total_distance_nm ELSE 0 END), 0)
    FROM vessel_status vs WHERE vs.timestamp BETWEEN t.start_timestamp AND t.end_timestamp),
  total_distance_reaching = (SELECT COALESCE(SUM(CASE WHEN vs.is_moored=0 AND vs.engine_on!=1
    AND vs.average_wind_angle_deg IS NOT NULL
    AND LEAST(vs.average_wind_angle_deg, 360-vs.average_wind_angle_deg) > 60
    AND LEAST(vs.average_wind_angle_deg, 360-vs.average_wind_angle_deg) < 120
    THEN vs.total_distance_nm ELSE 0 END), 0)
    FROM vessel_status vs WHERE vs.timestamp BETWEEN t.start_timestamp AND t.end_timestamp),
  total_distance_running = (SELECT COALESCE(SUM(CASE WHEN vs.is_moored=0 AND vs.engine_on!=1
    AND vs.average_wind_angle_deg IS NOT NULL
    AND LEAST(vs.average_wind_angle_deg, 360-vs.average_wind_angle_deg) >= 120
    THEN vs.total_distance_nm ELSE 0 END), 0)
    FROM vessel_status vs WHERE vs.timestamp BETWEEN t.start_timestamp AND t.end_timestamp),
  total_time_upwind = (SELECT COALESCE(SUM(CASE WHEN vs.is_moored=0 AND vs.engine_on!=1
    AND vs.average_wind_angle_deg IS NOT NULL
    AND LEAST(vs.average_wind_angle_deg, 360-vs.average_wind_angle_deg) <= 60
    THEN vs.total_time_ms ELSE 0 END), 0)
    FROM vessel_status vs WHERE vs.timestamp BETWEEN t.start_timestamp AND t.end_timestamp),
  total_time_reaching = (SELECT COALESCE(SUM(CASE WHEN vs.is_moored=0 AND vs.engine_on!=1
    AND vs.average_wind_angle_deg IS NOT NULL
    AND LEAST(vs.average_wind_angle_deg, 360-vs.average_wind_angle_deg) > 60
    AND LEAST(vs.average_wind_angle_deg, 360-vs.average_wind_angle_deg) < 120
    THEN vs.total_time_ms ELSE 0 END), 0)
    FROM vessel_status vs WHERE vs.timestamp BETWEEN t.start_timestamp AND t.end_timestamp),
  total_time_running = (SELECT COALESCE(SUM(CASE WHEN vs.is_moored=0 AND vs.engine_on!=1
    AND vs.average_wind_angle_deg IS NOT NULL
    AND LEAST(vs.average_wind_angle_deg, 360-vs.average_wind_angle_deg) >= 120
    THEN vs.total_time_ms ELSE 0 END), 0)
    FROM vessel_status vs WHERE vs.timestamp BETWEEN t.start_timestamp AND t.end_timestamp);

COMMIT;

-- ---------------------------------------------------------------------------
-- Step 3: Verify. upwind + reaching + running should be <= sailed for every row
-- (the gap is sailing time/distance with no recorded wind angle).
-- ---------------------------------------------------------------------------
SELECT COUNT(*) AS total_trips,
       SUM(CASE WHEN total_distance_upwind + total_distance_reaching + total_distance_running > 0
                THEN 1 ELSE 0 END) AS trips_now_backfilled,
       ROUND(SUM(total_distance_upwind),1) AS sum_upwind_nm,
       ROUND(SUM(total_distance_reaching),1) AS sum_reaching_nm,
       ROUND(SUM(total_distance_running),1) AS sum_running_nm,
       ROUND(SUM(total_distance_sailed),1) AS sum_sailed_nm
FROM trips;

SELECT id, description,
       ROUND(total_distance_sailed,1) AS sailed,
       ROUND(total_distance_upwind,1) AS upwind,
       ROUND(total_distance_reaching,1) AS reaching,
       ROUND(total_distance_running,1) AS running
FROM trips WHERE total_distance_sailed > 0 ORDER BY end_timestamp DESC LIMIT 5;

-- ---------------------------------------------------------------------------
-- Step 4: Reset the two dependent caches so legs and yearly stats recompute
-- with point-of-sail data on next view. Both self-migrate their own schema at
-- read time, so no ALTER is needed for them — just clear the stale rows.
-- ---------------------------------------------------------------------------
DELETE FROM trip_legs_cache;
DELETE FROM heatmap_cache;
