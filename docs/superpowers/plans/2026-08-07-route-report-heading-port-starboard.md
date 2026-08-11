# Route Report: Heading + Port/Starboard Wind Qualifier Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a "Hdg" (heading) column and a port/starboard qualifier on the wind-angle column to the main "Step-by-step" route report table on the planning page.

**Architecture:** A new `heading_deg: Option<f64>` field is threaded through the existing route-point pipeline (`RouteTrackPoint` → `RouteOverlayPoint`) in `src/forecast.rs`, populated from the `bearing` value both `generate_route_track` (forecast.rs) and `get_optimal_route`'s manual per-step loop (`src/web/api.rs`) already compute locally but currently discard. The frontend (`static/plan.html`) renders it as a new table column and derives the port/starboard side client-side by comparing `wind_direction_deg` against the new `heading_deg` — no new backend "side" field needed, since `compute_twa` already exists purely for the unsigned magnitude.

**Tech Stack:** Rust (axum, serde), vanilla JavaScript, no new dependencies.

## Global Constraints

- Backend: Rust only. Frontend: HTML + vanilla JavaScript. (CLAUDE.md)
- Angles are decimal degrees, 0–360, unless a function's own doc comment says otherwise (`compute_twa` returns 0–180 by design — unchanged here). (CLAUDE.md)
- Distance/bearing: Haversine formula only — reuse the existing `haversine_heading`, do not add a second implementation. (CLAUDE.md)
- Never call `now()` inside business logic. (Not applicable to this change — no new timestamps.)
- No unused imports, no partial implementations committed to main.
- Do not run `git commit` or `git push` — the user reviews and commits manually (this project's CLAUDE.md). Every "Commit" step below is written for a context where that restriction does *not* apply (e.g. a worktree the user explicitly asked to commit in); when running in *this* repo's main flow, treat each "Commit" step as "stop and let the user review" instead of actually committing.

---

## File Structure

- Modify `src/forecast.rs`: add `heading_deg` to `RouteTrackPoint` and `RouteOverlayPoint`; populate it in `generate_route_track`; pass it through in `compute_route_overlay`; extend the existing test module.
- Modify `src/web/api.rs`: populate `heading_deg` in `get_optimal_route`'s manual per-step `RouteTrackPoint` construction.
- Modify `static/plan.html`: add the "Hdg" column to the report table header and to `renderRouteReportTable`; append the port/starboard suffix to the TWD cell.

No new files. This is a small, single-subsystem change — one plan, no decomposition needed.

---

## Task 1: Add `heading_deg` to the backend route-point pipeline

**Files:**
- Modify: `src/forecast.rs` (struct defs ~lines 19-45, `generate_route_track` ~lines 464-522, `compute_route_overlay` ~lines 548-563, test module ~line 688 onward)
- Modify: `src/web/api.rs` (`get_optimal_route`'s per-step loop, ~lines 1907-1961)
- Test: `src/forecast.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `crate::utilities::haversine_heading(lat1_deg: f64, lon1_deg: f64, lat2_deg: f64, lon2_deg: f64) -> f64` (already used by both call sites for `bearing`).
- Produces: `RouteTrackPoint.heading_deg: Option<f64>` and `RouteOverlayPoint.heading_deg: Option<f64>`, consumed by Task 2 (frontend) via the JSON field `heading_deg` on each point returned by `/api/optimal-route`'s `route` array.

- [ ] **Step 1: Write the failing test in `src/forecast.rs`**

Add this test in the `#[cfg(test)] mod tests` block, right after `test_generate_route_track_relative_wind_deg_matches_sail_decision` (ends at line 1107):

```rust
#[test]
fn test_generate_route_track_heading_deg_matches_haversine_bearing() {
    use chrono::TimeZone;
    use crate::db::operations::forecast::{FetchWithHourly, ForecastHourlyPoint};

    let dep = Utc.with_ymd_and_hms(2026, 5, 14, 6, 0, 0).unwrap();
    let hourly = vec![ForecastHourlyPoint {
        timestamp: dep.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        wind_speed_kn: Some(12.0),
        wind_direction_deg: Some(90.0),
        wind_gust_kn: None, wave_height_m: None, wave_period_s: None,
        wave_direction_deg: None, cape_j_kg: None,
    }];
    let fetches = vec![FetchWithHourly { lat: 43.0, lon: 8.0, model: "ecmwf".to_string(), hourly }];

    // No polars → always motors, but heading_deg must still be recorded regardless of
    // the sail/motor decision (it's a property of the leg's course, not of sailing).
    let wpts = vec![(43.0_f64, 8.0_f64), (43.12_f64, 8.0_f64)];
    let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, 60.0, None, &fetches);

    assert!(track.len() >= 2);
    assert_eq!(track[0].heading_deg, None, "departure point has no incoming leg yet");

    let expected_bearing = crate::utilities::haversine_heading(43.0, 8.0, 43.12, 8.0);
    let heading = track[1].heading_deg.expect("heading_deg should be set once the boat has moved");
    assert!(
        (heading - expected_bearing).abs() < 0.01,
        "expected heading_deg to equal haversine_heading's bearing ({}), got {}",
        expected_bearing, heading
    );
}

#[test]
fn test_compute_route_overlay_passes_through_heading_deg() {
    use chrono::TimeZone;
    use crate::db::operations::forecast::{FetchWithHourly, ForecastHourlyPoint};

    let dep = Utc.with_ymd_and_hms(2026, 5, 14, 9, 0, 0).unwrap();
    let wpts = vec![(43.5_f64, 9.0_f64), (43.5, 9.6)];
    let ts = dep.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let hourly = vec![ForecastHourlyPoint {
        timestamp: ts,
        wind_speed_kn: Some(12.0),
        wind_direction_deg: Some(180.0),
        wind_gust_kn: None, wave_height_m: None, wave_period_s: None,
        wave_direction_deg: None, cape_j_kg: None,
    }];
    let fetches = vec![
        FetchWithHourly { lat: 43.5, lon: 9.0, model: "ecmwf".to_string(), hourly: hourly.clone() },
        FetchWithHourly { lat: 43.5, lon: 9.6, model: "ecmwf".to_string(), hourly },
    ];
    let track = generate_route_track(&wpts, dep, 60.0, 1.0, 0.0, 0.0, None, &fetches);
    assert_eq!(track.len(), 2, "leg should complete in a single fast step");

    let overlay = compute_route_overlay(&track, &fetches);
    assert!(overlay.len() >= 2);
    assert_eq!(overlay[0].heading_deg, None);
    assert_eq!(
        overlay[1].heading_deg, track[1].heading_deg,
        "compute_route_overlay must pass heading_deg through unchanged"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --bin nmea_router forecast::tests::test_generate_route_track_heading_deg_matches_haversine_bearing forecast::tests::test_compute_route_overlay_passes_through_heading_deg`
Expected: FAIL to compile — `heading_deg` is not a field on `RouteTrackPoint`/`RouteOverlayPoint`.

- [ ] **Step 3: Add the field to both structs**

In `src/forecast.rs`, `RouteOverlayPoint` (currently lines 19-35):

```rust
#[derive(Debug, serde::Serialize, Clone)]
pub struct RouteOverlayPoint {
    pub lat: f64,
    pub lon: f64,
    pub timestamp: String,
    pub wind_speed_kn: Option<f64>,
    pub wind_direction_deg: Option<f64>,
    pub wind_gust_kn: Option<f64>,
    pub wave_height_m: Option<f64>,
    pub wave_period_s: Option<f64>,
    pub wave_direction_deg: Option<f64>,
    pub cape_j_kg: Option<f64>,
    pub speed_kn: Option<f64>,
    pub twa_deg: Option<f64>,
    pub wind_model: Option<String>,
    pub relative_wind_deg: Option<f64>,
    pub heading_deg: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct RouteTrackPoint {
    pub lat: f64,
    pub lon: f64,
    pub time: DateTime<Utc>,
    pub speed_kn: Option<f64>,
    pub twa_deg: Option<f64>,
    pub relative_wind_deg: Option<f64>,
    pub heading_deg: Option<f64>,
}
```

- [ ] **Step 4: Populate `heading_deg` in `generate_route_track`**

Still in `src/forecast.rs`. The departure-point push (currently, ~line 465):

```rust
track.push(RouteTrackPoint { lat: pos.0, lon: pos.1, time: t, speed_kn: None, twa_deg: None, relative_wind_deg: None, heading_deg: None });
```

The per-step push at the end of the `loop` body (currently, ~line 512) — `bearing` is already in scope a few lines above (`let bearing = crate::utilities::haversine_heading(pos.0, pos.1, to_lat, to_lon);`):

```rust
track.push(RouteTrackPoint { lat: pos.0, lon: pos.1, time: t, speed_kn: Some(speed_kn), twa_deg: twa, relative_wind_deg, heading_deg: Some(bearing) });
```

- [ ] **Step 5: Pass `heading_deg` through `compute_route_overlay`**

In `src/forecast.rs`, inside the `Some(RouteOverlayPoint { ... })` construction (currently ~lines 548-563), add one line alongside the existing `relative_wind_deg: pt.relative_wind_deg,`:

```rust
relative_wind_deg: pt.relative_wind_deg,
heading_deg: pt.heading_deg,
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --bin nmea_router forecast::tests::test_generate_route_track_heading_deg_matches_haversine_bearing forecast::tests::test_compute_route_overlay_passes_through_heading_deg`
Expected: PASS.

- [ ] **Step 7: Fix the other `RouteTrackPoint` construction site in `src/web/api.rs`**

`get_optimal_route`'s manual per-step loop builds `RouteTrackPoint` values directly (not via `generate_route_track`) from `crate::routing::run_isochrone`'s output track. Two construction sites there need the new field. The departure branch (currently ~lines 1914-1922):

```rust
if i == 0 {
    route_points.push(crate::forecast::RouteTrackPoint {
        lat,
        lon,
        time,
        speed_kn: None,
        twa_deg: None,
        relative_wind_deg: None,
        heading_deg: None,
    });
    continue;
}
```

The per-step push at the end of the loop body (currently ~lines 1953-1960) — `bearing` is already computed a few lines above at (currently) line 1933 (`let bearing = crate::utilities::haversine_heading(prev_lat, prev_lon, lat, lon);`):

```rust
route_points.push(crate::forecast::RouteTrackPoint {
    lat,
    lon,
    time,
    speed_kn: Some(speed_kn),
    twa_deg,
    relative_wind_deg,
    heading_deg: Some(bearing),
});
```

- [ ] **Step 8: Build to confirm `src/web/api.rs` compiles with the new struct field**

Run: `cargo build`
Expected: builds cleanly (this confirms no other `RouteTrackPoint`/`RouteOverlayPoint` literal construction site was missed — the compiler will error with "missing field `heading_deg`" on any that were).

- [ ] **Step 9: Run the full forecast test suite**

Run: `cargo test --bin nmea_router forecast::`
Expected: all pass, including the two new tests and the pre-existing `relative_wind_deg` tests (unaffected by this change).

- [ ] **Step 10: Commit**

```bash
git add src/forecast.rs src/web/api.rs
git commit -m "Add heading_deg to route report point pipeline"
```

---

## Task 2: Show heading and port/starboard qualifier in the report table

**Files:**
- Modify: `static/plan.html` (table header ~line 320, `renderRouteReportTable` ~lines 1793-1810)

**Interfaces:**
- Consumes: each point object `p` passed into `renderRouteReportTable(pts)` now has `p.heading_deg: number | null` (from Task 1's `RouteOverlayPoint.heading_deg`, serialized as JSON `null`/number) in addition to the pre-existing `p.wind_direction_deg`, `p.relative_wind_deg`, `p.timestamp`, `p.speed_kn`, `p.twa_deg`, `p.wind_speed_kn`, `p.wind_gust_kn`.
- Produces: no new interface — this is a leaf rendering function with no downstream JS consumers.

- [ ] **Step 1: Add the "Hdg" column header**

In `static/plan.html`, the report table's `<thead>` (currently line 320):

```html
<tr><th>Time</th><th>Speed</th><th>Engine</th><th>TWS</th><th>TWD</th><th>Gust</th><th>AWA</th><th>AWS</th></tr>
```

becomes:

```html
<tr><th>Time</th><th>Hdg</th><th>Speed</th><th>Engine</th><th>TWS</th><th>TWD</th><th>Gust</th><th>AWA</th><th>AWS</th></tr>
```

- [ ] **Step 2: Add the `hdg` value and the port/starboard suffix in `renderRouteReportTable`**

Current function (lines 1793-1810):

```javascript
function renderRouteReportTable(pts) {
    const body = document.getElementById('routeReportTableBody');
    body.innerHTML = pts.map(p => {
        const t = new Date(p.timestamp);
        const time = t.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', timeZone: 'UTC' })
            + ' UTC ' + t.toLocaleDateString('en-GB', { day: '2-digit', month: 'short', timeZone: 'UTC' });
        const spd = p.speed_kn != null ? p.speed_kn.toFixed(1) + ' kn' : '—';
        const eng = p.speed_kn == null ? '—' : (p.twa_deg === null ? '⚙ Motoring' : '⛵ Sailing');
        const tws = p.wind_speed_kn != null ? p.wind_speed_kn.toFixed(1) + ' kn' : '—';
        const twd = p.relative_wind_deg != null ? p.relative_wind_deg.toFixed(0) + '°' : '—';
        const gust = p.wind_gust_kn != null ? p.wind_gust_kn.toFixed(1) + ' kn' : '—';
        const apparent = trueToApparentWind(p.wind_speed_kn, p.relative_wind_deg, p.speed_kn);
        const awa = apparent ? apparent.awaDeg.toFixed(0) + '°' : '—';
        const aws = apparent ? apparent.awsKn.toFixed(1) + ' kn' : '—';
        return `<tr><td>${time}</td><td>${spd}</td><td>${eng}</td><td>${tws}</td><td>${twd}</td>` +
            `<td>${gust}</td><td>${awa}</td><td>${aws}</td></tr>`;
    }).join('');
}
```

Replace it with:

```javascript
function renderRouteReportTable(pts) {
    const body = document.getElementById('routeReportTableBody');
    body.innerHTML = pts.map(p => {
        const t = new Date(p.timestamp);
        const time = t.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', timeZone: 'UTC' })
            + ' UTC ' + t.toLocaleDateString('en-GB', { day: '2-digit', month: 'short', timeZone: 'UTC' });
        const hdg = p.heading_deg != null ? p.heading_deg.toFixed(0) + '°' : '—';
        const spd = p.speed_kn != null ? p.speed_kn.toFixed(1) + ' kn' : '—';
        const eng = p.speed_kn == null ? '—' : (p.twa_deg === null ? '⚙ Motoring' : '⛵ Sailing');
        const tws = p.wind_speed_kn != null ? p.wind_speed_kn.toFixed(1) + ' kn' : '—';
        let twd = p.relative_wind_deg != null ? p.relative_wind_deg.toFixed(0) + '°' : '—';
        if (p.relative_wind_deg != null && p.wind_direction_deg != null && p.heading_deg != null) {
            // Wind's FROM-direction clockwise of the boat's heading (0-180°) is on the
            // starboard side; counter-clockwise (180-360°) is on the port side.
            const diff = ((p.wind_direction_deg - p.heading_deg) % 360 + 360) % 360;
            twd += diff <= 180 ? ' S' : ' P';
        }
        const gust = p.wind_gust_kn != null ? p.wind_gust_kn.toFixed(1) + ' kn' : '—';
        const apparent = trueToApparentWind(p.wind_speed_kn, p.relative_wind_deg, p.speed_kn);
        const awa = apparent ? apparent.awaDeg.toFixed(0) + '°' : '—';
        const aws = apparent ? apparent.awsKn.toFixed(1) + ' kn' : '—';
        return `<tr><td>${time}</td><td>${hdg}</td><td>${spd}</td><td>${eng}</td><td>${tws}</td><td>${twd}</td>` +
            `<td>${gust}</td><td>${awa}</td><td>${aws}</td></tr>`;
    }).join('');
}
```

- [ ] **Step 3: Manual verification — run the app**

Run: `cargo build --release && ./target/release/nmea_router` (or the project's existing dev-run process), open `/plan.html` in a browser.

- Draw a 2+ waypoint route with legs on different courses (e.g. a dogleg), set a departure time with wind data available, click Compute (or Optimize).
- Confirm the Step-by-step table now has an "Hdg" column between Time and Speed, blank (`—`) on the first row.
- Confirm the TWD column shows a trailing ` S` or ` P` on rows with sail/motor data, and that switching to a leg on a different course (same absolute wind direction) can flip the letter.
- Confirm no console errors, and confirm a route promoted via "Select this route" from the alternative-route modal still renders — Hdg column shows `—` and TWD has no letter suffix on those rows (expected, per spec's scope).

- [ ] **Step 4: Commit**

```bash
git add static/plan.html
git commit -m "Show heading and port/starboard qualifier in route report table"
```

---

## Self-Review

**1. Spec coverage:**
- "Add `heading_deg` to `RouteTrackPoint`/`RouteOverlayPoint`, populate at both construction sites, thread through `compute_route_overlay`" → Task 1, Steps 3-7.
- "Hdg column in the table" → Task 2, Step 1-2.
- "Port/starboard suffix on the TWD cell, single-letter, gated on `relative_wind_deg`" → Task 2, Step 2.
- "Frontier-promoted routes show `—`/no suffix (out of scope, not touched)" → not modified by either task; verified manually in Task 2, Step 3.
- "Tests for `heading_deg` None-on-departure and matches `haversine_heading`" → Task 1, Step 1.

**2. Placeholder scan:** No TBD/TODO; every step has literal code or an exact runnable command.

**3. Type consistency:** `heading_deg: Option<f64>` is identical across `RouteTrackPoint`, `RouteOverlayPoint`, and both construction sites. Frontend reads `p.heading_deg` (JS `number | null`, matching serde's `Option<f64>` → `number | null`) consistently in Task 2.
