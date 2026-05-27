# Polyline Route Planning — Design Spec

**Date:** 2026-05-17
**Status:** Approved

## Summary

Replace the existing two-point (FROM → TO) route planner in `plan.html` with a multi-waypoint polyline. The user clicks to add any number of waypoints, clicks "Done" to end drawing, then clicks "Compute" to fetch the wind forecast along the route. The backend API is extended to accept an ordered sequence of waypoints instead of a single from/to pair; the track is generated leg by leg. No new endpoints, no DB changes.

---

## Files Changed

| File | Change |
|---|---|
| `static/plan.html` | Replace 2-point state with N-waypoint array; new route bar; Undo/Done controls; leg-by-leg dashed preview |
| `src/web/api.rs` | `ForecastRouteQuery` — replace four lat/lon fields with a single `waypoints` string |
| `src/forecast.rs` | `generate_route_track` — accept `&[(f64,f64)]` slice; walk legs sequentially; update unit tests |

`compute_route_overlay`, `drawRouteLine`, and the DB layer are unchanged.

---

## Frontend (`static/plan.html`)

### State

Replace `routeFrom`, `routeTo`, `fromMarker`, `toMarker` with:

```js
let routeWaypoints = [];    // L.LatLng[] in order
let waypointMarkers = [];   // L.CircleMarker[] — one per waypoint
let previewLines = [];      // L.Polyline[] dashed segments between consecutive waypoints
let routeDone = false;      // true after "Done" clicked — map clicks no longer add waypoints
```

### Drawing mode

**`onMapClick(e)`**: if `!routeMode || routeDone` return immediately. Otherwise:
1. Push `e.latlng` to `routeWaypoints`
2. Add a `L.circleMarker` — green (index 0), gray (intermediates), red (last); recolour previous last marker to gray on each new addition
3. If `routeWaypoints.length >= 2`, draw a dashed `L.polyline` from the previous waypoint to the new one and push to `previewLines`
4. Call `updateRouteBar()`

**`undoWaypoint()`**:
1. Remove last marker from map, pop from `waypointMarkers`
2. Remove last preview line from map, pop from `previewLines`
3. Recolour new last marker red (if any remain)
4. Pop from `routeWaypoints`
5. Call `updateRouteBar()`

**`doneDrawing()`**: set `routeDone = true`, hide Undo/Done buttons, call `updateRouteBar()`.

**`clearRoute()`**: remove all markers and preview lines from map, reset all four state variables to empty/false, hide route bar, reset "Plan Route" button and cursor — same contract as today.

### Route bar HTML

Replace the existing FROM/TO span elements with:

```html
<span id="routeWptCount" style="color:var(--text-primary); font-weight:600;"></span>
```

Add Undo and Done buttons next to the existing Compute button:

```html
<button id="undoBtn"  onclick="undoWaypoint()"  ...>↩ Undo</button>
<button id="doneBtn"  onclick="doneDrawing()"   ...>✓ Done</button>
<button id="computeBtn" onclick="computeRoute()" ...>Compute</button>
```

### `updateRouteBar()`

Replaces `checkComputeReady()` as the single function that syncs all route bar state:

```js
function updateRouteBar() {
    const n   = routeWaypoints.length;
    const nm  = totalRouteNm();
    document.getElementById('routeWptCount').textContent =
        n ? `${n} waypoint${n !== 1 ? 's' : ''} · ${nm.toFixed(1)} nm` : 'click map to add waypoints';

    const undoBtn = document.getElementById('undoBtn');
    const doneBtn = document.getElementById('doneBtn');
    undoBtn.disabled = (n === 0 || routeDone);
    doneBtn.disabled = (n < 2  || routeDone);
    undoBtn.style.display = routeDone ? 'none' : '';
    doneBtn.style.display = routeDone ? 'none' : '';

    const dep   = document.getElementById('depInput').value;
    const speed = parseFloat(document.getElementById('speedInput').value);
    const ready = routeDone && n >= 2 && !!dep && speed > 0;
    document.getElementById('computeBtn').disabled = !ready;
    document.getElementById('computeBtn').style.opacity = ready ? '1' : '0.4';
}
```

### `totalRouteNm()`

```js
function totalRouteNm() {
    let d = 0;
    for (let i = 1; i < routeWaypoints.length; i++)
        d += degDistNm(routeWaypoints[i-1].lat, routeWaypoints[i-1].lng,
                       routeWaypoints[i].lat,   routeWaypoints[i].lng);
    return d;
}
```

Uses the existing `degDistNm()` helper already present in the file.

### `computeRoute()`

Build the waypoints query parameter and call the API:

```js
const waypointsParam = routeWaypoints
    .map(p => `${p.lat.toFixed(6)},${p.lng.toFixed(6)}`)
    .join(';');
const url = `/api/forecast/route?trip_id=${tripId}` +
    `&waypoints=${encodeURIComponent(waypointsParam)}` +
    `&departure=${encodeURIComponent(departure)}&speed_kn=${speed}`;
```

`drawRouteLine` removes `previewLines` as part of its cleanup at the top (extend existing cleanup to also clear `previewLines` array and remove them from the map).

`depInput` and `speedInput` event listeners call `updateRouteBar()` instead of `checkComputeReady()`.

---

## Backend

### `ForecastRouteQuery` (`src/web/api.rs`)

```rust
// Before
pub struct ForecastRouteQuery {
    pub trip_id: u32,
    pub from_lat: f64, pub from_lon: f64,
    pub to_lat: f64,   pub to_lon: f64,
    pub departure: String,
    pub speed_kn: f64,
}

// After
pub struct ForecastRouteQuery {
    pub trip_id: u32,
    pub waypoints: String,   // "lat1,lon1;lat2,lon2;..." — at least 2 pairs
    pub departure: String,
    pub speed_kn: f64,
}
```

### Handler (`get_forecast_route`)

Parse `waypoints` before calling `generate_route_track`:

```rust
let wpts: Vec<(f64, f64)> = params.waypoints
    .split(';')
    .map(|pair| {
        let mut it = pair.splitn(2, ',');
        let lat = it.next().and_then(|s| s.trim().parse::<f64>().ok());
        let lon = it.next().and_then(|s| s.trim().parse::<f64>().ok());
        lat.zip(lon)
    })
    .collect::<Option<Vec<_>>>()
    .filter(|v| v.len() >= 2)
    .unwrap_or_default();

if wpts.len() < 2 {
    return Ok(Json(ApiResponse::error(
        "waypoints must contain at least 2 valid lat,lon pairs".to_string()
    )));
}
```

### `generate_route_track` (`src/forecast.rs`)

New signature:

```rust
pub fn generate_route_track(
    waypoints: &[(f64, f64)],
    departure: DateTime<Utc>,
    speed_kn: f64,
) -> Vec<(f64, f64, DateTime<Utc>)>
```

Implementation: iterate consecutive pairs. For each leg, run the existing linear interpolation at 1-hour steps (same logic as today). The departure time for leg N is the arrival time of leg N-1. Concatenate results; skip the first point of each leg after the first (it is identical to the last point of the previous leg) to avoid duplicates.

```rust
pub fn generate_route_track(
    waypoints: &[(f64, f64)],
    departure: DateTime<Utc>,
    speed_kn: f64,
) -> Vec<(f64, f64, DateTime<Utc>)> {
    let mut track = Vec::new();
    let mut leg_start = departure;
    for w in waypoints.windows(2) {
        let (from_lat, from_lon) = w[0];
        let (to_lat, to_lon)     = w[1];
        let leg = generate_leg(from_lat, from_lon, to_lat, to_lon, leg_start, speed_kn);
        if leg.is_empty() { continue; }
        leg_start = leg.last().unwrap().2;
        // Skip first point of subsequent legs (duplicate of previous leg's last point)
        let skip = if track.is_empty() { 0 } else { 1 };
        track.extend_from_slice(&leg[skip..]);
    }
    track
}
```

`generate_leg` is the existing body of `generate_route_track` (renamed, private).

### Unit tests

- `test_generate_route_track_point_count` and `test_generate_route_track_timestamps_advance_hourly`: update to pass a `&[(f64,f64)]` slice instead of four scalars.
- Add `test_generate_route_track_two_legs`: verify that a two-leg route produces the correct total point count and that timestamps are monotonically increasing across the leg boundary.

---

## Data flow

```
Browser (drawing):
  onMapClick → routeWaypoints.push(latlng) → updateRouteBar()
  undoWaypoint → routeWaypoints.pop() → updateRouteBar()
  doneDrawing → routeDone = true → updateRouteBar()

Browser (compute):
  computeRoute()
    → GET /api/forecast/route?waypoints=lat1,lon1;lat2,lon2;...&departure=...&speed_kn=...
    → drawRouteLine(json.data)  [clears previewLines, renders colored segments + ETA]

Backend:
  parse waypoints string → Vec<(f64,f64)>
  generate_route_track(waypoints, departure, speed_kn) → flat Vec<(lat,lon,ts)>
  compute_route_overlay(track, fetches) → Vec<RouteOverlayPoint>
```

---

## Edge cases

- **Single waypoint placed, Done disabled**: Compute never becomes reachable — enforced by `updateRouteBar()`.
- **Undo back to 0 waypoints**: bar shows "click map to add waypoints", all buttons disabled or hidden.
- **Very short leg (< 0.1 nm)**: `generate_leg` returns a single point (existing behaviour); the leg is included but contributes no intermediate track points.
- **Waypoints string parse failure**: handler returns `ApiResponse::error(...)` — frontend shows "Error — retry" on the Compute button (existing error path).
- **previewLines not cleared before drawRouteLine**: `drawRouteLine` extends existing cleanup to also remove `previewLines` markers from the map and reset the array.
