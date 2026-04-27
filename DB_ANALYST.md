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
- `description` — auto-generated "Trip YYYY-MM-DD"; user-editable

### vessel_status
- One row every 30 s while underway, every 30 min while moored
- `is_moored` (BOOLEAN) — TRUE when position stable within 30 m radius for 2+ min
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
SELECT
  SUM(CASE WHEN engine_on = 0 AND is_moored = 0 THEN total_distance_nm ELSE 0 END) AS sailed,
  SUM(CASE WHEN engine_on = 1                   THEN total_distance_nm ELSE 0 END) AS motored,
  SUM(CASE WHEN engine_on = 0 AND is_moored = 0 THEN total_time_ms ELSE 0 END) AS time_sailing,
  SUM(CASE WHEN engine_on = 1                   THEN total_time_ms ELSE 0 END) AS time_motoring,
  SUM(CASE WHEN is_moored = 1                   THEN total_time_ms ELSE 0 END) AS time_moored
FROM vessel_status WHERE timestamp BETWEEN '<new_start>' AND '<new_end>';

-- 9. Update the trip record
UPDATE trips SET
  start_timestamp        = '<new_start>',
  end_timestamp          = '<new_end>',
  total_distance_sailed  = <sailed>,
  total_distance_motoring= <motored>,
  total_time_sailing     = <time_sailing>,
  total_time_motoring    = <time_motoring>,
  total_time_moored      = <time_moored>
WHERE id = <id>;

-- 10. Invalidate heatmap_cache
DELETE FROM heatmap_cache
  WHERE date BETWEEN DATE('<new_start>') AND DATE('<new_end>');
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
-- All three in a transaction
START TRANSACTION;
DELETE FROM vessel_status    WHERE timestamp BETWEEN '<start>' AND '<end>';
DELETE FROM environmental_data WHERE timestamp BETWEEN '<start>' AND '<end>';
DELETE FROM heatmap_cache    WHERE date BETWEEN DATE('<start>') AND DATE('<end>');
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
