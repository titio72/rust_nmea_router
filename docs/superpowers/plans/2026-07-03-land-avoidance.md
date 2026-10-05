# Land Avoidance for Isochrone Routing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Drop any isochrone candidate whose `advance_position()` result falls on land, using a GeoJSON-sourced raster land mask, so the `⚡ Optimize` route button never produces routes through islands or coastlines.

**Architecture:** A new `src/land_mask.rs` module loads a GeoJSON land-polygon file at startup, rasterizes it into a compact packed-bit grid via a scanline fill algorithm, and exposes a single O(1) `is_land(lat, lon) -> bool` call. `run_isochrone()` gains an `Option<&LandMask>` parameter and `continue`s for any candidate that returns `true`. Two new optional config fields (`land_mask_path`, `land_mask_resolution_deg`) follow the same pattern as the existing `polars_file_path`.

**Tech Stack:** Rust, `serde_json` (already a dep, no new crates), Natural Earth `ne_10m_land.geojson`.

## Global Constraints

- No new crate dependencies — use only `serde_json` (already in `Cargo.toml`) for JSON parsing.
- GeoJSON coordinate order is `[longitude, latitude]` — index 0 is longitude, index 1 is latitude throughout.
- Fixed Mediterranean bounding box: lat 28–48°N, lon −8–42°E. Points outside return `false` from `is_land`.
- `land_mask_path` is `Option<String>`. If absent or file is missing/malformed, land avoidance is silently disabled — the router must not fail to start.
- Follow project naming: `snake_case` functions/modules, `PascalCase` structs. No comments unless the WHY is non-obvious.
- Do NOT run `git commit` or `git push`. Stop after writing code and passing tests.
- Run `cargo test` (not `cargo test -- --include-ignored`) — DB integration tests are excluded by default and must stay that way.

---

## File Map

| File | Action | Responsibility |
|---|---|---|
| `src/land_mask.rs` | Create | `LandMask` struct, GeoJSON loader, scanline rasterizer, `is_land()`, unit tests |
| `src/config.rs` | Modify | Add `land_mask_path: Option<String>`, `land_mask_resolution_deg: f64` |
| `config.example.json` | Modify | Document the two new config fields |
| `src/main.rs` | Modify | Add `pub mod land_mask;` |
| `src/routing.rs` | Modify | Add `land_mask: Option<&LandMask>` param; one `continue` in inner loop |
| `src/web/api.rs` | Modify | Add `land_mask` field to `AppState`; `land_mask()` helper; pass to `run_isochrone` |
| `src/web/server.rs` | Modify | Load `LandMask` at startup; store in `AppState` |

---

## Task 1: `src/land_mask.rs` — Core module

**Files:**
- Create: `src/land_mask.rs`

**Interfaces:**
- Produces: `pub struct LandMask`, `pub fn from_geojson(path: &str, resolution_deg: f64) -> Result<LandMask, Box<dyn std::error::Error>>`, `pub fn is_land(&self, lat: f64, lon: f64) -> bool`
- Also produces (pub for tests): none — `from_geojson_value` stays private but accessible to the in-file `#[cfg(test)]` module

- [ ] **Step 1: Write failing tests for `is_land`**

Add the test module at the bottom of the (not-yet-existing) `src/land_mask.rs`. These tests use a synthetic GeoJSON value — no file I/O, no external data needed.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_mask() -> LandMask {
        // A 1°×1° land square: lon 10–11, lat 40–41 (inside Med bbox)
        let json = serde_json::json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [[[10.0, 40.0], [11.0, 40.0], [11.0, 41.0], [10.0, 41.0], [10.0, 40.0]]]
                },
                "properties": {}
            }]
        });
        from_geojson_value(&json, 0.05).unwrap()
    }

    #[test]
    fn test_is_land_inside_polygon() {
        let mask = synthetic_mask();
        assert!(mask.is_land(40.5, 10.5), "center of polygon should be land");
    }

    #[test]
    fn test_is_sea_outside_polygon() {
        let mask = synthetic_mask();
        assert!(!mask.is_land(39.5, 10.5), "south of polygon should be sea");
        assert!(!mask.is_land(40.5, 9.5),  "west of polygon should be sea");
    }

    #[test]
    fn test_is_sea_outside_bbox() {
        let mask = synthetic_mask();
        assert!(!mask.is_land(60.0, 10.5), "above lat 48 is outside bbox");
        assert!(!mask.is_land(40.5, 50.0), "east of lon 42 is outside bbox");
        assert!(!mask.is_land(27.0, 10.5), "below lat 28 is outside bbox");
    }

    #[test]
    fn test_multipolygon_geometry() {
        let json = serde_json::json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "geometry": {
                    "type": "MultiPolygon",
                    "coordinates": [
                        [[[10.0, 40.0], [11.0, 40.0], [11.0, 41.0], [10.0, 41.0], [10.0, 40.0]]],
                        [[[15.0, 35.0], [16.0, 35.0], [16.0, 36.0], [15.0, 36.0], [15.0, 35.0]]]
                    ]
                },
                "properties": {}
            }]
        });
        let mask = from_geojson_value(&json, 0.05).unwrap();
        assert!(mask.is_land(40.5, 10.5));
        assert!(mask.is_land(35.5, 15.5));
        assert!(!mask.is_land(38.0, 13.0)); // gap between polygons
    }

    #[test]
    fn test_from_geojson_missing_file() {
        let result = LandMask::from_geojson("/nonexistent/path.geojson", 0.05);
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cd /home/aboni/dev/rust_nmea_router
cargo test land_mask 2>&1 | head -20
```

Expected: compile error — `land_mask` module does not exist yet.

- [ ] **Step 3: Create `src/land_mask.rs` with the full implementation**

```rust
use std::error::Error;

const LAT_MIN: f64 = 28.0;
const LON_MIN: f64 = -8.0;
const LAT_RANGE: f64 = 20.0; // 28..48
const LON_RANGE: f64 = 50.0; // -8..42

pub struct LandMask {
    lat_min: f64,
    lat_step: f64,
    rows: usize,
    lon_min: f64,
    lon_step: f64,
    cols: usize,
    grid: Vec<u8>,
}

impl LandMask {
    pub fn from_geojson(path: &str, resolution_deg: f64) -> Result<Self, Box<dyn Error>> {
        let contents = std::fs::read_to_string(path)?;
        let json: serde_json::Value = serde_json::from_str(&contents)?;
        from_geojson_value(&json, resolution_deg)
    }

    pub fn is_land(&self, lat: f64, lon: f64) -> bool {
        let lat_max = self.lat_min + self.lat_step * self.rows as f64;
        let lon_max = self.lon_min + self.lon_step * self.cols as f64;
        if lat < self.lat_min || lat >= lat_max { return false; }
        if lon < self.lon_min || lon >= lon_max { return false; }
        let row = ((lat - self.lat_min) / self.lat_step) as usize;
        let col = ((lon - self.lon_min) / self.lon_step) as usize;
        if row >= self.rows || col >= self.cols { return false; }
        let idx = row * self.cols + col;
        (self.grid[idx / 8] >> (idx % 8)) & 1 == 1
    }
}

fn from_geojson_value(json: &serde_json::Value, resolution_deg: f64) -> Result<LandMask, Box<dyn Error>> {
    let lat_step = resolution_deg;
    let lon_step = resolution_deg;
    let rows = (LAT_RANGE / lat_step).ceil() as usize;
    let cols = (LON_RANGE / lon_step).ceil() as usize;
    let mut grid = vec![0u8; (rows * cols + 7) / 8];

    let features = json["features"]
        .as_array()
        .ok_or("GeoJSON missing 'features' array")?;

    for feature in features {
        let geom = &feature["geometry"];
        for ring in extract_rings(geom) {
            rasterize_ring(&ring, &mut grid, LAT_MIN, lat_step, rows, LON_MIN, lon_step, cols);
        }
    }

    Ok(LandMask { lat_min: LAT_MIN, lat_step, rows, lon_min: LON_MIN, lon_step, cols, grid })
}

fn extract_rings(geom: &serde_json::Value) -> Vec<Vec<[f64; 2]>> {
    let mut rings = Vec::new();
    match geom["type"].as_str() {
        Some("Polygon") => {
            if let Some(poly_rings) = geom["coordinates"].as_array() {
                for ring_arr in poly_rings {
                    if let Some(ring) = parse_ring(ring_arr) {
                        rings.push(ring);
                    }
                }
            }
        }
        Some("MultiPolygon") => {
            if let Some(polys) = geom["coordinates"].as_array() {
                for poly in polys {
                    if let Some(poly_rings) = poly.as_array() {
                        for ring_arr in poly_rings {
                            if let Some(ring) = parse_ring(ring_arr) {
                                rings.push(ring);
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
    rings
}

fn parse_ring(arr: &serde_json::Value) -> Option<Vec<[f64; 2]>> {
    arr.as_array()?.iter().map(|pt| {
        let coords = pt.as_array()?;
        let lon = coords.get(0)?.as_f64()?;
        let lat = coords.get(1)?.as_f64()?;
        Some([lon, lat])
    }).collect()
}

fn rasterize_ring(
    ring: &[[f64; 2]],
    grid: &mut Vec<u8>,
    lat_min: f64, lat_step: f64, rows: usize,
    lon_min: f64, lon_step: f64, cols: usize,
) {
    if ring.len() < 3 { return; }

    let lat_min_ring = ring.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let lat_max_ring = ring.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
    let lat_bound_max = lat_min + lat_step * rows as f64;

    if lat_max_ring < lat_min || lat_min_ring >= lat_bound_max { return; }

    let row_start = ((lat_min_ring - lat_min) / lat_step).floor().max(0.0) as usize;
    let row_end = (((lat_max_ring - lat_min) / lat_step).ceil() as usize).min(rows);
    let lon_bound_max = lon_min + lon_step * cols as f64;
    let n = ring.len();

    for row in row_start..row_end {
        let lat_c = lat_min + (row as f64 + 0.5) * lat_step;
        let mut crossings: Vec<f64> = Vec::new();

        for i in 0..n {
            let a = ring[i];
            let b = ring[(i + 1) % n];
            let (lat_a, lat_b) = (a[1], b[1]);
            if (lat_a <= lat_c && lat_b > lat_c) || (lat_b <= lat_c && lat_a > lat_c) {
                let t = (lat_c - lat_a) / (lat_b - lat_a);
                let lon_x = a[0] + t * (b[0] - a[0]);
                if lon_x >= lon_min && lon_x < lon_bound_max {
                    crossings.push(lon_x);
                }
            }
        }

        if crossings.is_empty() { continue; }
        crossings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let mut i = 0;
        while i + 1 < crossings.len() {
            let col_start = ((crossings[i] - lon_min) / lon_step).floor().max(0.0) as usize;
            let col_end = (((crossings[i + 1] - lon_min) / lon_step).ceil() as usize).min(cols);
            for col in col_start..col_end {
                let idx = row * cols + col;
                grid[idx / 8] |= 1 << (idx % 8);
            }
            i += 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_mask() -> LandMask {
        let json = serde_json::json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [[[10.0, 40.0], [11.0, 40.0], [11.0, 41.0], [10.0, 41.0], [10.0, 40.0]]]
                },
                "properties": {}
            }]
        });
        from_geojson_value(&json, 0.05).unwrap()
    }

    #[test]
    fn test_is_land_inside_polygon() {
        let mask = synthetic_mask();
        assert!(mask.is_land(40.5, 10.5));
    }

    #[test]
    fn test_is_sea_outside_polygon() {
        let mask = synthetic_mask();
        assert!(!mask.is_land(39.5, 10.5));
        assert!(!mask.is_land(40.5, 9.5));
    }

    #[test]
    fn test_is_sea_outside_bbox() {
        let mask = synthetic_mask();
        assert!(!mask.is_land(60.0, 10.5));
        assert!(!mask.is_land(40.5, 50.0));
        assert!(!mask.is_land(27.0, 10.5));
    }

    #[test]
    fn test_multipolygon_geometry() {
        let json = serde_json::json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "geometry": {
                    "type": "MultiPolygon",
                    "coordinates": [
                        [[[10.0, 40.0], [11.0, 40.0], [11.0, 41.0], [10.0, 41.0], [10.0, 40.0]]],
                        [[[15.0, 35.0], [16.0, 35.0], [16.0, 36.0], [15.0, 36.0], [15.0, 35.0]]]
                    ]
                },
                "properties": {}
            }]
        });
        let mask = from_geojson_value(&json, 0.05).unwrap();
        assert!(mask.is_land(40.5, 10.5));
        assert!(mask.is_land(35.5, 15.5));
        assert!(!mask.is_land(38.0, 13.0));
    }

    #[test]
    fn test_from_geojson_missing_file() {
        let result = LandMask::from_geojson("/nonexistent/path.geojson", 0.05);
        assert!(result.is_err());
    }
}
```

Also add a test-only helper to `LandMask` impl (still inside `src/land_mask.rs`, outside the test module, so routing tests can call it across files):

```rust
impl LandMask {
    // ... existing pub fn from_geojson and pub fn is_land ...

    #[cfg(test)]
    pub fn from_geojson_value_for_test(
        json: &serde_json::Value,
        resolution_deg: f64,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        from_geojson_value(json, resolution_deg)
    }
}
```

- [ ] **Step 4: Register the module in `src/main.rs`**

Add after `pub mod polars;` (line 28):

```rust
pub mod land_mask;
```

- [ ] **Step 5: Run the land_mask tests**

```bash
cargo test land_mask 2>&1
```

Expected output (all 5 tests pass):
```
test land_mask::tests::test_from_geojson_missing_file ... ok
test land_mask::tests::test_is_land_inside_polygon ... ok
test land_mask::tests::test_is_sea_outside_bbox ... ok
test land_mask::tests::test_is_sea_outside_polygon ... ok
test land_mask::tests::test_multipolygon_geometry ... ok
```

---

## Task 2: Config additions

**Files:**
- Modify: `src/config.rs`
- Modify: `config.example.json`

**Interfaces:**
- Consumes: nothing new
- Produces: `Config.land_mask_path: Option<String>`, `Config.land_mask_resolution_deg: f64` (default `0.05`)

- [ ] **Step 1: Write the failing config test**

Add to the `#[cfg(test)] mod tests` block at the bottom of `src/config.rs`:

```rust
#[test]
fn test_land_mask_config_defaults() {
    let json = r#"{
        "can": {"interface": "vcan0", "enabled": false},
        "time": {"skew_threshold_ms": 500},
        "database": {
            "connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"},
            "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30},
            "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}
        }
    }"#;
    let config: Config = serde_json::from_str(json).unwrap();
    assert!(config.land_mask_path.is_none());
    assert!((config.land_mask_resolution_deg - 0.05).abs() < 1e-9);
}

#[test]
fn test_land_mask_config_explicit() {
    let json = r#"{
        "can": {"interface": "vcan0", "enabled": false},
        "time": {"skew_threshold_ms": 500},
        "land_mask_path": "/etc/nmea_router/land.geojson",
        "land_mask_resolution_deg": 0.03,
        "database": {
            "connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"},
            "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30},
            "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}
        }
    }"#;
    let config: Config = serde_json::from_str(json).unwrap();
    assert_eq!(config.land_mask_path.as_deref(), Some("/etc/nmea_router/land.geojson"));
    assert!((config.land_mask_resolution_deg - 0.03).abs() < 1e-9);
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

```bash
cargo test config::tests::test_land_mask 2>&1 | head -20
```

Expected: compile error — fields do not exist yet.

- [ ] **Step 3: Add the new fields to `Config` in `src/config.rs`**

After the existing `polars_file_path` field in the `Config` struct (around line 58), add:

```rust
    /// Path to GeoJSON land polygon file for land avoidance in isochrone routing.
    /// When absent, land avoidance is disabled.
    #[serde(default)]
    pub land_mask_path: Option<String>,
    /// Grid resolution in degrees for the land mask raster (default 0.05 ≈ 3 nm).
    #[serde(default = "default_land_mask_resolution_deg")]
    pub land_mask_resolution_deg: f64,
```

Add the default function alongside the other `default_*` functions (before the `impl Config` block):

```rust
fn default_land_mask_resolution_deg() -> f64 {
    0.05
}
```

Update `Config::new_default_instance()` to include the new fields. Find the closing brace of the struct literal and add before it:

```rust
            polars_file_path: None,
            land_mask_path: None,
            land_mask_resolution_deg: default_land_mask_resolution_deg(),
```

(The existing `polars_file_path: None,` line is already there; add the two new lines directly after it.)

- [ ] **Step 4: Update `config.example.json`**

After the existing `"polars_file_path"` line, add:

```json
  "land_mask_path": "/etc/nmea_router/land.geojson",
  "land_mask_resolution_deg": 0.05,
```

- [ ] **Step 5: Run the config tests**

```bash
cargo test config::tests 2>&1 | tail -10
```

Expected: all config tests pass, including the two new ones.

---

## Task 3: Routing integration

**Files:**
- Modify: `src/routing.rs`

**Interfaces:**
- Consumes: `crate::land_mask::LandMask::is_land(&self, lat: f64, lon: f64) -> bool`
- Produces: updated `pub fn run_isochrone(..., land_mask: Option<&crate::land_mask::LandMask>) -> IsochroneResult`

- [ ] **Step 1: Write the failing test**

Add to `src/routing.rs` `#[cfg(test)] mod tests`:

```rust
#[test]
fn test_land_mask_blocks_candidate() {
    // A mask with a 10°×10° land square covering lon 7–17, lat 43–53
    // (chosen to block the northward heading from our test origin)
    let json = serde_json::json!({
        "type": "FeatureCollection",
        "features": [{
            "type": "Feature",
            "geometry": {
                "type": "Polygon",
                "coordinates": [[[7.0, 43.0], [17.0, 43.0], [17.0, 53.0], [7.0, 53.0], [7.0, 43.0]]]
            },
            "properties": {}
        }]
    });
    let mask = crate::land_mask::LandMask::from_geojson_value_for_test(&json, 0.1).unwrap();

    let from = (42.0, 8.0);  // just south of the land square
    let to   = (43.29, 8.0); // inside the land square — should not be reachable
    let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
    let polars = dummy_polars();

    let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, &polars, &[], Some(&mask));
    // The destination is inside the land square. The frontier can never get within 3 nm
    // of it (all candidates above 43°N are blocked), so reached_destination must be false.
    assert!(!result.reached_destination, "should not reach destination blocked by land");
}
```

The `from_geojson_value_for_test` method was added to `LandMask` in Task 1 — no changes to `src/land_mask.rs` needed here.

- [ ] **Step 2: Update all existing tests in `src/routing.rs` to pass `None`**

Every existing `run_isochrone(...)` call in the test module needs a new final argument `None`. The calls in `test_isochrone_reaches_nearby_destination`, `test_backtrack_produces_monotonic_timestamps`, and `test_sail_weight_prefers_sailing_candidate` all call `run_isochrone`. Update each:

```rust
// Before:
let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, &polars, &[]);
// After:
let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, &polars, &[], None);
```

(There are 2 such calls — in `test_isochrone_reaches_nearby_destination` and `test_backtrack_produces_monotonic_timestamps`. `test_sail_weight_prefers_sailing_candidate` does not call `run_isochrone` directly, it calls `prune_isochrone`.)

- [ ] **Step 3: Run tests to confirm they fail**

```bash
cargo test routing::tests 2>&1 | head -20
```

Expected: compile error — `run_isochrone` signature mismatch.

- [ ] **Step 4: Update `run_isochrone` in `src/routing.rs`**

Change the function signature (around line 62):

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
    land_mask: Option<&crate::land_mask::LandMask>,
) -> IsochroneResult {
```

Add the land filter immediately after `let new_pos = advance_position(...)` inside the heading loop (around line 126):

```rust
                let new_pos = advance_position(parent.lat, parent.lon, heading, speed_kn * STEP_HOURS);
                if land_mask.map_or(false, |m| m.is_land(new_pos.0, new_pos.1)) {
                    continue;
                }
```

- [ ] **Step 5: Run routing tests**

```bash
cargo test routing::tests 2>&1
```

Expected: all routing tests pass including the new `test_land_mask_blocks_candidate`.

---

## Task 4: Server & API wiring

**Files:**
- Modify: `src/web/api.rs` (lines 38–57 — `AppState` struct and impl)
- Modify: `src/web/api.rs` (around line 1723 — `get_optimal_route` handler)
- Modify: `src/web/server.rs` (lines 55–77 — startup and `AppState` construction)

**Interfaces:**
- Consumes: `crate::land_mask::LandMask` (from Task 1), `Config.land_mask_path`, `Config.land_mask_resolution_deg` (from Task 2), updated `run_isochrone` (from Task 3)
- Produces: `AppState.land_mask: Option<Arc<crate::land_mask::LandMask>>`

- [ ] **Step 1: Add `land_mask` to `AppState` in `src/web/api.rs`**

In the `AppState` struct (line 38), add after `pub polars`:

```rust
    pub land_mask: Option<std::sync::Arc<crate::land_mask::LandMask>>,
```

Add a helper method to the `impl AppState` block (after `pub fn polars()`):

```rust
    pub fn land_mask(&self) -> Option<&crate::land_mask::LandMask> {
        self.land_mask.as_deref()
    }
```

- [ ] **Step 2: Load `LandMask` in `src/web/server.rs`**

After the `polars` loading block (after line 66), add:

```rust
    let land_mask = config.land_mask_path.as_deref().and_then(|path| {
        match crate::land_mask::LandMask::from_geojson(path, config.land_mask_resolution_deg) {
            Ok(m) => {
                tracing::info!(path, "Land mask loaded");
                Some(std::sync::Arc::new(m))
            }
            Err(e) => {
                tracing::warn!(path, error = %e, "Failed to load land mask — land avoidance disabled");
                None
            }
        }
    });
```

In the `AppState { ... }` literal (around line 68), add after `polars,`:

```rust
        land_mask,
```

- [ ] **Step 3: Pass land mask in `get_optimal_route` in `src/web/api.rs`**

Find the `run_isochrone(...)` call (around line 1723) and add the final argument:

```rust
    let result = crate::routing::run_isochrone(
        (params.from_lat, params.from_lon),
        (params.to_lat, params.to_lon),
        departure,
        params.motoring_speed_kn,
        params.polar_efficiency,
        params.min_sail_speed_kn,
        params.sail_weight_kn,
        polars,
        &fetches,
        state.land_mask(),
    );
```

- [ ] **Step 4: Run full test suite**

```bash
cargo test 2>&1 | tail -20
```

Expected: all non-ignored tests pass. Zero compile errors or warnings about unused variables.

- [ ] **Step 5: Verify the build compiles clean**

```bash
cargo build 2>&1 | grep -E "^error|^warning\[" | head -20
```

Expected: no errors, no new warnings.
