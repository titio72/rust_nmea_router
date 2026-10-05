# TWD Relative-to-Heading — Design

## Purpose

The step-by-step report tables added in
[2026-07-10-route-report-table-design.md](2026-07-10-route-report-table-design.md) (the inline
"Step-by-step" table in the Route Summary panel, and the pre-existing `altRouteModal` table shown
when clicking an alternative frontier route) both label a column "TWD" but populate it from
`wind_direction_deg`/`wind_dir_deg` — the wind's absolute compass bearing (direction it's blowing
FROM, relative to true north). Per this project's own convention, "TWD" means the angle between
the boat's heading and the wind direction (the value the codebase elsewhere computes via
`compute_twa()` and stores as `twa_deg` for sailing legs). The two tables' TWD columns must show
that heading-relative angle instead.

This is not a pure frontend fix: the existing `twa_deg` field cannot be reused as-is, because it
is `None` whenever the routing engine chose to motor (too little wind, angle too tight, or no wind
data) — a sentinel for "not sailing", not merely "unknown angle". A real relative-wind angle is
computable in the motoring case too (the boat still has a heading), so a new field must be
computed unconditionally, alongside but separate from `twa_deg`.

## Scope

- Backend: `src/forecast.rs` (`RouteTrackPoint`, `RouteOverlayPoint`, `compute_route_overlay`),
  `src/web/api.rs` (`get_optimal_route`'s manual per-point loop), `src/routing.rs`
  (`IsochronePoint`, `FrontierPoint`).
- Frontend: `static/plan.html` — `renderRouteReportTable`, `backtrackFrontierReport`,
  `showFrontierReport`, `selectFrontierRoute`.
- No change to `compute_twa()` itself (`src/forecast.rs`) — it already computes exactly the angle
  needed; it's just not being called at every point today.
- No change to `min_twa_deg` gating, sail/motor decision logic, or `twa_deg`'s existing meaning —
  this is a strictly additive, parallel field.

## New field: `relative_wind_deg`

A new `Option<f64>` field, name-consistent across both structs it's added to, meaning "angle
(0–180°) between the boat's heading and the true wind direction at this point, whenever both are
known — independent of whether the boat is sailing or motoring." `None` only when there's no
heading yet (the departure point) or no wind data at all.

## Backend: `src/forecast.rs`

- `RouteTrackPoint` gains `pub heading_deg: Option<f64>` — the bearing of the leg leading into this
  point. `None` for the departure point (index 0, no incoming leg yet); `Some(bearing)` for every
  other point, where `bearing` is the same `haversine_heading(...)` value `generate_route_track`
  already computes per step (currently used only for `advance_position` and `compute_twa`, not
  stored).
- `RouteOverlayPoint` gains `pub relative_wind_deg: Option<f64>`.
- `compute_route_overlay`: after computing `interp` (the freshly IDW-interpolated forecast for this
  point, already the source of the `wind_direction_deg` field), compute:
  ```rust
  let relative_wind_deg = pt.heading_deg
      .zip(interp.wind_direction_deg)
      .map(|(h, wd)| compute_twa(h, wd));
  ```
  and add it to the constructed `RouteOverlayPoint`. Using `interp.wind_direction_deg` (rather than
  whatever wind sample the routing search used) keeps the displayed TWS/TWD self-consistent — both
  come from the same interpolation the table already shows.

## Backend: `src/web/api.rs`

`get_optimal_route`'s manual per-step loop (~line 1858-1900) already computes `bearing` via
`haversine_heading` for its own `twa_deg` derivation. Add `heading_deg: None` to the `i == 0`
`RouteTrackPoint` push, and `heading_deg: Some(bearing)` to the subsequent push. No other change —
`compute_route_overlay` (shared with the plain Compute path) handles the rest.

## Backend: `src/routing.rs`

- `IsochronePoint` and `FrontierPoint` both gain `relative_wind_deg: Option<f64>` (`pub` on
  `FrontierPoint`, matching its existing fields).
- In the candidate loop (inside `run_isochrone`, where `heading` and `wind` are already in scope),
  compute once per candidate, before the existing sail/motor `match`:
  ```rust
  let relative_wind_deg = wind.map(|(_, wind_dir)| compute_twa(heading, wind_dir));
  ```
  This is well-defined for every candidate that reaches `candidates.push(...)`: candidates are only
  created for headings that either have no wind data at all (`relative_wind_deg = None`, consistent
  with no wind sample) or that already passed the `min_twa_deg` gate inside the existing `match`
  (wind present, so `relative_wind_deg = Some(...)`) — including headings that end up motoring
  because the polar speed was too low, which is exactly the case `twa_deg`-reuse would have gotten
  wrong.
- Store `relative_wind_deg` on the pushed `IsochronePoint`; the seed point gets `None`.
- The `frontiers` mapping (`isochrones[1..]` → `FrontierPoint`) copies `relative_wind_deg` across
  like every other field.

## Frontend: `static/plan.html`

- **`renderRouteReportTable(pts)`**: the `twd` line changes from reading `p.wind_direction_deg` to
  `p.relative_wind_deg`. Since `relative_wind_deg` is naturally `null` at the departure row (no
  `heading_deg` there), the earlier index-based special case (`pts.map((p, i) => ... i === 0 ? '—'
  : ...)`, added as a quick fix before this backend work existed) is removed — the function goes
  back to a plain `pts.map(p => ...)` with a uniform `!= null ? ... : '—'` check for every column,
  same pattern as TWS/Boat Speed.
- **`backtrackFrontierReport(stepIdx, ptIdx)`**: each row gains `relative_wind_deg:
  pt.relative_wind_deg` (read off the `FrontierPoint` at `lastFrontiers[s][i]`, alongside the
  existing `wind_speed_kn`/`wind_dir_deg` reads). The manually-constructed departure row and the
  extrapolated final-destination-leg row (from `frontierDestinationLeg`) both get
  `relative_wind_deg: null` explicitly (neither has a real heading/wind sample).
- **`showFrontierReport(stepIdx, ptIdx)`**: the table body's `twd` computation switches from
  `r.wind_dir_deg` to `r.relative_wind_deg`.
- **`selectFrontierRoute()`**: the `pts` array built for promoting an alternative into the main
  displayed route gains `relative_wind_deg: r.relative_wind_deg` per point (alongside the existing
  `wind_speed_kn`/`wind_direction_deg` copy), so the promoted route's step-by-step report shows the
  correct TWD via the same `renderRouteReportTable` path as a freshly computed/optimized route.

## Consistency note (accepted, not fixed here)

The two tables' TWD values come from different underlying data: the main report's
`relative_wind_deg` is computed from a freshly IDW-interpolated wind sample at overlay-build time;
the alt-route-modal's comes from the single nearest-forecast wind sample used during the isochrone
search itself. This mirrors an existing, already-accepted asymmetry for TWS/wind_speed_kn between
the same two paths (noted as Minor in the prior feature's final review) — not introduced or
worsened by this change.

## Testing

- Rust: extend or add unit tests in `src/forecast.rs` and `src/routing.rs` asserting
  `relative_wind_deg`/`heading_deg` values for known heading/wind-direction pairs (e.g. the existing
  TWA test table at `src/forecast.rs:846-870` — heading north + wind from east ⇒ 90°, etc. — reused
  for the new field), and that both are `None` on the departure/seed point and `None` when no wind
  data is available at a point.
- Manual (no JS test suite for `plan.html`): run Compute, Optimize, and Select-this-route and
  confirm the "TWD" column in both the inline report and the alt-route-modal shows a heading-relative
  angle (0–180°) that changes independently of the wind's compass direction as the route's heading
  changes leg-to-leg — e.g. sailing the same wind on two different legs with different headings
  should show two different TWD values for the same TWS/absolute-wind-direction.
