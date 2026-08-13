# Fix Engine Status — Expanded Map & Vessel Status Table Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand the map on `static/fix-engine-status.html` to fill the available viewport (matching `static/plan.html`'s full-bleed layout) and add a side table of raw `vessel_status` rows within ±15 minutes of the first selected point, so the second correction point can be picked from the table as well as the map.

**Architecture:** Pure frontend change to one file, `static/fix-engine-status.html`. No backend changes: `GET /api/track?trip_id=<id>` is already called with no `max_points`, so the page's in-memory `trackData` array already holds every raw `vessel_status` row (`decimate()` is a no-op with no cap) — the new table is a client-side filter of data already loaded. The existing two-click selection flow (`onMapClick` → nearest-point snap → mark A → mark B → highlight → selection panel) is factored into a shared `selectPoint(point)` function so both a map click and a table row click drive the exact same logic.

**Tech Stack:** Vanilla JS + Leaflet, no build step, no new dependencies.

## Global Constraints

- Backend: Rust only. Frontend: HTML + vanilla JavaScript (CLAUDE.md). This plan only touches the frontend.
- No unused imports/code, no partial implementations committed to main (CLAUDE.md).
- Per this repo's CLAUDE.md: do not run `git commit` or `git push`, do not stage files — stop after writing code for the user to review. (Every "Commit" step below is written for completeness per the writing-plans template, but per project rules must NOT actually be run unless the user explicitly asks.)
- Per CLAUDE.md's testing rule for UI changes: test the golden path and edge cases in a real browser before calling the task complete — this page has no automated frontend test harness, so browser verification steps replace unit tests in this plan.

---

### Task 1: Full-bleed layout — map fills the viewport, selection panel moves to a sidebar shell

**Files:**
- Modify: `static/fix-engine-status.html` (full-file rewrite of the `<style>` block and `<body>`, targeted edits to the `<script>` block)

**Interfaces:**
- Consumes: nothing new — same `GET /api/trip?id=`, `GET /api/track?trip_id=`, `GET /api/trip_legs?id=` calls already in the file.
- Produces: DOM elements `#mapTableWrap`, `#map`, `#statusPanel`, `#statusTableWrap` (empty in this task, populated in Task 2), relocated `#selectionPanel`/`#statusBar` — Task 2 adds content inside `#statusTableWrap` and depends on this structure existing.

This task does **not** add any vessel-status-table behavior. It only restructures the page so the map fills the available height and the existing selection-panel/status-bar UI still works exactly as before, just repositioned into a right-hand sidebar shell.

- [ ] **Step 1: Rewrite the `<style>` block**

Replace the entire `<style>...</style>` block (current lines 15-97) with:

```html
    <style>
        .fix-engine-topbar {
            max-width: 1500px;
            margin: 0 auto 12px auto;
        }
        .fix-engine-header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            gap: 12px;
        }
        .fix-engine-help {
            color: var(--text-secondary);
            font-size: 13px;
            margin: 8px 0;
        }
        #mapTableWrap {
            display: flex;
            gap: 16px;
            width: 100%;
            min-height: 380px;
        }
        #map {
            flex: 1;
            border-radius: 8px;
        }
        #statusPanel {
            width: 340px;
            flex-shrink: 0;
            display: flex;
            flex-direction: column;
            border: 1px solid var(--border-color);
            border-radius: 8px;
            background: var(--bg-secondary);
            overflow: hidden;
        }
        #statusTableWrap {
            flex: 1;
            overflow-y: auto;
        }
        .selection-panel {
            display: none;
            margin: 12px;
            padding: 12px 14px;
            border-radius: 6px;
            border: 1px solid var(--border-color);
            background: var(--bg-tertiary);
            flex-shrink: 0;
        }
        .selection-panel.visible {
            display: block;
        }
        .selection-range {
            font-size: 13px;
            color: var(--text-primary);
            margin-bottom: 10px;
        }
        .engine-choice {
            display: flex;
            gap: 8px;
            margin-bottom: 12px;
        }
        .engine-choice-btn {
            flex: 1;
            padding: 8px 10px;
            border-radius: 6px;
            border: 1px solid var(--border-color);
            background: var(--bg-tertiary);
            color: var(--text-primary);
            cursor: pointer;
            font-size: 13px;
        }
        .engine-choice-btn.selected {
            background: var(--link-color);
            color: #fff;
            border-color: var(--link-color);
        }
        .selection-actions {
            display: flex;
            gap: 8px;
        }
        .status-bar {
            margin: 0 12px 12px 12px;
            padding: 10px 14px;
            border-radius: 6px;
            font-size: 13px;
            display: none;
            flex-shrink: 0;
        }
        .status-bar.success {
            background: #d4edda;
            color: #155724;
            border: 1px solid #c3e6cb;
        }
        body.dark-theme .status-bar.success {
            background: #1a3d2b;
            color: #7bcea0;
            border-color: #2d6a4f;
        }
        .status-bar.error {
            background: #f8d7da;
            color: #721c24;
            border: 1px solid #f5c6cb;
        }
        body.dark-theme .status-bar.error {
            background: #3d1a1e;
            color: #f1a8ad;
            border-color: #6a2d32;
        }
    </style>
```

(This drops the old fixed `#map { height: 520px; }` in favor of `#map { flex: 1; }` inside a JS-sized `#mapTableWrap`, and re-homes `.selection-panel`/`.status-bar` styling for their new position inside `#statusPanel` — smaller margins since they're now nested in a bordered sidebar instead of sitting directly on the page background.)

- [ ] **Step 2: Rewrite the `<body>` structure**

Replace everything from `<div id="headerContainer"></div>` through the closing `</div>` of `.level-1-container` (current lines 99-129) with:

```html
    <div id="headerContainer"></div>

    <div class="fix-engine-topbar">
        <div class="fix-engine-header">
            <div>
                <h2 id="tripTitle" style="margin: 0;">Fix Engine Status</h2>
                <div id="tripSubtitle" style="color: var(--text-secondary); font-size: 13px;"></div>
            </div>
            <a class="app-btn" id="backLink" href="/trip.html">&larr; Back to Trip</a>
        </div>
        <div class="fix-engine-help">
            Click a start point and an end point on the track, choose Engine ON or OFF, then Apply.
            Grey segments are currently marked engine-on; gold segments are unknown. The panel on the
            right shows vessel status data near your first click — the second point can be picked
            there too.
        </div>
    </div>

    <div id="mapTableWrap">
        <div id="map"></div>
        <div id="statusPanel">
            <div id="statusTableWrap"></div>
            <div class="selection-panel" id="selectionPanel">
                <div class="selection-range" id="selectionRange"></div>
                <div class="engine-choice">
                    <button class="engine-choice-btn" id="choiceOn" onclick="chooseEngineState(true)">Engine ON</button>
                    <button class="engine-choice-btn" id="choiceOff" onclick="chooseEngineState(false)">Engine OFF</button>
                </div>
                <div class="selection-actions">
                    <button class="app-btn" id="applyBtn" onclick="applyCorrection()" disabled>Apply</button>
                    <button class="app-btn" onclick="clearSelection()">Cancel</button>
                </div>
            </div>
            <div id="statusBar" class="status-bar"></div>
        </div>
    </div>
```

- [ ] **Step 3: Add `sizeMapWrap()` and wire it into `init()` and `resize`**

In the `<script>` block, right after the existing state variable declarations (after `let pendingEngineOn = null; // true/false once chosen, null before`), add:

```javascript
        // Sizes #mapTableWrap to fill the viewport below the topbar. Leaflet needs the
        // container to have a real pixel height before L.map() runs, so this is called
        // synchronously before map creation in init(), then again on resize.
        function sizeMapWrap() {
            const wrap = document.getElementById('mapTableWrap');
            if (!wrap) return;
            const top = wrap.getBoundingClientRect().top;
            const height = Math.max(window.innerHeight - top - 20, 380);
            wrap.style.height = height + 'px';
            if (map) map.invalidateSize();
        }
        window.addEventListener('resize', sizeMapWrap);
```

Then, in `init()`, find this block:

```javascript
                map = L.map('map').setView([(minLat + maxLat) / 2, (minLng + maxLng) / 2], 12);
```

and add a call to `sizeMapWrap()` immediately before it, so the container has its final height before Leaflet measures it:

```javascript
                sizeMapWrap();
                map = L.map('map').setView([(minLat + maxLat) / 2, (minLng + maxLng) / 2], 12);
```

- [ ] **Step 4: Verify the app still builds and serves static files**

Run: `cargo build`
Expected: builds with no errors (this is a static asset; confirms the workspace still compiles after the file edit).

- [ ] **Step 5: Manual browser verification**

Run the app (`cargo run` or `./target/release/nmea_router`) against a database with at least one trip. Navigate to `/fix-engine-status.html?id=<a real trip id>` and verify:
- The map fills essentially the full viewport height below the title/help text, with a ~340px sidebar column to its right (empty for now except border/background).
- Resizing the browser window resizes the map (no leftover blank space, no Leaflet grey-tile glitches — `map.invalidateSize()` is firing).
- The existing two-click flow still works exactly as before: click two points, see the highlight and selection panel appear inside the right sidebar (below the empty table area), choose Engine ON/OFF, Apply, Cancel — all unchanged in behavior, just relocated visually.
- The leg-scoped URL variant (`/fix-engine-status.html?id=<id>&leg=<n>`) still loads and titles correctly.

- [ ] **Step 6: Commit**

```bash
git add static/fix-engine-status.html
git commit -m "Expand fix-engine-status map to fill the viewport, add sidebar shell"
```

(Per this repo's CLAUDE.md, only run this commit step if the user has explicitly asked you to commit — otherwise stop after Step 5 and leave the change for review.)

---

### Task 2: Vessel status table — populate on first click, select point B from a row

**Files:**
- Modify: `static/fix-engine-status.html` (adds table CSS, table markup inside `#statusTableWrap`, and the table/selection JS logic)

**Interfaces:**
- Consumes: `trackData` (array of `TrackPoint`-shaped objects: `{timestamp, latitude, longitude, avg_speed_kn, max_speed_kn, moored, engine_on, ...}`, already loaded by `loadTrack()` from Task 1's unchanged code), `segmentColor(point)` (existing function, current lines 167-171), `showSelectionPanel()` (existing function, current lines 235-243), `#statusTableWrap` (empty div from Task 1).
- Produces: `selectPoint(point)` — the single entry point both `onMapClick` and table row clicks call; `renderStatusTable(centerPoint)` / `clearStatusTable()` — used by `selectPoint`/`clearSelection`. Nothing outside this file consumes these.

- [ ] **Step 1: Add table CSS**

In the `<style>` block, right after the `#statusTableWrap { flex: 1; overflow-y: auto; }` rule added in Task 1, add:

```css
        .status-table-placeholder {
            padding: 16px 14px;
            font-size: 13px;
            color: var(--text-secondary);
        }
        #statusTable {
            width: 100%;
            border-collapse: collapse;
            font-size: 12px;
            display: none;
        }
        #statusTable th {
            position: sticky;
            top: 0;
            background: var(--bg-secondary);
            text-align: left;
            padding: 6px 8px;
            color: var(--text-secondary);
            font-size: 10px;
            text-transform: uppercase;
            letter-spacing: 0.3px;
            border-bottom: 1px solid var(--border-color);
        }
        #statusTable td {
            padding: 5px 8px;
            color: var(--text-primary);
            border-bottom: 1px solid var(--border-color);
            cursor: pointer;
            white-space: nowrap;
        }
        #statusTable tbody tr:hover {
            background: var(--bg-hover);
        }
        #statusTable tbody tr.row-selected-a {
            background: color-mix(in srgb, #3388ff 25%, transparent);
        }
        #statusTable tbody tr.row-selected-b {
            background: color-mix(in srgb, #e74c3c 25%, transparent);
        }
        .engine-dot {
            display: inline-block;
            width: 8px;
            height: 8px;
            border-radius: 50%;
            margin-right: 5px;
        }
```

- [ ] **Step 2: Add the table markup**

Replace the (currently empty) `<div id="statusTableWrap"></div>` from Task 1 with:

```html
            <div id="statusTableWrap">
                <div id="statusTablePlaceholder" class="status-table-placeholder">
                    Click a point on the track to see nearby vessel status data.
                </div>
                <table id="statusTable">
                    <thead>
                        <tr><th>Time</th><th>Lat</th><th>Lon</th><th>Spd (kn)</th><th>Engine</th><th>Status</th></tr>
                    </thead>
                    <tbody id="statusTableBody"></tbody>
                </table>
            </div>
```

- [ ] **Step 3: Add the table render/clear functions**

In the `<script>` block, right after `segmentColor(point)` (current lines 167-171), add:

```javascript
        const STATUS_WINDOW_MS = 15 * 60 * 1000;

        function computeStatusWindow(centerTimestamp) {
            const centerMs = new Date(centerTimestamp).getTime();
            return trackData.filter(p => {
                const ms = new Date(p.timestamp).getTime();
                return ms >= centerMs - STATUS_WINDOW_MS && ms <= centerMs + STATUS_WINDOW_MS;
            });
        }

        function engineLabel(engineOn) {
            if (engineOn === 1) return 'On';
            if (engineOn === 2) return 'Unknown';
            return 'Off';
        }

        function formatRowTime(ts) {
            try {
                return new Date(ts).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
            } catch (e) {
                return ts;
            }
        }

        function renderStatusTable(centerPoint) {
            const windowPoints = computeStatusWindow(centerPoint.timestamp);
            const tbody = document.getElementById('statusTableBody');
            tbody.innerHTML = '';
            windowPoints.forEach(p => {
                const tr = document.createElement('tr');
                tr.dataset.timestamp = p.timestamp;
                tr.innerHTML =
                    '<td>' + formatRowTime(p.timestamp) + '</td>' +
                    '<td>' + (p.latitude != null ? p.latitude.toFixed(5) : '—') + '</td>' +
                    '<td>' + (p.longitude != null ? p.longitude.toFixed(5) : '—') + '</td>' +
                    '<td>' + (p.avg_speed_kn != null ? p.avg_speed_kn.toFixed(1) : '—') + '</td>' +
                    '<td><span class="engine-dot" style="background:' + segmentColor(p) + ';"></span>' + engineLabel(p.engine_on) + '</td>' +
                    '<td>' + (p.moored ? 'Moored' : 'Underway') + '</td>';
                tr.addEventListener('click', () => selectPoint(p));
                tbody.appendChild(tr);
            });
            document.getElementById('statusTablePlaceholder').style.display = 'none';
            document.getElementById('statusTable').style.display = '';
            highlightTableRows();
        }

        function clearStatusTable() {
            document.getElementById('statusTableBody').innerHTML = '';
            document.getElementById('statusTable').style.display = 'none';
            document.getElementById('statusTablePlaceholder').style.display = '';
        }

        function highlightTableRows() {
            document.querySelectorAll('#statusTableBody tr').forEach(row => {
                row.classList.remove('row-selected-a', 'row-selected-b');
                if (selectedA && row.dataset.timestamp === selectedA.timestamp) row.classList.add('row-selected-a');
                if (selectedB && row.dataset.timestamp === selectedB.timestamp) row.classList.add('row-selected-b');
            });
        }
```

- [ ] **Step 4: Extract `selectPoint(point)` from `onMapClick` and wire the table into it**

Replace the existing `onMapClick` function (current lines 245-277) with:

```javascript
        function selectPoint(point) {
            if (!point) return;

            if (selectedA && selectedB) {
                clearSelection();
            }

            if (!selectedA) {
                selectedA = point;
                markerA = pointMarker(point, '#3388ff');
                renderStatusTable(point);
                return;
            }

            if (point.timestamp === selectedA.timestamp) return;

            if (point.timestamp < selectedA.timestamp) {
                selectedB = selectedA;
                markerB = markerA;
                selectedA = point;
                markerA = pointMarker(point, '#3388ff');
            } else {
                selectedB = point;
                markerB = pointMarker(point, '#e74c3c');
            }

            const rangeLatLngs = trackData
                .filter(p => p.timestamp >= selectedA.timestamp && p.timestamp <= selectedB.timestamp)
                .map(p => [p.latitude, p.longitude]);
            highlightLayer = L.polyline(rangeLatLngs, { color: '#3388ff', weight: 6, opacity: 0.5 }).addTo(map);

            showSelectionPanel();
            highlightTableRows();
        }

        function onMapClick(e) {
            selectPoint(findNearestPoint(e.latlng));
        }
```

(Note: unlike the old `onMapClick`, `selectPoint` does not re-run `renderStatusTable` on the swap branch — the vessel-status window stays anchored on whichever point was clicked *first*, even if a later, chronologically-earlier click makes that original point become `selectedB` internally. This matches the design: the table populates once, on the first point of a selection, and stays put while the user picks/re-picks the second point from either the map or the table.)

- [ ] **Step 5: Clear the table alongside the rest of the selection**

In `clearSelection()` (current lines 222-233), add a call to `clearStatusTable()` at the end, so the function reads:

```javascript
        function clearSelection() {
            if (markerA) { markerA.remove(); markerA = null; }
            if (markerB) { markerB.remove(); markerB = null; }
            if (highlightLayer) { highlightLayer.remove(); highlightLayer = null; }
            selectedA = null;
            selectedB = null;
            pendingEngineOn = null;
            document.getElementById('selectionPanel').classList.remove('visible');
            document.getElementById('choiceOn').classList.remove('selected');
            document.getElementById('choiceOff').classList.remove('selected');
            document.getElementById('applyBtn').disabled = true;
            clearStatusTable();
        }
```

- [ ] **Step 6: Verify it compiles / loads**

Run: `cargo build`
Expected: builds with no errors (static asset only; confirms nothing else broke).

- [ ] **Step 7: Manual browser verification**

Open `/fix-engine-status.html?id=<a real trip id>` and verify:
- Before any click, the sidebar shows the placeholder text ("Click a point on the track...").
- Clicking a track point on the map populates the table with rows within ±15 minutes of that point, in chronological order, and highlights that point's row in blue.
- Clicking a different row in the table selects it as point B: the map shows both markers and the blue highlight polyline between them (same as clicking a second map point would), the selection panel appears with the correct time range and point count, and the clicked row highlights in red (or blue if it turned out to be chronologically earlier and became the new A — the earlier-in-time row is always blue, the later one red, regardless of click order).
- Clicking a table row for a point *before* the current point A correctly swaps: that row becomes the blue-highlighted row, the old point A's row becomes red, and the map markers/highlight update to match.
- Choosing Engine ON/OFF and clicking Apply still updates `vessel_status` and reloads the track (colors refresh); the table returns to its placeholder state afterward.
- Clicking Cancel clears the map selection and returns the table to its placeholder state.
- Clicking a third point (on the map or in a currently-visible table row area, after both A and B are set) restarts the selection: old markers/highlight clear, the table repopulates around the new point, treated as a fresh point A.
- Scrolling the table when the ±15 minute window has many rows works smoothly and the header stays pinned (`position: sticky`).

- [ ] **Step 8: Commit**

```bash
git add static/fix-engine-status.html
git commit -m "Add vessel status table to fix-engine-status for picking the second correction point"
```

(Only if the user has explicitly asked you to commit.)
