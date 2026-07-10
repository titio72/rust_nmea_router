# Isochrone Alternative-Route Hover Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a user hover the mouse over an isochrone search frontier on `plan.html` (after running "Optimize") and see the discarded path from the route origin to the hovered point highlighted on the map.

**Architecture:** Backend already tracks each frontier point's `parent_idx` internally but discards it before returning `frontiers` to the API. Expose it as a `FrontierPoint { lat, lon, parent_idx }` struct instead of a bare `(f64, f64)` tuple. The frontend stores the full frontier data client-side and, on hover, walks `parent_idx` back to the origin to build a highlighted polyline — no new API call per hover.

**Tech Stack:** Rust (Axum/Serde) backend, vanilla JS + Leaflet frontend (`static/plan.html`). No build step for the frontend.

## Global Constraints

- Backend: Rust only. Frontend: HTML + vanilla JavaScript (CLAUDE.md).
- `snake_case` for Rust functions/fields; JSON field names inherit `snake_case` directly from struct fields (no `#[serde(rename...)]`) — matches existing structs like `RouteOverlayPoint` (`src/forecast.rs:20-32`).
- Do NOT run `git commit` or `git push` — per this repo's CLAUDE.md, stop after code changes; the user commits.
- No change to `MAX_STEPS`, `prune_isochrone`, stagnation detection, or any other isochrone search/pruning behavior — this is visualization only.
- No new API endpoint — backtracking happens entirely client-side using data already in the `/api/forecast/optimal-route` response.
- Hovering snaps to the nearest frontier point to the cursor (no segment interpolation) — spec explicitly rejects interpolation for this iteration.
- No tooltip with ETA/distance on hover — spec explicitly scopes this out to avoid growing the frontier JSON payload with per-point timestamps.

---

### Task 1: Backend — expose `parent_idx` on frontier points

**Files:**
- Modify: `src/routing.rs:28-32` (`IsochroneResult`), `src/routing.rs:177-182` (frontier-building), `src/routing.rs:484-510` (`test_frontiers_exclude_seed_and_respect_sector_count`)
- Modify: `src/web/api.rs:1775-1779` (`OptimalRouteResponse`), `src/web/api.rs:1903-1906` (response construction — no change needed here beyond the type following through)

**Interfaces:**
- Produces: `pub struct FrontierPoint { pub lat: f64, pub lon: f64, pub parent_idx: usize }` in `src/routing.rs`, and `IsochroneResult.frontiers: Vec<Vec<FrontierPoint>>` (previously `Vec<Vec<(f64, f64)>>`). `OptimalRouteResponse.frontiers` in `src/web/api.rs` picks up the same type.
- Consumes: nothing new — uses the existing `IsochronePoint.parent_idx: Option<usize>` field already set on every point pushed into `candidates` (`src/routing.rs:147-153`).

- [ ] **Step 1: Add `FrontierPoint` and update `IsochroneResult`**

In `src/routing.rs`, change (current lines 28-32):

```rust
pub struct IsochroneResult {
    pub track: Vec<(f64, f64, DateTime<Utc>)>,
    pub reached_destination: bool,
    pub frontiers: Vec<Vec<(f64, f64)>>,
}
```

to:

```rust
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct FrontierPoint {
    pub lat: f64,
    pub lon: f64,
    pub parent_idx: usize,
}

pub struct IsochroneResult {
    pub track: Vec<(f64, f64, DateTime<Utc>)>,
    pub reached_destination: bool,
    pub frontiers: Vec<Vec<FrontierPoint>>,
}
```

- [ ] **Step 2: Update the frontier-building map to include `parent_idx`**

In `src/routing.rs`, change (current lines 177-182):

```rust
    // Every step's frontier after the seed (step 0, which is just the origin), stripped down
    // to (lat, lon) — the internal time/sailed_hours/parent_idx fields aren't needed for display.
    let frontiers: Vec<Vec<(f64, f64)>> = isochrones[1..]
        .iter()
        .map(|frontier| frontier.iter().map(|p| (p.lat, p.lon)).collect())
        .collect();
```

to:

```rust
    // Every step's frontier after the seed (step 0, which is just the origin). parent_idx
    // indexes into the previous step's frontier (or, for step 0 of this slice, always 0,
    // meaning "the search origin" — the seed itself is never exposed as its own frontier
    // entry). Every point here comes from `isochrones[1..]`, so `parent_idx` is always
    // `Some(_)` by construction (only the seed at `isochrones[0]` has `None`).
    let frontiers: Vec<Vec<FrontierPoint>> = isochrones[1..]
        .iter()
        .map(|frontier| {
            frontier
                .iter()
                .map(|p| FrontierPoint {
                    lat: p.lat,
                    lon: p.lon,
                    parent_idx: p.parent_idx.unwrap(),
                })
                .collect()
        })
        .collect();
```

- [ ] **Step 3: Update the existing frontier test for the new point type**

In `src/routing.rs`, change (current lines 484-510):

```rust
    #[test]
    fn test_frontiers_exclude_seed_and_respect_sector_count() {
        let from = (43.0, 8.0);
        let to = (43.29, 8.0); // ~20 nm north
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let polars = dummy_polars();

        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, 0.0, &polars, &[], None);

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

to:

```rust
    #[test]
    fn test_frontiers_exclude_seed_and_respect_sector_count() {
        let from = (43.0, 8.0);
        let to = (43.29, 8.0); // ~20 nm north
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let polars = dummy_polars();

        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, 0.0, &polars, &[], None);

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
            !result
                .frontiers
                .iter()
                .any(|f| f.len() == 1 && (f[0].lat, f[0].lon) == from),
            "seed (origin-only) frontier should be excluded"
        );
    }

    #[test]
    fn test_frontier_parent_idx_chains_to_origin() {
        let from = (43.0, 8.0);
        let to = (43.29, 8.0); // ~20 nm north
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let polars = dummy_polars();

        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, 0.0, &polars, &[], None);
        assert!(!result.frontiers.is_empty());

        // Every point's parent_idx must be a valid index into the previous step (or 0 for
        // step 0, meaning "the origin").
        for (s, frontier) in result.frontiers.iter().enumerate() {
            let prev_len = if s == 0 {
                1 // the seed step, never exposed, has exactly one point
            } else {
                result.frontiers[s - 1].len()
            };
            for pt in frontier {
                assert!(
                    pt.parent_idx < prev_len,
                    "step {} point has parent_idx {} but previous step has {} points",
                    s,
                    pt.parent_idx,
                    prev_len
                );
            }
        }

        // Following parent_idx from any point in the last frontier must terminate at step 0
        // index 0 within frontiers.len() hops.
        let last_step = result.frontiers.len() - 1;
        let mut s = last_step;
        let mut i = 0usize; // arbitrary starting point in the last frontier
        let mut hops = 0usize;
        loop {
            let pt = result.frontiers[s][i];
            if s == 0 {
                assert_eq!(pt.parent_idx, 0, "step 0 parent_idx must always be 0 (origin)");
                break;
            }
            i = pt.parent_idx;
            s -= 1;
            hops += 1;
            assert!(hops <= result.frontiers.len(), "parent_idx chain did not terminate");
        }
    }
```

- [ ] **Step 4: Update the API response type**

In `src/web/api.rs`, change (current lines 1775-1779):

```rust
#[derive(Debug, Serialize)]
pub struct OptimalRouteResponse {
    pub route: Vec<crate::forecast::RouteOverlayPoint>,
    pub frontiers: Vec<Vec<(f64, f64)>>,
}
```

to:

```rust
#[derive(Debug, Serialize)]
pub struct OptimalRouteResponse {
    pub route: Vec<crate::forecast::RouteOverlayPoint>,
    pub frontiers: Vec<Vec<crate::routing::FrontierPoint>>,
}
```

No change is needed at the construction site (`src/web/api.rs:1903-1906`, `frontiers: result.frontiers`) — the type flows through unchanged since `IsochroneResult.frontiers` now already matches.

- [ ] **Step 5: Run the routing test suite**

Run: `cargo test routing::`
Expected: all tests pass, including the two frontier tests (`test_frontiers_exclude_seed_and_respect_sector_count`, `test_frontier_parent_idx_chains_to_origin`).

- [ ] **Step 6: Run a full build to confirm the API layer compiles against the new type**

Run: `cargo build`
Expected: no errors. (`OptimalRouteResponse` derives `Serialize` and `FrontierPoint` derives `Serialize` too, so no manual JSON code needs updating.)

- [ ] **Step 7: Commit**

```bash
git add src/routing.rs src/web/api.rs
```

(Per this repo's CLAUDE.md, do not run `git commit` — leave the change staged/unstaged for the user to review and commit themselves. Skip the actual commit command.)

---

### Task 2: Frontend — hover-to-reveal alternative route

**Files:**
- Modify: `static/plan.html:250-256` (module-level state), `static/plan.html:725-746` (`clearRoute`), `static/plan.html:806-809` (`computeRoute` reset block), `static/plan.html:851-854` (`optimizeRoute` reset block), `static/plan.html:910-934` (`drawFrontierRun` / `drawFrontiers`)

**Interfaces:**
- Consumes: `Task 1`'s new JSON shape — each frontier point arrives as `{ lat, lon, parent_idx }` instead of `[lat, lon]`. Existing `pointInArea(lat, lon, area)` / `pointInAnyArea(lat, lon)` (`static/plan.html:593-600`) and `routeWaypoints` (`L.LatLng[]`, existing module-level array) are used unchanged.
- Produces: `lastFrontiers` (module-level `FrontierPoint[][]`, mirrors the raw API response), `alternativeLine` (module-level `L.Polyline | null`, the current hover highlight), `backtrackFrontierPath(stepIdx, ptIdx)` → `[lat, lon][]`, `showFrontierAlternative(e, stepIdx, ptIdxs, latlngs)`, `hideFrontierAlternative()`. No other task depends on these — this is the last task in the plan.

- [ ] **Step 1: Add module-level state for hover backtracking**

In `static/plan.html`, change (current lines 250-256):

```javascript
        let previewLines = [];      // L.Polyline[] dashed segments between consecutive waypoints
        let routeSegments = [];     // coloured segments from drawRouteLine
        let frontierLines = [];     // muted backdrop polylines from isochrone search frontiers — session-only, never persisted

        let lastGridPts = [];     // raw GridPointForecast[] from last API call
        let showGust = false;     // when true, color arrows by gust speed instead of wind speed
        let lastRouteOverlay = []; // last computed overlay — persisted across reloads
```

to:

```javascript
        let previewLines = [];      // L.Polyline[] dashed segments between consecutive waypoints
        let routeSegments = [];     // coloured segments from drawRouteLine
        let frontierLines = [];     // muted backdrop polylines from isochrone search frontiers — session-only, never persisted
        let lastFrontiers = [];     // raw FrontierPoint[][] from last optimize response — used to backtrack alternative paths on hover
        let alternativeLine = null; // highlighted discarded path shown on frontier hover — replaced/cleared on mousemove/mouseout

        let lastGridPts = [];     // raw GridPointForecast[] from last API call
        let showGust = false;     // when true, color arrows by gust speed instead of wind speed
        let lastRouteOverlay = []; // last computed overlay — persisted across reloads
```

- [ ] **Step 2: Reset hover state in `clearRoute`**

In `static/plan.html`, change (current lines within `clearRoute`, around line 734-736):

```javascript
            frontierLines.forEach(l => planMap.removeLayer(l));
            frontierLines = [];
            document.getElementById('routeBar').style.display = 'none';
```

to:

```javascript
            frontierLines.forEach(l => planMap.removeLayer(l));
            frontierLines = [];
            lastFrontiers = [];
            if (alternativeLine) { planMap.removeLayer(alternativeLine); alternativeLine = null; }
            document.getElementById('routeBar').style.display = 'none';
```

- [ ] **Step 3: Reset hover state in `computeRoute`**

In `static/plan.html`, change (current lines within `computeRoute`, around line 806-809):

```javascript
            drawRouteLine([]);
            frontierLines.forEach(l => planMap.removeLayer(l));
            frontierLines = [];
            lastRouteOverlay = [];
```

to:

```javascript
            drawRouteLine([]);
            frontierLines.forEach(l => planMap.removeLayer(l));
            frontierLines = [];
            lastFrontiers = [];
            if (alternativeLine) { planMap.removeLayer(alternativeLine); alternativeLine = null; }
            lastRouteOverlay = [];
```

- [ ] **Step 4: Reset hover state in `optimizeRoute`**

In `static/plan.html`, change (current lines within `optimizeRoute`, around line 851-854):

```javascript
            drawRouteLine([]);
            frontierLines.forEach(l => planMap.removeLayer(l));
            frontierLines = [];
            lastRouteOverlay = [];
```

to:

```javascript
            drawRouteLine([]);
            frontierLines.forEach(l => planMap.removeLayer(l));
            frontierLines = [];
            lastFrontiers = [];
            if (alternativeLine) { planMap.removeLayer(alternativeLine); alternativeLine = null; }
            lastRouteOverlay = [];
```

(The call site later in this same function, `drawFrontiers(json.data.frontiers || [])`, does not need to change — it already passes the raw frontier array, which now contains `{lat, lon, parent_idx}` objects instead of `[lat, lon]` tuples; `drawFrontiers` itself is rewritten in Step 5 to handle the new shape.)

- [ ] **Step 5: Rewrite `drawFrontierRun`/`drawFrontiers` and add the backtracking/hover functions**

In `static/plan.html`, change (current lines 910-934):

```javascript
        function drawFrontierRun(latlngs) {
            const line = L.polyline(latlngs, {
                color: '#444',
                weight: 1,
                opacity: 0.6
            }).addTo(planMap);
            frontierLines.push(line);
        }

        function drawFrontiers(frontiers) {
            frontierLines.forEach(l => planMap.removeLayer(l));
            frontierLines = [];
            for (const frontier of frontiers) {
                let run = [];
                for (const [lat, lon] of frontier) {
                    if (pointInAnyArea(lat, lon)) {
                        run.push([lat, lon]);
                    } else {
                        if (run.length >= 2) drawFrontierRun(run);
                        run = [];
                    }
                }
                if (run.length >= 2) drawFrontierRun(run);
            }
        }
```

to:

```javascript
        // Walks parent_idx from (stepIdx, ptIdx) in lastFrontiers back to the route origin,
        // returning an origin-to-point path of [lat, lon] pairs.
        function backtrackFrontierPath(stepIdx, ptIdx) {
            const path = [];
            let s = stepIdx, i = ptIdx;
            while (s >= 0) {
                const pt = lastFrontiers[s][i];
                path.push([pt.lat, pt.lon]);
                i = pt.parent_idx;
                s -= 1;
            }
            const origin = routeWaypoints[0];
            path.push([origin.lat, origin.lng]);
            path.reverse();
            return path;
        }

        // latlngs/ptIdxs are parallel arrays for one drawn frontier run: latlngs[i] is the
        // [lat, lon] of the point whose original index (into lastFrontiers[stepIdx]) is ptIdxs[i].
        function showFrontierAlternative(e, stepIdx, ptIdxs, latlngs) {
            let nearest = 0;
            let nearestDist = Infinity;
            for (let i = 0; i < latlngs.length; i++) {
                const dLat = latlngs[i][0] - e.latlng.lat;
                const dLon = latlngs[i][1] - e.latlng.lng;
                const dist = dLat * dLat + dLon * dLon;
                if (dist < nearestDist) {
                    nearestDist = dist;
                    nearest = i;
                }
            }
            const path = backtrackFrontierPath(stepIdx, ptIdxs[nearest]);
            if (alternativeLine) planMap.removeLayer(alternativeLine);
            alternativeLine = L.polyline(path, {
                color: '#ff8c00',
                weight: 2,
                opacity: 0.9,
                dashArray: '4,4'
            }).addTo(planMap);
        }

        function hideFrontierAlternative() {
            if (alternativeLine) {
                planMap.removeLayer(alternativeLine);
                alternativeLine = null;
            }
        }

        function drawFrontierRun(latlngs, stepIdx, ptIdxs) {
            const line = L.polyline(latlngs, {
                color: '#444',
                weight: 1,
                opacity: 0.6
            }).addTo(planMap);
            line.on('mousemove', e => showFrontierAlternative(e, stepIdx, ptIdxs, latlngs));
            line.on('mouseout', hideFrontierAlternative);
            frontierLines.push(line);
        }

        function drawFrontiers(frontiers) {
            frontierLines.forEach(l => planMap.removeLayer(l));
            frontierLines = [];
            hideFrontierAlternative();
            lastFrontiers = frontiers;
            for (let s = 0; s < frontiers.length; s++) {
                const frontier = frontiers[s];
                let run = [];
                let runIdx = [];
                for (let i = 0; i < frontier.length; i++) {
                    const { lat, lon } = frontier[i];
                    if (pointInAnyArea(lat, lon)) {
                        run.push([lat, lon]);
                        runIdx.push(i);
                    } else {
                        if (run.length >= 2) drawFrontierRun(run, s, runIdx);
                        run = [];
                        runIdx = [];
                    }
                }
                if (run.length >= 2) drawFrontierRun(run, s, runIdx);
            }
        }
```

- [ ] **Step 6: Verify the file is well-formed**

Run: `node --check <(sed -n '/<script>/,/<\/script>/p' static/plan.html | sed '1d;$d')`
Expected: no output (syntax OK). If `node` rejects the process-substitution syntax in your shell, extract the `<script>...</script>` body to a temp `.js` file first and run `node --check` on that file instead.

There is no build step or automated test suite for this file (plain JS in an HTML page) — this is the only mechanical verification available. The rest is manual browser verification in Step 7.

- [ ] **Step 7: Build the backend and manually verify in the browser**

Run: `cargo build --release` (Task 1's backend change must be built for the new `{lat, lon, parent_idx}` JSON shape to actually be served).

Run the project's existing app-launch process (e.g. via the project's `run` skill/process) with a `config.json` that has a polar table and at least one forecast area configured covering part of a planned route, plus forecast data fetched for it.

In the browser, open `plan.html`, plan a route whose isochrone search produces multiple frontier steps, and click "Optimize". Then:

- Hover the mouse over a frontier line. Confirm an orange dashed path (`#ff8c00`) appears, running from the route origin to a point near the cursor.
- Move the mouse along the frontier line. Confirm the highlighted path updates smoothly, following the cursor to different frontier points.
- Move the mouse off the frontier line (`mouseout`). Confirm the orange path disappears.
- Click "Clear Route" (or plan a new route) after hovering. Confirm no orange line lingers on the map.
- Run "Optimize" a second time. Confirm hovering the new frontiers works correctly and no stale highlight from the previous run appears.
- If forecast-area clipping split a frontier into multiple disconnected line segments (per the prior styling/clipping feature), hover each segment and confirm the backtracked path is still correct (i.e. index mapping through `ptIdxs` survived the clipping).

- [ ] **Step 8: Commit**

```bash
git add static/plan.html
```

(Per this repo's CLAUDE.md, do not run `git commit` — leave the change staged/unstaged for the user to review and commit themselves. Skip the actual commit command.)

---

## Self-Review Notes

- **Spec coverage:** Backend `FrontierPoint`/`parent_idx` exposure (Task 1, Steps 1-2), API type flow-through (Task 1, Step 4), existing-test fixup for the new point type (Task 1, Step 3), new parent-chain validity test (Task 1, Step 3), frontend hover snapping to nearest point (Task 2, Step 5 `showFrontierAlternative`), client-side backtracking with no new API call (Task 2, Step 5 `backtrackFrontierPath`), orange dashed highlight styling (Task 2, Step 5), cleanup on clear/recompute (Task 2, Steps 2-4), index-preserving forecast-area clipping (Task 2, Step 5 `drawFrontiers`/`ptIdxs`). All spec sections have a corresponding task.
- **Placeholder scan:** No TBDs; every step has complete, copy-pasteable code or an exact command with expected output.
- **Type consistency:** `FrontierPoint { lat, lon, parent_idx }` (Task 1, Step 1) is what `drawFrontiers` destructures via `{ lat, lon } = frontier[i]` and `frontier[i].parent_idx` implicitly via `lastFrontiers[s][i].parent_idx` in `backtrackFrontierPath` (Task 2, Step 5) — consistent. `drawFrontierRun(latlngs, stepIdx, ptIdxs)` signature matches both its definition and its call site in `drawFrontiers`. `showFrontierAlternative(e, stepIdx, ptIdxs, latlngs)` parameter order matches the arrow-function call site in `drawFrontierRun`.
