# Forecast & Planning — Trip Decoupling Design Spec

**Date:** 2026-05-29  
**Status:** Approved  
**Replaces:** `docs/superpowers/specs/2026-05-11-trip-forecast-redesign-design.md`, `docs/superpowers/specs/2026-05-12-trip-planning-design.md`

---

## Overview

Decouple the forecast and planning subsystems from trips entirely. Forecast areas become a global application concept. The background poller runs whenever at least one area is defined, regardless of trip state. The planning page becomes a top-level nav item accessible at any time. The trip overlay feature (forecast-vs-actual on completed trip charts) is dropped.

---

## Motivation

The previous design tied forecast areas to an active trip, which forced the user to start a trip before doing any passage planning or forecast review. Areas are conceptually independent of trips — the user defines where they intend to sail, downloads the forecast, and plans passages before and regardless of any trip.

---

## Schema

### Renamed table: `trip_forecast_area` → `forecast_area`

Remove the `trip_id` column:

```sql
CREATE TABLE forecast_area (
    id         INT AUTO_INCREMENT PRIMARY KEY,
    lat_min    DECIMAL(9,6) NOT NULL,
    lat_max    DECIMAL(9,6) NOT NULL,
    lon_min    DECIMAL(9,6) NOT NULL,
    lon_max    DECIMAL(9,6) NOT NULL,
    created_at DATETIME NOT NULL
);
```

### Modified table: `forecast_fetch`

Drop `trip_id`. Keep `area_id` FK → `forecast_area.id`:

```sql
ALTER TABLE forecast_fetch
    DROP FOREIGN KEY fk_forecast_fetch_trip,
    DROP COLUMN trip_id;
```

Cascade delete on `forecast_area` → `forecast_fetch` → `forecast_hourly` is preserved.

### Removed

- `trip_id` column from `forecast_fetch`
- `trip_forecast_area` table (renamed)

### Unchanged

- `forecast_fetch` structure otherwise unchanged
- `forecast_hourly` unchanged

---

## Background Poller (`src/forecast_poller.rs`)

Remove the active-trip check entirely. The new loop condition:

```
loop:
  1. Query all forecast areas → empty: sleep 5 min, continue
  2. Query time since last fetch (global MAX)
     → < 3 hours: sleep until 3h mark, continue
  3. For each area:
       fetch bbox from Open-Meteo (forecast + marine)
       on error: sleep 15 min, retry from step 3
       on success: store all grid points with area_id
  4. Record successful fetch timestamp
  5. Sleep 3 hours
```

`ForecastPollerStatus` struct is unchanged (`online`, `last_fetch`, `next_fetch`).

---

## DB Layer (`src/db/operations/forecast.rs`)

**Updated signatures (drop `trip_id` parameter):**

| Function | Change |
|---|---|
| `list_forecast_areas()` | No parameter — queries full table |
| `create_forecast_area(area)` | `area` struct no longer has `trip_id` |
| `insert_forecast_fetch(area_id, ...)` | Drop `trip_id` argument |
| `get_last_fetch_time()` | No parameter — `MAX(fetched_at)` across all fetches |
| `get_forecast_counts()` | No parameter — global counts |
| `fetch_forecast_fetches()` | Drop `trip_id` parameter; SQL `WHERE trip_id = :trip_id` removed — selects most recent fetch per lat/lon globally |

**Removed functions:**

- `fetch_trip_forecast_inputs(trip_id)` — trip overlay dropped
- `get_active_trip_id()` — only caller is the poller; delete both the call site and the DB method

**Removed in `src/forecast.rs`:**

- `compute_trip_overlay(...)` — trip overlay dropped (note: `compute_route_overlay` is a different function and is kept)

---

## API Endpoints

### Removed

| Method | Path |
|---|---|
| `GET` | `/api/forecast/trip-overlay?trip_id=X` |

### Updated (drop `trip_id` parameter)

| Method | Path | Notes |
|---|---|---|
| `GET` | `/api/forecast/areas` | No `trip_id` query param; `ForecastAreaTripQuery` struct removed |
| `POST` | `/api/forecast/areas` | Body no longer includes `trip_id` |
| `DELETE` | `/api/forecast/areas/:id` | Unchanged |
| `GET` | `/api/forecast/status` | No `trip_id`; global poller status |
| `GET` | `/api/forecast/grid-points?timestamp=ISO` | Drop `trip_id` from `ForecastGridPointsQuery` |
| `GET` | `/api/forecast/route?...` | Drop `trip_id` from `ForecastRouteQuery`; uses global forecast data |
| `GET` | `/api/forecast/optimal-route?...` | Drop `trip_id` from `OptimalRouteQuery`; uses global forecast data |

---

## UI Changes

### `static/trip.html`

Remove entirely:
- "Forecast Areas" draw/manage section
- "Planning →" button
- Forecast chart overlays (dashed lines on wind, wave, CAPE charts)
- Any JS that calls `/api/forecast/*` endpoints

### `static/plan.html`

- Remove `?trip_id=X` query parameter from page load logic
- All API calls drop the `trip_id` argument
- Area management (draw bbox, list areas, delete area) now manages the global `forecast_area` table directly — no trip association shown or implied
- Route planning unchanged in behaviour; operates on the full global forecast dataset

### `static/js/shared-theme.js`

Add "Forecast" as a top-level nav entry in `createHeaderBar()`, pointing to `plan.html`.

---

## Migration

```sql
-- 1. Rename table and drop trip_id
RENAME TABLE trip_forecast_area TO forecast_area;
ALTER TABLE forecast_area DROP FOREIGN KEY fk_forecast_area_trip;
ALTER TABLE forecast_area DROP COLUMN trip_id;

-- 2. Drop trip_id from forecast_fetch
ALTER TABLE forecast_fetch DROP FOREIGN KEY fk_forecast_fetch_trip;
ALTER TABLE forecast_fetch DROP COLUMN trip_id;

-- 3. Truncate existing forecast data (grid points are trip-scoped; stale without trip context)
TRUNCATE TABLE forecast_hourly;
TRUNCATE TABLE forecast_fetch;
```

Existing `forecast_area` rows (formerly `trip_forecast_area`) are retained as global areas.

---

## What Is Dropped

- Trip overlay: no forecast-vs-actual dashed lines on trip charts
- Trip-gated access to planning: `plan.html` no longer requires an active trip
- Trip-gated forecast fetching: poller no longer checks for an active trip

---

## Constraints

- All timestamps UTC.
- Units: wind speed in knots, wave height in metres, wave period in seconds, CAPE in J/kg.
- Angular averaging via `atan2` — never arithmetic mean.
- Haversine for all distance calculations.
- All DB queries use parameterised statements (`params!` macro).
