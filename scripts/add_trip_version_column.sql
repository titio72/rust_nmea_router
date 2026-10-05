-- One-off: introduce the `version` column used by the new version-based trip sync
-- (see docs/superpowers/specs/2026-09-06-trip-version-sync-design.md).
--
-- Run this against the Railway remote NOW, while its trip data is confirmed to match
-- the boat exactly, so both sides start the new scheme at the same baseline (version 1
-- for every trip) and the first version-aware sync does not think everything changed.
-- The matching boat-side migration ships as part of the version-sync implementation
-- itself; this script only prepares Railway ahead of that.
--
-- Run manually, e.g.:
--   mysql -h <host> -P <port> -u <user> -p<password> <database> < scripts/add_trip_version_column.sql
--
-- Follows this project's write protocol (see DB_ANALYST.md): preview, transact, verify.
-- Safe to re-run: the ALTER is idempotent (ignore "Duplicate column" if it fires), and
-- the UPDATE is a plain constant backfill.

-- ---------------------------------------------------------------------------
-- Step 1: Preview — confirm current row count and that the column is absent.
-- ---------------------------------------------------------------------------
SELECT COUNT(*) AS trip_count FROM trips;

-- ---------------------------------------------------------------------------
-- Step 2: Add the column.
-- ---------------------------------------------------------------------------
ALTER TABLE trips ADD COLUMN version BIGINT UNSIGNED NOT NULL DEFAULT 1
  COMMENT 'Bumped on every change to this row; drives remote sync change-detection';

-- ---------------------------------------------------------------------------
-- Step 3: Backfill. Everything starts at 1 — there is no prior version history,
-- and Railway's data is confirmed to already match the boat's, so 1 is the
-- correct baseline for every row (not just newly-added ones).
-- ---------------------------------------------------------------------------
UPDATE trips SET version = 1;

-- ---------------------------------------------------------------------------
-- Step 4: Verify — every row should show version = 1, none NULL.
-- ---------------------------------------------------------------------------
SELECT COUNT(*) AS total, SUM(version = 1) AS at_version_1, SUM(version IS NULL) AS nulls
FROM trips;
