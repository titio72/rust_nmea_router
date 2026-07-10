# Isochrone Alternative-Route Hover — Design

## Purpose

The isochrone search frontiers already rendered on `plan.html`
([2026-07-05-isochrone-frontier-visualization-design.md](2026-07-05-isochrone-frontier-visualization-design.md),
[2026-07-06-isochrone-frontier-styling-and-area-clip-design.md](2026-07-06-isochrone-frontier-styling-and-area-clip-design.md))
show *where* the search explored, but not *how* the boat could have gotten there. Every
frontier point was reached via a specific chain of discarded headings/steps from the
origin — that chain is computed internally (`IsochronePoint.parent_idx` in `src/routing.rs`)
but thrown away before the frontier data reaches the client.

This feature lets the user hover the mouse over a frontier line, after running "Optimize",
and see the discarded path from the route origin to the hovered point highlighted on the map.

This is visualization only — no change to routing search/pruning behavior, and no new API
endpoint (backtracking happens client-side from data already sent).

## Scope

- Applies only to the isochrone "Optimize" flow (`/api/forecast/optimal-route`) and its
  existing frontier rendering in `static/plan.html`. The straight-course "Compute" flow
  is unaffected (it has no frontiers to hover).
- Hovering snaps to the nearest frontier point to the cursor (no segment interpolation).
- Only the path line is shown on hover — no tooltip with ETA/distance, to avoid growing the
  already-large frontier JSON payload (see "Known limitation" in the prior frontier-visualization
  spec) with per-point timestamps.
- No new API endpoint — backtracking is done entirely client-side using `parent_idx` data
  added to the existing `/api/forecast/optimal-route` response.

## Backend (`src/routing.rs`)

`run_isochrone` already tracks `IsochronePoint.parent_idx: Option<usize>`, an index into the
previous step's point array, for every candidate point. `IsochroneResult.frontiers` currently
strips this down to `Vec<Vec<(f64, f64)>>` before returning. Change it to preserve the parent
pointer:

```rust
#[derive(Debug, Serialize, Clone, Copy)]
pub struct FrontierPoint {
    pub lat: f64,
    pub lon: f64,
    pub parent_idx: usize,
}
```

- `IsochroneResult.frontiers` becomes `Vec<Vec<FrontierPoint>>`.
- The mapping in `run_isochrone` that currently produces `(p.lat, p.lon)` per point instead
  produces `FrontierPoint { lat: p.lat, lon: p.lon, parent_idx: p.parent_idx.unwrap() }`.
  Every point in `frontiers` (i.e. every point in `isochrones[1..]`) always has
  `parent_idx: Some(_)` — only the seed at `isochrones[0]` (never included in `frontiers`) has
  `None` — so the `unwrap()` is safe by construction.
- Indexing convention: for a point in `frontiers[s]`, `parent_idx` indexes into `frontiers[s-1]`
  when `s > 0`. For `s == 0`, `parent_idx` is always `0` and means "the search origin" (there is
  exactly one seed point, at index 0 of the internal `isochrones[0]`, which is never exposed as
  its own frontier entry).
- No change to `MAX_STEPS`, `prune_isochrone`, stagnation detection, or any other search
  behavior — this purely widens the data already computed and already exposed.

## API (`src/web/api.rs`)

`OptimalRouteResponse.frontiers: Vec<Vec<(f64, f64)>>` becomes `Vec<Vec<FrontierPoint>>`. Since
`FrontierPoint` derives `Serialize`, this is a mechanical type change — no handler logic changes.
The JSON shape for each frontier point changes from `[lat, lon]` to `{ "lat", "lon", "parent_idx" }`.
This is a breaking change to `/api/forecast/optimal-route`'s response shape (already broken once
by the original frontier feature) — `/api/forecast/route` is untouched.

## Frontend (`static/plan.html`)

**Data storage**: new module-level `lastFrontiers = []`, storing the raw `frontiers` array
(now point-with-parent_idx) exactly as received, alongside the existing `lastRouteOverlay`
pattern. Also track the route origin (`routeWaypoints[0]`) for backtracking to terminate at.
`lastFrontiers` is reset to `[]` everywhere `frontierLines` is currently cleared: `clearRoute()`,
`computeRoute()`, and before redraw in `optimizeRoute()`.

**Index-preserving draw**: `drawFrontiers()` currently clips each frontier to forecast-area runs,
discarding each surviving point's position within `frontiers[stepIdx]`. Change the run-building
loop to also track a parallel `ptIdxs` array (the original index within `frontiers[stepIdx]` for
each surviving point), and pass `stepIdx` and `ptIdxs` into `drawFrontierRun` alongside the
existing `latlngs` array.

**Hover wiring**: `drawFrontierRun(latlngs, stepIdx, ptIdxs)` attaches to the polyline it creates:
- `mousemove` — find the index in `latlngs` nearest the event's `e.latlng` (squared lat/lon
  distance is sufficient given point density up to `SECTOR_COUNT` = 180/step), map it through
  `ptIdxs[nearestIdx]` to get the real index into `lastFrontiers[stepIdx]`, then call
  `backtrackFrontierPath(stepIdx, ptIdx)`.
- `mouseout` — remove the highlight layer from the map.

**Backtracking** (`backtrackFrontierPath(stepIdx, ptIdx)`): walks `lastFrontiers` from
`(stepIdx, ptIdx)` down through each point's `parent_idx` to step 0, then appends the route
origin, reversing to produce an origin-to-point path:

```javascript
function backtrackFrontierPath(stepIdx, ptIdx) {
    const path = [];
    let s = stepIdx, i = ptIdx;
    while (s >= 0) {
        const pt = lastFrontiers[s][i];
        path.push([pt.lat, pt.lon]);
        i = pt.parent_idx;
        s -= 1;
    }
    path.push([lastOptimizeOrigin.lat, lastOptimizeOrigin.lng]);
    path.reverse();
    return path;
}
```

**Highlight rendering**: a single reused module-level layer `alternativeLine` (not part of
`frontierLines`, so it isn't touched by frontier clear/redraw logic). On `mousemove`, remove any
existing `alternativeLine` and draw a new one from `backtrackFrontierPath()`'s result, styled to
stand out from both the wind-colored route segments and the muted `#444` frontier lines:
`{ color: '#ff8c00', weight: 2, dashArray: '4,4', opacity: 0.9 }`. On `mouseout`, remove it.

**Cleanup**: `alternativeLine` is removed/reset in the same places `frontierLines` is cleared
(`clearRoute()`, `computeRoute()`, before redraw in `optimizeRoute()`), so a stale highlight can't
survive a route recompute or clear.

## Testing

- Rust: extend the existing frontier test in `routing.rs` (or add a new one) to assert that
  every `FrontierPoint` in `result.frontiers` has a `parent_idx` valid for the previous step
  (`< frontiers[s-1].len()` for `s > 0`, or `== 0` for `s == 0`), and that repeatedly following
  `parent_idx` from an arbitrary point in the last frontier terminates at step 0 index 0 within
  `frontiers.len()` hops.
- Manual (no JS test suite exists for `plan.html`, consistent with prior frontier work): run
  "Optimize" on a real route and confirm:
  - Hovering over a frontier line shows an orange dashed path from the route origin to the
    hovered point.
  - The highlighted path updates smoothly as the mouse moves along the frontier line.
  - The highlight disappears on mouseout.
  - The highlight does not linger after a route Clear or a second Optimize run.
  - Hovering over a frontier line that was split into multiple forecast-area-clipped runs still
    backtracks correctly (index mapping through `ptIdxs` survives clipping).

## Known limitation

Adding `parent_idx` roughly grows the already-documented large frontier JSON payload (up to 336
steps × up to 180 points) by one integer per point. This is accepted as consistent with the
existing "every step, no decimation" tradeoff; the same future decimation mitigation noted in the
original frontier-visualization spec would apply here too if payload size proves a problem in
practice.
