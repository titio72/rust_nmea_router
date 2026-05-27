# Dense Wind Arrow Grid — Design Spec

**Date:** 2026-05-16  
**Status:** Approved

## Summary

Two coordinated changes improve wind arrow density on the planning map:

1. **Backend** (`src/forecast.rs`): Change the Open-Meteo fetch grid from 0.25° (~25km) to 0.08° (~9km), matching the ECMWF IFS model's native horizontal resolution. Large areas are chunked into 1°×1° sub-requests to stay within HTTP URL length limits.

2. **Frontend** (`static/plan.html`): Add zoom-adaptive IDW display interpolation. The raw grid points returned by the API are interpolated to a synthetic finer grid scaled to the current map viewport, so arrow density stays visually consistent at any zoom level.

No DB schema changes. No new API endpoints. Old 25km stored data remains usable; new fetches produce 9km data.

---

## Backend Changes (`src/forecast.rs`)

### Grid step

```rust
const GRID_STEP_DEG: f64 = 0.08;   // was 0.25 → ~9km at mid-latitudes
```

### Chunked fetching

At 0.08° step, a 2°×2° forecast area requires 676 lat/lon pairs in the URL (~11KB), which exceeds common HTTP server limits. The fix: split any area into 1°×1° chunks before calling Open-Meteo. Each 1°×1° chunk has at most 14×14 = 196 points (~3KB URL — acceptable).

**New helper: `bbox_1deg_chunks(lat_min, lat_max, lon_min, lon_max) → Vec<(f64, f64, f64, f64)>`**

Returns a list of non-overlapping 1°×1° sub-bboxes that together tile the input area. Each chunk is [floor(lat_min + k), min(floor(lat_min + k) + 1, lat_max)] × equivalent for lon.

**Refactored `fetch_area_forecast`:**

```rust
pub async fn fetch_area_forecast(
    lat_min: f64, lat_max: f64, lon_min: f64, lon_max: f64,
) -> Result<Vec<FetchedForecast>, AppError> {
    let client = reqwest::Client::new();
    let chunks = bbox_1deg_chunks(lat_min, lat_max, lon_min, lon_max);
    let mut results = Vec::new();
    for (c_lat_min, c_lat_max, c_lon_min, c_lon_max) in chunks {
        let chunk = fetch_chunk(&client, c_lat_min, c_lat_max, c_lon_min, c_lon_max).await?;
        results.extend(chunk);
    }
    Ok(results)
}
```

`fetch_chunk` contains the current body of `fetch_area_forecast` (meteo + marine fetch, parse, combine). Chunks are fetched sequentially; for typical areas (≤9 chunks × 2 APIs = 18 HTTP calls) this adds a few seconds to each poller cycle, which runs every 3 hours.

### Unit test update

The existing test `test_bbox_url_contains_expected_params` asserts 25 grid points for a 1°×1° bbox at 0.25° step. It needs updating for the new step:

```rust
// 1°×1° bbox at 0.08° step → 14×14=196 pairs
assert_eq!(lat_param.split(',').count(), 196, "expected 196 grid points");
```

---

## Frontend Changes (`static/plan.html`)

### New state

```js
let lastGridPts = [];   // raw GridPointForecast[] from last API call
```

### New functions

**`degDistNm(lat1, lon1, lat2, lon2)`** — fast degree-based distance approximation in nautical miles (avoids trig in IDW hot path):

```js
function degDistNm(lat1, lon1, lat2, lon2) {
    const dlat = lat2 - lat1;
    const dlon = (lon2 - lon1) * Math.cos(lat1 * Math.PI / 180);
    return Math.sqrt(dlat * dlat + dlon * dlon) * 60;
}
```

**`idwInterpolate(targetLat, targetLon, sourcePts)`** — IDW from source grid to a single target point. Returns a `GridPointForecast`-shaped object or `null` if no source points are within 25nm:

- Scalar fields (wind_speed_kn, wind_gust_kn, wave_height_m, wave_period_s, cape_j_kg): weighted mean, weight = 1/d²
- Angular fields (wind_direction_deg, wave_direction_deg): vector mean via sin/cos, same as Rust `forecast.rs`
- Exact match (d < 0.01nm): return source point directly

**`interpolateDisplayGrid(sourcePts)`** — generates the zoom-adaptive display grid:

1. Get `bounds = planMap.getBounds()`
2. `displayStep = Math.max(0.01, (bounds.getEast() - bounds.getWest()) / 25)`  
   → ~25 arrows across viewport width at any zoom
3. Compute display grid bounds = intersection of source data bbox and viewport (skip if no intersection)
4. For each (lat, lon) in the display grid, call `idwInterpolate` and collect non-null results
5. Return array of interpolated points

### Modified functions

**`loadGridPoints()`:**

```js
async function loadGridPoints() {
    const ts = getSelectedISO();
    if (!ts) return;
    const resp = await fetch(`/api/forecast/grid-points?trip_id=${tripId}&timestamp=${encodeURIComponent(ts)}`);
    const json = await resp.json();
    lastGridPts = json.data || [];
    const displayPts = interpolateDisplayGrid(lastGridPts);
    renderArrows(displayPts);
    renderStats(lastGridPts);   // stats use raw source points, not interpolated
}
```

**`renderArrows(pts)`:** unchanged — already accepts any array of points with lat/lon/wind fields.

### Zoom/pan re-render

Two new Leaflet event listeners added during `init()`:

```js
planMap.on('zoomend moveend', () => {
    if (lastGridPts.length) {
        renderArrows(interpolateDisplayGrid(lastGridPts));
    }
});
```

No new API call on zoom/pan — re-uses the last fetched data.

---

## Data flow

```
Poller (every 3h):
  fetch_area_forecast(bbox)
    → bbox_1deg_chunks → N sequential HTTP calls to Open-Meteo
    → store FetchedForecast in forecast_fetch + forecast_hourly

Browser (slider change):
  loadGridPoints()
    → GET /api/forecast/grid-points?trip_id=X&timestamp=T
    → lastGridPts = raw 9km grid points
    → interpolateDisplayGrid(lastGridPts) → zoom-adaptive display pts
    → renderArrows(display pts)   ← more arrows, zoom-aware
    → renderStats(lastGridPts)    ← unaffected, uses raw data

Browser (zoom or pan):
    → interpolateDisplayGrid(lastGridPts) → re-render at new density
    → renderArrows(display pts)
```

---

## Edge cases

- **Small area (< 1°×1°):** Single chunk, no change in HTTP behaviour beyond the step change.
- **No source pts in viewport:** `interpolateDisplayGrid` returns `[]` → `renderArrows` clears markers cleanly.
- **Old 25km stored data:** IDW from sparser source just gives fewer arrows; all functions handle arbitrary source density.
- **Zoom very far in (step hits 0.01° floor):** At most ~100km²/point → still only a handful of display points; no performance concern.
- **IDW with no points within 25nm:** Returns `null` → display point skipped; viewport arrows near edges may be sparse if source data doesn't extend there.

---

## Files changed

| File | Change |
|---|---|
| `src/forecast.rs` | `GRID_STEP_DEG` → 0.08; add `bbox_1deg_chunks`; refactor `fetch_area_forecast` to loop over chunks; update unit test |
| `static/plan.html` | Add `lastGridPts`, `degDistNm`, `idwInterpolate`, `interpolateDisplayGrid`; update `loadGridPoints`; add zoom/pan event listeners |
