# GPU Wind Particle Advection — Design Spec

**Date:** 2026-07-04
**Status:** Approved

## Summary

Replace the static, IDW-interpolated wind arrow markers on the planning map (`static/plan.html`) with an animated GPU particle field — the same visual technique used by windy.com / earth.nullschool.net — showing particles streaming along the current wind vector field inside each drawn Forecast Area.

- **Vendor** `mapbox/webgl-wind` (`dist/wind-gl.js`, ISC license, zero runtime deps) unmodified via `download_libs.sh`, the same pattern already used for Leaflet/Chart.js.
- **No backend changes.** The existing `/api/forecast/grid-points?timestamp=...` endpoint and the existing client-side IDW helpers (`idwInterpolate`, `interpolateDisplayGrid`) already produce everything needed; the particle texture is built client-side as an `ImageData`, bypassing the PNG format the library's own demo uses.
- **One `WindGL` instance per Forecast Area**, each on its own canvas clipped to that area's on-screen rectangle, repositioned/resized on `moveend`/`zoomend` ("snap-to-view" — matches how arrows already redraw after pan/zoom today, not continuous mid-drag tracking).
- The field is static per selected timestamp (existing day/hour tabs); particles animate spatially but the underlying vector field only changes when the user picks a new tab.
- Arrows are removed entirely — this is a replacement, not a toggle.

Rejected alternatives: a single shared canvas over the union bounding box of all areas (requires patching the vendored shaders to mask uncovered pixels — more invasive, wastes texture resolution when areas are far apart); server-side (Rust) PNG rasterization per area/timestamp (duplicates interpolation logic that already exists client-side, adds a new endpoint and an image-encoding dependency for a purely presentational feature).

---

## Vendoring (`download_libs.sh`, `static/libs/`)

`webgl-wind` has no tagged releases, so it's pinned to a specific commit SHA (same rigor as pinning Leaflet/Chart.js to exact versions):

```bash
download "wind-gl.js" \
    "https://cdn.jsdelivr.net/gh/mapbox/webgl-wind@b1f6468d90d2f39763a8795a5042f316a32ff3c8/dist/wind-gl.js"
```

This is the UMD build committed to the repo's `dist/` directory; it exposes a global `WindGL` constructor. Loaded in `plan.html` via `<script src="/libs/wind-gl.js"></script>`, same as the other vendored libs.

### `WindGL` API surface used

```
new WindGL(gl)                    // gl: WebGLRenderingContext
wind.numParticles = N              // setter, rebuilds particle state textures
wind.setColorRamp({0.0: '#...', ...})
wind.setWind({ image, width, height, uMin, uMax, vMin, vMax })
wind.resize()                      // call after canvas width/height change
wind.draw()                        // call once per animation frame
wind.fadeOpacity / speedFactor / dropRate / dropRateBump   // tuning knobs
```

`setWind`'s `image` field accepts any `TexImageSource` (confirmed from `util.createTexture`, which falls through to the `gl.texImage2D(..., format, type, source)` overload when no explicit width/height are passed) — so a plain `ImageData` object works without ever producing a PNG.

---

## New file: `static/js/wind-particle-layer.js`

### `WindParticleLayer` class

Plain JS class (Leaflet duck-types layers — no need to extend `L.Layer`), one instance per active Forecast Area:

```js
class WindParticleLayer {
    constructor(area) {
        this.area = area;              // { id, lat_min, lat_max, lon_min, lon_max }
        this.canvas = null;
        this.gl = null;
        this.wind = null;              // WindGL instance
        this.visible = false;
    }

    onAdd(map) {
        this.map = map;
        this.canvas = document.createElement('canvas');
        this.canvas.style.position = 'absolute';
        this.canvas.style.pointerEvents = 'none';
        map.getPanes().overlayPane.appendChild(this.canvas);
        const gl = this.canvas.getContext('webgl', { antialias: false });
        if (!gl) { console.warn('WebGL unavailable; wind particles disabled for area', this.area.id); return; }
        this.gl = gl;
        this.wind = new WindGL(gl);
        this.wind.numParticles = 4096;
        this.wind.setColorRamp(WIND_COLOR_RAMP);
        registerActiveLayer(this);
        this.reposition();
    }

    onRemove() {
        unregisterActiveLayer(this);
        if (this.canvas) this.canvas.remove();
    }

    // Called on moveend/zoomend and on initial add.
    reposition() {
        if (!this.gl) return;
        const rect = screenRectForArea(this.map, this.area); // null if fully off-screen
        if (!rect) { this.visible = false; this.canvas.style.display = 'none'; return; }
        this.visible = true;
        this.canvas.style.display = '';
        this.canvas.style.left = rect.left + 'px';
        this.canvas.style.top = rect.top + 'px';
        this.canvas.width = rect.width;
        this.canvas.height = rect.height;
        this.wind.resize();
        this.rebuildTexture();
    }

    // Called after reposition(), and whenever lastGridPts changes (new timestamp).
    rebuildTexture() {
        if (!this.gl || !this.visible) return;
        const pts = interpolateDisplayGrid(pointsInArea(lastGridPts, this.area));
        const tex = buildWindImageData(pts, this.canvas.width, this.canvas.height, this.area);
        if (tex) this.wind.setWind(tex);
    }

    draw() {
        if (this.visible && this.wind && this.wind.windData) this.wind.draw();
    }
}
```

### Shared helpers

- **`screenRectForArea(map, area)`** — projects the area's 4 corners via `map.latLngToContainerPoint`, intersects with the map container's pixel rect, returns `{left, top, width, height}` or `null` if there's no intersection.
- **`buildWindImageData(pts, width, height, area)`** — rasterizes `pts` onto a `width × height` grid aligned to `area`'s lat/lon bounds:
  1. For each raster cell, IDW-interpolate wind at that lat/lon from `pts` (reuses existing `idwInterpolate`).
  2. Convert `(wind_speed_kn, wind_direction_deg)` → u/v in m/s, "blowing towards" convention: `u = speed_ms * sin((dir+180) % 360 * DEG2RAD)`, `v = speed_ms * -cos(...)` (same +180 rotation already used by `renderArrows` today).
  3. Track `uMin/uMax/vMin/vMax` across all cells.
  4. Byte-pack: `R = 255*(u-uMin)/(uMax-uMin)`, `G = 255*(v-vMin)/(vMax-vMin)`, `B=0`, `A=255`.
  5. Return `{ image: new ImageData(new Uint8ClampedArray(bytes), width, height), width, height, uMin, uMax, vMin, vMax }`, or `null` if `pts` is empty (no data yet for this area).
- **`pointsInArea(pts, area)`** — filter by bounding box.
- **Shared animation loop** — one module-level `requestAnimationFrame` ticker that calls `.draw()` on every layer in the active-layer set (populated by `registerActiveLayer`/`unregisterActiveLayer`), rather than one rAF loop per layer.
- **`WIND_COLOR_RAMP`** — derived from the existing `windColor()` stops (0/4/7/10/16/22/28 kn → same RGB stops), expressed as the `{0.0: '#hex', ...}` object `setColorRamp` expects, so particle color matches the wind-speed legend used elsewhere on the page.

---

## Changes to `static/plan.html`

### Removed
- `renderArrows()`, `arrowMarkers` array, and the inline `<svg>`/`divIcon` arrow-drawing code. `windColor()` itself is kept — it's also called from the route/track rendering code (polyline and marker coloring), which is unaffected by this change.

### Added
- `<script src="/js/wind-particle-layer.js"></script>` after `leaflet.min.js` and `wind-gl.js`.
- `let windLayers = new Map();  // area.id -> WindParticleLayer`
- **`syncWindLayers(areas)`** — called whenever the Forecast Areas list changes (create/delete): diffs `areas` against `windLayers`' keys, calling `layer.onAdd(planMap)` for new areas and `layer.onRemove()` + `windLayers.delete(id)` for removed ones. Mirrors the existing full-rebuild pattern for arrows, but keyed by area id since each layer owns a live WebGL context that shouldn't be torn down and recreated on every redraw.
- Two Leaflet listeners registered during `init()`:
  ```js
  planMap.on('moveend zoomend', () => {
      windLayers.forEach(layer => layer.reposition());
  });
  ```
- **`loadGridPoints()`** modified: after fetching, instead of `renderArrows(interpolateDisplayGrid(lastGridPts))`, calls:
  ```js
  windLayers.forEach(layer => layer.rebuildTexture());
  ```
  (`renderStats()` call is unchanged.)
- Popup-on-click behavior (previously per-arrow `bindPopup`) reattached as a `click` listener on each layer's canvas: looks up the nearest point via the existing IDW helper against `lastGridPts` filtered to that area, and opens a Leaflet popup at the clicked `LatLng` with the same wind/gust/wave/CAPE content as today.

---

## Data flow

```
Forecast Areas panel (create/delete area)
  → syncWindLayers(areas) → add/remove WindParticleLayer per area

Day/hour tab change
  → loadGridPoints()
      → GET /api/forecast/grid-points?timestamp=T   (unchanged endpoint)
      → lastGridPts = raw grid points
      → windLayers.forEach(layer => layer.rebuildTexture())
          → pointsInArea(lastGridPts, area) → interpolateDisplayGrid (existing helpers)
          → buildWindImageData(...) → wind.setWind(...)
      → renderStats(lastGridPts)   (unchanged)

Pan or zoom (moveend/zoomend)
  → windLayers.forEach(layer => layer.reposition())
      → screenRectForArea(map, area) → resize/reposition canvas, wind.resize()
      → rebuildTexture() (re-rasterize from already-fetched lastGridPts, no network call)

Every animation frame
  → shared rAF ticker → each visible layer's wind.draw()
```

---

## Edge cases

- **Area with no grid data yet:** `buildWindImageData` returns `null`, `rebuildTexture` skips `setWind` — canvas stays blank, no error. Matches today's behavior of `renderArrows` seeing an empty array.
- **Area fully off-screen:** `screenRectForArea` returns `null` → canvas hidden, excluded from texture rebuild and from the active-layer set the rAF ticker iterates, so panned-away areas cost nothing per frame.
- **Overlapping forecast areas:** each area keeps its own independent canvas/layer; where two overlap, both draw (later-added on top). Acceptable — overlapping areas are a user-created edge case, not a designed-for scenario.
- **WebGL unavailable:** `getContext('webgl')` returns `null` → `onAdd` logs a console warning and leaves that layer permanently inert; the rest of the page (including other layers) is unaffected. This is the one real environment boundary worth guarding.
- **Rapid tab switching or area add/delete:** texture rebuild is a pure in-memory `Uint8ClampedArray` fill (no network round-trip), so no debouncing is needed beyond what the existing grid-points fetch already has.

---

## Testing

No Rust or DB surface changes — DB_ANALYST.md protocols don't apply. This is presentation-layer WebGL code, verified manually:

- Load `plan.html`, draw a Forecast Area over water with real forecast data, confirm particles render, animate, and stay clipped to the area's boundary.
- Pan and zoom: confirm the canvas repositions/resizes cleanly at `moveend`/`zoomend` with no leftover misaligned frames.
- Switch day/hour tabs: confirm the field updates (direction/speed/color) without a page reload.
- Create/delete a Forecast Area while particles are running: confirm layers are added/removed without leaking WebGL contexts (check via repeated create/delete + browser task manager GPU memory).
- Simulate WebGL absence (stub `HTMLCanvasElement.prototype.getContext` to return `null`) and confirm the page still loads with a console warning, no thrown error.

---

## Files changed

| File | Change |
|---|---|
| `download_libs.sh` | Add `wind-gl.js` download, pinned to commit `b1f6468d90d2f39763a8795a5042f316a32ff3c8` of `mapbox/webgl-wind` |
| `static/js/wind-particle-layer.js` | New file: `WindParticleLayer` class, `screenRectForArea`, `buildWindImageData`, `pointsInArea`, shared rAF ticker, `WIND_COLOR_RAMP` |
| `static/plan.html` | Remove `renderArrows`/`arrowMarkers`/arrow SVG code; add `windLayers` map, `syncWindLayers`, `moveend`/`zoomend` listeners, updated `loadGridPoints`, canvas click-to-popup handler; load `wind-gl.js` and `wind-particle-layer.js` |
