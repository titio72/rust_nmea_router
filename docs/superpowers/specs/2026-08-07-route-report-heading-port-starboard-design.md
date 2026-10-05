# Route Report: Heading + Port/Starboard Wind Qualifier — Design

## Purpose

The "Step-by-step" table in the Route Summary panel (`routeReportTableBody` in
[plan.html](../../../static/plan.html), added in
[2026-07-10-route-report-table-design.md](2026-07-10-route-report-table-design.md) and later
corrected in
[2026-07-10-twd-relative-to-heading-design.md](2026-07-10-twd-relative-to-heading-design.md)) shows
boat speed, engine mode, TWS, TWD (heading-relative wind angle), gust, AWA and AWS per step — but
never shows the boat's own heading, and the wind-angle columns give only a magnitude (0-180°) with
no indication of which side of the boat the wind is on. This change adds both.

Scope is the main report only (`routeReportTableBody`); the secondary alternative-route preview
modal (`altRouteModal`, populated by `showFrontierReport`) is explicitly out of scope for this
change.

## Scope

- Backend: `src/forecast.rs` (`RouteTrackPoint`, `RouteOverlayPoint`, `generate_route_track`,
  `compute_route_overlay`), `src/web/api.rs` (`get_optimal_route`'s manual per-point loop).
- Frontend: `static/plan.html` — `renderRouteReportTable` only.
- No change to `src/routing.rs`, `backtrackFrontierReport`, `showFrontierReport`, or
  `selectFrontierRoute` — the alt-route-modal path and frontier-promoted routes are unaffected.
- No change to `compute_twa()` — it already discards the sign we need, so the port/starboard side
  is computed directly from `wind_direction_deg` vs. the new `heading_deg`, not from `compute_twa`'s
  output.

## New field: `heading_deg`

A new `Option<f64>` field on `RouteTrackPoint` and `RouteOverlayPoint`: the boat's course over
ground (bearing, 0-360°) for the leg leading into this point — the same `haversine_heading(...)`
value both call sites already compute locally for their own `twa_deg`/`relative_wind_deg`
derivation, just not currently retained. `None` for the departure point (index 0 — no incoming leg
yet), `Some(bearing)` for every subsequent point.

## Backend: `src/forecast.rs`

- `RouteTrackPoint` gains `pub heading_deg: Option<f64>`.
- `RouteOverlayPoint` gains `pub heading_deg: Option<f64>`.
- `generate_route_track`: the departure-point push (`track.is_empty()` branch) gets
  `heading_deg: None`; the per-step push inside the `loop` gets `heading_deg: Some(bearing)`, using
  the `bearing` local already computed via `haversine_heading` a few lines above.
- `compute_route_overlay`: add `heading_deg: pt.heading_deg` to the constructed
  `RouteOverlayPoint`, alongside the existing `twa_deg`/`relative_wind_deg` copies.

## Backend: `src/web/api.rs`

`get_optimal_route`'s manual per-step loop (~line 1907-1961) already computes `bearing` via
`haversine_heading` at line 1933 for its own `twa_deg`/`relative_wind_deg` derivation. Add
`heading_deg: None` to the `i == 0` `RouteTrackPoint` push (~line 1915-1922), and
`heading_deg: Some(bearing)` to the subsequent push (~line 1953-1960). No other change —
`compute_route_overlay` (shared with the plain Compute path) handles the rest.

## Frontend: `static/plan.html`

- **Table header** (~line 320): add a `<th>Hdg</th>` column, positioned right after `<th>Time</th>`
  (before Speed), since heading is a property of the leg's course, read most naturally alongside
  time.
- **`renderRouteReportTable(pts)`**:
  - New `hdg` value: `p.heading_deg != null ? p.heading_deg.toFixed(0) + '°' : '—'`, inserted as its
    own `<td>` after the time column.
  - Port/starboard qualifier appended to the *existing* `twd` cell (no new column, to keep the table
    from overflowing its narrow 300px-default panel):
    ```js
    let twd = p.relative_wind_deg != null ? p.relative_wind_deg.toFixed(0) + '°' : '—';
    if (p.relative_wind_deg != null && p.wind_direction_deg != null && p.heading_deg != null) {
        const diff = ((p.wind_direction_deg - p.heading_deg) % 360 + 360) % 360;
        twd += diff <= 180 ? ' S' : ' P';
    }
    ```
    `diff <= 180` means the wind's FROM-direction is clockwise of (to the right of) the boat's
    heading — wind on the starboard side (`S`); otherwise it's to the left — port (`P`). Single-letter
    suffix, matching the table's existing terse style (TWS/TWD/AWA/AWS).
  - The qualifier is gated on `relative_wind_deg` being non-null (not just `heading_deg`/
    `wind_direction_deg`) so it never appears on a row where the angle itself reads `—`.
- Frontier-promoted routes (`selectFrontierRoute`) build their `pts` array without a `heading_deg`
  field (isochrone frontier telemetry never tracked a bearing, and that path is out of scope here —
  see Scope). Their rows will render `—` for Hdg and no qualifier suffix on TWD, consistent with
  other fields already `null` on that path (e.g. `wave_height_m`).

## Testing

- Rust: extend `src/forecast.rs` unit tests (e.g. near the existing TWA/relative-wind-deg test
  table) to assert `heading_deg` is `None` on the departure point and `Some(bearing)` matching
  `haversine_heading` on subsequent points, for both `generate_route_track` and the values reaching
  `compute_route_overlay`'s output.
- Manual (no JS test suite for `plan.html`): run Compute or Optimize on a route with at least two
  legs of different course, and confirm:
  - The Hdg column shows a heading per row (blank on the departure row).
  - The TWD cell's P/S suffix flips appropriately when comparing two legs sailing the same absolute
    wind direction on opposite tacks (e.g. beating north with wind from the NE vs. beating
    north-west with wind from the NE should show opposite letters).
  - A route promoted via "Select this route" from the alternative-route modal shows `—` in the Hdg
    column and no P/S suffix (expected, out of scope).
