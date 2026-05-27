# Polyline Route Planning Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the two-point (FROM → TO) route planner with a multi-waypoint polyline: click to add waypoints, "Done" to finish, "Compute" to fetch wind forecast along each leg.

**Architecture:** Backend: extract `generate_leg` from `generate_route_track`, add new `generate_route_track(&[(f64,f64)])` that chains legs, add `parse_waypoints` helper, update `ForecastRouteQuery` to a single `waypoints` string. Frontend: replace `routeFrom/routeTo` state with `routeWaypoints[]`, new route bar with live nm counter, Undo/Done buttons.

**Tech Stack:** Rust (Axum, Chrono), Vanilla JS, Leaflet 1.9.4

---

## File Map

| File | Changes |
|---|---|
| `src/forecast.rs` | Add `parse_waypoints`; rename `generate_route_track` body to `generate_leg` (private); new `generate_route_track(&[(f64,f64)], …)`; update + add unit tests |
| `src/web/api.rs` | Replace four lat/lon fields in `ForecastRouteQuery` with `waypoints: String`; update handler to call `parse_waypoints` then `generate_route_track` |
| `static/plan.html` | Replace route bar HTML; replace 2-point state with N-waypoint arrays; add `totalRouteNm`, `updateRouteBar`, `undoWaypoint`, `doneDrawing`; update `onMapClick`, `clearRoute`, `computeRoute`, `drawRouteLine` |

---

### Task 1: Backend — multi-waypoint `generate_route_track`

**Files:**
- Modify: `src/forecast.rs:315–341` (function body), `src/forecast.rs:540–598` (tests)

- [ ] **Step 1: Write the failing test**

At the bottom of the `#[cfg(test)]` block in `src/forecast.rs` (after line 598, before the closing `}`), add:

```rust
#[test]
fn test_generate_route_track_two_legs() {
    use chrono::TimeZone;
    let dep = Utc.with_ymd_and_hms(2026, 5, 14, 6, 0, 0).unwrap();
    let wpts = vec![(43.55_f64, 10.29_f64), (43.05, 9.84), (42.70, 9.45)];
    let track = generate_route_track(&wpts, dep, 5.0);
    // First point at first waypoint
    assert!((track[0].0 - 43.55).abs() < 0.01, "first lat wrong");
    assert!((track[0].1 - 10.29).abs() < 0.01, "first lon wrong");
    // Last point at last waypoint
    let last = track.last().unwrap();
    assert!((last.0 - 42.70).abs() < 0.01, "last lat wrong: {}", last.0);
    assert!((last.1 - 9.45).abs() < 0.01, "last lon wrong: {}", last.1);
    // Timestamps strictly increasing (no duplicates at leg boundary)
    for i in 1..track.len() {
        assert!(track[i].2 > track[i - 1].2,
            "Timestamps not strictly increasing at index {}", i);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test test_generate_route_track_two_legs 2>&1 | tail -20
```

Expected: compile error — `generate_route_track` has wrong signature (`f64, f64, f64, f64, …` not `&[(f64,f64)]`).

- [ ] **Step 3: Refactor `generate_route_track` in `src/forecast.rs`**

Replace the existing `generate_route_track` function (lines 315–341) with:

```rust
/// Single-leg internal helper — same logic as the old `generate_route_track`.
fn generate_leg(
    from_lat: f64,
    from_lon: f64,
    to_lat: f64,
    to_lon: f64,
    departure: DateTime<Utc>,
    speed_kn: f64,
) -> Vec<(f64, f64, DateTime<Utc>)> {
    let distance_nm = haversine_distance_nm(from_lat, from_lon, to_lat, to_lon);
    if distance_nm < 0.1 || speed_kn <= 0.0 {
        return vec![(from_lat, from_lon, departure)];
    }
    let total_hours = distance_nm / speed_kn;
    let num_steps = total_hours.ceil() as i64;
    let mut track: Vec<(f64, f64, DateTime<Utc>)> = (0..num_steps)
        .map(|h| {
            let frac = (h as f64 / total_hours).min(1.0);
            let lat = from_lat + frac * (to_lat - from_lat);
            let lon = from_lon + frac * (to_lon - from_lon);
            let ts = departure + Duration::hours(h);
            (lat, lon, ts)
        })
        .collect();
    let arrival_secs = (total_hours * 3600.0).round() as i64;
    track.push((to_lat, to_lon, departure + Duration::seconds(arrival_secs)));
    track
}

/// Chains `generate_leg` across consecutive waypoint pairs.
/// The departure time for each leg equals the arrival time of the previous leg.
/// The junction point between legs appears only once (no duplicates).
pub fn generate_route_track(
    waypoints: &[(f64, f64)],
    departure: DateTime<Utc>,
    speed_kn: f64,
) -> Vec<(f64, f64, DateTime<Utc>)> {
    let mut track: Vec<(f64, f64, DateTime<Utc>)> = Vec::new();
    let mut leg_start = departure;
    for w in waypoints.windows(2) {
        let (from_lat, from_lon) = w[0];
        let (to_lat, to_lon)     = w[1];
        let leg = generate_leg(from_lat, from_lon, to_lat, to_lon, leg_start, speed_kn);
        if leg.is_empty() { continue; }
        leg_start = leg.last().unwrap().2;
        // Skip first point of subsequent legs — it duplicates the previous leg's last point
        let skip = if track.is_empty() { 0 } else { 1 };
        track.extend_from_slice(&leg[skip..]);
    }
    track
}
```

- [ ] **Step 4: Update the three existing tests that use the old 6-argument signature**

In `src/forecast.rs`, find `test_generate_route_track_point_count` (line 541) and replace:

```rust
#[test]
fn test_generate_route_track_point_count() {
    use chrono::TimeZone;
    let dep = Utc.with_ymd_and_hms(2026, 5, 14, 6, 0, 0).unwrap();
    // Livorno → Capraia ≈ 35.9 nm at 5 kn → 7.18 h → ceil=8 → 8 hourly + 1 destination = 9 points
    let wpts = vec![(43.55_f64, 10.29_f64), (43.05, 9.84)];
    let track = generate_route_track(&wpts, dep, 5.0);
    assert_eq!(track.len(), 9, "Expected 9 points, got {}", track.len());
    assert!((track[0].0 - 43.55).abs() < 0.01);
    assert!((track[0].2 - dep).num_seconds() == 0);
    let last = track.last().unwrap();
    assert!((last.0 - 43.05).abs() < 0.001, "Expected 43.05, got {}", last.0);
    assert!((last.1 - 9.84).abs() < 0.001,  "Expected 9.84, got {}",  last.1);
}
```

Find `test_generate_route_track_timestamps_advance_hourly` (line 557) and replace:

```rust
#[test]
fn test_generate_route_track_timestamps_advance_hourly() {
    use chrono::TimeZone;
    let dep = Utc.with_ymd_and_hms(2026, 5, 14, 6, 0, 0).unwrap();
    let wpts = vec![(43.55_f64, 10.29_f64), (43.05, 9.84)];
    let track = generate_route_track(&wpts, dep, 5.0);
    // All but the last step are exactly 1 hour apart; last step may be a partial hour
    for i in 1..track.len() - 1 {
        let diff = (track[i].2 - track[i - 1].2).num_hours();
        assert_eq!(diff, 1, "Expected 1-hour steps at index {}", i);
    }
}
```

Find `test_compute_route_overlay_returns_points_with_coords` (line 569) and replace the `generate_route_track` call:

```rust
let wpts = vec![(43.5_f64, 9.0_f64), (43.5, 9.5)];
let track = generate_route_track(&wpts, dep, 10.0);
```

- [ ] **Step 5: Run all forecast tests**

```bash
cargo test -- src/forecast 2>&1 | tail -30
```

Expected: all pass, no compile errors.

---

### Task 2: Backend — `parse_waypoints` + updated API handler

**Files:**
- Modify: `src/forecast.rs` (add `parse_waypoints` + its tests)
- Modify: `src/web/api.rs:253–261` (`ForecastRouteQuery`), `src/web/api.rs:1585–1613` (handler)

- [ ] **Step 1: Write failing tests for `parse_waypoints`**

Add to the `#[cfg(test)]` block in `src/forecast.rs`:

```rust
#[test]
fn test_parse_waypoints_valid() {
    let wpts = parse_waypoints("43.55,10.29;43.05,9.84;42.70,9.45").unwrap();
    assert_eq!(wpts.len(), 3);
    assert!((wpts[0].0 - 43.55).abs() < 1e-9);
    assert!((wpts[0].1 - 10.29).abs() < 1e-9);
    assert!((wpts[2].0 - 42.70).abs() < 1e-9);
    assert!((wpts[2].1 - 9.45).abs() < 1e-9);
}

#[test]
fn test_parse_waypoints_too_few() {
    assert!(parse_waypoints("43.55,10.29").is_err());
    assert!(parse_waypoints("").is_err());
}

#[test]
fn test_parse_waypoints_invalid_format() {
    assert!(parse_waypoints("43.55;10.29;bad").is_err());   // missing comma in pair
    assert!(parse_waypoints("43.55,abc;10.29,9.0").is_err()); // non-numeric
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test test_parse_waypoints 2>&1 | tail -10
```

Expected: compile error — `parse_waypoints` not found.

- [ ] **Step 3: Add `parse_waypoints` to `src/forecast.rs`**

Add this function before `generate_leg` (around line 315):

```rust
/// Parses "lat1,lon1;lat2,lon2;…" into a Vec of (lat, lon) pairs.
/// Returns Err if fewer than 2 pairs or any pair is malformed.
pub fn parse_waypoints(s: &str) -> Result<Vec<(f64, f64)>, String> {
    let pairs: Vec<(f64, f64)> = s
        .split(';')
        .filter(|p| !p.trim().is_empty())
        .map(|pair| {
            let mut it = pair.splitn(2, ',');
            let lat = it.next().and_then(|v| v.trim().parse::<f64>().ok());
            let lon = it.next().and_then(|v| v.trim().parse::<f64>().ok());
            lat.zip(lon).ok_or_else(|| format!("invalid waypoint pair: '{}'", pair))
        })
        .collect::<Result<_, _>>()?;
    if pairs.len() < 2 {
        return Err(format!("at least 2 waypoints required, got {}", pairs.len()));
    }
    Ok(pairs)
}
```

- [ ] **Step 4: Run `parse_waypoints` tests**

```bash
cargo test test_parse_waypoints 2>&1 | tail -10
```

Expected: all 3 pass.

- [ ] **Step 5: Update `ForecastRouteQuery` in `src/web/api.rs`**

Replace lines 252–261:

```rust
#[derive(Debug, Deserialize)]
pub struct ForecastRouteQuery {
    pub trip_id: u32,
    pub waypoints: String,   // "lat1,lon1;lat2,lon2;…" — at least 2 pairs
    pub departure: String,
    pub speed_kn: f64,
}
```

- [ ] **Step 6: Update `get_forecast_route` handler in `src/web/api.rs`**

Replace the body of `get_forecast_route` (lines 1585–1613) with:

```rust
pub async fn get_forecast_route(
    State(state): State<AppState>,
    Query(params): Query<ForecastRouteQuery>,
) -> Result<Json<ApiResponse<Vec<crate::forecast::RouteOverlayPoint>>>, StatusCode> {
    let departure = match chrono::DateTime::parse_from_rfc3339(&params.departure) {
        Ok(dt) => dt.with_timezone(&chrono::Utc),
        Err(_) => {
            return Ok(Json(ApiResponse::error(format!(
                "Invalid departure timestamp: {}",
                params.departure
            ))));
        }
    };
    let waypoints = match crate::forecast::parse_waypoints(&params.waypoints) {
        Ok(w) => w,
        Err(e) => return Ok(Json(ApiResponse::error(e))),
    };
    let fetches = match state.db().fetch_forecast_fetches(params.trip_id) {
        Ok(f) => f,
        Err(e) => {
            error!(error = %e, trip_id = params.trip_id, "Failed to load forecast fetches for route");
            return Ok(Json(ApiResponse::error(e.to_string())));
        }
    };
    let track = crate::forecast::generate_route_track(&waypoints, departure, params.speed_kn);
    let overlay = crate::forecast::compute_route_overlay(&track, &fetches);
    Ok(Json(ApiResponse::ok(overlay)))
}
```

- [ ] **Step 7: Verify the build compiles cleanly**

```bash
cargo build 2>&1 | grep -E "^error" | head -20
```

Expected: no output (zero errors).

- [ ] **Step 8: Run all non-DB tests**

```bash
cargo test 2>&1 | tail -20
```

Expected: all pass.

---

### Task 3: Frontend — route bar HTML

**Files:**
- Modify: `static/plan.html:55–83` (route bar `<div>`)

- [ ] **Step 1: Replace the route bar inner HTML**

Find the route bar div (starting at `<!-- Route bar (hidden until route mode) -->`). Replace the entire inner `<div style="display:flex; …">` block with:

```html
        <!-- Route bar (hidden until route mode) -->
        <div id="routeBar" class="level-1-container"
             style="display:none; padding:10px 20px; margin-bottom:10px;">
            <div style="display:flex; align-items:center; gap:14px; flex-wrap:wrap; font-size:13px;">
                <span id="routeWptCount"
                    style="color:var(--text-primary); font-weight:600; min-width:200px;">
                    click map to add waypoints
                </span>
                <span style="color:var(--border-color); margin:0 4px;">|</span>
                <label style="color:var(--text-secondary);">Dep (UTC):
                    <input type="datetime-local" id="depInput"
                        style="margin-left:6px; background:var(--bg-secondary);
                               border:1px solid var(--border-color); border-radius:4px;
                               padding:3px 8px; color:var(--text-primary); font-size:13px;">
                </label>
                <label style="color:var(--text-secondary);">Speed:
                    <input type="number" id="speedInput" value="5.5" min="1" max="20" step="0.5"
                        style="width:64px; margin-left:6px; background:var(--bg-secondary);
                               border:1px solid var(--border-color); border-radius:4px;
                               padding:3px 8px; color:var(--text-primary); font-size:13px;"> kn
                </label>
                <button id="undoBtn" onclick="undoWaypoint()" disabled
                    style="padding:5px 12px; background:var(--bg-secondary); color:var(--text-secondary);
                           border:1px solid var(--border-color); border-radius:4px;
                           cursor:pointer; font-size:13px;">
                    ↩ Undo
                </button>
                <button id="doneBtn" onclick="doneDrawing()" disabled
                    style="padding:5px 12px; background:#48bb78; color:#fff; border:none;
                           border-radius:4px; cursor:pointer; font-size:13px;">
                    ✓ Done
                </button>
                <button id="computeBtn" onclick="computeRoute()" disabled
                    style="padding:5px 14px; background:var(--link-color); color:#fff; border:none;
                           border-radius:4px; cursor:pointer; font-size:13px; opacity:0.4;">
                    Compute
                </button>
            </div>
        </div>
```

- [ ] **Step 2: Verify HTML is well-formed**

Open `static/plan.html` in a browser (or run `cargo build` to confirm the file is found). Check the dev console shows no parse errors.

---

### Task 4: Frontend — state variables and helper functions

**Files:**
- Modify: `static/plan.html` (JS section)

- [ ] **Step 1: Replace the route state variables**

Find the state block (around line 118):

```js
let routeMode = false;
let routeFrom = null, routeTo = null;
let fromMarker = null, toMarker = null;
let routeSegments = [];
```

Replace with:

```js
let routeMode = false;
let routeDone = false;
let routeWaypoints = [];    // L.LatLng[] in order
let waypointMarkers = [];   // L.CircleMarker[] one per waypoint
let previewLines = [];      // L.Polyline[] dashed segments between consecutive waypoints
let routeSegments = [];     // coloured segments from drawRouteLine
```

- [ ] **Step 2: Add `totalRouteNm` and `updateRouteBar`**

Add these two functions immediately before the existing `// ── Route mode ──` comment block:

```js
function totalRouteNm() {
    let d = 0;
    for (let i = 1; i < routeWaypoints.length; i++)
        d += degDistNm(routeWaypoints[i - 1].lat, routeWaypoints[i - 1].lng,
                       routeWaypoints[i].lat,     routeWaypoints[i].lng);
    return d;
}

function updateRouteBar() {
    const n  = routeWaypoints.length;
    const nm = totalRouteNm();
    document.getElementById('routeWptCount').textContent = n
        ? `${n} waypoint${n !== 1 ? 's' : ''} · ${nm.toFixed(1)} nm`
        : 'click map to add waypoints';

    const undoBtn    = document.getElementById('undoBtn');
    const doneBtn    = document.getElementById('doneBtn');
    undoBtn.disabled = (n === 0 || routeDone);
    doneBtn.disabled = (n < 2  || routeDone);
    undoBtn.style.display = routeDone ? 'none' : '';
    doneBtn.style.display = routeDone ? 'none' : '';

    const dep   = document.getElementById('depInput').value;
    const speed = parseFloat(document.getElementById('speedInput').value);
    const ready = routeDone && n >= 2 && !!dep && speed > 0;
    const computeBtn = document.getElementById('computeBtn');
    computeBtn.disabled = !ready;
    computeBtn.style.opacity = ready ? '1' : '0.4';
}
```

- [ ] **Step 3: Add `undoWaypoint` and `doneDrawing`**

Add immediately after `updateRouteBar`:

```js
function undoWaypoint() {
    if (!routeWaypoints.length) return;
    planMap.removeLayer(waypointMarkers.pop());
    if (previewLines.length) planMap.removeLayer(previewLines.pop());
    routeWaypoints.pop();
    const newN = routeWaypoints.length;
    if (newN >= 1) {
        const lastIdx = newN - 1;
        // First waypoint stays green; any other new-last becomes red
        const fillColor = (lastIdx === 0) ? '#48bb78' : '#fc8181';
        waypointMarkers[lastIdx].setStyle({ fillColor, color: '#fff' });
    }
    updateRouteBar();
}

function doneDrawing() {
    if (routeWaypoints.length < 2) return;
    routeDone = true;
    planMap.getContainer().style.cursor = '';
    updateRouteBar();
}
```

- [ ] **Step 4: Update `depInput` and `speedInput` event listeners**

Find:

```js
document.getElementById('depInput').addEventListener('change', checkComputeReady);
document.getElementById('speedInput').addEventListener('input', function () {
    localStorage.setItem('plan_speed_kn', this.value);
    checkComputeReady();
});
```

Replace with:

```js
document.getElementById('depInput').addEventListener('change', updateRouteBar);
document.getElementById('speedInput').addEventListener('input', function () {
    localStorage.setItem('plan_speed_kn', this.value);
    updateRouteBar();
});
```

---

### Task 5: Frontend — update `onMapClick`, `clearRoute`, `computeRoute`, `drawRouteLine`

**Files:**
- Modify: `static/plan.html` (JS section)

- [ ] **Step 1: Replace `onMapClick`**

Find the existing `function onMapClick(e)` and replace it entirely:

```js
function onMapClick(e) {
    if (!routeMode || routeDone) return;
    const n = routeWaypoints.length;

    if (n > 0) {
        const prev = routeWaypoints[n - 1];
        previewLines.push(
            L.polyline([[prev.lat, prev.lng], [e.latlng.lat, e.latlng.lng]], {
                color: '#888', weight: 2, dashArray: '6,6', opacity: 0.7
            }).addTo(planMap)
        );
        // Previous last marker → gray (first marker always stays green)
        if (n > 1) waypointMarkers[n - 1].setStyle({ fillColor: '#888', color: '#888' });
    }

    routeWaypoints.push(e.latlng);
    const fillColor = (n === 0) ? '#48bb78' : '#fc8181';
    waypointMarkers.push(
        L.circleMarker(e.latlng, {
            color: '#fff', fillColor, fillOpacity: 1, radius: 7, weight: 2
        }).addTo(planMap)
    );
    updateRouteBar();
}
```

- [ ] **Step 2: Replace `clearRoute`**

Find `function clearRoute()` and replace it entirely:

```js
function clearRoute() {
    routeMode = false;
    routeDone = false;
    waypointMarkers.forEach(m => planMap.removeLayer(m));
    waypointMarkers = [];
    previewLines.forEach(l => planMap.removeLayer(l));
    previewLines = [];
    routeWaypoints = [];
    routeSegments.forEach(l => planMap.removeLayer(l));
    routeSegments = [];
    document.getElementById('routeBar').style.display = 'none';
    document.getElementById('routeWptCount').textContent = 'click map to add waypoints';
    document.getElementById('planRouteBtn').textContent = 'Plan Route';
    document.getElementById('planRouteBtn').style.background = '';
    planMap.getContainer().style.cursor = '';
}
```

- [ ] **Step 3: Replace `computeRoute`**

Find `async function computeRoute()` and replace it entirely:

```js
async function computeRoute() {
    const dep   = document.getElementById('depInput').value;
    const speed = parseFloat(document.getElementById('speedInput').value);
    if (!routeDone || routeWaypoints.length < 2 || !dep || !(speed > 0)) return;

    const departure      = dep.slice(0, 16) + ':00Z';
    const waypointsParam = routeWaypoints
        .map(p => `${p.lat.toFixed(6)},${p.lng.toFixed(6)}`)
        .join(';');
    const url = `/api/forecast/route?trip_id=${tripId}` +
        `&waypoints=${encodeURIComponent(waypointsParam)}` +
        `&departure=${encodeURIComponent(departure)}&speed_kn=${speed}`;

    const btn = document.getElementById('computeBtn');
    btn.textContent = 'Computing…';
    btn.disabled = true;
    try {
        const resp = await fetch(url);
        const json = await resp.json();
        if (json.status === 'error') {
            console.error('Route API error:', json.message);
            btn.textContent = 'Error — retry';
            return;
        }
        drawRouteLine(json.data || []);
    } catch (err) {
        console.error('Route forecast failed', err);
    } finally {
        btn.textContent = 'Compute';
        updateRouteBar();
    }
}
```

- [ ] **Step 4: Update `drawRouteLine` to also clear `previewLines`**

Find the top of `function drawRouteLine(pts)`:

```js
function drawRouteLine(pts) {
    routeSegments.forEach(l => planMap.removeLayer(l));
    routeSegments = [];
```

Replace those two lines with:

```js
function drawRouteLine(pts) {
    routeSegments.forEach(l => planMap.removeLayer(l));
    routeSegments = [];
    previewLines.forEach(l => planMap.removeLayer(l));
    previewLines = [];
```

- [ ] **Step 5: Remove the now-dead `checkComputeReady` function**

Find and delete the entire `function checkComputeReady() { … }` block (it is fully replaced by `updateRouteBar`).

- [ ] **Step 6: Manual smoke test**

Start the server (`cargo run` or use the running instance). Open `plan.html?id=<any-trip-with-forecast>`.

Verify:
1. Click "Plan Route" → route bar appears with "click map to add waypoints", Undo/Done disabled.
2. Click 3 map points → bar shows "3 waypoints · X.X nm", green/gray/red markers, dashed preview lines.
3. Click "↩ Undo" → last point removed, bar updates, previous marker turns red.
4. Click "✓ Done" → Undo/Done buttons hidden, Compute enabled (if departure + speed filled).
5. Click "Compute" → colored wind segments appear, ETA tooltip at last waypoint, preview lines gone.
6. Click "Plan Route" again (now "Clear Route") → everything cleared, back to initial state.