# Split plan.html into Navigation Areas + Planning pages

## Problem

`static/plan.html` (1805 lines) currently bundles two distinct concerns into one page:

1. **Navigation area management** — drawing/listing/deleting forecast bounding boxes, and manually refreshing forecast data.
2. **Route planning** — time scrubber, wind field map, waypoint drawing, route compute/optimize, route summary.

This makes the file hard to navigate and forces the planning UI into the same cramped, 1500px-centered layout as every other page, even though it's fundamentally a full-screen map tool. The nav bar also currently exposes both concerns behind a single "Forecast" link.

## Goals

- Separate area definition from route planning into two pages.
- Give the planning page a full-viewport-width layout, since it doesn't fit the standard content-page mold.
- Gate both pages to the full (non-read-only) version of the app, consistent with the server already 403-ing all `/api/forecast/*` routes when `read_only: true` ([src/web/api.rs:1970-1977](../../../src/web/api.rs#L1970-L1977)).
- Keep the manual "Refresh forecast" action available on both pages, since users may want to trigger a refetch while actively planning a route, not just while managing areas.

## Non-goals

- No change to backend/API behavior — this is a static-file-only reorganization.
- No change to the read-only gating logic itself (already correct server-side); we're only adding a client-side redirect for UX.
- No shared JS module extraction. This codebase's static pages are self-contained (aside from `shared-theme.js`/`shared.css`); duplicating the ~40-line refresh/poll functions across two pages matches that existing convention rather than introducing a new shared-script pattern.

## File structure

| File | Purpose | Layout |
|---|---|---|
| `static/navigation-areas.html` (new) | Define/list/delete navigation (forecast) areas | Standard 1500px-centered, like `backup.html` |
| `static/plan.html` (trimmed) | Route planning: time scrubber, wind map, routing | Full-viewport-width, like `index.html`'s `.container { max-width:100%; margin:0 }` |

## Navigation bar changes

`static/js/shared-theme.js` `createHeaderBar()`'s `navItems` currently has one entry:
```js
{ href: '/plan.html', label: 'Forecast', page: 'forecast', roHidden: true }
```
This becomes two entries, in this order (areas before planning, matching the natural workflow of defining an area before planning a route in it):
```js
{ href: '/navigation-areas.html', label: 'Navigation Areas', page: 'navigation-areas', roHidden: true }
{ href: '/plan.html', label: 'Planning', page: 'planning', roHidden: true }
```
Both keep `roHidden: true` (nav links hidden in read-only mode, same as today).

## navigation-areas.html

Moved from `plan.html`, largely verbatim:

- **Panel**: "Navigation Areas" (renamed from "Forecast Areas"). The collapse/chevron toggle (`toggleForecastAreas`, `forecast_areas_collapsed` localStorage key) is dropped — it existed only because the panel shared page space with the time scrubber and map; on its own page it's just shown directly inside a `level-1-container`.
- **Left column**: area list (`forecastAreaList`), "Draw Area" / "Cancel" buttons, drawing hint text.
- **Right column**: small definition map (`forecastAreaMapEl`, 200px tall) with bounding-box drag-to-draw.
- **Header row**: "↻ Refresh" button (`forecastRefreshBtn`) + poller status badge (`forecastPollerStatus`), auto-polled every 60s.

JS carried over, trimmed of planning-page coupling:
- `initForecastAreaMap`, `startDrawArea`, `cancelDrawArea`, `overlayLatLng`, `onAreaMapMouseDown/Move/Up`
- `deleteForecastArea`, `renderForecastAreaList`, `renderForecastAreasOnMap`
- `loadForecastAreas` — drops the `if (planMap) syncWindLayers(areas)` call (no `planMap` on this page)
- `updateForecastStatus`
- `refreshForecast` — drops the `await loadAvailableDays()` call (no day tabs on this page); just re-polls status after the refresh completes
- `window.addEventListener('load', ...)` bootstrap: `initForecastAreaMap()`, `loadForecastAreas()`, `updateForecastStatus()`, starts the 60s interval

Title: `Navigation Areas - NMEA Router`. Header bar: `createHeaderBar('navigation-areas')`.

## plan.html (trimmed, renamed "Planning")

Everything **except** the area-management panel stays: time scrubber, route bar, big wind/route map, stats bar, route summary panel, alt-route modal, and all associated JS (IDW interpolation, wind particle layers, waypoint drawing, route compute/optimize, isochrone frontier logic, route persistence in localStorage).

**Layout**: outer wrapper changes from
```html
<div style="max-width:1500px; margin:0 auto; padding:20px;">
```
to a full-width container matching `index.html`'s pattern:
```html
<div class="container">
```
```css
.container { max-width: 100%; margin: 0; padding: 0 20px 20px; }
```

**Area data for wind layers**: `plan.html` still needs area bounding boxes to build `WindParticleLayer`s on `planMap` and for `pointInArea`/`pointInAnyArea` lookups (popups, frontier-run coloring). Since the drawing/management UI moves out, this becomes a one-shot load inside `init()`:
```js
async function loadPlanAreas() {
    try {
        const resp = await fetch('/api/forecast/areas');
        const json = await resp.json();
        syncWindLayers(json.data || []);
    } catch (err) {
        console.error('Failed to load forecast areas', err);
    }
}
```
`syncWindLayers` itself (sets `planAreas`, adds/removes `WindParticleLayer`s keyed by area id) is unchanged. No polling — if areas change while the Planning tab is open, a reload picks it up; no cross-tab sync is needed for this workflow.

**Refresh forecast, moved here too**: `refreshForecast()`, `updateForecastStatus()`, and the 60s poll interval are duplicated onto this page (per the Non-goals note — no shared module). Placement: top-right of the time-scrubber panel, alongside the existing "Show Gust" / "Plan Route" buttons — a new `forecastRefreshBtn` + `forecastPollerStatus` pair. On this page, `refreshForecast()` calls `updateForecastStatus()` **and** `loadAvailableDays()` afterward (new forecast data can extend the available day range), matching today's plan.html behavior.

Title: `Planning - NMEA Router`. Header bar: `createHeaderBar('planning')`.

## View-only enforcement

Both pages currently rely solely on the nav link being hidden (`data-ro-hidden`) — a read-only user hitting the URL directly still gets a fully working (if API-error-riddled) page, since every `/api/forecast/*` call 403s but nothing redirects. Both new pages add, as the first step of their init sequence:

```js
fetchUiMode().then(readOnly => { if (readOnly) window.location.href = '/'; });
```

This runs before any other fetches/DOM setup, so read-only users get a clean bounce to `/` instead of a broken map full of failed requests.

## Testing

Static-file-only change — no Rust unit tests apply. Verification is manual:
- Load `navigation-areas.html`: draw/delete an area, confirm refresh button updates the status badge and poller.
- Load `plan.html`: confirm areas drawn on the other page render as wind layers, route planning (draw/compute/optimize) still works, refresh button updates status and re-renders day tabs.
- Set `read_only: true` in config, confirm both pages redirect to `/` and their nav links are hidden.
- Confirm `plan.html` now spans full viewport width; `navigation-areas.html` stays 1500px-centered like other management pages.
