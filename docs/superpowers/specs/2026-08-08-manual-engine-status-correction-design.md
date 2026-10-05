# Manual Engine Status Correction — Design

## Purpose

Automatic engine detection (`src/vessel_monitor.rs` — RPM > 100 = on) sometimes misfires, leaving
`vessel_status.engine_on` wrong for a stretch of a trip. This skews the trip's sailing/motoring
distance and time breakdown. This change adds a manual correction tool: the user picks a trip,
clicks a start point and an end point on its track, chooses On or Off, and the tool sets
`engine_on` for every `vessel_status` row in that range and recomputes everything that derives from
it.

## Scope

- New page: `static/fix-engine-status.html`.
- New link/button on `static/trip.html` to reach it for the currently viewed trip.
- New backend endpoint: `POST /api/correct_engine_status` in `src/web/api.rs`.
- New DB operation: `correct_engine_status` in `src/db/operations/trip.rs`.
- No schema changes. No new tables. No audit/history of corrections — a plain overwrite of
  `vessel_status.engine_on`, the same pattern DB_ANALYST.md already documents under "Fix Anomalous
  Sensor Readings".
- Out of scope: improving the automatic RPM-based detection algorithm itself, an undo/history UI
  for corrections, and any change to `trip_legs_nav_overrides` (a separate, unrelated override
  mechanism for nav-window trimming).

## Frontend: `static/fix-engine-status.html`

A new page following the standard structure (`header-bar` + `level-1-container`,
`shared-theme.js`/`shared.css`, 1500px centered layout).

**Entry point:** `trip.html` gets a new button (near the existing leg-navigation controls) linking
to `fix-engine-status.html?id=<id>` (matching `trip.html`'s own `?id=` query param convention).

**Load:**
1. Fetch the trip via `GET /api/trip?id=<id>`.
2. Fetch its track via `GET /api/track?trip_id=<id>` with **no `max_points`** — every point must
   be a real `vessel_status` row so its timestamp is a valid range boundary. (Downsampled points
   from other pages are interpolated/thinned and would not map to real rows.)
3. Render the track on a Leaflet map (`/libs/leaflet.min.js`, matching `trip.html`/`plan.html`),
   colored with the same per-segment logic as `trip.html`'s `createColoredTrack`
   (`engine_on === 1` → grey, `engine_on === 2` → gold/unknown, otherwise speed-gradient). This
   immediately shows the user where engine-on segments currently are, without needing to guess.

**Selection flow:**
1. First map click snaps to the nearest track point → marker A.
2. Second map click snaps to the nearest remaining track point → marker B. The two points are
   sorted by timestamp (order of clicking doesn't matter).
3. The sub-path between A and B is highlighted (a distinct-color overlay polyline drawn on top),
   and an inline panel appears showing: start time, end time, number of points in range, an
   On/Off choice (radio buttons or toggle), an Apply button, and a Cancel button.
4. Clicking the map again before Apply discards the current selection and starts a new one at the
   click location (mirrors `trip.html`'s existing `map.on('click', () => resetSegmentMarkers())`
   pattern).
5. Apply calls `POST /api/correct_engine_status`. On success: refetch the track, redraw it (now
   reflecting the correction), clear the selection and panel, show a brief success indicator. On
   failure: show the error message returned by the API, keep the selection so the user can retry.

## Backend: `POST /api/correct_engine_status`

Request body:
```json
{ "trip_id": 123, "start_timestamp": "2026-08-01T10:00:00Z", "end_timestamp": "2026-08-01T10:45:00Z", "engine_on": true }
```

Handler in `src/web/api.rs`, following the existing thin-wrapper convention (`trim_trip`,
`update_trip_description`): deserialize, call `state.db().correct_engine_status(...)`, map
`Ok`/`Err` to `ApiResponse`, log success/failure with `info!`/`error!` + backtrace, same as
neighboring handlers.

### DB operation: `correct_engine_status` in `src/db/operations/trip.rs`

Placed next to `trim_trip` since it needs the same trip-aggregate-recompute + cache-invalidation
logic.

```rust
pub fn correct_engine_status(
    &self,
    trip_id: u32,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    engine_on: EngineStatus, // On or Off only — never Unknown
) -> Result<(), AppError>
```

Steps, all in one transaction (`conn.start_transaction(...)`):

1. `SELECT start_timestamp, end_timestamp FROM trips WHERE id = :trip_id` — error if not found.
2. Clamp `start`/`end` to the trip's own window (defense in depth; the frontend already only
   offers points within the trip's track). Error if `start >= end`.
3. `UPDATE vessel_status SET engine_on = :val WHERE timestamp BETWEEN :start AND :end` — `:val` is
   always `0` or `1`. Error if the affected row count is 0 ("No track points in range").
4. Recompute aggregates over the trip's **full** window (not just the corrected sub-range, since
   the edit shifts the sailing/motoring split for the whole trip) — same `SUM(...)` query
   `trim_trip` already runs (step 8 in DB_ANALYST.md's Trim protocol):
   ```sql
   SELECT
     SUM(CASE WHEN engine_on = 0 AND is_moored = 0 THEN total_distance_nm ELSE 0 END) AS sailed,
     SUM(CASE WHEN engine_on = 1                   THEN total_distance_nm ELSE 0 END) AS motored,
     SUM(CASE WHEN engine_on = 0 AND is_moored = 0 THEN total_time_ms ELSE 0 END) AS time_sailing,
     SUM(CASE WHEN engine_on = 1                   THEN total_time_ms ELSE 0 END) AS time_motoring,
     SUM(CASE WHEN is_moored = 1                   THEN total_time_ms ELSE 0 END) AS time_moored
   FROM vessel_status WHERE timestamp BETWEEN :trip_start AND :trip_end
   ```
5. `UPDATE trips SET total_distance_sailed = ..., total_distance_motoring = ...,
   total_time_sailing = ..., total_time_motoring = ..., total_time_moored = ... WHERE id =
   :trip_id`.
6. `DELETE FROM trip_legs_cache WHERE trip_id = :trip_id`.
7. `DELETE FROM heatmap_cache WHERE date BETWEEN DATE(:trip_start) AND DATE(:trip_end)`.
8. Commit.

`trip_legs_nav_overrides` is untouched — it's keyed by `(trip_id, leg_number)` and stores nav-window
corrections independent of `engine_on`; it survives this cascade the same way it survives
`trim_trip`.

## Error handling

- Trip not found → `AppError` → `ApiResponse::error("Trip not found")`.
- `start >= end` after clamping → `ApiResponse::error("Invalid time range")`.
- No `vessel_status` rows in range → `ApiResponse::error("No track points in range")`.
- Any DB failure mid-transaction → transaction is not committed (implicit rollback on drop),
  `ApiResponse::error(e.to_string())`, matching every other mutating handler in `api.rs`.

## Testing

- `#[test] #[ignore]` in `src/db/operations/trip.rs` (or a `tests` submodule alongside
  `trim_trip`'s tests), using `setup_db()` / `add_test_trip()` / `add_test_vessel_status()`:
  - Seed a trip with mixed `engine_on` rows.
  - Call `correct_engine_status` for a sub-range with `EngineStatus::On`.
  - Assert only rows within the range changed.
  - Assert trip aggregates match a hand-computed expectation.
  - Assert `trip_legs_cache` rows for the trip are gone.
- Manual browser verification of the click → select → apply flow on `fix-engine-status.html`
  against a real trip, per CLAUDE.md's UI-change testing rule (golden path: correct a clearly-wrong
  grey/non-grey segment; edge case: range touching the very first/last track point).
