# Route Report Table — Design

## Purpose

`static/plan.html` already shows a per-step Time/Boat Speed/Engine/TWS/TWD table when the
user clicks a frontier line during "Optimize" (the `altRouteModal` popup, driven by
`showFrontierReport()` / `backtrackFrontierReport()` — see
[2026-07-09-isochrone-alternative-route-hover-design.md](2026-07-09-isochrone-alternative-route-hover-design.md)).
That table only exists for *candidate* alternative routes the user is considering, shown
in a modal. There is no equivalent breakdown for the route that is actually currently
selected/displayed — the existing "Route Summary" panel (`#routeSummary`) only shows
aggregate stats (distance, departure, arrival, duration, sail/motor split, speed chart).

This feature adds the same kind of step-by-step table, inline in the page, for whichever
route is currently drawn.

## Scope

- Applies to `static/plan.html` only. No backend or API changes.
- Reuses data already available in `lastRouteOverlay` (the `RouteTrackPoint[]` passed to
  `drawRouteLine`/`drawRouteSummary`) — no new fetch, no new fields.
- Covers the route currently on screen regardless of how it got there: "Compute", "Optimize",
  or "Select this route" from an alternative-route modal — all three already flow through
  `drawRouteSummary(pts)`.
- Does not touch the existing `altRouteModal` popup or its logic — that remains as-is for
  previewing alternatives before selecting them.

## Placement

The table is added inside the existing `#routeSummary` panel, directly below the speed
chart (`#speedChartContainer`), as a new block. It does not get its own
ETA/Duration header — those are already shown in the stats row at the top of the panel.

## Markup & Styling

New container in the `#routeSummary` panel, after the speed chart div:

```html
<div style="font-size:11px; color:var(--text-secondary); margin-top:14px; margin-bottom:4px;">
    Step-by-step
</div>
<div class="route-report-table-wrap">
    <table>
        <thead>
            <tr><th>Time</th><th>Boat Speed</th><th>Engine</th><th>TWS</th><th>TWD</th></tr>
        </thead>
        <tbody id="routeReportTableBody"></tbody>
    </table>
</div>
```

New CSS, modeled on `.alt-route-modal-table-wrap`/`table`/`th`/`td` but under its own class
(the modal's table rules are scoped under `.alt-route-modal`, a fixed-position overlay
wrapper that shouldn't wrap this inline panel):

```css
.route-report-table-wrap { max-height: 320px; overflow-y: auto; }
.route-report-table-wrap table { width: 100%; border-collapse: collapse; font-size: 13px; }
.route-report-table-wrap th {
    position: sticky; top: 0; background: var(--bg-secondary);
    text-align: left; padding: 6px 10px; color: var(--text-secondary);
    font-size: 11px; text-transform: uppercase; letter-spacing: 0.4px;
    border-bottom: 1px solid var(--border-color);
}
.route-report-table-wrap td {
    padding: 5px 10px; color: var(--text-primary);
    border-bottom: 1px solid var(--border-color);
}
```

The `max-height`/`overflow-y:auto` keeps a long multi-day route's table from stretching the
page indefinitely, with the header staying pinned while scrolling — matching the modal's
existing behavior.

## Rendering Logic

New function `renderRouteReportTable(pts)`, called at the end of `drawRouteSummary(pts)`
alongside the existing `drawSpeedChart(pts, depMs, totalHours)` call. Builds one row per
point in `pts` (same array the rest of the panel already uses), reusing the same
formatting conventions as `showFrontierReport`'s table body render:

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
        const twd = p.wind_direction_deg != null ? p.wind_direction_deg.toFixed(0) + '°' : '—';
        return `<tr><td>${time}</td><td>${spd}</td><td>${eng}</td><td>${tws}</td><td>${twd}</td></tr>`;
    }).join('');
}
```

- Time uses `p.timestamp` (already present on every `RouteTrackPoint`), formatted the same
  way as the modal table.
- Engine status is derived the same way `drawRouteLine` already infers motoring for segment
  dash-styling: `twa_deg === null` (with `speed_kn` present) means motoring, otherwise
  sailing. The departure row (`speed_kn == null`) shows `—`, matching the modal's treatment
  of its own first row.
- TWS/TWD read `wind_speed_kn`/`wind_direction_deg` directly off the point, same fields the
  segment popups (`buildSegmentPopup`) already use.

## Lifecycle

Because rendering happens inside `drawRouteSummary(pts)`, the table automatically:
- Populates after "Compute", "Optimize", or "Select this route" (all three call
  `drawRouteSummary`).
- Is hidden whenever the summary panel itself is hidden, i.e. `pts.length < 2` (the early
  return in `drawRouteSummary` before `panel.style.display = ''`) — no separate show/hide
  wiring needed since it lives inside `#routeSummary`.
- Reflects the same route across page reloads via the existing `restoreRoute()` /
  `saveRouteState()` persistence, since that already round-trips `lastRouteOverlay` through
  `drawRouteLine` → `drawRouteSummary`.

## Testing

Manual only (no JS test suite exists for `plan.html`, consistent with prior frontier work):
- Run "Compute" on a route and confirm the step-by-step table appears below the speed chart
  with one row per 30-minute step, correct Engine/TWS/TWD values matching the map segment
  popups.
- Run "Optimize" and confirm the same table reflects the optimized route.
- Click an alternative frontier route and "Select this route"; confirm the table updates to
  the newly selected route's data.
- Reload the page with a route persisted in `localStorage`; confirm the table repopulates
  from the restored route.
- Confirm a long (multi-day) route's table scrolls internally (sticky header) rather than
  growing the page without bound.
