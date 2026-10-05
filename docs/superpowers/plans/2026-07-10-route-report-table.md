# Route Report Table Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a step-by-step Time/Boat Speed/Engine/TWS/TWD table, inline in the existing
Route Summary panel of `static/plan.html`, showing the currently displayed route — the same
kind of per-step breakdown already shown in the `altRouteModal` popup for candidate
alternative routes, but for whichever route is actually selected.

**Architecture:** Pure frontend change to a single file (`static/plan.html`). No backend, no
API, no new data fetch — the table is rendered from `lastRouteOverlay`'s existing
`RouteTrackPoint[]`, the same array `drawRouteSummary()` already uses for the aggregate
stats and speed chart. A new `renderRouteReportTable(pts)` function builds the table body and
is called from the end of `drawRouteSummary(pts)`, so it inherits that function's existing
show/hide and update lifecycle for free.

**Tech Stack:** Vanilla JavaScript, inline `<style>` CSS, HTML — no build step, no test
framework (this file has no JS test suite; verification is manual in a browser).

## Global Constraints

- Frontend: HTML + vanilla JavaScript only (per project CLAUDE.md).
- No new API endpoint or backend change — this is presentation-only, reusing data already
  sent to the client.
- Follow existing `static/plan.html` UI conventions: theme-aware colors via `var(--text-...)`
  / `var(--bg-...)` / `var(--border-color)` custom properties, not hardcoded colors.
- Do not modify the existing `altRouteModal` markup, CSS, or JS logic — it stays as-is.
- Design source: [docs/superpowers/specs/2026-07-10-route-report-table-design.md](../specs/2026-07-10-route-report-table-design.md).

---

### Task 1: Add step-by-step table to Route Summary panel

**Files:**
- Modify: `static/plan.html` (CSS block near line 89, panel markup near line 296-298,
  `drawRouteSummary` near line 1299-1346)

**Interfaces:**
- Consumes: `pts` — the `RouteTrackPoint[]` array already passed into `drawRouteSummary(pts)`
  (each point has `.lat`, `.lon`, `.timestamp` (ISO string), `.speed_kn` (number|null),
  `.twa_deg` (number|null), `.wind_speed_kn` (number|null), `.wind_direction_deg`
  (number|null) — see existing usage in `buildSegmentPopup` at line 1014 and `drawRouteLine`
  at line 1243).
- Produces: `renderRouteReportTable(pts)` — no return value, writes into DOM node
  `#routeReportTableBody`. Called only from `drawRouteSummary`; no other task depends on it.

- [ ] **Step 1: Add the table CSS**

In `static/plan.html`, find the existing modal CSS block ending at line 89
(`.alt-route-modal-select:hover { background-color: #229954; }`). Immediately after it, add:

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

- [ ] **Step 2: Add the table markup to the Route Summary panel**

Find this block (around line 295-298):

```html
            <!-- Speed chart -->
            <div style="font-size:11px; color:var(--text-secondary); margin-bottom:4px;">Boat speed (kn)</div>
            <div id="speedChartContainer"></div>
        </div>

    </div>
```

Replace it with (adds the new block between `#speedChartContainer` and the panel's closing
`</div>`; the outer `</div>` after the blank line is unchanged):

```html
            <!-- Speed chart -->
            <div style="font-size:11px; color:var(--text-secondary); margin-bottom:4px;">Boat speed (kn)</div>
            <div id="speedChartContainer"></div>

            <!-- Step-by-step table -->
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
        </div>

    </div>
```

- [ ] **Step 3: Add the `renderRouteReportTable` function**

Find the `drawSpeedChart` function's closing brace (ends with `document.getElementById('speedChartContainer').innerHTML = svg;` then `}` — around line 1422-1423, immediately before the `// ── Route persistence ──` comment). Immediately after that closing `}`, add:

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

- [ ] **Step 4: Wire the call into `drawRouteSummary`**

Find this line inside `drawRouteSummary` (around line 1344):

```javascript
            drawSpeedChart(pts, depMs, totalHours);
            panel.style.display = '';
```

Replace it with:

```javascript
            drawSpeedChart(pts, depMs, totalHours);
            renderRouteReportTable(pts);
            panel.style.display = '';
```

- [ ] **Step 5: Manual verification — Compute**

Start the server (`cargo run --release` or the project's existing run method) and open
`plan.html` in a browser. Enter route mode, place at least two waypoints inside a forecast
area, set a departure time and speed, and click "Compute".

Expected: the Route Summary panel shows the existing stats/speed chart as before, and below
the speed chart a new "Step-by-step" table appears with one row per 30-minute step. Confirm:
- Row count matches the number of segments implied by the route duration (duration in hours
  × 2, +1 for the departure row).
- The first row (departure) shows `—` for Boat Speed, Engine, TWS, and TWD.
- Boat Speed/TWS/TWD values in a row match the popup shown when clicking that same point's
  marker on the map (`buildSegmentPopup`).
- Engine column reads "⛵ Sailing" or "⚙ Motoring" consistent with whether that segment on
  the map is drawn dashed (motoring) or solid (sailing).

- [ ] **Step 6: Manual verification — Optimize and Select this route**

With the same waypoints, click "Optimize". Confirm the step-by-step table updates to reflect
the optimized route (different row count/values than the Compute run, matching the new
route's ETA shown in the stats above).

Hover over a frontier line until the alternative-route modal information is available, click
it to open the `altRouteModal`, then click "Select this route". Confirm the step-by-step
table updates again to match the newly selected route (same values as the modal's own table
for that alternative).

- [ ] **Step 7: Manual verification — persistence and long routes**

Reload the page. Confirm the step-by-step table repopulates from the route restored via
`restoreRoute()`/`localStorage` without needing to click Compute/Optimize again.

If practical, compute a long (multi-day) route and confirm the table scrolls internally
(the header stays pinned at the top of the `route-report-table-wrap` container) rather than
extending the page indefinitely.

- [ ] **Step 8: Stop for review**

Per this project's CLAUDE.md, do not run `git add`, `git commit`, or `git push` — leave the
modified `static/plan.html` unstaged for the user to review and commit themselves.
