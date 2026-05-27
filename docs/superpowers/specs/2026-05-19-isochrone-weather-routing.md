# Isochrone Weather Routing — Design Spec

**Date:** 2026-05-19
**Status:** Approved

**Prerequisite:** `2026-05-19-polar-aware-routing.md` must be implemented first. This spec depends on `PolarTable`, `compute_twa`, `advance_position`, and the `polars` field on `AppState`.

---

## Summary

Add an **"Optimize Route"** button to `plan.html` that finds the fastest passage between the first and last waypoints using the isochrone method. The user sets departure point (first waypoint), destination (last waypoint), departure time, and motoring speed; the system fans out from the departure through the 7-day forecast using the vessel's polar table and returns the optimal path as a solid colored polyline alongside the user-drawn dashed route. Optimizes for fastest passage time only.

---

## Files Changed

| File | Change |
|---|---|
| `src/routing.rs` | New — `IsochronePoint`, `run_isochrone()`, backtracking |
| `src/web/api.rs` | Add `get_optimal_route` handler |
| `src/web/server.rs` | Wire `GET /api/forecast/optimal-route` |
| `static/plan.html` | "Optimize Route" button, render optimized route as solid polyline |

---

## Algorithm (`src/routing.rs`)

### Data structures

```rust
/// One point on an isochrone frontier.
#[derive(Clone)]
struct IsochronePoint {
    lat: f64,
    lon: f64,
    time: DateTime<Utc>,
    /// Index into the previous isochrone's vec. None for the seed point.
    parent_idx: Option<usize>,
}

/// Result of a successful (or best-effort) isochrone run.
pub struct IsochroneResult {
    /// Optimal route from departure to destination, inclusive, one point per hour.
    pub track: Vec<(f64, f64, DateTime<Utc>)>,
    /// true if destination was reached within the forecast horizon.
    pub reached_destination: bool,
}
```

### Public function

```rust
pub fn run_isochrone(
    from: (f64, f64),
    to: (f64, f64),
    departure: DateTime<Utc>,
    motoring_speed_kn: f64,
    polars: &crate::polars::PolarTable,
    forecast_inputs: &crate::forecast::TripForecastInputs,
) -> IsochroneResult
```

### Algorithm steps

```
CONSTANTS:
    HEADING_STEP_DEG = 5.0          // 72 candidate headings per point
    SECTOR_COUNT = 72               // pruning sectors around origin
    MAX_STEPS = 168                 // 7 days × 24 hours
    ARRIVAL_THRESHOLD_NM = 5.0     // destination reached when within this distance

seed = IsochronePoint { lat: from.0, lon: from.1, time: departure, parent_idx: None }
isochrones: Vec<Vec<IsochronePoint>> = vec![vec![seed]]

for step in 1..=MAX_STEPS:
    candidates: Vec<(IsochronePoint, usize)> = []   // (new point, parent_idx in prev isochrone)

    for (parent_idx, parent) in isochrones.last().enumerate():
        wind = nearest_forecast_wind(forecast_inputs, parent.lat, parent.lon, parent.time)

        for heading in (0..72).map(|i| i as f64 * 5.0):
            speed_kn = match wind:
                Some(w) if w.speed_kn >= 5.0 =>
                    let twa = compute_twa(heading, w.direction_deg)
                    polars.boat_speed(twa, w.speed_kn).unwrap_or(motoring_speed_kn)
                _ => motoring_speed_kn

            new_pos = advance_position(parent.lat, parent.lon, heading, speed_kn)  // 1h step
            new_pt = IsochronePoint {
                lat: new_pos.0, lon: new_pos.1,
                time: parent.time + 1h,
                parent_idx: Some(parent_idx),
            }
            candidates.push((new_pt, parent_idx))

    pruned = prune_isochrone(candidates, origin=from)
    isochrones.push(pruned)

    // Check for arrival
    for (idx, pt) in pruned.enumerate():
        if haversine_distance_nm(pt.lat, pt.lon, to.0, to.1) <= ARRIVAL_THRESHOLD_NM:
            track = backtrack(&isochrones, arrival_idx=idx, destination=to)
            return IsochroneResult { track, reached_destination: true }

// Destination not reached — return best-effort track to closest point
let (best_idx, _) = isochrones.last()
    .enumerate()
    .min_by(|(_, a), (_, b)|
        haversine_distance_nm(a.lat, a.lon, to.0, to.1)
            .partial_cmp(&haversine_distance_nm(b.lat, b.lon, to.0, to.1))
            .unwrap()
    ).unwrap()
track = backtrack(&isochrones, best_idx, destination=to)
return IsochroneResult { track, reached_destination: false }
```

### Pruning (`prune_isochrone`)

Divide the compass rose into `SECTOR_COUNT` equal sectors **measured from the origin (departure point)**. For each candidate point, compute its bearing from the origin and assign it to the matching sector. Within each sector, keep only the point farthest from the origin (by great-circle distance from `from`). The pruned isochrone has at most 72 points.

```rust
fn prune_isochrone(
    candidates: Vec<IsochronePoint>,
    origin: (f64, f64),
) -> Vec<IsochronePoint> {
    let mut sectors: Vec<Option<IsochronePoint>> = vec![None; SECTOR_COUNT];
    let mut sector_dist: Vec<f64> = vec![0.0; SECTOR_COUNT];

    for pt in candidates {
        let bearing = haversine_bearing(origin.0, origin.1, pt.lat, pt.lon);
        let sector = (bearing / (360.0 / SECTOR_COUNT as f64)) as usize % SECTOR_COUNT;
        let dist = haversine_distance_nm(origin.0, origin.1, pt.lat, pt.lon);
        if dist > sector_dist[sector] {
            sector_dist[sector] = dist;
            sectors[sector] = Some(pt);
        }
    }

    sectors.into_iter().flatten().collect()
}
```

### Backtracking

```rust
fn backtrack(
    isochrones: &[Vec<IsochronePoint>],
    arrival_idx: usize,
    destination: (f64, f64),
) -> Vec<(f64, f64, DateTime<Utc>)> {
    let mut path: Vec<(f64, f64, DateTime<Utc>)> = Vec::new();

    // Append the actual destination as the final point at the arrival isochrone's time.
    // The remaining distance is ≤ ARRIVAL_THRESHOLD_NM (5 nm) — acceptable ETA error.
    let arrival = &isochrones.last().unwrap()[arrival_idx];
    path.push((destination.0, destination.1, arrival.time));

    let mut cur_idx = arrival_idx;
    for step in (0..isochrones.len()).rev() {
        let pt = &isochrones[step][cur_idx];
        path.push((pt.lat, pt.lon, pt.time));
        match pt.parent_idx {
            Some(idx) => cur_idx = idx,
            None => break,
        }
    }

    path.reverse();
    path
}
```

---

## New API endpoint (`src/web/api.rs`)

### Query struct

```rust
#[derive(Debug, Deserialize)]
pub struct OptimalRouteQuery {
    pub trip_id: u32,
    pub from_lat: f64,
    pub from_lon: f64,
    pub to_lat: f64,
    pub to_lon: f64,
    pub departure: String,          // ISO 8601 UTC
    pub motoring_speed_kn: f64,
}
```

### Handler

```rust
pub async fn get_optimal_route(
    State(state): State<AppState>,
    Query(params): Query<OptimalRouteQuery>,
) -> Result<Json<ApiResponse<Vec<RouteOverlayPoint>>>, StatusCode> {
    let polars = match state.polars() {
        Some(p) => p,
        None => return Ok(Json(ApiResponse::error(
            "No polar table configured — cannot run isochrone routing".to_string()
        ))),
    };

    let departure = params.departure.parse::<DateTime<Utc>>()
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let forecast_inputs = {
        let db = state.db();
        db.fetch_trip_forecast_inputs(params.trip_id)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    if forecast_inputs.fetches.is_empty() {
        return Ok(Json(ApiResponse::error(
            "No forecast data available for this trip".to_string()
        )));
    }

    let result = crate::routing::run_isochrone(
        (params.from_lat, params.from_lon),
        (params.to_lat, params.to_lon),
        departure,
        params.motoring_speed_kn,
        polars,
        &forecast_inputs,
    );

    let overlay = crate::forecast::compute_route_overlay(&result.track, &forecast_inputs);
    Ok(Json(ApiResponse::ok(overlay)))
}
```

### Route (`src/web/server.rs`)

In the `!read_only` block:

```rust
.route("/forecast/optimal-route", get(get_optimal_route))
```

---

## Frontend (`static/plan.html`)

### New state variable

```js
let optimizedSegments = [];   // L.Polyline[] — solid colored segments from Optimize
```

### "Optimize Route" button

Add next to the existing "Compute" button in the route bar:

```html
<button id="optimizeBtn" onclick="optimizeRoute()" disabled
    style="padding:6px 16px; background:var(--accent-color); color:#fff;
           border:none; border-radius:4px; cursor:pointer; font-size:13px; opacity:0.4;">
    ⚡ Optimize
</button>
```

### `updateRouteBar()` change

Add to the existing `updateRouteBar()` function:

```js
const optimizeBtn = document.getElementById('optimizeBtn');
if (optimizeBtn) {
    // Optimize needs polars on server — only first and last waypoints matter
    const canOptimize = routeDone && routeWaypoints.length >= 2 && !!dep && speed > 0;
    optimizeBtn.disabled = !canOptimize;
    optimizeBtn.style.opacity = canOptimize ? '1' : '0.4';
}
```

### `optimizeRoute()`

```js
async function optimizeRoute() {
    const btn = document.getElementById('optimizeBtn');
    const orig = btn.textContent;
    btn.disabled = true;
    btn.textContent = '⚡ Optimizing…';

    const from = routeWaypoints[0];
    const to   = routeWaypoints[routeWaypoints.length - 1];
    const departure = document.getElementById('depInput').value;
    const speed = parseFloat(document.getElementById('speedInput').value);
    const dep = new Date(departure).toISOString();

    try {
        const url = `/api/forecast/optimal-route?trip_id=${tripId}`
            + `&from_lat=${from.lat.toFixed(6)}&from_lon=${from.lng.toFixed(6)}`
            + `&to_lat=${to.lat.toFixed(6)}&to_lon=${to.lng.toFixed(6)}`
            + `&departure=${encodeURIComponent(dep)}&motoring_speed_kn=${speed}`;

        const resp = await fetch(url);
        const json = await resp.json();
        if (json.status !== 'ok' || !json.data?.length) {
            btn.textContent = '✗ ' + (json.error || 'Error');
            setTimeout(() => { btn.textContent = orig; btn.disabled = false; }, 3000);
            return;
        }
        drawOptimizedRoute(json.data);
        btn.textContent = orig;
        btn.disabled = false;
    } catch (_) {
        btn.textContent = '✗ Error';
        setTimeout(() => { btn.textContent = orig; btn.disabled = false; }, 3000);
    }
}
```

### `buildSegmentPopup(point)` — new helper extracted from `drawRouteLine`

Extract the popup HTML that is currently built inline inside `drawRouteLine` into a standalone helper:

```js
function buildSegmentPopup(p) {
    const ts   = p.timestamp ? new Date(p.timestamp).toUTCString().slice(0, 22) : '—';
    const spd  = p.wind_speed_kn   != null ? p.wind_speed_kn.toFixed(1)   + ' kn'  : '—';
    const dir  = p.wind_dir_deg    != null ? p.wind_dir_deg.toFixed(0)    + '°'    : '—';
    const gust = p.wind_gust_kn    != null ? p.wind_gust_kn.toFixed(1)    + ' kn'  : '—';
    const wh   = p.wave_height_m   != null ? p.wave_height_m.toFixed(1)   + ' m'   : '—';
    const wp   = p.wave_period_s   != null ? p.wave_period_s.toFixed(0)   + ' s'   : '—';
    const spd2 = p.speed_kn        != null ? p.speed_kn.toFixed(1)        + ' kn'  : '—';
    const twa  = p.twa_deg         != null ? p.twa_deg.toFixed(0)         + '°'    : '—';
    return `<b>${ts}</b><br>Wind: ${spd} · ${dir} (gust ${gust})<br>`
         + `Wave: ${wh} / ${wp}<br>Boat speed: ${spd2} · TWA: ${twa}`;
}
```

Replace the inline popup HTML in `drawRouteLine` with `seg.bindPopup(buildSegmentPopup(curr))`.

### `drawOptimizedRoute(points)`

Mirrors `drawRouteLine` but renders **solid** polylines (not dashed) and clears `optimizedSegments` instead of `routeSegments`:

```js
function drawOptimizedRoute(points) {
    optimizedSegments.forEach(s => planMap.removeLayer(s));
    optimizedSegments = [];
    if (!points.length) return;

    for (let i = 1; i < points.length; i++) {
        const prev = points[i - 1];
        const curr = points[i];
        const color = windColor(curr.wind_speed_kn ?? 0);
        const seg = L.polyline(
            [[prev.lat, prev.lon], [curr.lat, curr.lon]],
            { color, weight: 4, opacity: 0.9 }   // solid, slightly thicker than dashed user route
        ).addTo(planMap);
        seg.bindPopup(buildSegmentPopup(curr));
        optimizedSegments.push(seg);
    }

    // ETA label at destination
    const last = points[points.length - 1];
    const eta = new Date(last.timestamp);
    const etaStr = eta.toUTCString().slice(0, 22);
    L.marker([last.lat, last.lon], {
        icon: L.divIcon({
            className: '',
            html: `<div style="background:var(--bg-primary);border:1px solid var(--border-color);
                               border-radius:3px;padding:2px 6px;font-size:11px;white-space:nowrap;">
                       ⚡ ETA ${etaStr} · ${(last.wind_speed_kn ?? 0).toFixed(0)} kn
                   </div>`,
            iconAnchor: [0, 0]
        })
    }).addTo(planMap);
}
```

### `clearRoute()` extension

Add to the existing `clearRoute()` to also remove the optimized route:

```js
optimizedSegments.forEach(s => planMap.removeLayer(s));
optimizedSegments = [];
```

### Visual distinction

| Route | Style |
|---|---|
| User-drawn (Compute) | dashed polyline, weight 3 |
| Optimized (Optimize) | solid polyline, weight 4 |

Both use the same wind-speed color scale.

---

## Tests (`src/routing.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dummy_polars() -> crate::polars::PolarTable {
        // A simple polar: constant 6 kn at any TWA/TWS ≥ 6 kn
        crate::polars::PolarTable::constant_for_test(6.0)
    }

    #[test]
    fn test_prune_retains_at_most_72_points() {
        // Feed 720 candidates (10 per sector) — pruned result must have ≤72 points
        let origin = (43.0, 8.0);
        let candidates: Vec<IsochronePoint> = (0..720).map(|i| {
            let bearing = (i as f64) * 0.5;
            let dist = 5.0 + (i % 10) as f64;
            let (lat, lon) = advance_position(origin.0, origin.1, bearing, dist);
            IsochronePoint { lat, lon, time: chrono::Utc::now(), parent_idx: Some(0) }
        }).collect();
        let pruned = prune_isochrone(candidates, origin);
        assert!(pruned.len() <= 72);
    }

    #[test]
    fn test_isochrone_reaches_nearby_destination() {
        // 20 nm straight north — should be reached in ~3–4 hours at 6 kn
        let from = (43.0, 8.0);
        let to = (43.29, 8.0);   // ~20 nm north
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let polars = dummy_polars();
        let forecast = crate::forecast::TripForecastInputs { fetches: vec![] };  // no wind → motoring

        let result = run_isochrone(from, to, departure, 6.0, &polars, &forecast);
        assert!(result.reached_destination);
        assert!(result.track.len() >= 2);
        // Destination should be last point
        let last = result.track.last().unwrap();
        let dist = crate::utilities::haversine_distance_nm(last.0, last.1, to.0, to.1);
        assert!(dist < 10.0, "last point is {}nm from destination", dist);
    }

    #[test]
    fn test_backtrack_produces_monotonic_timestamps() {
        let from = (43.0, 8.0);
        let to = (43.29, 8.0);
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let polars = dummy_polars();
        let forecast = crate::forecast::TripForecastInputs { fetches: vec![] };

        let result = run_isochrone(from, to, departure, 6.0, &polars, &forecast);
        let times: Vec<_> = result.track.iter().map(|p| p.2).collect();
        for w in times.windows(2) {
            assert!(w[1] >= w[0], "timestamps not monotonic: {:?} then {:?}", w[0], w[1]);
        }
    }
}
```

`PolarTable::constant_for_test(speed)` is a `#[cfg(test)]` constructor that returns a polar always yielding the given speed for any TWA ≥ 42° and TWS ≥ 5 kn.

---

## Constraints

- All speeds in knots, distances in nautical miles, angles in degrees, timestamps UTC.
- Haversine for all distance and bearing calculations.
- No new Rust dependencies.
- When `polars` is `None` in `AppState`, the `/api/forecast/optimal-route` endpoint returns an error response (not a 500). The "Optimize" button remains visible but the error is surfaced in the button state.
- The isochrone runs synchronously in the request handler. At 72 × 72 × 168 steps it completes in well under 1 second; no async task needed.
- Maximum forecast horizon is bounded by available data in `TripForecastInputs` (7 days). If a grid point has no forecast entry, `nearest_forecast_wind` returns `None` and motoring speed is used.
