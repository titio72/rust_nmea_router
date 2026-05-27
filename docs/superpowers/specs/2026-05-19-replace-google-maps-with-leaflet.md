# Spec: Replace Google Maps with Leaflet in trip.html

**Date:** 2026-05-19
**Status:** Approved

## Summary

Replace the Google Maps JavaScript API with Leaflet in `static/trip.html` for the main trip-track visualization. Leaflet is already loaded on the page (used by the forecast-area drawing widget). The change eliminates the API key dependency, the async key-fetch loading path, and the canvas fallback. The backend config field and endpoint that served the key are also removed.

## Scope

### Frontend — `static/trip.html`

**Remove entirely:**
- The `<!-- Google Maps API will be loaded dynamically -->` comment in `<head>`
- `googleMapsLoaded` flag and `loadGoogleMapsAPI()` function (async key fetch + dynamic script injection)
- The `mapsPromise` / late-loading recovery block in the page-init flow (lines ~649–713)
- `drawCanvasTrack()` canvas fallback function — not needed since Leaflet requires no API key
- `showMapFallback()` calls that were specific to Google Maps unavailability; keep the call for "No GPS track data available"

**Replace — API mapping:**

| Google Maps | Leaflet |
|---|---|
| `new google.maps.Map(el, {center, zoom, mapTypeId})` | `L.map(el).setView([lat,lng], zoom)` + OSM tile layer |
| `map.fitBounds({north,south,east,west})` | `map.fitBounds([[s,w],[n,e]])` |
| `new google.maps.Polyline({path, strokeColor, strokeWeight, geodesic})` | `L.polyline(latlngs, {color, weight}).addTo(map)` |
| `polyline.setMap(null)` | `polyline.remove()` |
| Shared hover `InfoWindow` (setContent + setPosition + open) | Single `L.popup()` reused across segments: `popup.setLatLng(e.latlng).setContent(html).openOn(map)` |
| `new google.maps.Marker({icon: {url: svgDataUrl, size, anchor}})` | `L.marker(latlng, {icon: L.icon({iconUrl, iconSize, iconAnchor})})` |
| Start/end flag markers using `google.maps.SymbolPath` SVG path string | `L.marker(latlng, {icon: L.divIcon({html: '<svg>…</svg>', iconSize, iconAnchor})})` |
| `SymbolPath.CIRCLE` nav-window markers | `L.circleMarker(latlng, {radius, fillColor, color, weight, fillOpacity})` |
| `marker.addListener('click', fn)` | `marker.on('click', fn)` |
| `polyline.addListener('mouseover'/'mouseout', fn)` | `polyline.on('mouseover'/'mouseout', fn)` |
| `infoWindow.open(map, marker)` on click | `marker.bindPopup(html).openPopup()` or `L.popup().setLatLng(…).setContent(html).openOn(map)` |

**Start/end flag markers detail:**
The current SVG path `M 0,0 C -2,-20 -10,-22 -10,-30 L -10,-50 L 10,-50 L 10,-30 C 10,-22 2,-20 0,0 z` (filled green or red, white stroke) is embedded in a `L.divIcon` using an inline `<svg>` element. `iconSize` and `iconAnchor` are set to preserve the existing visual anchor point at the base of the pin.

**`currentMap` reference:**
`currentMap` stays as the global Leaflet map reference. All call sites that currently call `currentMap.fitBounds(...)` use the Leaflet bounds format instead.

**Map re-initialization:**
`initializeMap` is called on each leg/trip switch. Leaflet throws if `L.map()` is called on an already-initialized container. At the top of `initializeMap`, if `currentMap` is non-null, call `currentMap.remove()` and set `currentMap = null` before creating the new instance.

**Tile layer:**
OpenStreetMap (`https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png`), matching the existing forecast-area map. `maxZoom: 19`.

### Backend — `src/config.rs`

- Remove `google_maps_api_key: Option<String>` from `WebConfig`
- Remove its `Default` impl entry (`google_maps_api_key: None`)
- Remove the `validate_google_maps_api_key` logic in `validate()` (line ~643)
- Remove the two config unit tests that assert on `google_maps_api_key`

### Backend — `src/web/api.rs`

- Remove `get_google_maps_key` handler function
- Remove the `/config/google_maps_key` route registration
- Remove the `test_get_google_maps_key` test
- Remove `google_maps_key` from the route coverage assert (line ~3498)

### Config files

- Remove `google_maps_api_key` key from `config.example.json`

## Out of Scope

- The forecast-area Leaflet map (`forecastAreaMap`, `initForecastAreaMap`) — already Leaflet, no changes needed
- Any other HTML pages — only `trip.html` references Google Maps

## Acceptance Criteria

1. `trip.html` loads and renders the track map with no Google Maps script or API key
2. Colored speed/engine track segments render correctly
3. Hover tooltip on track segments shows time, speed, heading, wind
4. Start (green) and end (red) flag markers render and show info popup on click
5. Hourly time markers render on single-leg view
6. Nav-window start/end circle markers render and show popup on click
7. Segment highlight from analytics panel zooms map to the highlighted segment
8. `cargo build --release` passes with no warnings related to removed config fields
9. `cargo test` passes — no references to removed `google_maps_api_key` or `get_google_maps_key`
