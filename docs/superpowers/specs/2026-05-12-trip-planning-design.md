# Trip Planning Page — Design Spec

**Date:** 2026-05-12  
**Status:** Approved

---

## Overview

A dedicated trip planning page (`static/plan.html`) that lets the user visualise the 7-day forecast across the active trip's sailing areas and plan a passage by defining a start point, end point, departure time, and average speed. The system estimates the vessel's position at each hour along the route and shows the forecast conditions at each position+time as a colour-coded line on the map.

The page is active-trip-only. It is reached via a **"Planning →"** button on the trip page, visible only when the trip is active.

---

## Page Layout

```
┌─────────────────────────────────────────────────────────────┐
│  Nav bar  (Trip Planning — <trip description>)               │
├─────────────────────────────────────────────────────────────┤
│  [ Tue 13 · Wed 14 · Thu 15 · Fri 16 · Sat 17 · Sun 18 · …]│  ← day tabs
│  [ 00:00 ───────●──────────────────────────── 23:00    Now ]│  ← hour slider
├─────────────────────────────────────────────────────────────┤
│  (route bar — hidden until route mode active)                │
│  ● 43.68N 10.27E → ● 43.05N 9.85E  Dep: Wed 14 06:00        │
│  Speed: 5.5 kn  [Compute]  [Clear Route]                    │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│              LEAFLET MAP  (fills remaining viewport height) │
│   • wind arrows at each 9 km grid point, colour = speed     │
│   • coloured route polyline when route is active            │
│                                                             │
├─────────────────────────────────────────────────────────────┤
│  Wind: 12 kn NW  Gust: 18 kn  Wave: 0.8 m 6 s  CAPE: 120  │  ← stats bar
└─────────────────────────────────────────────────────────────┘
```

---

## Time Scrubber

- **Day tabs** are generated from available forecast data (up to 7 days from the latest fetch for the active trip).
- Selecting a day resets the hour slider to 00:00 and loads that hour.
- **Hour slider** (0–23): dragging fires a debounced fetch (150 ms) to avoid hammering the API.
- The selected timestamp is displayed as `Wed 14 May · 09:00 UTC`.
- **"Now" button** jumps to the current UTC hour and selects the correct day tab.
- The scrubber controls the background wind arrows independently of any active route.

---

## Map Layer — Wind Arrows

**API:** `GET /api/forecast/grid-points?trip_id=X&timestamp=ISO`  
Returns all grid points for that trip at the nearest stored hour.

Each grid point is rendered as a small SVG arrow on the map:
- **Colour** encodes wind speed on a continuous green → yellow → red scale (0–30 kn).
- **Rotation** encodes wind direction (meteorological convention: arrow points where wind is going).
- Clicking an arrow opens a Leaflet popup showing: wind speed, wind direction, gust, wave height, wave period, CAPE.

The **stats bar** at the bottom shows IDW-averaged values across all grid points for the selected hour. Computed client-side from the grid-points response.

Arrow rendering uses `L.divIcon` with inline SVG. All arrows are re-rendered on every scrubber change (previous markers cleared, new ones added).

---

## Route Planning

### Entry

A **"Plan Route"** button in the page header activates route mode and shows the route bar. When a route is active the button label changes to **"Clear Route"**; clicking it removes all markers, the route line, and hides the route bar.

### Input

The compact route bar (shown above the map in route mode) contains:
- **FROM** — lat/lon set by clicking the map (first click places green marker)
- **TO** — lat/lon set by clicking the map (second click places red marker)
- **Departure** — `<input type="datetime-local">`, default = currently selected scrubber time
- **Speed** — number input (knots), default 5.5, persisted in `localStorage` key `plan_speed_kn`
- **Compute** button — active once FROM, TO, departure, and speed are all set
- **Clear Route** button — resets to a clean state

Map click behaviour: first click in route mode places FROM, second click places TO. Clicking again after both are set replaces FROM (restarting the input cycle).

### Computation

`GET /api/forecast/route?trip_id=X&from_lat=F&from_lon=G&to_lat=T&to_lon=U&departure=ISO&speed_kn=N`

Backend logic:
1. Compute total distance (Haversine) and total passage duration = distance / speed_kn.
2. Generate one synthetic track point per hour: position interpolated linearly along the great-circle bearing from FROM to TO.
3. Load trip forecast grid data using the existing `fetch_trip_forecast_inputs`.
4. Assemble a `TripForecastInputs` with the synthetic track substituted for the real vessel track.
5. Call the existing `compute_trip_overlay` — returns `Vec<TripOverlayPoint>`.

Returns: `Vec<TripOverlayPoint>` (same type as `/api/forecast/trip-overlay`; no new Rust types needed).

### Result on the Map

- A polyline of 10–20 segments drawn from FROM to TO.
- Each segment is coloured by the wind speed at its midpoint's ETA: green (0 kn) → yellow (15 kn) → red (30 kn).
- A label at the TO marker: `ETA Wed 14 · 11:30 · 22 kn NW`.
- Clicking a segment shows a Leaflet popup: UTC timestamp, position, wind speed, wind direction, gust, wave height.
- The route line persists when the scrubber is moved (background arrows update, route stays).

---

## Entry Point on Trip Page

A **"Planning →"** link/button is added to `static/trip.html` near the forecast status line, rendered only when `isActive` is true. It navigates to `plan.html?trip_id=X`.

---

## New API Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/forecast/grid-points?trip_id=X&timestamp=ISO` | All grid points for a trip at one hour |
| `GET` | `/api/forecast/route?trip_id=X&from_lat&from_lon&to_lat&to_lon&departure=ISO&speed_kn=N` | Forecast along a planned straight-line passage |

### `GridPointForecast` response item

```rust
pub struct GridPointForecast {
    pub lat: f64,
    pub lon: f64,
    pub wind_speed_kn: Option<f64>,
    pub wind_direction_deg: Option<f64>,
    pub wind_gust_kn: Option<f64>,
    pub wave_height_m: Option<f64>,
    pub wave_period_s: Option<f64>,
    pub cape_j_kg: Option<f64>,
}
```

---

## Files Changed

| File | Change |
|------|--------|
| `static/plan.html` | New page |
| `src/web/api.rs` | `get_forecast_grid_points`, `get_forecast_route` handlers |
| `src/web/server.rs` | Wire `/api/forecast/grid-points` and `/api/forecast/route` |
| `src/db/operations/forecast.rs` | `get_grid_points_at(trip_id, timestamp)` DB method |
| `static/trip.html` | "Planning →" button for active trips |

---

## Constraints

- All timestamps UTC.
- Wind speed in knots, wave height in metres, wave period in seconds, CAPE in J/kg.
- Haversine for all distance calculations.
- Parameterised SQL queries (`params!` macro).
- The page uses Leaflet (already loaded on trip.html — include same CDN link).
- No new Rust dependencies.
- The page follows existing UI conventions: 1500 px wide, `shared-theme.js`, `shared.css`, `createHeaderBar`.
