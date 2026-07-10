# Isochrone Frontier Darker Styling + Forecast-Area Clipping Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the isochrone search frontier polylines on the `plan.html` map darker/more visible, and restrict them to only the portions inside a configured forecast area.

**Architecture:** Extract the existing point-in-forecast-area bounding-box test (already used by `showWindPopupAt`) into a shared helper, then rewrite `drawFrontiers()` to darken its line style and split each frontier into contiguous in-area runs before drawing.

**Tech Stack:** vanilla JS + Leaflet (`static/plan.html`). No backend changes.

## Global Constraints

- Frontend must be HTML + vanilla JavaScript (CLAUDE.md).
- Do NOT run `git commit` or `git push` — per this repo's CLAUDE.md, stop after code changes; the user commits.
- No backend changes — `IsochroneResult.frontiers` and `/api/forecast/optimal-route`'s response shape are unchanged.
- Forecast areas are rectangular lat/lon boxes (`lat_min`, `lat_max`, `lon_min`, `lon_max`) held client-side in `planAreas`.
- If `planAreas` is empty, no frontier lines draw at all — this is the intended behavior, not a bug to work around.

---

### Task 1: Darken frontier styling and clip to forecast areas

**Files:**
- Modify: `static/plan.html:585-589` (`showWindPopupAt`), `static/plan.html:888-900` (`drawFrontiers`)

**Interfaces:**
- Produces: `pointInArea(lat, lon, area)` — pure function, returns `true` if `(lat, lon)` falls inside the given `ForecastArea`-shaped box (`{lat_min, lat_max, lon_min, lon_max}`). `pointInAnyArea(lat, lon)` — returns `true` if `(lat, lon)` falls inside any box in `planAreas`.
- Consumes: `planAreas` (existing module-level array, populated by `syncWindLayers()` at `static/plan.html:1318`), `frontierLines` / `drawFrontiers(frontiers)` (from the prior isochrone-frontier-visualization feature).

- [ ] **Step 1: Add `pointInArea` and `pointInAnyArea` helpers, and use them in `showWindPopupAt`**

In `static/plan.html`, change (current lines 585-589):

```javascript
        function showWindPopupAt(latlng) {
            const area = planAreas.find(a =>
                latlng.lat >= a.lat_min && latlng.lat <= a.lat_max &&
                latlng.lng >= a.lon_min && latlng.lng <= a.lon_max);
            if (!area) return;
```

to:

```javascript
        function pointInArea(lat, lon, area) {
            return lat >= area.lat_min && lat <= area.lat_max &&
                   lon >= area.lon_min && lon <= area.lon_max;
        }

        function pointInAnyArea(lat, lon) {
            return planAreas.some(a => pointInArea(lat, lon, a));
        }

        function showWindPopupAt(latlng) {
            const area = planAreas.find(a => pointInArea(latlng.lat, latlng.lng, a));
            if (!area) return;
```

- [ ] **Step 2: Rewrite `drawFrontiers` to darken styling and clip to forecast-area runs**

In `static/plan.html`, change (current lines 888-900):

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

to:

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

- [ ] **Step 3: Verify the file is well-formed**

Run: `node --check <(sed -n '/<script>/,/<\/script>/p' static/plan.html | sed '1d;$d')`
Expected: no output (syntax OK). If `node` rejects the process-substitution syntax in your shell, extract the `<script>...</script>` body to a temp `.js` file first and run `node --check` on that file instead.

There is no build step or automated test suite for this file (plain JS in an HTML page) — this is the only mechanical verification available. The rest is manual browser verification in Step 4.

- [ ] **Step 4: Manually verify in the browser**

Run the project's existing app-launch process (e.g. via the project's `run` skill/process) with a `config.json` that has a polar table and at least one forecast area configured covering part of a planned route, plus forecast data fetched for it.

In the browser, open `plan.html`, plan a route whose isochrone search extends both inside and outside that forecast area's bounding box, and click "Optimize".

Expected:
- Frontier lines are visibly darker (`#444` at `opacity: 0.6`) than the previous faint gray.
- Frontier lines only appear inside the forecast area's bounding box — no lines draw over map regions outside all configured areas.
- A frontier that exits and re-enters the area's coverage renders as separate disconnected line segments, not one line jumping across the gap.
- Clicking the map for a wind popup (`showWindPopupAt`) still works exactly as before (popup appears only when clicking inside a forecast area, with correct interpolated wind data).

- [ ] **Step 5: Commit**

```bash
git add static/plan.html
```

(Per this repo's CLAUDE.md, do not run `git commit` — leave the change staged/unstaged for the user to review and commit themselves. Skip the actual commit command.)

---

## Self-Review Notes

- **Spec coverage:** Styling change (Step 2), forecast-area clipping with contiguous-run splitting (Step 2), `pointInArea`/`pointInAnyArea` extraction and reuse in `showWindPopupAt` (Step 1), empty-`planAreas` consequence (implicit in `pointInAnyArea` returning `false` for every point — no separate code path needed, matches spec's "known consequence, not a bug"). All covered by this one task.
- **Placeholder scan:** No TBDs; both code blocks are complete and copy-pasteable.
- **Type consistency:** `pointInArea(lat, lon, area)` and `pointInAnyArea(lat, lon)` signatures are used identically in both call sites (`showWindPopupAt`, `drawFrontiers`). `drawFrontierRun(latlngs)` takes an array of `[lat, lon]` pairs, matching the `run` array built in `drawFrontiers`.
