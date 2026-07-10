# Isochrone Frontier Visualization — Design

## Purpose

The isochrone route optimizer (`/api/forecast/optimal-route`, backed by `run_isochrone` in
`src/routing.rs`) already computes a full set of expanding search frontiers — one per 30-minute
step — but discards all of them except the single backtracked best path. This feature surfaces
those frontiers on the `plan.html` map so the user can see the "wavefront" of reachable positions
at each step, giving visibility into how the router explored and how the chosen route compares to
the surrounding possibility space.

This is visualization only — no change to the routing algorithm's search or pruning behavior.

## Scope

- Applies only to the isochrone-based "Optimize" flow (`/api/forecast/optimal-route`).
- The straight-course "Compute" flow (`/api/forecast/route`, `generate_route_track`) is unaffected;
  it has no frontiers to show.
- Frontiers are shown automatically whenever an optimal-route computation completes — no separate
  opt-in toggle.
- Frontiers are drawn as connected polylines (one per step), not discrete points.
- No decimation: every computed step's frontier is drawn. See "Known limitation" below.

## Backend (`src/routing.rs`)

`run_isochrone` builds `isochrones: Vec<Vec<IsochronePoint>>` internally — one frontier per step.
Each frontier is already produced by `prune_isochrone`, which fills a `Vec<Option<IsochronePoint>>`
indexed by bearing-sector-from-origin and then flattens it — so within a frontier, points are
already in ascending-bearing order from the origin. This means each frontier can be connected into
a polyline directly, without re-sorting.

Changes:

- `IsochroneResult` gains a new field:
  ```rust
  pub struct IsochroneResult {
      pub track: Vec<(f64, f64, DateTime<Utc>)>,
      pub reached_destination: bool,
      pub frontiers: Vec<Vec<(f64, f64)>>,
  }
  ```
- Populated from `isochrones`, skipping the trivial single-point seed frontier (step 0, which is
  just the origin) and stripping down to `(lat, lon)` — the internal `time`, `sailed_hours`, and
  `parent_idx` fields aren't needed for display.
- Populated regardless of `reached_destination` — even a failed search's explored frontier is
  useful to show.
- No change to `MAX_STEPS`, `prune_isochrone`, stagnation detection, or any other search behavior.
  This is purely exposing data that's already computed.

## API (`src/web/api.rs`)

`get_optimal_route` currently returns `ApiResponse<Vec<RouteOverlayPoint>>` (a flat route array as
`data`). Introduce a wrapper response type:

```rust
#[derive(Debug, Serialize)]
pub struct OptimalRouteResponse {
    pub route: Vec<RouteOverlayPoint>,
    pub frontiers: Vec<Vec<(f64, f64)>>,
}
```

- `get_optimal_route`'s return type becomes `Result<Json<ApiResponse<OptimalRouteResponse>>, StatusCode>`.
- The handler wraps the existing `overlay` (currently returned directly) together with
  `result.frontiers` into an `OptimalRouteResponse`, then `ApiResponse::ok(...)`.
- This changes the shape of `/api/forecast/optimal-route`'s `data` field from an array to an
  object (`{ route, frontiers }`). This is a breaking change for that one endpoint only —
  `/api/forecast/route` is untouched.

## Frontend (`static/plan.html`)

- New module-level array `frontierLines = []`, parallel to the existing `routeSegments` /
  `previewLines` arrays. Never written to `localStorage` / `saveRouteState()` — frontiers are
  session-only and won't reappear after a page reload (the final route line still persists as
  today).
- New function `drawFrontiers(frontiers)`:
  - Clears any existing `frontierLines` (remove each from `planMap`, reset the array).
  - For each frontier array with ≥ 2 points, draws one `L.polyline` with muted, uniform styling:
    `color: '#888'`, `weight: 1`, `opacity: 0.35`, no popup binding.
  - Pushes each polyline onto `frontierLines`.
  - Called before `drawRouteLine()` in the optimize flow, so the colored route segments and step
    dots render visually on top of the frontier backdrop.
- `optimizeRoute()` updates:
  - `json.data` is now `{ route, frontiers }` instead of a flat array.
  - The success/empty check (`!json.data?.length`) becomes `!json.data?.route?.length`.
  - Call `drawFrontiers(json.data.frontiers)` then `drawRouteLine(json.data.route)`.
- Any existing "clear the map" paths that reset `routeSegments` / `previewLines` (start of
  `optimizeRoute()`, the route-clear/reset handler) also clear `frontierLines` the same way, so
  frontiers from a previous run don't linger after a new computation or an explicit clear.
- `/api/forecast/route`'s handling (`drawRouteLine(json.data || [])`) is unchanged — that endpoint's
  response shape doesn't change.

## Testing

- Rust: extend `routing.rs`'s test module (or add a new test) asserting that after a successful
  `run_isochrone` call, `result.frontiers` is non-empty, each frontier has at most `SECTOR_COUNT`
  points, and the seed (origin-only) frontier is not included.
- Manual: in `plan.html`, run "Optimize" on a real route and confirm:
  - Frontier polylines fan out beneath the colored route line.
  - Frontiers clear and redraw cleanly across repeated Optimize runs.
  - Frontiers do not reappear after a page reload (only the persisted route line does).

## Known limitation

With "every step, no decimation" as the chosen display behavior, a long multi-day route (up to
336 steps, each up to `SECTOR_COUNT` = 180 points) could produce a multi-MB JSON payload and
thousands of DOM polylines. This is accepted for now; if it proves slow in practice, a future
iteration could decimate frontiers by a fixed time interval or fixed count before serialization.
