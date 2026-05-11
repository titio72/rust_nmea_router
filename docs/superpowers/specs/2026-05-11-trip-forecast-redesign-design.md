# Trip Forecast Redesign — Design Spec

**Date:** 2026-05-11
**Status:** Approved
**Replaces:** `docs/superpowers/specs/2026-05-06-meteo-forecast-design.md`

---

## Overview

Replace the manual POI-based forecast system with an automatic trip-area forecast system. When a trip is active, the user draws bounding boxes on the trip detail page to define the expected sailing area. The system automatically fetches a 7-day forecast from Open-Meteo every 3 hours and keeps it up to date for as long as the trip is active. Forecast data persists after the trip ends so completed trips can show forecasted vs actual conditions.

---

## Behaviour

- **Trigger:** Automatic. No manual fetch button. The background poller runs continuously.
- **Condition to fetch:** Active trip exists AND at least one forecast area is defined.
- **Interval:** Every 3 hours from the last successful fetch.
- **Connectivity:** If the HTTP call fails (any reason), retry every 15 minutes until it succeeds, then resume the normal 3-hour cadence.
- **No active trip or no areas:** Poller sleeps in 5-minute checks, no data fetched.
- **Completed trips:** Forecast data is retained and displayed as overlays on the trip charts.

---

## Data Source

Open-Meteo bounding box API — same two endpoints as the existing implementation, called once per area per poll cycle:

- **Forecast API:** `https://api.open-meteo.com/v1/forecast?...&bbox=lat_min,lon_min,lat_max,lon_max`
- **Marine API:** `https://marine-api.open-meteo.com/v1/marine?...&bbox=lat_min,lon_min,lat_max,lon_max`

Each bounding box call returns an array of forecast objects aligned with the ECMWF 9 km model grid. Each grid point within the bbox is a separate element in the response array.

Variables fetched per grid point: same as current implementation (wind speed/direction/gusts, wave height/period/direction, CAPE).

---

## Data Model

### New table: `trip_forecast_area`

```sql
id         INT AUTO_INCREMENT PRIMARY KEY
trip_id    INT NOT NULL              -- FK → trip
lat_min    DECIMAL(9,6) NOT NULL
lat_max    DECIMAL(9,6) NOT NULL
lon_min    DECIMAL(9,6) NOT NULL
lon_max    DECIMAL(9,6) NOT NULL
created_at DATETIME NOT NULL
```

### Modified table: `forecast_fetch`

Two columns added:

```sql
trip_id    INT NOT NULL    -- FK → trip
area_id    INT NOT NULL    -- FK → trip_forecast_area
```

`trip_id` is denormalised for efficient per-trip queries without joining through `trip_forecast_area`. `area_id` enables clean cascade deletion when an area is removed.

### Removed table: `forecast_poi`

Deleted entirely. The POI-based system is replaced.

### Migration note

Existing `forecast_fetch` and `forecast_hourly` rows (from the old POI system) are incompatible with the new `trip_id`/`area_id` columns. Both tables are truncated during the schema migration. The old `forecast_poi` table is dropped.

### Unchanged: `forecast_hourly`

No schema change. Linked to `forecast_fetch` via `fetch_id` as before.

---

## Fetch Flow (per poll cycle)

```
For each trip_forecast_area of the active trip:
  1. Call Forecast API with bbox → array of N 9km grid points
  2. Call Marine API with same bbox → array of N 9km grid points
  3. Merge arrays by index (same grid, same order)
  4. For each merged grid point:
     INSERT forecast_fetch (trip_id, area_id, lat, lon, fetched_at, ...)
     INSERT forecast_hourly rows for that fetch
```

A full poll cycle for a trip with K areas makes exactly 2K HTTP calls.

---

## Background Poller

**New module:** `src/forecast_poller.rs`

A Tokio task spawned once at server startup, sharing `AppState`:

```
loop:
  1. Query active trip → None: sleep 5 min, continue
  2. Query forecast areas for active trip → empty: sleep 5 min, continue
  3. Query time since last fetch for this trip
     → < 3 hours: sleep until 3h mark, continue
  4. For each area:
       fetch bbox from Open-Meteo (forecast + marine)
       on error: sleep 15 min, retry from step 4
       on success: store all grid points with trip_id + area_id
  5. Record successful fetch timestamp
  6. Sleep 3 hours
```

No separate connectivity check — the HTTP call outcome determines online/offline state.

The poller exposes its current state via `Arc<RwLock<ForecastPollerStatus>>` held in `AppState`. The `/api/forecast/status` endpoint reads this shared state directly without querying the DB:

```rust
pub struct ForecastPollerStatus {
    pub online: bool,
    pub last_fetch: Option<DateTime<Utc>>,
    pub next_fetch: Option<DateTime<Utc>>,
}
```

The poller updates this struct after every fetch attempt (success or failure).

---

## API Changes

### Removed

| Method | Path |
|--------|------|
| `GET` | `/api/forecast/pois` |
| `POST` | `/api/forecast/pois` |
| `DELETE` | `/api/forecast/pois/:id` |
| `POST` | `/api/forecast/fetch` |
| `GET` | `/api/forecast/data` |

### Added

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/forecast/areas?trip_id=X` | List forecast areas for a trip |
| `POST` | `/api/forecast/areas` | Create area `{ trip_id, lat_min, lat_max, lon_min, lon_max }` |
| `DELETE` | `/api/forecast/areas/:id` | Delete area and all its forecast data |
| `GET` | `/api/forecast/status?trip_id=X` | `{ last_fetch, next_fetch, online, area_count, point_count }` |

### Unchanged

| Method | Path | Notes |
|--------|------|-------|
| `GET` | `/api/forecast/trip-overlay?trip_id=X` | Filter now uses `trip_id` on `forecast_fetch` instead of time-window + distance rule |

---

## Spatial Interpolation

Unchanged from the current implementation. The `trip-overlay` endpoint selects all `forecast_fetch` rows for the given `trip_id`, then applies IDW (weight = 1/d², angular atan2 averaging for directions, 25 NM cutoff) to produce an hourly time series aligned to the trip track.

---

## UI Changes

### Removed

- `static/meteo.html` — deleted
- "Meteo" entry removed from `createHeaderBar()` nav items in `static/js/shared-theme.js`

### Modified: `static/trip.html`

**New "Forecast Areas" collapsible section** added below the wave/CAPE panels.

**When trip is active** (full controls):
- Leaflet map with rectangle draw mode: "Draw area" button activates draw mode; user clicks and drags to define bounding box; `mouseup` commits via `POST /api/forecast/areas`
- Area list: coordinate bounds, delete button per area (calls `DELETE /api/forecast/areas/:id`)
- Status line: last fetch time, next scheduled fetch, online/offline badge — updated every 60 s via `GET /api/forecast/status?trip_id=X`

**When trip is completed** (read-only):
- Area list shown without draw controls or delete buttons
- Status line shows last fetch timestamp only

**Forecast chart overlays** (existing dashed lines on wind/wave/CAPE charts): no change to rendering logic. Data sourced from `GET /api/forecast/trip-overlay?trip_id=X` as before.

---

## Constraints

- All timestamps in UTC.
- Units: wind speed in knots, wave height in metres, wave period in seconds, CAPE in J/kg.
- Angular averaging via `atan2` — never arithmetic mean.
- Distance calculations via Haversine.
- All DB queries use parameterised statements (`params!` macro).
- HTTP calls use `reqwest` (existing dependency, `rustls-tls` feature).
- The poller shares `AppState`; no new external dependencies.
