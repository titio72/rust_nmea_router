# Fix Engine Status — Expanded Map & Vessel Status Table — Design

## Purpose

`static/fix-engine-status.html` renders a trip's track on a fixed-height (520px) Leaflet map and
lets the user click two points to correct `vessel_status.engine_on` for the range between them.
The map is cramped on larger screens, and the only way to pick the second point is a second,
somewhat blind map click. This change (1) expands the map to fill the available viewport, the way
`static/plan.html` already does, and (2) adds a side table of the raw `vessel_status` rows near
the first selected point, so the second point can be picked precisely by clicking a row instead of
squinting at the map.

## Scope

- Modify: `static/fix-engine-status.html` only.
- No backend changes. No new endpoint, no new DB operation, no schema changes.
- Out of scope: changing the correction semantics, the engine-color legend, or
  `POST /api/correct_engine_status` itself (all unchanged from the existing implementation).

## Why no backend changes

`GET /api/track?trip_id=<id>` is already called with no `max_points`, so `fetch_track`
(`src/db/operations/query.rs:621`) returns every raw `vessel_status` row unmodified —
`decimate(track, max_points)` (`query.rs:94`) is a no-op when `max_points` is `None`. The page's
existing `trackData` array in memory therefore already contains, for every point, exactly the
fields the new table needs: `timestamp`, `latitude`, `longitude`, `avg_speed_kn`, `engine_on`,
`moored`. The table is populated by filtering this array client-side; nothing new is fetched.

## Layout change

Replace the current structure (title/help text, then a fixed `520px` map, then the selection panel
and status bar stacked below it, all inside one `app-card`) with a `plan.html`-style full-bleed
layout:

```
#headerContainer
.fix-engine-topbar          <- title, subtitle, back link, help text (compact, non-scrolling)
#mapTableWrap                <- flex row, height = viewport height - topbar bottom - margin
    #map                     <- flex: 1
    #statusPanel              <- fixed width (~340px), flex column, own scroll for the table
        #statusTableWrap      <- flex: 1, overflow-y: auto, holds the vessel-status table
        #selectionPanel        <- existing engine-choice / Apply / Cancel block, moved here
        #statusBar              <- existing success/error bar, moved here
```

- `sizeMapWrap()` (copied/adapted from `plan.html`): reads `#mapTableWrap`'s `getBoundingClientRect().top`,
  sets its height to `max(window.innerHeight - top - 20, 380)`, and calls `map.invalidateSize()` if
  the map already exists. Called once in `init()` after the topbar is populated (so its real height
  is known) and again on every `window resize` event.
- `#map` and `#statusPanel` are plain flex children (`display:flex` on `#mapTableWrap`), not
  absolutely positioned — unlike `plan.html`'s floating overlay panels, the table is a permanent
  side column, not something drawn on top of the map.
- `#statusPanel` keeps its own internal `overflow-y: auto` on the table area only, so the
  selection panel and status bar at the bottom stay pinned and visible regardless of table length.

## Vessel status table

**Columns:** Time, Lat, Lon, Avg Speed (kn), Engine, Status.
- Engine renders `engine_on` (0/1/2) as `Off` / `On` / `Unknown`, matching the map's color legend
  (green/grey/gold) with a small colored dot per row for visual continuity.
- Status renders `moored` as `Moored` / `Underway`.
- Lat/Lon rounded to 5 decimals for readability.

**Population:**
- Before point A is selected: `#statusTableWrap` shows a placeholder row/message: "Click a point on
  the track to see nearby vessel status data."
- Once point A is selected (via map click, as today): filter `trackData` to rows with
  `timestamp` within 15 minutes before/after `selectedA.timestamp` (using the same lexicographic
  ISO-8601 string comparison the rest of the page already relies on — no `Date` parsing needed),
  sorted chronologically, and render them as table rows.
- The window stays anchored to point A for as long as the current selection is active — selecting
  or changing point B does not re-center the table. This lets the user compare several candidate
  B points from the same table without re-clicking A.

**Interaction — shared selection path:**
- `onMapClick`'s point-selection logic (nearest-point snap, first click → A, second click → B with
  chronological swap, highlight polyline, show selection panel) is factored out into a new
  `selectPoint(point)` function.
- Map clicks call `selectPoint(findNearestPoint(e.latlng))`, same as today.
- A new `onTableRowClick(point)` handler (bound per `<tr>`) calls the same `selectPoint(point)` —
  clicking a row behaves exactly like clicking that point on the map, including the swap-if-earlier
  logic and re-highlighting.
- After `selectPoint` runs, the row(s) matching `selectedA`/`selectedB` (by `timestamp` equality)
  get a highlighted background/border in the two marker colors (`#3388ff` for A, `#e74c3c` for B),
  kept in sync by re-applying the highlight class on every `selectPoint` call.
- A third click (map or table row) when both A and B are already set restarts selection exactly as
  today (`clearSelection()` then treat the click as a fresh point A) — this also empties the table
  back to its placeholder state, then immediately repopulates it around the new point A.
- `clearSelection()` (used by both the restart path and the existing Cancel button) also clears the
  table back to the placeholder state.

## Error handling

No new failure modes are introduced — the table is a pure derived view of data already loaded and
already handled by the existing `loadTrack()`/`init()` error paths (empty track, fetch failure).
If the ±15 minute window around point A contains no other points (extremely sparse track), the
table simply renders with only point A's own row.

## Testing

Frontend-only static page; no Rust code changes, so no new automated tests. Per CLAUDE.md's UI
testing rule, verify manually in a browser against a real trip:
- Map fills the available viewport height on initial load, and on window resize.
- Clicking a track point populates the side table with rows in the ±15 minute window, ordered by
  time.
- Clicking a different table row selects it as point B: the map highlight, markers, and selection
  panel all update the same way a second map click would; the corresponding table rows are
  highlighted.
- Clicking a table row chronologically *before* the current point A correctly swaps A/B (mirrors
  the existing map-click swap behavior).
- Applying a correction still works end-to-end (track reloads, colors update, table clears back to
  placeholder).
- Cancel clears both the map selection and the table.
- A third click (map or table) after A and B are both set restarts selection and repopulates the
  table around the new point.
