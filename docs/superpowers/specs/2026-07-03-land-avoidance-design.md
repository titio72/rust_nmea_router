# Land Avoidance for Isochrone Route Optimisation

**Date:** 2026-07-03  
**Scope:** Tyrrhenian / Mediterranean sailing, `⚡ Optimize` route button in `plan.html`  
**Status:** Approved

---

## Problem

The isochrone algorithm in `src/routing.rs` expands a frontier of candidate positions outward in 72 headings every 30 minutes with no awareness of land. Candidate points can be placed on coastlines, islands, or inland — producing routes that sail through Corsica or run aground on Elba.

---

## Decision Summary

| Question | Decision |
|---|---|
| Geographic scope | Mediterranean; primarily Tyrrhenian Sea |
| Required resolution | 2 nm target, 5 nm maximum |
| Land data source | GeoJSON file loaded at startup, rasterized in memory |
| Implementation style | Hand-rolled scanline rasterizer, no new crate deps |
| Check strategy | Point-only: drop candidates whose position falls on land |

---

## Data Source

**File:** `ne_10m_land.geojson` from [Natural Earth](https://www.naturalearthdata.com/downloads/10m-physical-vectors/) (1:10m scale land polygons).

The operator downloads this file once and places it on the server, e.g. `/etc/nmea_router/land.geojson`. No runtime network access is required. The file is a standard GeoJSON `FeatureCollection` containing `Polygon` and `MultiPolygon` geometries for all land masses.

---

## Configuration

Two new fields in `config.rs` / `config.example.json`:

```json
"land_mask_path": "/etc/nmea_router/land.geojson",
"land_mask_resolution_deg": 0.05
```

- `land_mask_path`: `Option<String>`. If absent, land avoidance is disabled and routing behaves exactly as before.
- `land_mask_resolution_deg`: `f64`, defaults to `0.05` (~3 nm). Can be reduced to `0.03` (~2 nm) for finer resolution at the cost of a ~2× larger grid.

If the path is configured but the file is missing or malformed, the router logs a warning and continues without land avoidance — it does not fail to start.

---

## Known Limitation: Harbor/Pier-Precision Coordinates

`ne_10m_land` is a heavily generalized coastline — despite the "10m" name (which refers to map scale, 1:10,000,000, not metre resolution), small islands and harbor indentations are simplified away. Capraia's entire coastline, for example, is represented with only 12 vertices, and the harbor's breakwater/pier cutout doesn't exist in the polygon at all.

Consequence: a route waypoint placed exactly at or very near a pier (e.g. the harbor entrance) can fall geometrically *inside* the simplified land polygon even though it's in open water in reality. This is not a rasterization bug — increasing `land_mask_resolution_deg` does not help, because the source polygon itself is wrong at that point, before any grid is applied. `is_land()` and `run_isochrone()`'s land checks are working correctly against the data they're given.

**Accepted for now.** If a `⚡ Optimize` request fails or produces an unexpected route because a waypoint sits inside a simplified landmass, move the waypoint a few hundred metres further offshore. Revisit only if this becomes a frequent complaint — the fix would be switching to a higher-fidelity coastline source (e.g. OpenStreetMap's land-polygons extract), which is a larger scope change (bigger file, different licensing/update process) deliberately deferred here.

---

## New Module: `src/land_mask.rs`

### Data structure

```rust
pub struct LandMask {
    lat_min: f64,
    lat_step: f64,
    rows: usize,
    lon_min: f64,
    lon_step: f64,
    cols: usize,
    grid: Vec<u8>,   // packed bits, 1 = land
}
```

### Bounding box

Fixed to the full Mediterranean with margin:

| | Min | Max |
|---|---|---|
| Latitude | 28°N | 48°N |
| Longitude | −8°E | 42°E |

At 0.05°: 400 rows × 1000 cols = 400 k cells = **50 KB**.  
At 0.03°: 667 rows × 1667 cols = 1.1 M cells = **138 KB**.

### `LandMask::from_geojson(path: &str, resolution_deg: f64) -> Result<Self, Box<dyn Error>>`

1. Read and parse the GeoJSON file with `serde_json` (already a project dependency).
2. Walk all `Feature` entries; extract `coordinates` from `Polygon` and `MultiPolygon` geometries. Ignore properties. **GeoJSON coordinate order is `[longitude, latitude]`** — index 0 is longitude, index 1 is latitude throughout.
3. For each polygon ring, run a **scanline fill**:
   - Determine the grid row range covered by the ring's latitude extent.
   - For each grid row, compute the center latitude.
   - Walk every edge of the ring; collect all longitudes where the edge crosses that latitude.
   - Sort the crossing longitudes; fill between pairs (even-odd rule): set the corresponding bits in `grid`.
4. Return the populated `LandMask`.

**Startup cost:** ~200–400 ms on a modern CPU; ≤2 s on the ARM cross-compile target. Runs once at boot.

### `LandMask::is_land(&self, lat: f64, lon: f64) -> bool`

Bounds-check lat/lon against the fixed bounding box; compute `row = ((lat - lat_min) / lat_step) as usize` and `col = ((lon - lon_min) / lon_step) as usize`; read one bit from `grid`. Returns `false` for any point outside the bounding box (open ocean beyond the Mediterranean fringe is not land).

**Complexity:** O(1), zero allocation.

---

## Changes to `src/routing.rs`

### Signature

```rust
pub fn run_isochrone(
    from: (f64, f64),
    to: (f64, f64),
    departure: DateTime<Utc>,
    motoring_speed_kn: f64,
    polar_efficiency: f64,
    min_sail_speed_kn: f64,
    sail_weight_kn: f64,
    polars: &crate::polars::PolarTable,
    fetches: &[FetchWithHourly],
    land_mask: Option<&LandMask>,   // ← new
) -> IsochroneResult
```

### Inner loop change

Immediately after `advance_position()`:

```rust
let new_pos = advance_position(parent.lat, parent.lon, heading, speed_kn * STEP_HOURS);
if land_mask.map_or(false, |m| m.is_land(new_pos.0, new_pos.1)) {
    continue;
}
```

No other routing logic is touched.

### Edge case: all candidates on land

If every heading from a frontier point lands on land, `prune_isochrone` returns an empty vec for that point's sector. The branch simply dies — no special handling required.

### Tests

All existing unit tests pass `None` for `land_mask` and require no changes.

---

## Changes to `src/main.rs`

After loading config and polars:

```rust
let land_mask: Option<Arc<LandMask>> =
    config.land_mask_path.as_ref().and_then(|path| {
        match LandMask::from_geojson(path, config.land_mask_resolution_deg) {
            Ok(m)  => { log::info!("Land mask loaded from {path}"); Some(Arc::new(m)) }
            Err(e) => { log::warn!("Land mask disabled: {e}"); None }
        }
    });
```

`land_mask` is stored in `AppState` alongside the existing polar table.

---

## Changes to `src/web/api.rs`

The `optimal-route` handler passes the mask through:

```rust
run_isochrone(..., state.land_mask.as_deref())
```

`Option<Arc<LandMask>>::as_deref()` yields `Option<&LandMask>` — no cloning.

The straight-line `/api/forecast/route` endpoint is **not changed** (it follows user-supplied waypoints directly and does not call `run_isochrone`).

---

## Files Changed

| File | Change |
|---|---|
| `src/land_mask.rs` | **New** — LandMask struct, GeoJSON loader, scanline rasterizer, `is_land()` |
| `src/config.rs` | Add `land_mask_path: Option<String>`, `land_mask_resolution_deg: f64` |
| `config.example.json` | Document the two new fields |
| `src/routing.rs` | Add `land_mask: Option<&LandMask>` param; one `continue` in inner loop |
| `src/main.rs` | Load mask at startup; store in `AppState` |
| `src/web/api.rs` | Pass `state.land_mask.as_deref()` to `run_isochrone` |

---

## Out of Scope

- Segment-crossing checks (leap-over detection) — excluded by design decision.
- Shallow-water or depth-contour avoidance.
- Dynamic exclusion zones.
- Any change to the straight-line route compute path.
