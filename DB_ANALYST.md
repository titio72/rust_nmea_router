# DB_ANALYST.md — Trip Data Analysis Reference

Loaded automatically in Claude Code sessions. Describes entity relationships, field semantics, and safe modification protocols for the nmea_router MariaDB database.

---

## Entity Relationships

There are NO foreign key columns linking vessel_status or environmental_data to trips.
Relationships are implicit, by timestamp range:

  vessel_status rows that belong to a trip:
    WHERE timestamp BETWEEN trip.start_timestamp AND trip.end_timestamp

  environmental_data rows that belong to a trip:
    WHERE timestamp BETWEEN trip.start_timestamp AND trip.end_timestamp

When modifying trip boundaries (start_timestamp / end_timestamp), always cascade
to vessel_status and environmental_data manually.

---

## Table Semantics

### trips
- `id` — auto-increment PK
- `uuid` — nullable CHAR(36); used for cross-device sync and deduplication; fill with UUID()
- `start_timestamp` / `end_timestamp` — UTC DATETIME(3); define the trip's time window
- `total_distance_sailed` / `total_distance_motoring` — nautical miles (DOUBLE)
- `total_time_sailing` / `total_time_motoring` / `total_time_moored` — milliseconds (BIGINT)
- `total_distance_upwind` — nautical miles (DOUBLE); sailing distance with folded TWA ≤ 60°
- `total_distance_reaching` — nautical miles (DOUBLE); sailing distance with folded TWA > 60° and < 120°
- `total_distance_running` — nautical miles (DOUBLE); sailing distance with folded TWA ≥ 120°
- `total_time_upwind` — milliseconds (BIGINT); sailing time with folded TWA ≤ 60°
- `total_time_reaching` — milliseconds (BIGINT); sailing time with folded TWA > 60° and < 120°
- `total_time_running` — milliseconds (BIGINT); sailing time with folded TWA ≥ 120°
  (folded TWA = `LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg)`; the three
  buckets partition the sailing rows that have a non-NULL wind angle, so they sum to at
  most `total_distance_sailed` / `total_time_sailing`)
- `description` — auto-generated "Trip YYYY-MM-DD"; user-editable

### vessel_status
- One row every 30 s while underway, every 30 min while moored
- `is_moored` (BOOLEAN) — TRUE when position stable within 45 m radius of a 10-min reference point for 2+ min
- `engine_on` (TINYINT) — 0 = off, 1 = on, 2 = unknown
- `average_wind_speed_kn` / `average_wind_angle_deg` — true wind (nullable)
- `average_speed_kn` / `max_speed_kn` — speed over ground in knots
- `total_distance_nm` — distance sailed in this reporting interval (not cumulative)
- `total_time_ms` — duration of this reporting interval in milliseconds
- `cog_deg` — course over ground; `average_heading_deg` — true heading

### environmental_data
- `metric_id` values:
  - 1 = Pressure (Pa)
  - 2 = CabinTemp (°C)
  - 3 = WaterTemp (°C)
  - 4 = Humidity (%)
  - 5 = WindSpeed (kn)
  - 6 = WindDir (degrees, 0–360)
  - 7 = Roll (degrees)
- `value_avg` / `value_max` / `value_min` — aggregated over the collection interval
- Unique constraint on (timestamp, metric_id)

### heatmap_cache
- Pre-computed per-day sailing distance; MUST be invalidated after any edit to vessel_status
- Delete affected rows; the app recomputes on next request:
  DELETE FROM heatmap_cache WHERE date BETWEEN DATE('<new_start>') AND DATE('<new_end>');

### trip_legs_cache
- Pre-computed leg breakdown per trip; PK is `(trip_id, leg_number)`
- `trip_id` mirrors `trips.id` — no FK constraint
- `leg_number` — 1-based sequence within the trip
- `start_timestamp` / `end_timestamp` — ISO-8601 strings (VARCHAR(30)), **not** DATETIME
- `total_distance_nm` / `sailing_distance_nm` / `motoring_distance_nm` — nautical miles (DOUBLE)
- `sailing_time_ms` / `motoring_time_ms` — milliseconds (BIGINT UNSIGNED)
- `start_lat` / `start_lon` / `end_lat` / `end_lon` — decimal degrees (DOUBLE, nullable)
- `nav_start_timestamp` / `nav_end_timestamp` — ISO-8601 strings (VARCHAR(30), nullable); define the pure-navigation window trimmed of marina exit/entry phases
- `nav_distance_nm` — nautical miles within the nav window (DOUBLE, 0 when no window detected)
- `nav_time_ms` — milliseconds within the nav window (BIGINT UNSIGNED)
- `nav_detection_method` — how the window was detected: `"engine_transition"` (engine off/on transitions), `"speed_fallback"` (first/last point ≥ 4 kn), `"user_override"`, or NULL (no window found)
- `sailing_time_formatted` / `motoring_time_formatted` are **not stored** — derive at read time
- Only **closed trips** (`end_timestamp > 24h ago`) are cached; open trips are always computed live
- Legs shorter than **0.5 nm** are excluded from the cache
- **Auto-invalidated** by `delete_trip` and `trim_trip` — no manual step needed for those operations
- For any other direct edit to `vessel_status` within a trip's window, manually invalidate:
  DELETE FROM trip_legs_cache WHERE trip_id = <id>;

### trip_legs_nav_overrides
- User corrections for the nav window; PK is `(trip_id, leg_number)`
- `nav_start` / `nav_end` — user-provided nav window timestamps (ISO-8601 VARCHAR(30), nullable)
- `auto_nav_start` / `auto_nav_end` — the algorithm's original detection at time of override (preserved for calibration analysis)
- `corrected_at` — DATETIME(3) UTC timestamp of the correction
- Overrides are applied **on top of** the computed/cached leg data at read time
- Overrides survive cache invalidation (separate table, not touched by `delete_trip` / `trim_trip`)
- To clear an override: DELETE FROM trip_legs_nav_overrides WHERE trip_id = <id> AND leg_number = <n>;

### system_status
- Key-value store for app runtime flags (tracking_enabled, metrics_enabled)
- Do not modify during normal data analysis

---

## Modification Protocols

### Protocol for every write operation
1. SELECT to preview affected rows and count
2. Show the exact SQL to the user; get confirmation
3. Execute in a transaction where multiple statements are needed
4. Verify with a follow-up SELECT
5. For large deletes (>1000 rows): suggest mysqldump backup first

### Remote sync scope
`trips.updated_at` is bumped automatically by MariaDB (`ON UPDATE CURRENT_TIMESTAMP`)
on any `UPDATE trips ...`, and the boat's push sync uses it to decide which trips to
re-send to the remote viewer. Any protocol below that ends with an `UPDATE trips SET
...` (totals recompute, description, uuid backfill) is automatically re-queued for
the next sync push — no extra step needed.

Edits that touch only `vessel_status` / `environmental_data` and never update the
trips row itself (e.g. NULLing an anomalous sensor reading) do **not** trigger
re-sync. If a correction needs to reach the remote viewer, follow it with a totals
recompute against the trip (see Trim a Trip, steps 8–9) so the trips row is touched.

---

### Trim a Trip

Goal: Narrow a trip's time window to remove moored periods at start/end.

```sql
-- 1. Find the trip
SELECT id, start_timestamp, end_timestamp FROM trips WHERE id = <id>;

-- 2. Find first underway moment; new_start = result - INTERVAL 1 HOUR
SELECT MIN(timestamp) FROM vessel_status
  WHERE timestamp BETWEEN '<start>' AND '<end>' AND is_moored = 0;

-- 3. Find last underway moment; new_end = result + INTERVAL 1 HOUR
SELECT MAX(timestamp) FROM vessel_status
  WHERE timestamp BETWEEN '<start>' AND '<end>' AND is_moored = 0;

-- 4. Clamp: new_start >= original start_timestamp, new_end <= original end_timestamp

-- 5. Preview rows to be deleted
SELECT COUNT(*) FROM vessel_status
  WHERE timestamp BETWEEN '<original_start>' AND '<original_end>'
  AND (timestamp < '<new_start>' OR timestamp > '<new_end>');

-- 6. Delete vessel_status outside new bounds
DELETE FROM vessel_status
  WHERE timestamp BETWEEN '<original_start>' AND '<original_end>'
  AND (timestamp < '<new_start>' OR timestamp > '<new_end>');

-- 7. Delete environmental_data outside new bounds
DELETE FROM environmental_data
  WHERE timestamp BETWEEN '<original_start>' AND '<original_end>'
  AND (timestamp < '<new_start>' OR timestamp > '<new_end>');

-- 8. Recompute trip aggregates from remaining rows
-- is_moored takes priority: a row with is_moored=1 counts as moored even if
-- engine_on=1 (e.g. engine idling at anchor). engine_on only splits the
-- non-moored rows into sailing vs motoring. This mirrors the app's own
-- recalculate_and_update_trip (src/db/operations/gap_fill.rs) — do not use
-- `engine_on = 1` alone for the motored bucket, it double-counts rows that
-- are both moored and engine-on.
-- The point-of-sail buckets fold the true wind angle to 0-180 via
-- LEAST(angle, 360 - angle), then split the sailing rows: upwind <= 60,
-- reaching > 60 and < 120, running >= 120. Rows with a NULL wind angle stay in
-- the sailing totals but fall into no bucket.
SELECT
  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 THEN total_distance_nm ELSE 0 END) AS sailed,
  SUM(CASE WHEN is_moored = 0 AND engine_on = 1  THEN total_distance_nm ELSE 0 END) AS motored,
  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 THEN total_time_ms ELSE 0 END) AS time_sailing,
  SUM(CASE WHEN is_moored = 0 AND engine_on = 1  THEN total_time_ms ELSE 0 END) AS time_motoring,
  SUM(CASE WHEN is_moored = 1                    THEN total_time_ms ELSE 0 END) AS time_moored,
  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 AND average_wind_angle_deg IS NOT NULL
           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) <= 60
           THEN total_distance_nm ELSE 0 END) AS dist_upwind,
  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 AND average_wind_angle_deg IS NOT NULL
           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) > 60
           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) < 120
           THEN total_distance_nm ELSE 0 END) AS dist_reaching,
  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 AND average_wind_angle_deg IS NOT NULL
           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) >= 120
           THEN total_distance_nm ELSE 0 END) AS dist_running,
  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 AND average_wind_angle_deg IS NOT NULL
           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) <= 60
           THEN total_time_ms ELSE 0 END) AS time_upwind,
  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 AND average_wind_angle_deg IS NOT NULL
           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) > 60
           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) < 120
           THEN total_time_ms ELSE 0 END) AS time_reaching,
  SUM(CASE WHEN is_moored = 0 AND engine_on != 1 AND average_wind_angle_deg IS NOT NULL
           AND LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg) >= 120
           THEN total_time_ms ELSE 0 END) AS time_running
FROM vessel_status WHERE timestamp BETWEEN '<new_start>' AND '<new_end>';

-- 9. Update the trip record
UPDATE trips SET
  start_timestamp        = '<new_start>',
  end_timestamp          = '<new_end>',
  total_distance_sailed  = <sailed>,
  total_distance_motoring= <motored>,
  total_time_sailing     = <time_sailing>,
  total_time_motoring    = <time_motoring>,
  total_time_moored      = <time_moored>,
  total_distance_upwind  = <dist_upwind>,
  total_distance_reaching= <dist_reaching>,
  total_distance_running = <dist_running>,
  total_time_upwind      = <time_upwind>,
  total_time_reaching    = <time_reaching>,
  total_time_running     = <time_running>
WHERE id = <id>;

-- 10. Invalidate heatmap_cache
DELETE FROM heatmap_cache
  WHERE date BETWEEN DATE('<new_start>') AND DATE('<new_end>');

-- 11. Invalidate trip_legs_cache (auto-done by trim_trip; required if editing SQL directly)
DELETE FROM trip_legs_cache WHERE trip_id = <id>;
```

---

### Add Missing UUIDs

```sql
-- Preview
SELECT id, description FROM trips WHERE uuid IS NULL;
-- Execute
UPDATE trips SET uuid = UUID() WHERE uuid IS NULL;
-- No cascade needed; uuid is trip-only
```

---

### Delete a Trip

```sql
-- All in a transaction
START TRANSACTION;
DELETE FROM vessel_status      WHERE timestamp BETWEEN '<start>' AND '<end>';
DELETE FROM environmental_data WHERE timestamp BETWEEN '<start>' AND '<end>';
DELETE FROM heatmap_cache      WHERE date BETWEEN DATE('<start>') AND DATE('<end>');
DELETE FROM trip_legs_cache    WHERE trip_id = <id>;
DELETE FROM trips WHERE id = <id>;
COMMIT;
```

---

### Fix Anomalous Sensor Readings

```sql
-- 1. Find the trip bounds
SELECT id, start_timestamp, end_timestamp FROM trips ORDER BY end_timestamp DESC LIMIT 1;

-- 2. Identify outliers in vessel_status (example: wind spikes)
SELECT id, timestamp, average_wind_speed_kn FROM vessel_status
  WHERE timestamp BETWEEN '<start>' AND '<end>'
  AND average_wind_speed_kn > <threshold>
  ORDER BY timestamp;

-- 3. Also check environmental_data WindSpeed (metric_id = 5)
SELECT id, timestamp, value_avg FROM environmental_data
  WHERE metric_id = 5 AND timestamp BETWEEN '<start>' AND '<end>'
  AND value_avg > <threshold>
  ORDER BY timestamp;

-- Choose one of these strategies (discuss with user first):

-- A) NULL out the value (preserve the row, mark reading invalid)
UPDATE vessel_status
  SET average_wind_speed_kn = NULL, average_wind_angle_deg = NULL
  WHERE id IN (<ids>);

-- B) Cap at a reasonable value
UPDATE vessel_status SET average_wind_speed_kn = <cap> WHERE id IN (<ids>);

-- C) Delete the rows entirely (if the whole reading is bad)
DELETE FROM vessel_status WHERE id IN (<ids>);
DELETE FROM environmental_data WHERE metric_id IN (5, 6)
  AND timestamp IN (SELECT timestamp FROM vessel_status WHERE id IN (<ids>));
```

---

### Fix a Mislabeled Mooring Period

Goal: a range of `vessel_status` rows has the wrong `is_moored` value (e.g. mooring
detection failed at anchor and logged dense underway-cadence reports for a period the
boat never actually left).

Prefer the `fix_mooring_status` MCP tool (or `POST /api/fix_mooring_status`) over manual
SQL — it does the following atomically:
1. Finds the trip covering `[start, end]` (clamps the window to the trip's own bounds).
2. Sets `is_moored` on every row in the window.
3. If the target is `true` (moored), resamples: collapses the dense rows down to the
   moored reporting interval (median position per bucket, vector distance/time/course
   from bucket to bucket instead of summing per-sample GPS jitter, circular-mean
   heading/wind-angle, arithmetic-mean wind speed, max speed per bucket). This matters —
   leaving dense rows under a moored label still double-counts jitter as travelled
   distance even after the flag is fixed.
4. Recomputes the trip's aggregate totals (never its `start_timestamp`/`end_timestamp`).
5. Invalidates `trip_legs_cache` and `heatmap_cache` for the affected range.

Before calling it, sanity-check the window the same way as any other correction:
`SELECT is_moored, engine_on, latitude, longitude FROM vessel_status WHERE timestamp
BETWEEN '<start>' AND '<end>' ORDER BY id` — confirm the position barely moves and
`engine_on = 0` before concluding it should be moored.

## MCP Tools

The `nmea_router` MCP server (`target/debug/mcp_server`, or `target/release/mcp_server` in production) exposes the following typed tools. Prefer these over raw SQL for all structured reads and every write operation.

**Use MCP tools when:**
- Executing any write (trim, delete, update description) — they run the full atomic cascade including cache invalidation
- Fetching trips, legs, track, or metrics — caching is transparent
- Any operation that maps to an existing `VesselDatabase` method

**Use the `mariadb` raw SQL server when:**
- Exploring schema structure or column names
- Ad-hoc aggregations not covered by a tool
- Anomaly analysis across raw `vessel_status` or `environmental_data` rows
- Verifying data after a write

| Tool | Description |
|---|---|
| `list_trips` | All trips, optional `year` / `last_months` filter |
| `get_trip` | Single trip by numeric `id` |
| `get_trip_by_uuid` | Single trip by `uuid` string |
| `get_trip_legs` | Legs for a trip (cached for closed trips, 0.5 nm minimum leg size) |
| `get_track` | Track points by `trip_id` or `start`/`end`; `max_points` downsamples |
| `get_metrics` | Environmental time-series: `wind_speed`, `wind_dir`, `roll`, `pressure`, `cabin_temp`, `water_temp`, `humidity` |
| `get_speed_distribution` | Speed histogram split by sailing vs motoring |
| `get_wind_statistics` | Wind rose data (72 × 5° buckets) |
| `get_monthly_statistics` | Monthly sailing/motoring distance; optional `year` filter |
| `trim_trip` | Remove moored padding, recalculate aggregates, invalidate caches (atomic) |
| `fix_mooring_status` | Correct a mislabeled mooring period: set `is_moored` for `[start, end]`; if `true`, also resamples the window to the moored cadence, recomputes trip aggregates, invalidates caches (atomic) |
| `delete_trip` | Delete trip + all vessel_status + environmental_data + caches (atomic) |
| `update_trip_description` | Change the free-text trip name |
| `invalidate_trip_legs` | Force-invalidate legs cache for a trip |

---

## Valid Data Ranges (for anomaly detection)

| Metric                | Normal range       | Suspect if              |
|-----------------------|--------------------|-------------------------|
| Speed (kn)            | 0 – 20             | > 25                    |
| Wind speed (kn)       | 0 – 60             | > 80                    |
| Wind direction (deg)  | 0 – 360            | outside range           |
| Heading / COG (deg)   | 0 – 360            | outside range           |
| Pressure (Pa)         | 95 000 – 105 000   | < 90 000 or > 110 000   |
| CabinTemp (°C)        | 5 – 50             | < -10 or > 60           |
| WaterTemp (°C)        | 0 – 35             | < -2 or > 40            |
| Humidity (%)          | 0 – 100            | outside range           |
| Roll (deg)            | -90 – 90           | outside range           |
