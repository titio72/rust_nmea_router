# Meteo Forecast Feature — Design Spec

**Date:** 2026-05-06
**Status:** Approved

---

## Overview

Add weather forecast capabilities to the NMEA router. When cruising, the user can pre-fetch forecasts for planned areas while in port or coastal range. Forecast data is displayed on a dedicated meteo page and overlaid on existing trip charts for visual comparison with actual telemetry.

---

## Data Source

**Open-Meteo** — free, no API key required, two endpoints per forecast point:

- **Forecast API** — ECMWF IFS HRES 9km model (1-hourly up to 90h, 3-hourly up to 144h, 6-hourly to 7 days):
  `https://api.open-meteo.com/v1/forecast?latitude=<lat1,lat2,...>&longitude=<lon1,lon2,...>&models=ecmwf_ifs&hourly=wind_speed_10m,wind_direction_10m,wind_gusts_10m,cape&wind_speed_unit=kn&forecast_days=7`

- **Marine API** — ECMWF WAM 9km model:
  `https://marine-api.open-meteo.com/v1/marine?latitude=<lat1,lat2,...>&longitude=<lon1,lon2,...>&hourly=wave_height,wave_direction,wave_period&forecast_days=7`

Open-Meteo accepts multiple coordinates as comma-separated values in a single request, returning an array of forecast objects — one per coordinate. A single fetch operation for all selected POIs therefore requires exactly **2 HTTP calls** (one to each endpoint) regardless of the number of POIs selected.

**Variables fetched per point:**
- Wind speed at 10m (knots)
- Wind direction at 10m (degrees)
- Wind gusts at 10m (knots)
- Significant wave height (metres)
- Wave period (seconds)
- Wave direction (degrees)
- CAPE — Convective Available Potential Energy (J/kg)

**Forecast horizon:** 7 days.

**Fetch trigger:** Manual only — user initiates via the meteo page while in port or coastal connectivity range.

---

## Points of Interest (POIs)

A persistent repository of named locations that guides where forecasts are fetched. POIs are management entities only — forecast data is stored with plain coordinates and has no hard dependency on the POI that triggered it. Renaming or deleting a POI does not affect stored forecast data.

---

## Data Model

### `forecast_poi`
Persistent named locations for fetch management.

```sql
id          INT AUTO_INCREMENT PRIMARY KEY
name        VARCHAR(100) NOT NULL
lat         DECIMAL(9,6) NOT NULL
lon         DECIMAL(9,6) NOT NULL
created_at  DATETIME NOT NULL
```

### `forecast_fetch`
One record per fetch operation. Stores the coordinates directly — no FK to `forecast_poi`.

```sql
id              INT AUTO_INCREMENT PRIMARY KEY
lat             DECIMAL(9,6) NOT NULL
lon             DECIMAL(9,6) NOT NULL
fetched_at      DATETIME NOT NULL
forecast_from   DATETIME NOT NULL
forecast_to     DATETIME NOT NULL
```

### `forecast_hourly`
One row per forecasted hour per fetch. All timestamps in UTC.

```sql
id                  INT AUTO_INCREMENT PRIMARY KEY
fetch_id            INT NOT NULL        -- FK → forecast_fetch
timestamp           DATETIME NOT NULL
wind_speed_kn       DECIMAL(6,2)
wind_direction_deg  DECIMAL(5,1)
wind_gust_kn        DECIMAL(6,2)
wave_height_m       DECIMAL(5,2)
wave_period_s       DECIMAL(5,2)
wave_direction_deg  DECIMAL(5,1)
cape_j_kg           DECIMAL(8,2)
```

Each new fetch for a location inserts a new `forecast_fetch` row and a full set of `forecast_hourly` rows. Historical fetches are retained.

**Fetch selection rules:**
- `/api/forecast/data` — returns the most recent `forecast_fetch` for the requested coordinates (nearest within 1NM).
- `trip-overlay` — for each `forecast_fetch` location within 25NM of the trip track, selects the most recent fetch whose `fetched_at` predates the trip start. Fetches made after the trip started are excluded so the overlay reflects the pre-departure forecast.

---

## Backend

### New module: `src/forecast.rs`
Fetch logic: makes exactly two HTTP calls (one to the forecast API, one to the marine API), each with all selected POI coordinates as comma-separated parameters. Merges the two response arrays by coordinate index and timestamp, then writes one `forecast_fetch` + N `forecast_hourly` rows per POI to the DB.

### New DB operations: `src/db/operations/forecast.rs`
- CRUD for POIs
- Insert for `forecast_fetch` + `forecast_hourly`
- Query for forecast data by location and time window
- Query for trip overlay (see below)

### New API endpoints

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/forecast/pois` | List all POIs |
| `POST` | `/api/forecast/pois` | Create POI |
| `DELETE` | `/api/forecast/pois/:id` | Delete POI |
| `POST` | `/api/forecast/fetch` | Trigger fetch for selected POI coordinates |
| `GET` | `/api/forecast/data?lat=&lon=&from=&to=` | Stored forecast for a fixed location (meteo page) |
| `GET` | `/api/forecast/trip-overlay?trip_id=` | IDW-interpolated forecast aligned to trip track (trip page) |

---

## Spatial Interpolation

The boat will not pass exactly through any forecast point. When displaying forecast alongside trip telemetry, values are interpolated from all `forecast_fetch` locations within **25 nautical miles** of the boat's position at each hour using **Inverse Distance Weighting (IDW)**:

```
weight_i = 1 / distance_i²
value = Σ(weight_i × value_i) / Σ(weight_i)
```

**Angular variables** (wind direction, wave direction) use `atan2(avg_sin, avg_cos)` — never arithmetic mean.

The `trip-overlay` endpoint handles all spatial logic server-side: for each hour of the trip it finds the boat's interpolated position from the track, selects eligible fetch locations within 25NM using the fetch selection rules above, applies IDW, and returns a single ready-to-plot time series. The frontend requires no spatial computation.

---

## UI

### New page: `meteo.html`

Follows project UI conventions (1500px wide, `shared-theme.js`, `shared.css`, `header-bar`, `level-1-container`).

**Three panels:**

1. **POI Manager**
   - Table of saved POIs: name, lat, lon, delete button
   - "Add POI" form: name field + lat/lon fields + map click to set coordinates (Leaflet map, consistent with `trip.html`)

2. **Fetch Panel**
   - POI list with checkboxes
   - "Fetch Forecast" button
   - Status log: last fetch timestamp and success/error per POI

3. **Forecast Viewer**
   - Select a POI; the viewer displays the most recent stored forecast for that POI's coordinates:
   - Wind speed + gust time series (line chart)
   - Wind direction markers (below wind chart)
   - Wave height + period time series (line chart)
   - CAPE bar chart, coloured by risk level:
     - Green: < 500 J/kg
     - Amber: 500–1500 J/kg
     - Red: > 1500 J/kg

### Changes to `trip.html`

When forecast data exists for locations within 25NM of the trip track, overlay dashed forecast lines on existing charts:
- Wind speed chart: forecast wind speed + gust as dashed lines
- Wind direction chart: forecast direction as dashed line

Two new panels added below existing charts, shown only when forecast data is available:
- **Wave forecast**: height + period time series for the trip time window
- **CAPE forecast**: coloured risk bar for the trip time window

If no forecast data is available for a trip, all forecast elements are hidden — no empty states shown.

---

## Constraints & Rules

- All timestamps stored and served in UTC.
- Wind speed in knots, wave height in metres, wave period in seconds, CAPE in J/kg.
- Angle averaging via `atan2` — never arithmetic mean.
- Distance calculations use Haversine formula.
- HTTP calls to Open-Meteo use `reqwest` (v0.12, already in `Cargo.toml` with `json` + `rustls-tls` features).
- All DB queries use parameterised statements (`params!` macro).
- SignalK broadcasts not affected — forecast data is not broadcast.
