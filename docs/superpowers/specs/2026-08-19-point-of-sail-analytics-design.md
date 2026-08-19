# Point-of-Sail Analytics (Upwind / Reaching / Running) — Design

## Purpose

Trip, leg, and yearly analytics currently break sailing time and distance down into
sailing-vs-motoring only. Sailors also care how much of that sailing time was spent upwind,
reaching, or running, since it reflects both conditions and boat handling. This adds that
breakdown at all three existing aggregation levels — `trips`, `trip_legs_cache`, and
`heatmap_cache` (rolled up into `yearly-stats.html` via `fetch_monthly_statistics`) — by
extending the exact same sailing/motoring bucketing pipeline that already runs at each level,
rather than introducing a parallel computation path.

## Classification

Per `vessel_status` row, when `is_moored = 0 AND engine_on != 1` (sailing) and
`average_wind_angle_deg IS NOT NULL`:

1. Fold the true wind angle (0–360°, relative to bow) to 0–180°: `twa = min(angle, 360 - angle)`.
2. Bucket: **upwind** `twa <= 60`, **reaching** `60 < twa < 120`, **running** `twa >= 120`.

Rows with `engine_on = 1` (motoring) are excluded entirely, matching the existing scope of
sailing-only metrics — point of sail has no meaning under power. Rows with no wind angle
(sensor gap) stay counted in the existing sailing totals but contribute to none of the three
new buckets, so `upwind + reaching + running <= sailing` for both distance and time; this is
the same "uncategorized remainder" behavior the codebase already accepts for e.g. legs that
lack nav-window detection.

Thresholds (60° / 120°) and scope (sailing-only) are fixed by explicit user decision, not
configurable.

## Scope

- Extend the three existing aggregation points below with parallel upwind/reaching/running
  sums, computed identically to the existing sailing/motoring sums at each site.
- Extend `TripSummary`, `TripLeg`, and `MonthlyStatistics` response structs (`src/web/api.rs`)
  and their MCP tool mirrors (`src/bin/mcp_server.rs`) with the 6 new fields each.
- Extend `trip.html` (trip-level and leg-detail stat tiles) and `yearly-stats.html` (stat cards
  + chart series) to display the new fields.
- One-time backfill of existing historical data (see Backfill).
- Out of scope: no new tables, no configurable angle thresholds, no motoring point-of-sail,
  no per-row persistence of the classification itself (only aggregated sums are stored, same
  as sailing/motoring today).

## Schema

New columns, all `NOT NULL DEFAULT 0`, added via the same best-effort
`ALTER TABLE ... ADD COLUMN` pattern already used for `heatmap_cache`'s `sailing_distance_nm`
and `trip_legs_cache`'s `nav_*` columns:

**`trips`** (naming matches its existing `total_distance_sailed` / `total_time_sailing` style):
```
total_distance_upwind    DOUBLE          NOT NULL DEFAULT 0   -- nautical miles
total_distance_reaching  DOUBLE          NOT NULL DEFAULT 0
total_distance_running   DOUBLE          NOT NULL DEFAULT 0
total_time_upwind        BIGINT UNSIGNED NOT NULL DEFAULT 0   -- milliseconds
total_time_reaching      BIGINT UNSIGNED NOT NULL DEFAULT 0
total_time_running       BIGINT UNSIGNED NOT NULL DEFAULT 0
```

**`trip_legs_cache`** (naming matches its existing `sailing_distance_nm` / `sailing_time_ms` style):
```
upwind_distance_nm    DOUBLE          NOT NULL DEFAULT 0
reaching_distance_nm  DOUBLE          NOT NULL DEFAULT 0
running_distance_nm   DOUBLE          NOT NULL DEFAULT 0
upwind_time_ms        BIGINT UNSIGNED NOT NULL DEFAULT 0
reaching_time_ms      BIGINT UNSIGNED NOT NULL DEFAULT 0
running_time_ms       BIGINT UNSIGNED NOT NULL DEFAULT 0
```

**`heatmap_cache`** (same 6 fields; this table has no time columns today at all, only
distance — these are new additions, not extensions of an existing sailing/motoring time metric):
```
upwind_distance_nm    DOUBLE          NOT NULL DEFAULT 0
reaching_distance_nm  DOUBLE          NOT NULL DEFAULT 0
running_distance_nm   DOUBLE          NOT NULL DEFAULT 0
upwind_time_ms        BIGINT UNSIGNED NOT NULL DEFAULT 0
reaching_time_ms      BIGINT UNSIGNED NOT NULL DEFAULT 0
running_time_ms       BIGINT UNSIGNED NOT NULL DEFAULT 0
```

## Backend changes

Each site below already buckets rows into sailing/motoring; each gains the same fold-and-bucket
logic on `average_wind_angle_deg`, scoped to the sailing branch only.

- **`src/trip.rs` `Trip::update()`** — live incremental path, called once per `vessel_status`
  report. Gains a `wind_angle_deg: Option<f64>` parameter; when the row is sailing, folds and
  buckets into the 3 new distance/time field pairs on `Trip`.
- **`src/db/operations/vessel_status.rs` `insert_status_and_trip`** — persists the 6 new `Trip`
  fields on both `CreateTrip` and `UpdateTrip` SQL statements.
- **`src/db/operations/gap_fill.rs` `recalculate_and_update_trip`** — correction/backfill path.
  Extends its single aggregate `SELECT ... SUM(CASE WHEN ...)` with 6 more `SUM(CASE WHEN
  is_moored = 0 AND engine_on != 1 AND <bucket> THEN ... ELSE 0 END)` branches, using
  `LEAST(average_wind_angle_deg, 360 - average_wind_angle_deg)` for the fold.
- **`src/db/operations/trip.rs` `trim_trip`** and **`src/db/operations/mooring_fix.rs`
  `fix_mooring_status`** — same SQL aggregate pattern, extended identically (both already
  mirror `recalculate_and_update_trip`'s aggregate query per DB_ANALYST.md).
- **`src/db/operations/query.rs` `LegRecord` / `finalize_leg`** — `LegRecord` gains
  `wind_angle_deg: Option<f64>`, populated in `compute_trip_legs`'s row scan alongside the
  existing fields. `finalize_leg`'s existing sailing/motoring accumulation loop gains the same
  bucket-and-sum logic, sailing branch only.
- **`src/db/operations/query.rs` `fetch_heatmap`** — both the cached-column `CREATE TABLE`/
  `ALTER TABLE` and the day-level recompute `SELECT ... GROUP BY DATE_FORMAT(...)` query gain
  the 6 new `SUM(CASE WHEN ...)` branches (`is_moored = 0 AND engine_on != 1` already the
  existing sailing condition at this site, per the current `sailing_distance` branch).
- **`src/db/operations/query.rs` `fetch_monthly_statistics`** — both the `heatmap_cache`
  `GROUP BY year, month` sum query and the live (uncached, post-last-cached-date) fallback
  query over raw `vessel_status` gain the same 6 sums.

## API / MCP

- `TripSummary` (`src/web/api.rs`) — add `distance_upwind_nm`, `distance_reaching_nm`,
  `distance_running_nm`, `time_upwind_ms`, `time_reaching_ms`, `time_running_ms`.
- `TripLeg` (`src/db/operations/query.rs`, serialized in `src/web/api.rs`) — same 6 fields.
- `MonthlyStatistics` (`src/db/operations/query.rs`) — same 6 fields per month entry.
- `src/bin/mcp_server.rs` — `get_trip`, `get_trip_legs`, `get_monthly_statistics` tool output
  schemas mirror the same additions (no new tools; existing ones just return more fields).

## Frontend

**`static/trip.html`**:
- Trip-level summary: 3 new `card-stat` tiles (Upwind / Reaching / Running) next to the
  existing Sailing/Motoring Time and Distance tiles, each showing distance (nm) and time,
  matching the existing tile markup/percentage-detail pattern (`sailingTimeStat` etc.).
- Leg-detail view (~line 785–801, where `selectedLeg.sailing_distance_nm` etc. are read): add
  the 6 corresponding `selectedLeg.*` reads for the leg's own breakdown.

**`static/yearly-stats.html`**:
- 3 new `analyticsCard` tiles (Total Upwind / Reaching / Running distance) alongside
  `totalSailing`/`totalMotoring`.
- Extend the monthly bar chart with 2 more series (or a stacked breakdown within the existing
  sailing series) and the data table with 3 more columns, following the existing
  `chartColors.sailing`/`chartColors.motoring` light/dark pair pattern.

## Backfill

Existing trips/legs/days have none of this data. After the schema and code changes ship:

1. **`trips`** — one-time SQL backfill via a correlated-subquery `UPDATE`, run directly against
   the database following DB_ANALYST.md's write protocol (preview affected rows, show exact
   SQL, execute, verify):
   ```sql
   UPDATE trips t SET
     total_distance_upwind = (SELECT COALESCE(SUM(CASE WHEN vs.is_moored=0 AND vs.engine_on!=1
       AND vs.average_wind_angle_deg IS NOT NULL
       AND LEAST(vs.average_wind_angle_deg, 360-vs.average_wind_angle_deg) <= 60
       THEN vs.total_distance_nm ELSE 0 END), 0)
       FROM vessel_status vs WHERE vs.timestamp BETWEEN t.start_timestamp AND t.end_timestamp),
     -- ...same pattern for reaching (60-120), running (>=120), and the 3 time_ms columns
   ;
   ```
   (Exact statement finalized and shown for confirmation at execution time, per protocol.)
2. **`trip_legs_cache`** — `DELETE FROM trip_legs_cache` (all rows). No new code needed: closed
   trips already lazily recompute and re-cache on next `fetch_trip_legs` call, same as any
   other cache miss today (same approach used for the fastest-segment-caching backfill).
3. **`heatmap_cache`** — `DELETE FROM heatmap_cache` (all rows). `fetch_heatmap` already
   recomputes any date range with no cached row from raw `vessel_status`; the next heatmap or
   monthly-statistics request pays a one-time full-history recompute, then re-caches.

Steps 2 and 3 reuse existing cache-miss recompute paths — no backfill-specific code is added
to the binary; this stays a data operation, not a shipped feature.

## Testing

- **Unit tests (no DB):**
  - Fold-and-bucket function (`twa <= 60` / `60 < twa < 120` / `twa >= 120`), including boundary
    values (exactly 60, exactly 120) and angles past 180 (e.g. 300° folds to 60°).
  - `Trip::update()` — extend existing tests (mirroring the current sailing/motoring assertions)
    to cover: sailing row with each of the 3 wind angle buckets accumulates into the right
    field; motoring row contributes to none of the 3; row with `average_wind_angle_deg = None`
    contributes to sailing totals but none of the 3 buckets.
  - `finalize_leg` — same three cases via `LegRecord`, following the existing
    `synthetic_leg_*` test helper pattern in `query.rs`.
- **DB-backed `#[ignore]` tests:**
  - `recalculate_and_update_trip`, `trim_trip`, `fix_mooring_status` — assert the 6 new trip
    columns compute correctly against seeded `vessel_status` rows with known wind angles.
  - `fetch_trip_legs` / `compute_trip_legs` — assert cache round-trip includes the 6 new
    fields (extend `test_trip_legs_cache_round_trips_speed_records`-style coverage).
  - `fetch_heatmap` / `fetch_monthly_statistics` — assert day-level and month-level sums.
- **Frontend:** no JS test framework in this repo; verify via manual load of `trip.html` (trip
  and leg level) and `yearly-stats.html` against a trip/period with known mixed-angle sailing
  data, confirming displayed values against a manual SQL sum.
