# Minimum True Wind Angle (TWA) Constraint — Design

## Purpose

Add a user-configurable minimum true wind angle (TWA) constraint to the sailing route
calculators. Currently the only gate on "is this heading sailable" is `min_sail_speed_kn`
(motor if the polar-derived speed is too slow) plus whatever minimum upwind angle the polar
table itself encodes. This adds an independent, user-set floor: don't sail closer to the wind
than `min_twa_deg`, regardless of what the polar table would otherwise report — e.g. a boat that
technically polars well at 45° TWA might still be undesirable to sail that close-hauled in
practice (pointing/comfort/leeway margin), so the user can require at least 60° (the default)
before the router will consider a heading "sailing" rather than "motoring."

This only restricts the close-hauled/upwind side of the wind. `compute_twa` (`src/forecast.rs:138`)
returns TWA in 0–180°, where 0° = wind dead ahead and 180° = wind dead astern; this constraint
rejects headings with `TWA < min_twa_deg` and leaves the downwind side (large TWA, near 180°)
completely unrestricted.

## Scope

Three call sites need the same gate, mirroring the three existing sites that already apply
`min_sail_speed_kn`:

1. `run_isochrone` (`src/routing.rs:94-113`) — the isochrone search's per-heading candidate loop.
2. `generate_route_track` (`src/forecast.rs:393-408`) — the straight-course "Compute" simulator.
3. `get_optimal_route`'s post-hoc per-point TWA recomputation (`src/web/api.rs:1762-1769`) — this
   reconstructs speed/TWA for display from the backtracked track after the search completes, and
   must apply the identical filter to stay consistent with what the search itself allowed.

No changes to `src/polars.rs` — the polar table itself is untouched; this is a gate applied at the
call sites, not a change to what the polar table reports.

## Behavior

At each of the three call sites, wherever a candidate heading/bearing's TWA is computed and then
checked against the polar table, add: if `twa < min_twa_deg`, treat the heading as unsailable
(motor at `motoring_speed_kn`) without even consulting the polar table for that heading — same
outcome as today's "polar returned no speed for this TWA" case, just gated earlier and by a
user-set value instead of (or in addition to) whatever the polar table's own minimum angle is.

This does not change: what counts as a valid destination-arrival heading in `run_isochrone`
(arrival still requires a genuinely sailable — or motored — candidate, per existing logic), nor
`prune_isochrone`'s pruning/scoring, nor `MAX_STEPS`/stagnation detection.

## API

New field on both query structs (`src/web/api.rs:252-284`):

```rust
/// Motor instead of sail when the true wind angle is tighter (closer to the wind) than this,
/// regardless of what the polar table would otherwise report. Default 60°.
#[serde(default = "default_min_twa_deg")]
pub min_twa_deg: f64,
```
```rust
fn default_min_twa_deg() -> f64 { 60.0 }
```

Added to `ForecastRouteQuery` and `OptimalRouteQuery`. Both `get_forecast_route` and
`get_optimal_route` validate `(0.0..=180.0).contains(&params.min_twa_deg)`, returning
`ApiResponse::error(...)` otherwise — same style as the existing `sail_weight_kn >= 0.0` check in
`get_optimal_route`.

## Frontend (`static/plan.html`)

New input alongside the existing `minSailInput`/`efficiencyInput`/`sailWeightInput` group
(`static/plan.html:125-130`), same markup style:

```html
<label style="color:var(--text-secondary);" title="Motor instead of sail when the wind is tighter than this off the bow">Min wind angle:
    <input type="number" id="minTwaInput" value="60" min="0" max="180" step="1"
        style="width:52px; margin-left:6px; background:var(--bg-secondary);
               border:1px solid var(--border-color); border-radius:4px;
               padding:3px 8px; color:var(--text-primary); font-size:13px;">°
</label>
```

- Persisted to `localStorage` under `plan_min_twa_deg`, same pattern as `plan_min_sail_speed` /
  `plan_polar_efficiency`.
- Read and sent as `min_twa_deg` on both the "Compute" (`computeRoute()`) and "Optimize"
  (`optimizeRoute()`) API calls — both endpoints need it, matching the backend scope above.

## Testing

**Existing tests need updating, not behavior changes.** `run_isochrone` and `generate_route_track`
gain a new required parameter, so every existing direct call site in the test modules needs the
new argument added:
- `src/routing.rs`: 5 existing `run_isochrone(...)` calls (lines 288, 306, 390, 448, 484).
- `src/forecast.rs`: 7 existing `generate_route_track(...)` calls (lines 724, 738, 753, 786, 788,
  796, 888, 900).

Each existing call passes `0.0` for the new parameter (no restriction), preserving each test's
original intent exactly — including `test_arrival_prefers_tacking_over_forced_motor`
(`src/routing.rs:414`), which specifically exercises tacking at TWA below what a `min_twa_deg` of
60 would allow, and must keep doing so unrestricted.

**New tests**, one per call site with logic (not the API layer, which has no direct test
precedent per the existing codebase convention):
- `src/routing.rs`: a test where a heading that the polar would happily sail (e.g. TWA 45°) is
  rejected once `min_twa_deg` excludes it, forcing the router to fall back to motoring for that
  candidate — verifying the search's resulting behavior changes accordingly (e.g. an arrival that
  was reached by tacking at 45° with `min_twa_deg = 0.0` no longer reaches the same way, or is
  slower/motors instead, once `min_twa_deg = 60.0`).
- `src/forecast.rs`: a test where a fixed leg bearing yields TWA below `min_twa_deg`, asserting
  the resulting `RouteTrackPoint` has `twa_deg: None` and `speed_kn` equal to `motoring_speed_kn`
  even though the polar table would otherwise report a valid sailing speed for that TWA.

No new tests for `get_optimal_route`'s post-hoc recomputation in `api.rs` — consistent with this
codebase's existing convention that API handler tests are DB-integration `#[ignore]` tests, and
this recomputation reuses the same `compute_twa` + polar-lookup pattern already covered by the
`routing.rs`/`forecast.rs` unit tests.
