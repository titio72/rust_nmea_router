# Isochrone Frontier Visualization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show the isochrone router's expanding search frontiers (one polyline per 30-min step) on the `plan.html` map alongside the optimal route, so the user can see the wavefront the search explored.

**Architecture:** `run_isochrone` (src/routing.rs) already computes per-step frontiers internally but discards them. Surface them on `IsochroneResult`, thread them through the `/api/forecast/optimal-route` handler in a new response wrapper, and render them client-side as muted backdrop polylines drawn under the existing colored route.

**Tech Stack:** Rust (axum, serde), vanilla JS + Leaflet (`static/plan.html`).

## Global Constraints

- Backend: Rust only; frontend: HTML + vanilla JS (CLAUDE.md).
- No `now()` calls in business logic — not applicable here (no new timestamps introduced).
- Do NOT run `git commit` or `git push` — per this repo's CLAUDE.md, stop after each task's code changes; the user commits.
- Frontiers are session-only: never written to `localStorage` / `saveRouteState()`.
- No decimation of frontiers — every computed step is surfaced, per the approved spec (`docs/superpowers/specs/2026-07-05-isochrone-frontier-visualization-design.md`).
- Only `/api/forecast/optimal-route` changes shape; `/api/forecast/route` is untouched.

---

### Task 1: Surface search frontiers from `run_isochrone`

**Files:**
- Modify: `src/routing.rs:28-31` (struct), `src/routing.rs:47-58` (early-return guards), `src/routing.rs:170-182` (final return logic)
- Test: `src/routing.rs` (existing `#[cfg(test)] mod tests`, add new test after line 463)

**Interfaces:**
- Produces: `IsochroneResult.frontiers: Vec<Vec<(f64, f64)>>` — one entry per search step after the seed, each an ordered (by bearing-from-origin) list of `(lat, lon)` points. Populated whether or not the destination was reached. This is what Task 2 reads as `result.frontiers`.

- [ ] **Step 1: Write the failing test**

Add this test at the end of the `mod tests` block in `src/routing.rs`, right before the module's closing `}` (currently line 464):

```rust
    #[test]
    fn test_frontiers_exclude_seed_and_respect_sector_count() {
        let from = (43.0, 8.0);
        let to = (43.29, 8.0); // ~20 nm north
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let polars = dummy_polars();

        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, &polars, &[], None);

        assert!(
            !result.frontiers.is_empty(),
            "expected at least one frontier to be recorded"
        );
        for frontier in &result.frontiers {
            assert!(
                frontier.len() <= SECTOR_COUNT,
                "frontier has {} points, expected <= {}",
                frontier.len(),
                SECTOR_COUNT
            );
            assert!(!frontier.is_empty(), "frontier must not be empty");
        }
        // The seed step is a single point at the origin — it must not appear as a "frontier".
        assert!(
            !result.frontiers.iter().any(|f| f.len() == 1 && f[0] == from),
            "seed (origin-only) frontier should be excluded"
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib routing::tests::test_frontiers_exclude_seed_and_respect_sector_count`
Expected: FAIL to compile — `no field \`frontiers\` on type \`IsochroneResult\`` (the field doesn't exist yet).

- [ ] **Step 3: Add the `frontiers` field to `IsochroneResult`**

In `src/routing.rs`, change (current lines 28-31):

```rust
pub struct IsochroneResult {
    pub track: Vec<(f64, f64, DateTime<Utc>)>,
    pub reached_destination: bool,
}
```

to:

```rust
pub struct IsochroneResult {
    pub track: Vec<(f64, f64, DateTime<Utc>)>,
    pub reached_destination: bool,
    pub frontiers: Vec<Vec<(f64, f64)>>,
}
```

- [ ] **Step 4: Populate `frontiers` in the early-return guards**

In `src/routing.rs`, change (current lines 47-58):

```rust
    if motoring_speed_kn <= 0.0 {
        return IsochroneResult {
            track: vec![],
            reached_destination: false,
        };
    }
    if land_mask.map_or(false, |m| m.is_land(to.0, to.1)) {
        return IsochroneResult {
            track: vec![],
            reached_destination: false,
        };
    }
```

to:

```rust
    if motoring_speed_kn <= 0.0 {
        return IsochroneResult {
            track: vec![],
            reached_destination: false,
            frontiers: vec![],
        };
    }
    if land_mask.map_or(false, |m| m.is_land(to.0, to.1)) {
        return IsochroneResult {
            track: vec![],
            reached_destination: false,
            frontiers: vec![],
        };
    }
```

- [ ] **Step 5: Populate `frontiers` in the final return logic**

In `src/routing.rs`, change (current lines 170-182):

```rust
    if let Some((eta, step_idx, pt_idx)) = best {
        let track = backtrack(&isochrones[..=step_idx], pt_idx, to, eta);
        return IsochroneResult {
            track,
            reached_destination: true,
        };
    }

    IsochroneResult {
        track: vec![],
        reached_destination: false,
    }
}
```

to:

```rust
    // Every step's frontier after the seed (step 0, which is just the origin), stripped down
    // to (lat, lon) — the internal time/sailed_hours/parent_idx fields aren't needed for display.
    let frontiers: Vec<Vec<(f64, f64)>> = isochrones[1..]
        .iter()
        .map(|frontier| frontier.iter().map(|p| (p.lat, p.lon)).collect())
        .collect();

    if let Some((eta, step_idx, pt_idx)) = best {
        let track = backtrack(&isochrones[..=step_idx], pt_idx, to, eta);
        return IsochroneResult {
            track,
            reached_destination: true,
            frontiers,
        };
    }

    IsochroneResult {
        track: vec![],
        reached_destination: false,
        frontiers,
    }
}
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test --lib routing::tests`
Expected: PASS — all tests in `routing::tests`, including the new one, succeed.

- [ ] **Step 7: Commit**

```bash
git add src/routing.rs
git commit -m "feat: surface isochrone search frontiers from run_isochrone"
```

---

### Task 2: Wire frontiers through the `/api/forecast/optimal-route` response

**Files:**
- Modify: `src/web/api.rs:1694-1775` (handler), add new struct just before it

**Interfaces:**
- Consumes: `crate::routing::run_isochrone(...) -> IsochroneResult` with `.frontiers: Vec<Vec<(f64, f64)>>` (from Task 1).
- Produces: `OptimalRouteResponse { route: Vec<RouteOverlayPoint>, frontiers: Vec<Vec<(f64, f64)>> }`, serialized as the `data` field of `/api/forecast/optimal-route`'s JSON response. This is what Task 3's frontend code reads as `json.data.route` / `json.data.frontiers`.

- [ ] **Step 1: Add the `OptimalRouteResponse` struct**

In `src/web/api.rs`, immediately before `pub async fn get_optimal_route(` (current line 1694), add:

```rust
#[derive(Debug, Serialize)]
pub struct OptimalRouteResponse {
    pub route: Vec<crate::forecast::RouteOverlayPoint>,
    pub frontiers: Vec<Vec<(f64, f64)>>,
}

```

- [ ] **Step 2: Change the handler's return type**

In `src/web/api.rs`, change (current lines 1696-1697):

```rust
) -> Result<Json<ApiResponse<Vec<crate::forecast::RouteOverlayPoint>>>, StatusCode> {
```

to:

```rust
) -> Result<Json<ApiResponse<OptimalRouteResponse>>, StatusCode> {
```

- [ ] **Step 3: Return both route and frontiers**

In `src/web/api.rs`, change the last two lines of `get_optimal_route` (current lines 1773-1774):

```rust
    let overlay = crate::forecast::compute_route_overlay(&route_points, &fetches);
    Ok(Json(ApiResponse::ok(overlay)))
}
```

to:

```rust
    let overlay = crate::forecast::compute_route_overlay(&route_points, &fetches);
    Ok(Json(ApiResponse::ok(OptimalRouteResponse {
        route: overlay,
        frontiers: result.frontiers,
    })))
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: builds with no errors. (`result` is still in scope from the earlier `crate::routing::run_isochrone(...)` call in this function; only its `.track` field was consumed by the `route_points` loop via borrow, so `.frontiers` remains accessible here.)

- [ ] **Step 5: Run the full non-DB test suite as a regression check**

Run: `cargo test`
Expected: PASS (all non-`#[ignore]` tests, including Task 1's new test).

- [ ] **Step 6: Commit**

```bash
git add src/web/api.rs
git commit -m "feat: include isochrone frontiers in optimal-route API response"
```

---

### Task 3: Render frontier polylines on the map

**Files:**
- Modify: `static/plan.html:245` (state array), `static/plan.html:709-728` (`clearRoute`), `static/plan.html:777-817` (`computeRoute`), `static/plan.html:819-864` (`optimizeRoute`), `static/plan.html:880` (new `drawFrontiers` function before `drawRouteLine`)

**Interfaces:**
- Consumes: `OptimalRouteResponse` JSON shape from Task 2 — `json.data.route` (array of `RouteOverlayPoint`, same shape as before) and `json.data.frontiers` (array of arrays of `[lat, lon]` pairs).
- Produces: `frontierLines: L.Polyline[]` module-level array and `drawFrontiers(frontiers)` function, called by `optimizeRoute()`.

- [ ] **Step 1: Add the `frontierLines` state array**

In `static/plan.html`, change (current line 245):

```javascript
        let routeSegments = [];     // coloured segments from drawRouteLine
```

to:

```javascript
        let routeSegments = [];     // coloured segments from drawRouteLine
        let frontierLines = [];     // muted backdrop polylines from isochrone search frontiers — session-only, never persisted
```

- [ ] **Step 2: Clear frontier lines in `clearRoute()`**

In `static/plan.html`, change (current lines 717-718, inside `clearRoute()`):

```javascript
            routeSegments.forEach(l => planMap.removeLayer(l));
            routeSegments = [];
```

to:

```javascript
            routeSegments.forEach(l => planMap.removeLayer(l));
            routeSegments = [];
            frontierLines.forEach(l => planMap.removeLayer(l));
            frontierLines = [];
```

- [ ] **Step 3: Clear frontier lines at the start of `computeRoute()`**

In `static/plan.html`, change (current lines 784-785, inside `computeRoute()`):

```javascript
            drawRouteLine([]);
            lastRouteOverlay = [];
```

to:

```javascript
            drawRouteLine([]);
            frontierLines.forEach(l => planMap.removeLayer(l));
            frontierLines = [];
            lastRouteOverlay = [];
```

- [ ] **Step 4: Clear frontier lines at the start of `optimizeRoute()`**

In `static/plan.html`, change (current lines 826-827, inside `optimizeRoute()`):

```javascript
            drawRouteLine([]);
            lastRouteOverlay = [];
```

to:

```javascript
            drawRouteLine([]);
            frontierLines.forEach(l => planMap.removeLayer(l));
            frontierLines = [];
            lastRouteOverlay = [];
```

- [ ] **Step 5: Update `optimizeRoute()`'s response handling for the new `{ route, frontiers }` shape**

In `static/plan.html`, change (current lines 850-859, inside `optimizeRoute()`):

```javascript
                const resp = await fetch(url);
                const json = await resp.json();
                if (json.status !== 'ok' || !json.data?.length) {
                    btn.textContent = '✗ ' + (json.error || 'Error');
                    setTimeout(() => { btn.textContent = orig; btn.disabled = false; }, 3000);
                    return;
                }
                drawRouteLine(json.data);
                btn.textContent = orig;
                btn.disabled = false;
```

to:

```javascript
                const resp = await fetch(url);
                const json = await resp.json();
                if (json.status !== 'ok' || !json.data?.route?.length) {
                    btn.textContent = '✗ ' + (json.error || 'Error');
                    setTimeout(() => { btn.textContent = orig; btn.disabled = false; }, 3000);
                    return;
                }
                drawFrontiers(json.data.frontiers || []);
                drawRouteLine(json.data.route);
                btn.textContent = orig;
                btn.disabled = false;
```

- [ ] **Step 6: Add the `drawFrontiers` function**

In `static/plan.html`, immediately before the `drawRouteLine` function (current line 880, `function drawRouteLine(pts) {`), add:

```javascript
        function drawFrontiers(frontiers) {
            frontierLines.forEach(l => planMap.removeLayer(l));
            frontierLines = [];
            for (const frontier of frontiers) {
                if (frontier.length < 2) continue;
                const line = L.polyline(frontier, {
                    color: '#888',
                    weight: 1,
                    opacity: 0.35
                }).addTo(planMap);
                frontierLines.push(line);
            }
        }

```

- [ ] **Step 7: Manually verify in the browser**

Run: `cargo run --bin nmea_router` (or whatever the project's existing `run` skill/process is) with a `config.json` that has a polar table and forecast data configured.

In the browser, open `plan.html`, click "Plan Route", place two waypoints with forecast coverage, set a departure/speed, click "Done", then click "Optimize".

Expected:
- A fan of thin, muted gray polylines (the frontiers) appears on the map, with the colored optimal route drawn on top of them.
- Clicking "Optimize" again clears and redraws the frontiers cleanly (no duplicate/stale lines).
- Reloading the page restores the colored route (as before) but not the frontier lines.
- Clicking "Compute" (the non-isochrone straight-course button) does not show any frontier lines.

- [ ] **Step 8: Commit**

```bash
git add static/plan.html
git commit -m "feat: render isochrone search frontiers as backdrop on the plan map"
```

---

## Self-Review Notes

- **Spec coverage:** Backend field (Task 1), API wrapper (Task 2), frontend rendering + clearing + persistence exclusion + styling (Task 3) all covered. The spec's "known limitation" (payload size) is accepted as-is, no task needed.
- **Placeholder scan:** No TBDs; all code blocks are complete and copy-pasteable.
- **Type consistency:** `Vec<Vec<(f64, f64)>>` is used identically in Task 1 (`IsochroneResult.frontiers`), Task 2 (`OptimalRouteResponse.frontiers`), and Task 3 (`json.data.frontiers`, consumed as arrays of `[lat, lon]` pairs — matches serde's default tuple-as-array serialization). Function name `drawFrontiers` and array name `frontierLines` are used consistently everywhere they're referenced.
