# Split plan.html into Navigation Areas + Planning pages Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split `static/plan.html` into `static/navigation-areas.html` (area definition/management) and a trimmed, full-width `static/plan.html` (route planning), each gated to the full (non-read-only) version of the app.

**Architecture:** Pure static-file reorganization. No backend changes — `/api/forecast/*` routes are already fully gated server-side when `read_only: true` ([src/web/api.rs:1970-1977](../../../src/web/api.rs#L1970-L1977)). The area-management panel and its JS move to a new standard-layout page; the routing/wind-map JS stays in `plan.html`, which switches to a full-viewport-width layout and gains its own lightweight area loader plus a duplicated refresh control. `shared-theme.js`'s nav bar gets two links instead of one.

**Tech Stack:** Vanilla JS, Leaflet, HTML/CSS. No build step — files are edited directly and served via `ServeDir`.

## Global Constraints

- Backend: Rust only (unchanged in this plan — no backend files touched). Frontend: HTML + vanilla JavaScript.
- No new shared JS module: per the design's Non-goals, the refresh/poll functions are duplicated on both new/trimmed pages, matching this codebase's existing per-page-self-contained convention.
- `snake_case` for JS functions/variables (existing file convention).
- Pages that need theme support load `shared-theme.js` and `shared.css`, use `id="headerContainer"` + `createHeaderBar(...)`, and call `initializeTheme()`.
- No git commit/push — per this project's CLAUDE.md, stop after writing code for user review.

---

## Reference: full current file

`static/plan.html` (pre-change) is 1805 lines. Exact line ranges cited below refer to that file as it exists before Task 1 begins. Re-fetch line numbers by reading the file fresh before editing if ranges drift (e.g. after Task 1's own edits shift later line numbers within the same task).

---

### Task 1: Create `static/navigation-areas.html`

**Files:**
- Create: `static/navigation-areas.html`

**Interfaces:**
- Consumes: `createHeaderBar`, `initializeTheme`, `fetchUiMode` from `/js/shared-theme.js` (unchanged by this task — Task 3 updates `createHeaderBar`'s nav items, but the function signature and these three globals are untouched).
- Produces: none consumed by later tasks — `plan.html` (Task 4) independently re-implements its own copies of `loadForecastAreas`-equivalent/`refreshForecast`/`updateForecastStatus`, it does not call into this file.

- [ ] **Step 1: Write the new file**

Create `static/navigation-areas.html` with this exact content:

```html
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Navigation Areas - NMEA Router</title>
    <link rel="icon" type="image/png" href="/images/nmeasail.png">
    <link rel="stylesheet" href="/shared.css">
    <link rel="stylesheet" href="/libs/leaflet.min.css" />
    <script src="/js/shared-theme.js?v=3"></script>
    <script src="/libs/leaflet.min.js"></script>
</head>
<body>
    <div id="headerContainer"></div>

    <div style="max-width:1500px; margin:0 auto; padding:20px;">

        <!-- Navigation Areas management -->
        <div class="level-1-container" style="padding:14px 20px;">
            <div style="display:flex; justify-content:space-between; align-items:center; margin-bottom:12px;">
                <h2 style="font-size:16px; font-weight:bold; color:var(--text-bold); margin:0;">
                    Navigation Areas
                </h2>
                <div style="display:flex; align-items:center; gap:14px;">
                    <span id="forecastPollerStatus" style="font-size:12px; color:var(--text-secondary);"></span>
                    <button id="forecastRefreshBtn" onclick="refreshForecast()"
                        style="padding:5px 14px; background:var(--bg-secondary); color:var(--text-secondary);
                               border:1px solid var(--border-color); border-radius:4px; cursor:pointer; font-size:13px;">
                        ↻ Refresh
                    </button>
                </div>
            </div>
            <div style="display:grid; grid-template-columns:1fr 1fr; gap:20px;">
                <div>
                    <div id="forecastAreaList" style="margin-bottom:12px;"></div>
                    <button id="drawAreaBtn" onclick="startDrawArea()"
                        style="padding:6px 14px; background:var(--link-color); color:#fff; border:none;
                               border-radius:4px; cursor:pointer; font-size:13px;">
                        Draw Area
                    </button>
                    <button id="cancelDrawBtn" onclick="cancelDrawArea()"
                        style="display:none; padding:6px 14px; background:#e74c3c; color:#fff; border:none;
                               border-radius:4px; cursor:pointer; font-size:13px; margin-left:8px;">
                        Cancel
                    </button>
                    <span id="drawAreaHint"
                        style="font-size:12px; color:var(--text-secondary); margin-left:10px; display:none;">
                        Click and drag on the map to draw a bounding box
                    </span>
                </div>
                <div id="forecastAreaMapEl"
                     style="height:200px; border-radius:6px; border:1px solid var(--border-color);"></div>
            </div>
        </div>

    </div>

    <script>
        document.getElementById('headerContainer').innerHTML = createHeaderBar('navigation-areas');
        initializeTheme();

        fetchUiMode().then(readOnly => { if (readOnly) window.location.href = '/'; });

        // ── Navigation Area Management ──────────────────────────────────────────

        let forecastAreaMap = null;
        let forecastAreaRectangles = [];
        let forecastDrawOverlay = null;
        let drawStart = null;
        let drawRect = null;
        let isDrawing = false;
        let forecastStatusInterval = null;

        function initForecastAreaMap() {
            if (forecastAreaMap) { forecastAreaMap.remove(); }
            forecastAreaMap = L.map('forecastAreaMapEl').setView([43.0, 9.0], 5);
            L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
                attribution: '© OpenStreetMap', maxZoom: 16
            }).addTo(forecastAreaMap);
            forecastDrawOverlay = document.createElement('div');
            forecastDrawOverlay.style.cssText =
                'position:absolute;inset:0;z-index:10000;display:none;cursor:crosshair;';
            forecastAreaMap.getContainer().appendChild(forecastDrawOverlay);
            forecastDrawOverlay.addEventListener('mousedown', onAreaMapMouseDown);
        }

        function startDrawArea() {
            isDrawing = true;
            document.getElementById('cancelDrawBtn').style.display = '';
            document.getElementById('drawAreaBtn').style.display = 'none';
            document.getElementById('drawAreaHint').style.display = '';
            forecastDrawOverlay.style.display = 'block';
        }

        function cancelDrawArea() {
            isDrawing = false;
            drawStart = null;
            if (drawRect) { forecastAreaMap.removeLayer(drawRect); drawRect = null; }
            document.getElementById('cancelDrawBtn').style.display = 'none';
            document.getElementById('drawAreaBtn').style.display = '';
            document.getElementById('drawAreaHint').style.display = 'none';
            forecastDrawOverlay.style.display = 'none';
        }

        function overlayLatLng(e) {
            const r = forecastDrawOverlay.getBoundingClientRect();
            return forecastAreaMap.containerPointToLatLng(L.point(e.clientX - r.left, e.clientY - r.top));
        }

        function onAreaMapMouseDown(e) {
            e.preventDefault();
            e.stopPropagation();
            drawStart = overlayLatLng(e);
            if (drawRect) { forecastAreaMap.removeLayer(drawRect); }
            drawRect = L.rectangle([drawStart, drawStart], {
                color: '#3b82f6', weight: 2, fillOpacity: 0.15, interactive: false
            }).addTo(forecastAreaMap);
            document.addEventListener('mousemove', onAreaMapMouseMove);
            document.addEventListener('mouseup', onAreaMapMouseUp);
        }

        function onAreaMapMouseMove(e) {
            if (!drawStart || !drawRect) return;
            drawRect.setBounds(L.latLngBounds(drawStart, overlayLatLng(e)));
        }

        async function onAreaMapMouseUp(e) {
            document.removeEventListener('mousemove', onAreaMapMouseMove);
            document.removeEventListener('mouseup', onAreaMapMouseUp);
            if (!drawStart || !drawRect) return;
            const bounds = drawRect.getBounds();
            const lat_min = Math.min(bounds.getSouthWest().lat, bounds.getNorthEast().lat);
            const lat_max = Math.max(bounds.getSouthWest().lat, bounds.getNorthEast().lat);
            const lon_min = Math.min(bounds.getSouthWest().lng, bounds.getNorthEast().lng);
            const lon_max = Math.max(bounds.getSouthWest().lng, bounds.getNorthEast().lng);
            cancelDrawArea();
            if (Math.abs(lat_max - lat_min) < 0.01 || Math.abs(lon_max - lon_min) < 0.01) return;
            try {
                await fetch('/api/forecast/areas', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ lat_min, lat_max, lon_min, lon_max }),
                });
                await loadForecastAreas();
                await updateForecastStatus();
            } catch (err) {
                console.error('Failed to save forecast area', err);
            }
        }

        async function deleteForecastArea(id) {
            try {
                await fetch('/api/forecast/areas?id=' + id, { method: 'DELETE' });
                await loadForecastAreas();
                await updateForecastStatus();
            } catch (err) {
                console.error('Failed to delete forecast area', err);
            }
        }

        function renderForecastAreaList(areas) {
            const container = document.getElementById('forecastAreaList');
            if (!areas.length) {
                container.innerHTML =
                    '<div style="font-size:13px; color:var(--text-secondary);">No areas defined. Draw a bounding box on the map.</div>';
                return;
            }
            container.innerHTML = areas.map(a => `
                <div style="display:flex; justify-content:space-between; align-items:center;
                            background:var(--bg-secondary); padding:6px 10px; border-radius:5px;
                            margin-bottom:5px; font-size:13px;">
                    <span>
                        <strong>Area ${a.id}</strong>
                        <span style="color:var(--text-secondary); font-size:11px; margin-left:8px;">
                            ${a.lat_min.toFixed(2)}–${a.lat_max.toFixed(2)}°N &nbsp;
                            ${a.lon_min.toFixed(2)}–${a.lon_max.toFixed(2)}°E
                        </span>
                    </span>
                    <button onclick="deleteForecastArea(${Number(a.id)})"
                        style="background:transparent; border:1px solid #e74c3c; color:#e74c3c;
                               border-radius:3px; padding:1px 8px; font-size:11px; cursor:pointer;">
                        Delete
                    </button>
                </div>`).join('');
        }

        function renderForecastAreasOnMap(areas) {
            forecastAreaRectangles.forEach(r => forecastAreaMap.removeLayer(r));
            forecastAreaRectangles = [];
            areas.forEach(a => {
                const r = L.rectangle(
                    [[a.lat_min, a.lon_min], [a.lat_max, a.lon_max]],
                    { color: '#3b82f6', weight: 2, fillOpacity: 0.1 }
                ).addTo(forecastAreaMap);
                forecastAreaRectangles.push(r);
            });
            if (areas.length) {
                const bounds = L.latLngBounds(
                    areas.map(a => [[a.lat_min, a.lon_min], [a.lat_max, a.lon_max]]).flat()
                );
                forecastAreaMap.fitBounds(bounds, { padding: [20, 20] });
            }
        }

        async function loadForecastAreas() {
            try {
                const resp = await fetch('/api/forecast/areas');
                const json = await resp.json();
                const areas = json.data || [];
                renderForecastAreaList(areas);
                renderForecastAreasOnMap(areas);
            } catch (err) {
                console.error('Failed to load forecast areas', err);
                renderForecastAreaList([]);
            }
        }

        async function updateForecastStatus() {
            try {
                const resp = await fetch('/api/forecast/status');
                const json = await resp.json();
                const s = json.data;
                if (!s) return;
                const statusEl = document.getElementById('forecastPollerStatus');
                const onlineBadge = s.online
                    ? '<span style="background:#14532d;color:#86efac;padding:2px 8px;border-radius:10px;font-size:11px;">● Fetching every 3h</span>'
                    : '<span style="background:#7c2d12;color:#fdba74;padding:2px 8px;border-radius:10px;font-size:11px;">⚠ Offline — retrying</span>';
                const lastFetch = s.last_fetch
                    ? 'Last fetch: ' + new Date(s.last_fetch).toLocaleTimeString() + ' UTC'
                    : 'No fetch yet';
                const nextFetch = s.next_fetch
                    ? ' · Next: ' + new Date(s.next_fetch).toLocaleTimeString()
                    : '';
                statusEl.innerHTML =
                    onlineBadge + ' <span style="font-size:11px;color:var(--text-secondary);margin-left:8px;">' +
                    lastFetch + nextFetch + ' · ' + s.point_count + ' pts</span>';
            } catch (_) {}
        }

        async function refreshForecast() {
            const btn = document.getElementById('forecastRefreshBtn');
            const orig = btn.textContent;
            btn.disabled = true;
            btn.textContent = '↻ Fetching…';
            try {
                const resp = await fetch('/api/forecast/refresh', { method: 'POST' });
                const json = await resp.json();
                btn.textContent = json.status === 'ok' ? '✓ Done' : '✗ Error';
                setTimeout(() => { btn.textContent = orig; btn.disabled = false; },
                           json.status === 'ok' ? 2000 : 3000);
            } catch (_) {
                btn.textContent = '✗ Error';
                setTimeout(() => { btn.textContent = orig; btn.disabled = false; }, 3000);
            }
            await updateForecastStatus();
        }

        window.addEventListener('load', () => {
            requestAnimationFrame(() => {
                initForecastAreaMap();
                loadForecastAreas();
                updateForecastStatus();
                if (forecastStatusInterval) clearInterval(forecastStatusInterval);
                forecastStatusInterval = setInterval(updateForecastStatus, 60000);
            });
        });
    </script>
</body>
</html>
```

- [ ] **Step 2: Verify the file was written correctly**

Run: `grep -c "function" static/navigation-areas.html`
Expected: a count greater than 0 (confirms the script block landed), and no shell errors opening the file.

Run: `python3 -c "import re,sys; s=open('static/navigation-areas.html').read(); assert s.count('<script>')==1 and s.count('</html>')==1; print('ok')"`
Expected: `ok`

- [ ] **Step 3: Commit**

```bash
git add static/navigation-areas.html
git commit -m "Add navigation-areas.html, extracted from plan.html's area management panel"
```

---

### Task 2: Update `shared-theme.js` nav bar to link both pages

**Files:**
- Modify: `static/js/shared-theme.js:139`

**Interfaces:**
- Consumes: none new.
- Produces: `createHeaderBar('navigation-areas')` and `createHeaderBar('planning')` become valid page-id arguments that highlight the correct active nav link (used by Task 1's file already, and by Task 3/4's edits to `plan.html`).

- [ ] **Step 1: Read the current nav items array**

The current block (`static/js/shared-theme.js` around line 134-142) reads:

```js
    const navItems = [
        { href: '/', label: 'Trips', page: 'trips', roHidden: false },
        { href: '/realtime.html', label: 'Monitor', page: 'monitor', roHidden: true },
        { href: '/ais.html', label: 'AIS', page: 'ais', roHidden: true },
        { href: '/yearly-stats.html', label: 'Stats', page: 'stats', roHidden: false },
        { href: '/plan.html', label: 'Forecast', page: 'forecast', roHidden: true },
        { href: '/signalk-browser.html', label: 'SignalK Browser', page: 'signalk-browser', roHidden: true },
        { href: '/backup.html', label: 'Backup', page: 'backup', roHidden: true }
    ];
```

- [ ] **Step 2: Replace the single 'Forecast' entry with two entries**

Use the Edit tool on `static/js/shared-theme.js` with:

old_string:
```js
        { href: '/plan.html', label: 'Forecast', page: 'forecast', roHidden: true },
```

new_string:
```js
        { href: '/navigation-areas.html', label: 'Navigation Areas', page: 'navigation-areas', roHidden: true },
        { href: '/plan.html', label: 'Planning', page: 'planning', roHidden: true },
```

- [ ] **Step 3: Verify the edit**

Run: `grep -n "Navigation Areas\|label: 'Planning'" static/js/shared-theme.js`
Expected output includes both new lines, e.g.:
```
139:        { href: '/navigation-areas.html', label: 'Navigation Areas', page: 'navigation-areas', roHidden: true },
140:        { href: '/plan.html', label: 'Planning', page: 'planning', roHidden: true },
```

- [ ] **Step 4: Commit**

```bash
git add static/js/shared-theme.js
git commit -m "Split Forecast nav link into Navigation Areas and Planning"
```

---

### Task 3: Trim `plan.html` — remove area-management panel, switch to full-width layout, add read-only redirect

**Files:**
- Modify: `static/plan.html`

**Interfaces:**
- Consumes: `createHeaderBar`, `initializeTheme`, `fetchUiMode` from `/js/shared-theme.js` (Task 2's edit doesn't change these signatures).
- Produces: leaves the `planAreas`, `windLayers`, `syncWindLayers` globals and `loadGridPoints`/`loadAvailableDays` functions intact and unchanged in place, for Task 4 to build on. Also produces the `.container` CSS class and full-width `<div class="container">` wrapper that Task 4's new refresh button lives inside.

- [ ] **Step 1: Update the page title**

Edit `static/plan.html`:

old_string:
```html
    <title>Forecast & Planning - NMEA Router</title>
```

new_string:
```html
    <title>Planning - NMEA Router</title>
```

- [ ] **Step 2: Add the full-width `.container` CSS rule**

Edit `static/plan.html`, adding the rule at the top of the existing `<style>` block:

old_string:
```html
    <style>
        #planMap { height: calc(100vh - 280px); min-height: 380px; border-radius: 6px; }
```

new_string:
```html
    <style>
        .container { max-width: 100%; margin: 0; padding: 0 20px 20px; }
        #planMap { height: calc(100vh - 280px); min-height: 380px; border-radius: 6px; }
```

- [ ] **Step 3: Remove the Forecast Areas management panel and switch the outer wrapper to full width**

Edit `static/plan.html`:

old_string:
```html
    <div style="max-width:1500px; margin:0 auto; padding:20px;">

        <!-- Forecast Areas management -->
        <div class="level-1-container" style="margin-bottom:10px; padding:14px 20px;">
            <div style="display:flex; justify-content:space-between; align-items:center; margin-bottom:12px;">
                <h2 onclick="toggleForecastAreas()"
                    style="font-size:16px; font-weight:bold; color:var(--text-bold); margin:0;
                           cursor:pointer; user-select:none; display:flex; align-items:center; gap:8px;">
                    <span id="forecastAreasChevron" style="font-size:12px;">▾</span>Forecast Areas
                </h2>
                <div style="display:flex; align-items:center; gap:14px;">
                    <span id="forecastPollerStatus" style="font-size:12px; color:var(--text-secondary);"></span>
                    <button id="forecastRefreshBtn" onclick="refreshForecast()"
                        style="padding:5px 14px; background:var(--bg-secondary); color:var(--text-secondary);
                               border:1px solid var(--border-color); border-radius:4px; cursor:pointer; font-size:13px;">
                        ↻ Refresh
                    </button>
                </div>
            </div>
            <div id="forecastAreasBody" style="display:grid; grid-template-columns:1fr 1fr; gap:20px;">
                <div>
                    <div id="forecastAreaList" style="margin-bottom:12px;"></div>
                    <button id="drawAreaBtn" onclick="startDrawArea()"
                        style="padding:6px 14px; background:var(--link-color); color:#fff; border:none;
                               border-radius:4px; cursor:pointer; font-size:13px;">
                        Draw Area
                    </button>
                    <button id="cancelDrawBtn" onclick="cancelDrawArea()"
                        style="display:none; padding:6px 14px; background:#e74c3c; color:#fff; border:none;
                               border-radius:4px; cursor:pointer; font-size:13px; margin-left:8px;">
                        Cancel
                    </button>
                    <span id="drawAreaHint"
                        style="font-size:12px; color:var(--text-secondary); margin-left:10px; display:none;">
                        Click and drag on the map to draw a bounding box
                    </span>
                </div>
                <div id="forecastAreaMapEl"
                     style="height:200px; border-radius:6px; border:1px solid var(--border-color);"></div>
            </div>
        </div>

        <!-- Time scrubber -->
```

new_string:
```html
    <div class="container">

        <!-- Time scrubber -->
```

- [ ] **Step 4: Add the Refresh button + status badge to the time-scrubber panel**

Edit `static/plan.html`:

old_string:
```html
                <div id="dayTabsContainer" style="display:flex; gap:5px; flex-wrap:wrap;"></div>
                <div style="display:flex; gap:8px; align-items:center;">
                    <button id="gustToggleBtn" onclick="toggleGustMode()"
```

new_string:
```html
                <div id="dayTabsContainer" style="display:flex; gap:5px; flex-wrap:wrap;"></div>
                <div style="display:flex; gap:8px; align-items:center;">
                    <span id="forecastPollerStatus" style="font-size:12px; color:var(--text-secondary);"></span>
                    <button id="forecastRefreshBtn" onclick="refreshForecast()"
                        style="padding:5px 14px; background:var(--bg-secondary); color:var(--text-secondary);
                               border:1px solid var(--border-color); border-radius:4px; cursor:pointer; font-size:13px;">
                        ↻ Refresh
                    </button>
                    <button id="gustToggleBtn" onclick="toggleGustMode()"
```

- [ ] **Step 5: Update the header bar call and add the read-only redirect**

Edit `static/plan.html`:

old_string:
```html
    <script>
        document.getElementById('headerContainer').innerHTML = createHeaderBar('forecast');
        initializeTheme();
```

new_string:
```html
    <script>
        document.getElementById('headerContainer').innerHTML = createHeaderBar('planning');
        initializeTheme();

        fetchUiMode().then(readOnly => { if (readOnly) window.location.href = '/'; });
```

- [ ] **Step 6: Verify the panel is gone and the page still parses**

Run: `grep -n "Forecast Areas\|forecastAreaMapEl\|toggleForecastAreas" static/plan.html`
Expected: no output (all removed from the body; note `forecastAreaMapEl` no longer exists anywhere in this file — Task 4 will also remove the JS functions that reference it).

Run: `grep -n "createHeaderBar('planning')\|class=\"container\"\|forecastRefreshBtn" static/plan.html`
Expected: three matches — the header call, the wrapper div, and the new button (the button `id` will appear once at this point, since Task 4 hasn't yet removed the old management-panel JS that also references `forecastRefreshBtn`/`forecastPollerStatus` by id — that's fine, it'll be reconciled in Task 4).

- [ ] **Step 7: Commit**

```bash
git add static/plan.html
git commit -m "Trim plan.html: remove area management panel, switch to full-width layout"
```

---

### Task 4: Replace `plan.html`'s area-management JS with a lightweight area loader + duplicated refresh/poll

**Files:**
- Modify: `static/plan.html`

**Interfaces:**
- Consumes: `planAreas` (declared `let planAreas = [];` near the top of the script, unchanged by this plan), `windLayers` (a `Map`, unchanged), `WindParticleLayer` (from `/js/wind-particle-layer.js`, unchanged), `planMap` (created in `init()`, unchanged), `loadAvailableDays()` (unchanged function defined earlier in the file).
- Produces: `loadPlanAreas()`, `updateForecastStatus()`, `refreshForecast()` — all called only from within this file's own `window.addEventListener('load', ...)` and the new Refresh button's `onclick`. Nothing outside `plan.html` depends on these.

- [ ] **Step 1: Read the current end-of-file JS block to confirm exact boundaries**

Run: `grep -n "Forecast Areas collapse\|Forecast Area Management\|window.addEventListener('load'" static/plan.html`

Expected output (line numbers may have shifted slightly from Task 3's edits, but the three markers should each appear once):
```
<N1>:        // ── Forecast Areas collapse ───────────────────────────────────────────────────
<N2>:        // ── Forecast Area Management ──────────────────────────────────────────────────
<N3>:        window.addEventListener('load', () => {
```

- [ ] **Step 2: Replace everything from the "Forecast Areas collapse" comment through the end of the file**

Edit `static/plan.html`:

old_string:
```js
        // ── Forecast Areas collapse ───────────────────────────────────────────────────
        function toggleForecastAreas() {
            const body = document.getElementById('forecastAreasBody');
            const chevron = document.getElementById('forecastAreasChevron');
            const collapsed = body.style.display === 'none';
            body.style.display = collapsed ? 'grid' : 'none';
            chevron.textContent = collapsed ? '▾' : '▸';
            localStorage.setItem('forecast_areas_collapsed', collapsed ? '0' : '1');
            if (collapsed && forecastAreaMap) forecastAreaMap.invalidateSize();
        }

        (function restoreForecastAreasCollapse() {
            if (localStorage.getItem('forecast_areas_collapsed') === '1') {
                document.getElementById('forecastAreasBody').style.display = 'none';
                document.getElementById('forecastAreasChevron').textContent = '▸';
            }
        })();

        // ── Forecast Area Management ──────────────────────────────────────────────────

        let forecastAreaMap = null;
        let forecastAreaRectangles = [];
        let forecastDrawOverlay = null;
        let drawStart = null;
        let drawRect = null;
        let isDrawing = false;
        let forecastStatusInterval = null;

        function initForecastAreaMap() {
            if (forecastAreaMap) { forecastAreaMap.remove(); }
            forecastAreaMap = L.map('forecastAreaMapEl').setView([43.0, 9.0], 5);
            L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
                attribution: '© OpenStreetMap', maxZoom: 16
            }).addTo(forecastAreaMap);
            forecastDrawOverlay = document.createElement('div');
            forecastDrawOverlay.style.cssText =
                'position:absolute;inset:0;z-index:10000;display:none;cursor:crosshair;';
            forecastAreaMap.getContainer().appendChild(forecastDrawOverlay);
            forecastDrawOverlay.addEventListener('mousedown', onAreaMapMouseDown);
        }

        function startDrawArea() {
            isDrawing = true;
            document.getElementById('cancelDrawBtn').style.display = '';
            document.getElementById('drawAreaBtn').style.display = 'none';
            document.getElementById('drawAreaHint').style.display = '';
            forecastDrawOverlay.style.display = 'block';
        }

        function cancelDrawArea() {
            isDrawing = false;
            drawStart = null;
            if (drawRect) { forecastAreaMap.removeLayer(drawRect); drawRect = null; }
            document.getElementById('cancelDrawBtn').style.display = 'none';
            document.getElementById('drawAreaBtn').style.display = '';
            document.getElementById('drawAreaHint').style.display = 'none';
            forecastDrawOverlay.style.display = 'none';
        }

        function overlayLatLng(e) {
            const r = forecastDrawOverlay.getBoundingClientRect();
            return forecastAreaMap.containerPointToLatLng(L.point(e.clientX - r.left, e.clientY - r.top));
        }

        function onAreaMapMouseDown(e) {
            e.preventDefault();
            e.stopPropagation();
            drawStart = overlayLatLng(e);
            if (drawRect) { forecastAreaMap.removeLayer(drawRect); }
            drawRect = L.rectangle([drawStart, drawStart], {
                color: '#3b82f6', weight: 2, fillOpacity: 0.15, interactive: false
            }).addTo(forecastAreaMap);
            document.addEventListener('mousemove', onAreaMapMouseMove);
            document.addEventListener('mouseup', onAreaMapMouseUp);
        }

        function onAreaMapMouseMove(e) {
            if (!drawStart || !drawRect) return;
            drawRect.setBounds(L.latLngBounds(drawStart, overlayLatLng(e)));
        }

        async function onAreaMapMouseUp(e) {
            document.removeEventListener('mousemove', onAreaMapMouseMove);
            document.removeEventListener('mouseup', onAreaMapMouseUp);
            if (!drawStart || !drawRect) return;
            const bounds = drawRect.getBounds();
            const lat_min = Math.min(bounds.getSouthWest().lat, bounds.getNorthEast().lat);
            const lat_max = Math.max(bounds.getSouthWest().lat, bounds.getNorthEast().lat);
            const lon_min = Math.min(bounds.getSouthWest().lng, bounds.getNorthEast().lng);
            const lon_max = Math.max(bounds.getSouthWest().lng, bounds.getNorthEast().lng);
            cancelDrawArea();
            if (Math.abs(lat_max - lat_min) < 0.01 || Math.abs(lon_max - lon_min) < 0.01) return;
            try {
                await fetch('/api/forecast/areas', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ lat_min, lat_max, lon_min, lon_max }),
                });
                await loadForecastAreas();
                await updateForecastStatus();
            } catch (err) {
                console.error('Failed to save forecast area', err);
            }
        }

        async function deleteForecastArea(id) {
            try {
                await fetch('/api/forecast/areas?id=' + id, { method: 'DELETE' });
                await loadForecastAreas();
                await updateForecastStatus();
            } catch (err) {
                console.error('Failed to delete forecast area', err);
            }
        }

        function renderForecastAreaList(areas) {
            const container = document.getElementById('forecastAreaList');
            if (!areas.length) {
                container.innerHTML =
                    '<div style="font-size:13px; color:var(--text-secondary);">No areas defined. Draw a bounding box on the map.</div>';
                return;
            }
            container.innerHTML = areas.map(a => `
                <div style="display:flex; justify-content:space-between; align-items:center;
                            background:var(--bg-secondary); padding:6px 10px; border-radius:5px;
                            margin-bottom:5px; font-size:13px;">
                    <span>
                        <strong>Area ${a.id}</strong>
                        <span style="color:var(--text-secondary); font-size:11px; margin-left:8px;">
                            ${a.lat_min.toFixed(2)}–${a.lat_max.toFixed(2)}°N &nbsp;
                            ${a.lon_min.toFixed(2)}–${a.lon_max.toFixed(2)}°E
                        </span>
                    </span>
                    <button onclick="deleteForecastArea(${Number(a.id)})"
                        style="background:transparent; border:1px solid #e74c3c; color:#e74c3c;
                               border-radius:3px; padding:1px 8px; font-size:11px; cursor:pointer;">
                        Delete
                    </button>
                </div>`).join('');
        }

        function renderForecastAreasOnMap(areas) {
            forecastAreaRectangles.forEach(r => forecastAreaMap.removeLayer(r));
            forecastAreaRectangles = [];
            areas.forEach(a => {
                const r = L.rectangle(
                    [[a.lat_min, a.lon_min], [a.lat_max, a.lon_max]],
                    { color: '#3b82f6', weight: 2, fillOpacity: 0.1 }
                ).addTo(forecastAreaMap);
                forecastAreaRectangles.push(r);
            });
            if (areas.length) {
                const bounds = L.latLngBounds(
                    areas.map(a => [[a.lat_min, a.lon_min], [a.lat_max, a.lon_max]]).flat()
                );
                forecastAreaMap.fitBounds(bounds, { padding: [20, 20] });
            }
        }

        // Adds/removes a WindParticleLayer on planMap for each area, keyed by area.id,
        // so existing layers (and their live WebGL contexts) aren't torn down and
        // recreated on every areas refresh — only actual additions/removals are applied.
        function syncWindLayers(areas) {
            planAreas = areas;
            const seen = new Set();
            areas.forEach(area => {
                seen.add(area.id);
                if (!windLayers.has(area.id)) {
                    const layer = new WindParticleLayer(area);
                    windLayers.set(area.id, layer);
                    layer.onAdd(planMap);
                }
            });
            windLayers.forEach((layer, id) => {
                if (!seen.has(id)) {
                    layer.onRemove();
                    windLayers.delete(id);
                }
            });
        }

        async function loadForecastAreas() {
            try {
                const resp = await fetch('/api/forecast/areas');
                const json = await resp.json();
                const areas = json.data || [];
                renderForecastAreaList(areas);
                renderForecastAreasOnMap(areas);
                if (planMap) syncWindLayers(areas);
            } catch (err) {
                console.error('Failed to load forecast areas', err);
                renderForecastAreaList([]);
            }
        }

        async function updateForecastStatus() {
            try {
                const resp = await fetch('/api/forecast/status');
                const json = await resp.json();
                const s = json.data;
                if (!s) return;
                const statusEl = document.getElementById('forecastPollerStatus');
                const onlineBadge = s.online
                    ? '<span style="background:#14532d;color:#86efac;padding:2px 8px;border-radius:10px;font-size:11px;">● Fetching every 3h</span>'
                    : '<span style="background:#7c2d12;color:#fdba74;padding:2px 8px;border-radius:10px;font-size:11px;">⚠ Offline — retrying</span>';
                const lastFetch = s.last_fetch
                    ? 'Last fetch: ' + new Date(s.last_fetch).toLocaleTimeString() + ' UTC'
                    : 'No fetch yet';
                const nextFetch = s.next_fetch
                    ? ' · Next: ' + new Date(s.next_fetch).toLocaleTimeString()
                    : '';
                statusEl.innerHTML =
                    onlineBadge + ' <span style="font-size:11px;color:var(--text-secondary);margin-left:8px;">' +
                    lastFetch + nextFetch + ' · ' + s.point_count + ' pts</span>';
            } catch (_) {}
        }

        async function refreshForecast() {
            const btn = document.getElementById('forecastRefreshBtn');
            const orig = btn.textContent;
            btn.disabled = true;
            btn.textContent = '↻ Fetching…';
            try {
                const resp = await fetch('/api/forecast/refresh', { method: 'POST' });
                const json = await resp.json();
                btn.textContent = json.status === 'ok' ? '✓ Done' : '✗ Error';
                setTimeout(() => { btn.textContent = orig; btn.disabled = false; },
                           json.status === 'ok' ? 2000 : 3000);
            } catch (_) {
                btn.textContent = '✗ Error';
                setTimeout(() => { btn.textContent = orig; btn.disabled = false; }, 3000);
            }
            await updateForecastStatus();
            await loadAvailableDays();
        }

        window.addEventListener('load', () => {
            requestAnimationFrame(() => {
                initForecastAreaMap();
                loadForecastAreas();
                updateForecastStatus();
                if (forecastStatusInterval) clearInterval(forecastStatusInterval);
                forecastStatusInterval = setInterval(updateForecastStatus, 60000);
            });
        });
    </script>
</body>
</html>
```

new_string:
```js
        // ── Navigation areas (wind field data) + forecast refresh ───────────────────

        // Adds/removes a WindParticleLayer on planMap for each area, keyed by area.id,
        // so existing layers (and their live WebGL contexts) aren't torn down and
        // recreated on every areas refresh — only actual additions/removals are applied.
        function syncWindLayers(areas) {
            planAreas = areas;
            const seen = new Set();
            areas.forEach(area => {
                seen.add(area.id);
                if (!windLayers.has(area.id)) {
                    const layer = new WindParticleLayer(area);
                    windLayers.set(area.id, layer);
                    layer.onAdd(planMap);
                }
            });
            windLayers.forEach((layer, id) => {
                if (!seen.has(id)) {
                    layer.onRemove();
                    windLayers.delete(id);
                }
            });
        }

        // Navigation areas are drawn/edited on navigation-areas.html; this page only
        // needs their bounds to build wind layers and for point-in-area lookups.
        async function loadPlanAreas() {
            try {
                const resp = await fetch('/api/forecast/areas');
                const json = await resp.json();
                syncWindLayers(json.data || []);
            } catch (err) {
                console.error('Failed to load forecast areas', err);
            }
        }

        let forecastStatusInterval = null;

        async function updateForecastStatus() {
            try {
                const resp = await fetch('/api/forecast/status');
                const json = await resp.json();
                const s = json.data;
                if (!s) return;
                const statusEl = document.getElementById('forecastPollerStatus');
                const onlineBadge = s.online
                    ? '<span style="background:#14532d;color:#86efac;padding:2px 8px;border-radius:10px;font-size:11px;">● Fetching every 3h</span>'
                    : '<span style="background:#7c2d12;color:#fdba74;padding:2px 8px;border-radius:10px;font-size:11px;">⚠ Offline — retrying</span>';
                const lastFetch = s.last_fetch
                    ? 'Last fetch: ' + new Date(s.last_fetch).toLocaleTimeString() + ' UTC'
                    : 'No fetch yet';
                const nextFetch = s.next_fetch
                    ? ' · Next: ' + new Date(s.next_fetch).toLocaleTimeString()
                    : '';
                statusEl.innerHTML =
                    onlineBadge + ' <span style="font-size:11px;color:var(--text-secondary);margin-left:8px;">' +
                    lastFetch + nextFetch + ' · ' + s.point_count + ' pts</span>';
            } catch (_) {}
        }

        async function refreshForecast() {
            const btn = document.getElementById('forecastRefreshBtn');
            const orig = btn.textContent;
            btn.disabled = true;
            btn.textContent = '↻ Fetching…';
            try {
                const resp = await fetch('/api/forecast/refresh', { method: 'POST' });
                const json = await resp.json();
                btn.textContent = json.status === 'ok' ? '✓ Done' : '✗ Error';
                setTimeout(() => { btn.textContent = orig; btn.disabled = false; },
                           json.status === 'ok' ? 2000 : 3000);
            } catch (_) {
                btn.textContent = '✗ Error';
                setTimeout(() => { btn.textContent = orig; btn.disabled = false; }, 3000);
            }
            await updateForecastStatus();
            await loadAvailableDays();
        }

        window.addEventListener('load', () => {
            requestAnimationFrame(() => {
                loadPlanAreas();
                updateForecastStatus();
                if (forecastStatusInterval) clearInterval(forecastStatusInterval);
                forecastStatusInterval = setInterval(updateForecastStatus, 60000);
            });
        });
    </script>
</body>
</html>
```

- [ ] **Step 3: Verify no leftover references to removed area-management-only symbols**

Run: `grep -n "forecastAreaMap\|initForecastAreaMap\|startDrawArea\|cancelDrawArea\|onAreaMapMouse\|deleteForecastArea\|renderForecastAreaList\|renderForecastAreasOnMap\|loadForecastAreas\b" static/plan.html`
Expected: no output (these symbols now exist only in `navigation-areas.html`; `plan.html` uses `loadPlanAreas()` instead of `loadForecastAreas()`).

Run: `grep -n "function loadPlanAreas\|function syncWindLayers\|function updateForecastStatus\|function refreshForecast\|window.addEventListener('load'" static/plan.html`
Expected: five matches, one per function/listener, each defined exactly once.

- [ ] **Step 4: Commit**

```bash
git add static/plan.html
git commit -m "Replace plan.html's area-management JS with a lightweight area loader + refresh"
```

---

### Task 5: Manual end-to-end verification

**Files:** none (verification only — no file changes).

**Interfaces:** N/A.

- [ ] **Step 1: Build**

Run: `cargo build --release`
Expected: builds successfully (no Rust files were touched, but this confirms the working tree is otherwise healthy before manual testing).

- [ ] **Step 2: Start the app and open navigation-areas.html**

Run the app per this project's `run` skill/normal startup (`./target/release/nmea_router` with a valid `config.json` in the CWD), then in a browser:
- Navigate to `/navigation-areas.html`.
- Confirm the nav bar shows a "Navigation Areas" link (active/highlighted) and a "Planning" link.
- Draw a bounding box on the small map; confirm it appears in the area list.
- Click "↻ Refresh"; confirm the button shows "Fetching…" then "✓ Done" (or "✗ Error" if no network access to the forecast provider — either is acceptable for this check, the point is the request round-trips) and the poller status badge updates.
- Delete the area you drew; confirm it disappears from the list and the map.

- [ ] **Step 3: Open plan.html and confirm it's full-width and functional**

- Navigate to `/plan.html`.
- Confirm the page content spans the full browser width (no longer capped at 1500px centered) and the nav bar's "Planning" link is active.
- Draw a navigation area on `/navigation-areas.html` again (or reuse an existing one), then reload `/plan.html`; confirm wind particles render over that area once forecast data exists.
- Confirm the "↻ Refresh" button now appears top-right of the time-scrubber panel (next to "Show Gust" / "Plan Route") and clicking it updates the status badge.
- Draw a short route (2+ waypoints), click "Done", set a departure time, click "Compute"; confirm the route line and Route Summary panel render as before.

- [ ] **Step 4: Confirm read-only redirect**

- Stop the app, set `"read_only": true` under `"web"` in `config.json` (or `test_config.json` if that's what's running), restart.
- Confirm the "Navigation Areas" and "Planning" nav links are hidden from the header bar on any page.
- Manually navigate to `/navigation-areas.html`; confirm it redirects to `/`.
- Manually navigate to `/plan.html`; confirm it redirects to `/`.
- Revert `read_only` back to `false` (or its original value) and restart.

- [ ] **Step 5: Report results to the user**

Summarize pass/fail for each check in Steps 2-4. Do not commit anything in this task (verification only).

---

## Self-Review Notes

- **Spec coverage:** File structure (Task 1, 3), nav bar split (Task 2), Navigation Areas page content (Task 1), Planning page trim + full-width layout (Task 3), area data reload via `loadPlanAreas`/`syncWindLayers` (Task 4), duplicated refresh/poll on both pages (Task 1 + Task 4), read-only redirect on both pages (Task 1 Step 1 script block + Task 3 Step 5), manual testing plan (Task 5) — all covered.
- **Placeholder scan:** No TBD/TODO markers; every step shows exact code or exact grep/build commands with expected output.
- **Type consistency:** `syncWindLayers(areas)`, `planAreas`, `windLayers`, `planMap`, `loadAvailableDays()` are used identically to their original definitions elsewhere in `plan.html` (untouched by this plan) — confirmed against the pre-change file read in full during design. `loadPlanAreas()` (Task 4) is the one new/renamed function, and it's referenced only within Task 4's own edit and Task 4's `window.addEventListener('load', ...)` — no stale calls to the old `loadForecastAreas()` name remain in `plan.html`.
