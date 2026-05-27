# Isochrone Weather Routing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an "Optimize Route" button that finds the fastest passage between start and end waypoints using the isochrone method with polar diagrams and 7-day forecast data.

**Architecture:** A new `src/routing.rs` module implements the isochrone fan-out algorithm (72 headings × 168 hours × ≤72 pruned points per isochrone). The handler in `api.rs` calls it synchronously — well under 1 second — and reuses `compute_route_overlay` to attach forecast weather to the result. The frontend renders the optimal path as a solid polyline alongside the user-drawn dashed route.

**Tech Stack:** Rust (axum, chrono), vanilla JavaScript, Leaflet.js. No new dependencies.

---

## Context for implementers

This feature builds on the polar-aware routing already in place. Key existing pieces:
- `src/polars.rs`: `PolarTable` with `boat_speed(twa_deg, tws_kn) -> Option<f64>` and `constant_for_test(speed) -> Self`
- `src/forecast.rs`: `compute_twa(cog, wind_dir) -> f64`, `compute_route_overlay(track, fetches) -> Vec<RouteOverlayPoint>`, private `nearest_forecast_wind(fetches, lat, lon, time) -> Option<(f64, f64)>`
- `src/utilities.rs`: `advance_position(lat, lon, bearing_deg, dist_nm) -> (f64, f64)`, `haversine_distance_nm(...)`, `haversine_heading(...)`
- `src/web/api.rs`: `AppState` with `polars: Option<Arc<PolarTable>>`, `polars()` helper, `ApiResponse<T>`, `get_forecast_route` handler as a pattern to follow
- `src/db/operations/forecast.rs`: `FetchWithHourly { lat, lon, hourly }`, `fetch_forecast_fetches(trip_id) -> Result<Vec<FetchWithHourly>>`
- `static/plan.html`: Route bar with `computeBtn`, state variables `routeDone`, `routeWaypoints`, `routeSegments`, `clearRoute()` function

**CLAUDE.md rule:** Do NOT run `git commit` or `git push`. Stop after writing code.

---

## File map

| File | Change |
|---|---|
| `src/forecast.rs` | Make `nearest_forecast_wind` pub(crate) |
| `src/routing.rs` | New — `IsochronePoint`, `IsochroneResult`, `prune_isochrone`, `backtrack`, `run_isochrone` |
| `src/main.rs` | Add `pub mod routing;` |
| `src/web/api.rs` | Add `OptimalRouteQuery`, `get_optimal_route` handler, register route |
| `static/plan.html` | `optimizedSegments` state, `buildSegmentPopup()`, Optimize button, `optimizeRoute()`, `drawOptimizedRoute()`, update `clearRoute()` |

---

## Task 1: Expose `nearest_forecast_wind` and create `src/routing.rs`

**Files:**
- Modify: `src/forecast.rs` (change `fn` to `pub(crate) fn` for `nearest_forecast_wind`)
- Create: `src/routing.rs`
- Modify: `src/main.rs` (add `pub mod routing;`)

### Background

The isochrone algorithm needs to look up forecast wind at any (lat, lon, time). That logic already exists in `src/forecast.rs` as the private function `nearest_forecast_wind`. We expose it as `pub(crate)` so `routing.rs` can call it as `crate::forecast::nearest_forecast_wind(...)`.

---

- [ ] **Step 1: Make `nearest_forecast_wind` pub(crate) in `src/forecast.rs`**

Find line ~314 in `src/forecast.rs`:
```rust
fn nearest_forecast_wind(
```
Change it to:
```rust
pub(crate) fn nearest_forecast_wind(
```

- [ ] **Step 2: Verify it compiles**

```bash
cargo check 2>&1 | head -20
```
Expected: no errors (the function was only used within the same file — making it `pub(crate)` is backward-compatible).

- [ ] **Step 3: Write the tests first in `src/routing.rs`**

Create `src/routing.rs` with only the test module (no implementation yet):

```rust
use chrono::{DateTime, Utc};
use crate::db::operations::forecast::FetchWithHourly;
use crate::forecast::compute_twa;
use crate::utilities::{advance_position, haversine_distance_nm, haversine_heading};

// ── Constants ─────────────────────────────────────────────────────────────────

const HEADING_STEP_DEG: f64 = 5.0;
const SECTOR_COUNT: usize = 72;
const MAX_STEPS: usize = 168;
const ARRIVAL_THRESHOLD_NM: f64 = 5.0;

// ── Data structures ───────────────────────────────────────────────────────────

#[derive(Clone)]
struct IsochronePoint {
    lat: f64,
    lon: f64,
    time: DateTime<Utc>,
    parent_idx: Option<usize>,
}

pub struct IsochroneResult {
    pub track: Vec<(f64, f64, DateTime<Utc>)>,
    pub reached_destination: bool,
}

// ── Public function (stub for now) ────────────────────────────────────────────

pub fn run_isochrone(
    _from: (f64, f64),
    _to: (f64, f64),
    _departure: DateTime<Utc>,
    _motoring_speed_kn: f64,
    _polars: &crate::polars::PolarTable,
    _fetches: &[FetchWithHourly],
) -> IsochroneResult {
    IsochroneResult { track: vec![], reached_destination: false }
}

// ── Private helpers (stubs) ───────────────────────────────────────────────────

fn prune_isochrone(
    _candidates: Vec<IsochronePoint>,
    _origin: (f64, f64),
) -> Vec<IsochronePoint> {
    vec![]
}

fn backtrack(
    _isochrones: &[Vec<IsochronePoint>],
    _arrival_idx: usize,
    _destination: (f64, f64),
) -> Vec<(f64, f64, DateTime<Utc>)> {
    vec![]
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dummy_polars() -> crate::polars::PolarTable {
        crate::polars::PolarTable::constant_for_test(6.0)
    }

    #[test]
    fn test_prune_retains_at_most_72_points() {
        let origin = (43.0, 8.0);
        let candidates: Vec<IsochronePoint> = (0..720).map(|i| {
            let bearing = (i as f64) * 0.5;
            let dist = 5.0 + (i % 10) as f64;
            let (lat, lon) = advance_position(origin.0, origin.1, bearing, dist);
            IsochronePoint { lat, lon, time: chrono::Utc::now(), parent_idx: Some(0) }
        }).collect();
        let pruned = prune_isochrone(candidates, origin);
        assert!(pruned.len() <= SECTOR_COUNT, "expected ≤72, got {}", pruned.len());
    }

    #[test]
    fn test_isochrone_reaches_nearby_destination() {
        let from = (43.0, 8.0);
        let to = (43.29, 8.0);   // ~20 nm north
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let polars = dummy_polars();

        let result = run_isochrone(from, to, departure, 6.0, &polars, &[]);
        assert!(result.reached_destination, "should reach destination in ~4h at 6 kn");
        assert!(result.track.len() >= 2);
        let last = result.track.last().unwrap();
        let dist = haversine_distance_nm(last.0, last.1, to.0, to.1);
        assert!(dist < 10.0, "last point is {}nm from destination", dist);
    }

    #[test]
    fn test_backtrack_produces_monotonic_timestamps() {
        let from = (43.0, 8.0);
        let to = (43.29, 8.0);
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let polars = dummy_polars();

        let result = run_isochrone(from, to, departure, 6.0, &polars, &[]);
        let times: Vec<_> = result.track.iter().map(|p| p.2).collect();
        for w in times.windows(2) {
            assert!(w[1] >= w[0], "timestamps not monotonic: {:?} then {:?}", w[0], w[1]);
        }
    }
}
```

- [ ] **Step 4: Add `pub mod routing;` to `src/main.rs`**

In `src/main.rs`, add after the `pub mod polars;` line:
```rust
pub mod routing;
```

- [ ] **Step 5: Run tests to confirm they fail (stubs return empty)**

```bash
cargo test routing:: 2>&1
```
Expected: `test_prune_retains_at_most_72_points` — PASS (empty vec has 0 ≤ 72)
`test_isochrone_reaches_nearby_destination` — FAIL (`reached_destination` is false)
`test_backtrack_produces_monotonic_timestamps` — PASS (empty track trivially monotonic)

- [ ] **Step 6: Implement `prune_isochrone`**

Replace the stub `prune_isochrone` with the real implementation:

```rust
fn prune_isochrone(
    candidates: Vec<IsochronePoint>,
    origin: (f64, f64),
) -> Vec<IsochronePoint> {
    let mut sectors: Vec<Option<IsochronePoint>> = vec![None; SECTOR_COUNT];
    let mut sector_dist: Vec<f64> = vec![0.0; SECTOR_COUNT];

    for pt in candidates {
        let bearing = haversine_heading(origin.0, origin.1, pt.lat, pt.lon);
        let sector = ((bearing / (360.0 / SECTOR_COUNT as f64)) as usize) % SECTOR_COUNT;
        let dist = haversine_distance_nm(origin.0, origin.1, pt.lat, pt.lon);
        if dist > sector_dist[sector] {
            sector_dist[sector] = dist;
            sectors[sector] = Some(pt);
        }
    }

    sectors.into_iter().flatten().collect()
}
```

- [ ] **Step 7: Implement `backtrack`**

Replace the stub `backtrack` with:

```rust
fn backtrack(
    isochrones: &[Vec<IsochronePoint>],
    arrival_idx: usize,
    destination: (f64, f64),
) -> Vec<(f64, f64, DateTime<Utc>)> {
    let mut path: Vec<(f64, f64, DateTime<Utc>)> = Vec::new();

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

- [ ] **Step 8: Implement `run_isochrone`**

Replace the stub `run_isochrone` with:

```rust
pub fn run_isochrone(
    from: (f64, f64),
    to: (f64, f64),
    departure: DateTime<Utc>,
    motoring_speed_kn: f64,
    polars: &crate::polars::PolarTable,
    fetches: &[FetchWithHourly],
) -> IsochroneResult {
    let seed = IsochronePoint { lat: from.0, lon: from.1, time: departure, parent_idx: None };
    let mut isochrones: Vec<Vec<IsochronePoint>> = vec![vec![seed]];

    for _step in 1..=MAX_STEPS {
        let prev = isochrones.last().unwrap();
        let mut candidates: Vec<IsochronePoint> = Vec::new();

        for (parent_idx, parent) in prev.iter().enumerate() {
            let wind = crate::forecast::nearest_forecast_wind(fetches, parent.lat, parent.lon, parent.time);

            for h in 0..SECTOR_COUNT {
                let heading = h as f64 * HEADING_STEP_DEG;

                let speed_kn = match wind {
                    Some((wind_spd, wind_dir)) if wind_spd >= 5.0 => {
                        let twa = compute_twa(heading, wind_dir);
                        polars.boat_speed(twa, wind_spd).unwrap_or(motoring_speed_kn)
                    }
                    _ => motoring_speed_kn,
                };

                let new_pos = advance_position(parent.lat, parent.lon, heading, speed_kn);
                candidates.push(IsochronePoint {
                    lat: new_pos.0,
                    lon: new_pos.1,
                    time: parent.time + chrono::Duration::hours(1),
                    parent_idx: Some(parent_idx),
                });
            }
        }

        let pruned = prune_isochrone(candidates, from);
        isochrones.push(pruned.clone());

        for (idx, pt) in pruned.iter().enumerate() {
            if haversine_distance_nm(pt.lat, pt.lon, to.0, to.1) <= ARRIVAL_THRESHOLD_NM {
                let track = backtrack(&isochrones, idx, to);
                return IsochroneResult { track, reached_destination: true };
            }
        }
    }

    // Destination not reached — return best-effort track to closest point
    let last_iso = isochrones.last().unwrap();
    let best_idx = last_iso
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            haversine_distance_nm(a.lat, a.lon, to.0, to.1)
                .partial_cmp(&haversine_distance_nm(b.lat, b.lon, to.0, to.1))
                .unwrap()
        })
        .map(|(i, _)| i)
        .unwrap_or(0);

    let track = backtrack(&isochrones, best_idx, to);
    IsochroneResult { track, reached_destination: false }
}
```

- [ ] **Step 9: Run tests to confirm all pass**

```bash
cargo test routing:: 2>&1
```
Expected: all 3 tests PASS.

- [ ] **Step 10: Run full test suite**

```bash
cargo test 2>&1 | tail -20
```
Expected: 119+ non-DB tests pass, 0 new failures.

---

## Task 2: API endpoint

**Files:**
- Modify: `src/web/api.rs` — add `OptimalRouteQuery`, `get_optimal_route` handler, wire route

### Background

`get_forecast_route` (line ~1638 in `api.rs`) is the direct pattern to follow. The handler:
1. Validates polars are loaded (returns error if not)
2. Parses `departure` via `DateTime::parse_from_rfc3339`
3. Calls `state.db().fetch_forecast_fetches(params.trip_id)` to get forecast grid data
4. Calls `crate::routing::run_isochrone(...)` 
5. Converts the result track to `Vec<RouteTrackPoint>` and calls `compute_route_overlay`

The route is registered in the `!read_only` block of `create_api_router` (around line 1710 in `api.rs`).

---

- [ ] **Step 1: Add `OptimalRouteQuery` struct to `src/web/api.rs`**

Add after `ForecastRouteQuery` (after line ~271 in `api.rs`):

```rust
#[derive(Debug, Deserialize)]
pub struct OptimalRouteQuery {
    pub trip_id: u32,
    pub from_lat: f64,
    pub from_lon: f64,
    pub to_lat: f64,
    pub to_lon: f64,
    pub departure: String,          // ISO 8601 UTC, e.g. "2026-06-01T06:00:00Z"
    pub motoring_speed_kn: f64,
}
```

- [ ] **Step 2: Add `get_optimal_route` handler to `src/web/api.rs`**

Add after `get_forecast_route` (after line ~1676 in `api.rs`):

```rust
pub async fn get_optimal_route(
    State(state): State<AppState>,
    Query(params): Query<OptimalRouteQuery>,
) -> Result<Json<ApiResponse<Vec<crate::forecast::RouteOverlayPoint>>>, StatusCode> {
    let polars = match state.polars() {
        Some(p) => p,
        None => return Ok(Json(ApiResponse::error(
            "No polar table configured — cannot run isochrone routing".to_string()
        ))),
    };

    let departure = match chrono::DateTime::parse_from_rfc3339(&params.departure) {
        Ok(dt) => dt.with_timezone(&chrono::Utc),
        Err(_) => return Ok(Json(ApiResponse::error(
            format!("Invalid departure timestamp: {}", params.departure)
        ))),
    };

    if params.motoring_speed_kn <= 0.0 {
        return Ok(Json(ApiResponse::error("motoring_speed_kn must be positive".to_string())));
    }

    let fetches = match state.db().fetch_forecast_fetches(params.trip_id) {
        Ok(f) => f,
        Err(e) => {
            error!(error = %e, trip_id = params.trip_id, "Failed to load forecast fetches for optimal route");
            return Ok(Json(ApiResponse::error(e.to_string())));
        }
    };

    if fetches.is_empty() {
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
        &fetches,
    );

    let route_points: Vec<crate::forecast::RouteTrackPoint> = result.track.iter()
        .map(|(lat, lon, time)| crate::forecast::RouteTrackPoint {
            lat: *lat, lon: *lon, time: *time, speed_kn: None, twa_deg: None,
        })
        .collect();

    let overlay = crate::forecast::compute_route_overlay(&route_points, &fetches);
    Ok(Json(ApiResponse::ok(overlay)))
}
```

- [ ] **Step 3: Register the route in `create_api_router`**

In `create_api_router` (around line 1710), find the `if !read_only {` block and add the route. The block starts with:
```rust
    if !read_only {
        router = router
            .route("/trip_description", post(update_trip_description))
```

Add the optimal-route registration at the end of the `!read_only` block, just before the final `router.with_state(state)`. Find the block that ends with `.route("/forecast/refresh", post(refresh_forecast));` (around line 1739) and add:

```rust
            .route("/forecast/optimal-route", get(get_optimal_route))
```

The complete end of the `!read_only` block should look like:
```rust
            .route("/forecast/areas", post(create_forecast_area))
            .route("/forecast/areas", delete(delete_forecast_area))
            .route("/forecast/refresh", post(refresh_forecast))
            .route("/forecast/optimal-route", get(get_optimal_route));
```

- [ ] **Step 4: Run cargo test to confirm all tests still pass**

```bash
cargo test 2>&1 | tail -20
```
Expected: 119+ non-DB tests pass, no new failures.

---

## Task 3: Frontend — "Optimize Route" button and result rendering

**Files:**
- Modify: `static/plan.html`

### Background

The existing `drawRouteLine` function builds per-segment popups inline. The spec requires extracting this to `buildSegmentPopup(p)` so both `drawRouteLine` and `drawOptimizedRoute` can share it.

The current route bar ends with `computeBtn`. Add the `optimizeBtn` after it.

The `clearRoute()` function already clears `routeSegments` — extend it to clear `optimizedSegments` too.

---

- [ ] **Step 1: Add `optimizedSegments` state variable**

In `static/plan.html`, find the state variable block (around line 188–202) that ends with:
```js
        let lastRouteOverlay = []; // last computed overlay — persisted across reloads
```

Add after that line:
```js
        let optimizedSegments = [];   // L.Polyline[] from Optimize button
```

- [ ] **Step 2: Add the "Optimize Route" button to the route bar**

Find the Compute button in the HTML (around line 99):
```html
                <button id="computeBtn" onclick="computeRoute()" disabled
                    style="padding:5px 14px; background:var(--link-color); color:#fff; border:none;
                           border-radius:4px; cursor:pointer; font-size:13px; opacity:0.4;">
                    Compute
                </button>
```

Add immediately after the closing `</button>` tag:
```html
                <button id="optimizeBtn" onclick="optimizeRoute()" disabled
                    style="padding:6px 16px; background:var(--accent-color); color:#fff;
                           border:none; border-radius:4px; cursor:pointer; font-size:13px; opacity:0.4;">
                    ⚡ Optimize
                </button>
```

Note: `var(--accent-color)` may not exist in the theme. If not, use `#805ad5` (purple) as fallback: `background:#805ad5`.

- [ ] **Step 3: Extract `buildSegmentPopup` from `drawRouteLine`**

In `static/plan.html`, find `drawRouteLine` (around line 852). The inline popup building currently looks like:

```js
                seg.bindPopup(
                    `<b>${ts.toUTCString()}</b><br>` +
                    `Wind: ${(p.wind_speed_kn || 0).toFixed(1)} kn ` +
                    `${(p.wind_direction_deg || 0).toFixed(0)}°<br>` +
                    `Gust: ${(p.wind_gust_kn || 0).toFixed(1)} kn<br>` +
                    `Wave: ${(p.wave_height_m || 0).toFixed(1)} m<br>` +
                    `Est. speed: ${pNext.speed_kn != null ? pNext.speed_kn.toFixed(1) + ' kn' : '—'}${modeStr}`
                );
```

Add this new helper function **before** `drawRouteLine`:

```js
        function buildSegmentPopup(p) {
            const ts   = p.timestamp ? new Date(p.timestamp).toUTCString().slice(0, 22) : '—';
            const spd  = p.wind_speed_kn   != null ? p.wind_speed_kn.toFixed(1)   + ' kn'  : '—';
            const dir  = p.wind_direction_deg != null ? p.wind_direction_deg.toFixed(0) + '°' : '—';
            const gust = p.wind_gust_kn    != null ? p.wind_gust_kn.toFixed(1)    + ' kn'  : '—';
            const wh   = p.wave_height_m   != null ? p.wave_height_m.toFixed(1)   + ' m'   : '—';
            const wp   = p.wave_period_s   != null ? p.wave_period_s.toFixed(0)   + ' s'   : '—';
            const spd2 = p.speed_kn        != null ? p.speed_kn.toFixed(1)        + ' kn'  : '—';
            const twa  = p.twa_deg         != null ? p.twa_deg.toFixed(0)         + '°'    : '—';
            return `<b>${ts}</b><br>Wind: ${spd} · ${dir} (gust ${gust})<br>`
                 + `Wave: ${wh} / ${wp}<br>Boat speed: ${spd2} · TWA: ${twa}`;
        }
```

Then replace the inline popup in `drawRouteLine` with:
```js
                seg.bindPopup(buildSegmentPopup(p));
```
(Note: `buildSegmentPopup` takes `p` — the current point that has wind data. The mode string will appear in TWA field.)

- [ ] **Step 4: Update `updateRouteBar()` to enable/disable the Optimize button**

In `updateRouteBar()` (around line 692), after the existing `computeBtn` enable/disable block:
```js
            const computeBtn = document.getElementById('computeBtn');
            computeBtn.disabled = !ready;
            computeBtn.style.opacity = ready ? '1' : '0.4';
```

Add:
```js
            const optimizeBtn = document.getElementById('optimizeBtn');
            if (optimizeBtn) {
                const canOptimize = routeDone && routeWaypoints.length >= 2 && !!dep && speed > 0;
                optimizeBtn.disabled = !canOptimize;
                optimizeBtn.style.opacity = canOptimize ? '1' : '0.4';
            }
```

- [ ] **Step 5: Add `optimizeRoute()` function**

Add after `computeRoute()` (around line 850, after the closing `}` of `computeRoute`):

```js
        async function optimizeRoute() {
            const btn = document.getElementById('optimizeBtn');
            const orig = btn.textContent;
            btn.disabled = true;
            btn.textContent = '⚡ Optimizing…';

            const from = routeWaypoints[0];
            const to   = routeWaypoints[routeWaypoints.length - 1];
            const dep  = document.getElementById('depInput').value;
            const speed = parseFloat(document.getElementById('speedInput').value);
            const departure = dep.slice(0, 16) + ':00Z';

            try {
                const url = `/api/forecast/optimal-route?trip_id=${tripId}`
                    + `&from_lat=${from.lat.toFixed(6)}&from_lon=${from.lng.toFixed(6)}`
                    + `&to_lat=${to.lat.toFixed(6)}&to_lon=${to.lng.toFixed(6)}`
                    + `&departure=${encodeURIComponent(departure)}`
                    + `&motoring_speed_kn=${speed}`;

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

- [ ] **Step 6: Add `drawOptimizedRoute()` function**

Add after `optimizeRoute()`:

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
                    { color, weight: 4, opacity: 0.9 }
                ).addTo(planMap);
                seg.bindPopup(buildSegmentPopup(curr));
                optimizedSegments.push(seg);
            }

            const last = points[points.length - 1];
            const eta = new Date(last.timestamp);
            const etaStr = eta.toUTCString().slice(0, 22);
            const marker = L.marker([last.lat, last.lon], {
                icon: L.divIcon({
                    className: '',
                    html: `<div style="background:var(--bg-primary);border:1px solid var(--border-color);
                                       border-radius:3px;padding:2px 6px;font-size:11px;white-space:nowrap;">
                               ⚡ ETA ${etaStr} · ${(last.wind_speed_kn ?? 0).toFixed(0)} kn
                           </div>`,
                    iconAnchor: [0, 0]
                })
            }).addTo(planMap);
            optimizedSegments.push(marker);
        }
```

- [ ] **Step 7: Update `clearRoute()` to remove optimized route**

Find `clearRoute()` (around line 753). It ends with:
```js
            document.getElementById('planRouteBtn').textContent = 'Plan Route';
            document.getElementById('planRouteBtn').style.background = '';
            planMap.getContainer().style.cursor = '';
            updateRouteBar();
        }
```

Add before `updateRouteBar();`:
```js
            optimizedSegments.forEach(s => planMap.removeLayer(s));
            optimizedSegments = [];
```

- [ ] **Step 8: Verify the build still compiles**

```bash
cargo build 2>&1 | grep -E "^error" | head -10
```
Expected: no errors.

---

## Self-review against spec

**Spec coverage check:**

| Spec requirement | Task/Step |
|---|---|
| `src/routing.rs` with `IsochronePoint`, `IsochroneResult`, `run_isochrone`, `prune_isochrone`, `backtrack` | Task 1 Step 8 |
| `HEADING_STEP_DEG=5`, `SECTOR_COUNT=72`, `MAX_STEPS=168`, `ARRIVAL_THRESHOLD_NM=5` constants | Task 1 Step 3 (constants in file header) |
| prune keeps farthest-from-origin per sector | Task 1 Step 6 |
| backtrack appends destination as final point | Task 1 Step 7 |
| best-effort fallback when destination not reached | Task 1 Step 8 |
| `OptimalRouteQuery` struct | Task 2 Step 1 |
| `get_optimal_route` handler returning error when no polars | Task 2 Step 2 |
| Route registered in `!read_only` block | Task 2 Step 3 |
| `optimizedSegments` state variable | Task 3 Step 1 |
| "Optimize Route" button in route bar | Task 3 Step 2 |
| `buildSegmentPopup()` helper | Task 3 Step 3 |
| `updateRouteBar()` enables/disables Optimize button | Task 3 Step 4 |
| `optimizeRoute()` function | Task 3 Step 5 |
| `drawOptimizedRoute()` — solid lines, wind-color scale, ETA marker | Task 3 Step 6 |
| `clearRoute()` removes optimized route | Task 3 Step 7 |
| 3 tests: prune≤72, reaches destination, monotonic timestamps | Task 1 Steps 3–9 |

**Spec says `compute_route_overlay(&result.track, &forecast_inputs)` where `forecast_inputs` is `TripForecastInputs`.** The plan instead passes `&fetches` (`Vec<FetchWithHourly>`) directly, consistent with the existing `get_forecast_route` handler pattern. This is intentional — `fetch_trip_forecast_inputs` returns `Option<TripForecastInputs>` requiring extra unwrapping that `fetch_forecast_fetches` avoids. Functionally identical.

**Spec note on `buildSegmentPopup`:** The spec popup includes `Boat speed` and `TWA` from the `speed_kn` and `twa_deg` fields. The optimized route `IsochroneResult` track is converted to `RouteTrackPoint` with `speed_kn: None` and `twa_deg: None`, so these will display as `—`. This is correct — the isochrone doesn't compute per-point speed data.

**Type consistency:** `IsochroneResult.track` is `Vec<(f64, f64, DateTime<Utc>)>` throughout. The handler converts to `Vec<RouteTrackPoint>` before calling `compute_route_overlay`. Types are consistent end-to-end.
