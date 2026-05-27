# Polar-Aware Route Planner — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a vessel polar diagram to the route planner so speed at each leg is estimated from actual wind conditions rather than a fixed user-supplied value.

**Architecture:** A new `PolarTable` type in `src/polars.rs` loads a CSV polar diagram at startup and bilinearly interpolates boat speed from TWA and TWS. `generate_route_track` is rewritten as a step-by-step simulation that looks up wind at each position and queries the polar table; the fixed speed becomes a motoring-speed fallback. `RouteOverlayPoint` gains `speed_kn` and `twa_deg` fields so the frontend can display per-segment performance.

**Tech Stack:** Rust, csv (std only — no new dependencies), chrono, existing `FetchWithHourly` / `ForecastHourlyPoint` DB types, Leaflet frontend.

**Spec:** `docs/superpowers/specs/2026-05-19-polar-aware-routing.md`

---

## File map

| File | Change |
|---|---|
| `src/utilities.rs` | Add `advance_position` |
| `src/polars.rs` | New — `PolarTable`, CSV parser, interpolation |
| `src/config.rs` | Add `polars_file_path: Option<String>` to `Config` |
| `config.example.json` | Add `polars_file_path` entry |
| `src/web/server.rs` | Load polars, add to `AppState` |
| `src/web/api.rs` | Add `polars` field to `AppState`; rename `speed_kn` → `motoring_speed_kn`; update handler |
| `src/forecast.rs` | Add `compute_twa`, `RouteTrackPoint`; rewrite `generate_route_track`; update `compute_route_overlay` |
| `static/plan.html` | Rename label; update API param name; add speed/TWA to popups |

---

## Task 1 — `advance_position` utility

**Files:**
- Modify: `src/utilities.rs`

- [ ] **Step 1: Write the failing test**

Add inside the existing `#[cfg(test)]` block at the bottom of `src/utilities.rs`:

```rust
#[test]
fn test_advance_position_north() {
    // From 43.0°N 8.0°E, head due north 60 nm (≈ 1°)
    let (lat, lon) = advance_position(43.0, 8.0, 0.0, 60.0);
    assert!((lat - 44.0).abs() < 0.02, "lat={}", lat);
    assert!((lon - 8.0).abs() < 0.001, "lon={}", lon);
}

#[test]
fn test_advance_position_east() {
    // From 0.0°N 0.0°E, head due east 60 nm — lon should increase
    let (lat, lon) = advance_position(0.0, 0.0, 90.0, 60.0);
    assert!(lat.abs() < 0.01, "lat should stay near 0, got {}", lat);
    assert!(lon > 0.5, "lon should increase, got {}", lon);
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd /home/aboni/dev/rust_nmea_router && cargo test test_advance_position 2>&1 | tail -5
```

Expected: compile error — `advance_position` not found.

- [ ] **Step 3: Implement `advance_position`**

Add after `haversine_distance_nm` in `src/utilities.rs`:

```rust
/// Advance a position by `dist_nm` nautical miles along `bearing_deg` (0=N, 90=E).
/// Returns (new_lat_deg, new_lon_deg).
pub fn advance_position(lat_deg: f64, lon_deg: f64, bearing_deg: f64, dist_nm: f64) -> (f64, f64) {
    let r = 3440.065_f64; // Earth radius in nm
    let lat = lat_deg.to_radians();
    let lon = lon_deg.to_radians();
    let bearing = bearing_deg.to_radians();
    let d = dist_nm / r;

    let new_lat = (lat.sin() * d.cos() + lat.cos() * d.sin() * bearing.cos()).asin();
    let new_lon = lon
        + (bearing.sin() * d.sin() * lat.cos()).atan2(d.cos() - lat.sin() * new_lat.sin());

    (new_lat.to_degrees(), new_lon.to_degrees())
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd /home/aboni/dev/rust_nmea_router && cargo test test_advance_position 2>&1 | tail -5
```

Expected: `test test_advance_position_north ... ok`, `test test_advance_position_east ... ok`.

- [ ] **Step 5: Commit**

```bash
cd /home/aboni/dev/rust_nmea_router
git add src/utilities.rs
git commit -m "feat: add advance_position utility for polar-aware routing"
```

---

## Task 2 — `PolarTable` in `src/polars.rs`

**Files:**
- Create: `src/polars.rs`
- Create: `tests/fixtures/dufour40.csv` (copy)

- [ ] **Step 1: Copy the test fixture**

```bash
mkdir -p /home/aboni/dev/rust_nmea_router/tests/fixtures
cp /home/aboni/IdeaProjects/router/nmearouter/web_classic/dufour40.csv \
   /home/aboni/dev/rust_nmea_router/tests/fixtures/dufour40.csv
```

- [ ] **Step 2: Create `src/polars.rs` with just the struct and a stub**

```rust
pub struct PolarTable {
    twa_breakpoints: Vec<f64>,
    tws_breakpoints: Vec<f64>,
    speeds: Vec<Vec<Option<f64>>>,
}

impl PolarTable {
    pub fn from_csv(_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        unimplemented!()
    }

    pub fn boat_speed(&self, _twa_deg: f64, _tws_kn: f64) -> Option<f64> {
        unimplemented!()
    }
}
```

- [ ] **Step 3: Register the module in `src/main.rs`**

Add near the top where other `mod` declarations are:

```rust
pub mod polars;
```

- [ ] **Step 4: Write the failing tests**

Add at the bottom of `src/polars.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn load() -> PolarTable {
        PolarTable::from_csv("tests/fixtures/dufour40.csv").expect("load polar")
    }

    #[test]
    fn test_polar_loads_and_has_breakpoints() {
        let p = load();
        // The CSV has data at TWS 6, 8, 10, 12, 14, 16, 18, 20
        assert!(p.tws_breakpoints.contains(&6.0), "expected 6 kn: {:?}", p.tws_breakpoints);
        assert!(p.tws_breakpoints.contains(&20.0), "expected 20 kn: {:?}", p.tws_breakpoints);
        // TWA rows: 42, 52, 60, 75, 90, 110, 120, 135, 150, 180
        assert!(p.twa_breakpoints.contains(&42.0));
        assert!(p.twa_breakpoints.contains(&180.0));
    }

    #[test]
    fn test_polar_exact_lookup_twa90_tws10() {
        let p = load();
        // From CSV: TWA=90, TWS=10 → 7.44 kn
        let spd = p.boat_speed(90.0, 10.0).expect("should have value");
        assert!((spd - 7.44).abs() < 0.05, "got {}", spd);
    }

    #[test]
    fn test_polar_returns_none_below_min_tws() {
        let p = load();
        // Below 6 kn (lowest populated column) → None
        assert!(p.boat_speed(90.0, 3.0).is_none());
    }

    #[test]
    fn test_polar_returns_none_below_min_twa() {
        let p = load();
        // Below 42° (lowest TWA row) → None
        assert!(p.boat_speed(30.0, 10.0).is_none());
    }

    #[test]
    fn test_polar_interpolates_between_tws() {
        let p = load();
        // TWS=9 is midway between 8 (6.63) and 10 (7.44) at TWA=90
        let spd = p.boat_speed(90.0, 9.0).expect("should interpolate");
        assert!(spd > 6.63 && spd < 7.44, "got {}", spd);
    }

    #[test]
    fn test_polar_interpolates_between_twa() {
        let p = load();
        // TWA=82 is between 75 (7.69) and 90 (7.82) at TWS=12
        let spd = p.boat_speed(82.0, 12.0).expect("should interpolate");
        assert!(spd > 7.0 && spd < 8.0, "got {}", spd);
    }

    #[test]
    fn test_polar_clamps_tws_above_max() {
        let p = load();
        // TWS=30 → clamp to 20, same result as TWS=20 at TWA=90
        let spd_20 = p.boat_speed(90.0, 20.0).unwrap();
        let spd_30 = p.boat_speed(90.0, 30.0).unwrap();
        assert!((spd_30 - spd_20).abs() < 0.01);
    }
}
```

- [ ] **Step 5: Run tests to verify they fail (not just compile error)**

```bash
cd /home/aboni/dev/rust_nmea_router && cargo test polars::tests 2>&1 | tail -10
```

Expected: `panicked at 'not yet implemented'`.

- [ ] **Step 6: Implement `from_csv`**

Replace the stub in `src/polars.rs` with the full implementation:

```rust
use std::collections::BTreeMap;

pub struct PolarTable {
    twa_breakpoints: Vec<f64>,
    tws_breakpoints: Vec<f64>,
    speeds: Vec<Vec<Option<f64>>>,
}

impl PolarTable {
    pub fn from_csv(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let mut lines = content.lines();

        // Skip the comment row (first line)
        lines.next();

        // Parse header row: "angle,1,2,3,...,20"
        let header = lines.next().ok_or("missing header row")?;
        let cols: Vec<&str> = header.split(',').collect();
        // cols[0] = "angle", cols[1..] = tws values
        let tws_all: Vec<Option<f64>> = cols[1..]
            .iter()
            .map(|s| s.trim().parse::<f64>().ok())
            .collect();

        // Collect all data rows into (twa → BTreeMap<tws_idx → speed>)
        let mut rows: Vec<(f64, Vec<(usize, f64)>)> = Vec::new();
        for line in lines {
            let cells: Vec<&str> = line.split(',').collect();
            if cells.is_empty() { continue; }
            let twa = match cells[0].trim().parse::<f64>() {
                Ok(v) if v >= 0.0 => v,
                _ => continue,
            };
            let mut entries: Vec<(usize, f64)> = Vec::new();
            for (i, cell) in cells[1..].iter().enumerate() {
                if let Ok(spd) = cell.trim().parse::<f64>() {
                    entries.push((i, spd));
                }
            }
            if !entries.is_empty() {
                rows.push((twa, entries));
            }
        }

        if rows.is_empty() {
            return Err("no data rows found in polar CSV".into());
        }

        // Determine tws_breakpoints: columns that have at least one non-empty entry
        let mut tws_set: BTreeMap<usize, f64> = BTreeMap::new();
        for (_, entries) in &rows {
            for &(idx, _) in entries {
                if let Some(Some(tws)) = tws_all.get(idx) {
                    tws_set.insert(idx, *tws);
                }
            }
        }
        let tws_col_indices: Vec<usize> = tws_set.keys().copied().collect();
        let tws_breakpoints: Vec<f64> = tws_col_indices.iter().map(|i| tws_set[i]).collect();

        // Build twa_breakpoints (sorted)
        let mut twa_breakpoints: Vec<f64> = rows.iter().map(|(t, _)| *t).collect();
        twa_breakpoints.sort_by(|a, b| a.partial_cmp(b).unwrap());

        // Build speeds[twa_idx][tws_col_idx]
        let mut speed_map: BTreeMap<(usize, usize), f64> = BTreeMap::new();
        for (twa, entries) in &rows {
            let ti = twa_breakpoints
                .iter()
                .position(|t| (*t - twa).abs() < 0.01)
                .unwrap();
            for &(col_idx, spd) in entries {
                if let Some(si) = tws_col_indices.iter().position(|&c| c == col_idx) {
                    speed_map.insert((ti, si), spd);
                }
            }
        }

        let speeds: Vec<Vec<Option<f64>>> = (0..twa_breakpoints.len())
            .map(|ti| {
                (0..tws_breakpoints.len())
                    .map(|si| speed_map.get(&(ti, si)).copied())
                    .collect()
            })
            .collect();

        Ok(Self { twa_breakpoints, tws_breakpoints, speeds })
    }

    /// Boat speed in knots for given true wind angle (0–180°) and true wind speed (kn).
    /// Returns None when TWA < minimum polar angle or TWS < minimum populated column.
    /// Clamps TWS at the maximum populated column.
    pub fn boat_speed(&self, twa_deg: f64, tws_kn: f64) -> Option<f64> {
        let twa = twa_deg.clamp(0.0, 180.0);
        let max_tws = *self.tws_breakpoints.last()?;
        let tws = tws_kn.clamp(0.0, max_tws);

        if tws < self.tws_breakpoints[0] { return None; }
        if twa < self.twa_breakpoints[0] { return None; }

        // Find bracketing TWA indices
        let ti_hi = self.twa_breakpoints.partition_point(|&v| v < twa).min(self.twa_breakpoints.len() - 1);
        let ti = ti_hi.saturating_sub(1);

        // Find bracketing TWS indices
        let si_hi = self.tws_breakpoints.partition_point(|&v| v < tws).min(self.tws_breakpoints.len() - 1);
        let si = si_hi.saturating_sub(1);

        let t_frac = if ti == ti_hi || (self.twa_breakpoints[ti_hi] - self.twa_breakpoints[ti]).abs() < 1e-9 {
            0.0
        } else {
            (twa - self.twa_breakpoints[ti]) / (self.twa_breakpoints[ti_hi] - self.twa_breakpoints[ti])
        };

        let s_frac = if si == si_hi || (self.tws_breakpoints[si_hi] - self.tws_breakpoints[si]).abs() < 1e-9 {
            0.0
        } else {
            (tws - self.tws_breakpoints[si]) / (self.tws_breakpoints[si_hi] - self.tws_breakpoints[si])
        };

        let v00 = self.speeds[ti][si]?;
        let v10 = self.speeds[ti_hi][si].unwrap_or(v00);
        let v01 = self.speeds[ti][si_hi].unwrap_or(v00);
        let v11 = self.speeds[ti_hi][si_hi].unwrap_or(v00);

        Some(
            v00 * (1.0 - t_frac) * (1.0 - s_frac)
            + v10 * t_frac          * (1.0 - s_frac)
            + v01 * (1.0 - t_frac) * s_frac
            + v11 * t_frac          * s_frac,
        )
    }

    /// Test-only constructor: returns a polar that always yields `speed_kn`
    /// for any TWA ≥ 42° and TWS ≥ 5 kn, and None otherwise.
    #[cfg(test)]
    pub fn constant_for_test(speed_kn: f64) -> Self {
        Self {
            twa_breakpoints: vec![42.0, 180.0],
            tws_breakpoints: vec![5.0, 20.0],
            speeds: vec![
                vec![Some(speed_kn), Some(speed_kn)],
                vec![Some(speed_kn), Some(speed_kn)],
            ],
        }
    }
}
```

- [ ] **Step 7: Run tests to verify they pass**

```bash
cd /home/aboni/dev/rust_nmea_router && cargo test polars::tests 2>&1 | tail -15
```

Expected: all 7 tests pass.

- [ ] **Step 8: Run all tests to confirm no regressions**

```bash
cd /home/aboni/dev/rust_nmea_router && cargo test 2>&1 | tail -10
```

Expected: all existing tests still pass.

- [ ] **Step 9: Commit**

```bash
cd /home/aboni/dev/rust_nmea_router
git add src/polars.rs src/main.rs tests/fixtures/dufour40.csv
git commit -m "feat: add PolarTable — CSV polar diagram parser and bilinear interpolation"
```

---

## Task 3 — `compute_twa` and `RouteTrackPoint` in `src/forecast.rs`

**Files:**
- Modify: `src/forecast.rs`

- [ ] **Step 1: Write failing tests for `compute_twa`**

Add inside the existing `#[cfg(test)]` block at the bottom of `src/forecast.rs`:

```rust
#[test]
fn test_compute_twa_upwind() {
    // Heading north (0°), wind from north (0°) → TWA = 0°
    assert!((compute_twa(0.0, 0.0) - 0.0).abs() < 0.01, "got {}", compute_twa(0.0, 0.0));
}

#[test]
fn test_compute_twa_beam_reach_port() {
    // Heading north (0°), wind from east (90°) → TWA = 90°
    assert!((compute_twa(0.0, 90.0) - 90.0).abs() < 0.01, "got {}", compute_twa(0.0, 90.0));
}

#[test]
fn test_compute_twa_beam_reach_starboard() {
    // Heading north (0°), wind from west (270°) → TWA = 90°
    assert!((compute_twa(0.0, 270.0) - 90.0).abs() < 0.01, "got {}", compute_twa(0.0, 270.0));
}

#[test]
fn test_compute_twa_downwind() {
    // Heading north (0°), wind from south (180°) → TWA = 180°
    assert!((compute_twa(0.0, 180.0) - 180.0).abs() < 0.01, "got {}", compute_twa(0.0, 180.0));
}

#[test]
fn test_compute_twa_reaching_on_easterly_heading() {
    // Heading east (90°), wind from north (0°) → TWA = 90°
    assert!((compute_twa(90.0, 0.0) - 90.0).abs() < 0.01, "got {}", compute_twa(90.0, 0.0));
}
```

- [ ] **Step 2: Run to verify they fail**

```bash
cd /home/aboni/dev/rust_nmea_router && cargo test test_compute_twa 2>&1 | tail -5
```

Expected: compile error — `compute_twa` not found.

- [ ] **Step 3: Add `compute_twa` and `RouteTrackPoint` to `src/forecast.rs`**

Add after the existing `RouteOverlayPoint` struct definition (around line 42):

```rust
/// A synthetic track point produced by `generate_route_track`.
/// Carries the estimated boat speed and true wind angle computed at that step.
#[derive(Debug, Clone)]
pub struct RouteTrackPoint {
    pub lat: f64,
    pub lon: f64,
    pub time: DateTime<Utc>,
    pub speed_kn: Option<f64>,
    pub twa_deg: Option<f64>,
}
```

Add `speed_kn` and `twa_deg` fields to `RouteOverlayPoint` (the existing struct at the top of `forecast.rs`):

```rust
#[derive(Debug, serde::Serialize, Clone)]
pub struct RouteOverlayPoint {
    pub lat: f64,
    pub lon: f64,
    pub timestamp: String,
    pub wind_speed_kn: Option<f64>,
    pub wind_direction_deg: Option<f64>,
    pub wind_gust_kn: Option<f64>,
    pub wave_height_m: Option<f64>,
    pub wave_period_s: Option<f64>,
    pub wave_direction_deg: Option<f64>,
    pub cape_j_kg: Option<f64>,
    pub speed_kn: Option<f64>,     // actual boat speed at this point
    pub twa_deg: Option<f64>,      // true wind angle at this point
}
```

Add the `compute_twa` function after the constants block (after `const MAX_DISTANCE_NM`):

```rust
/// Normalise true wind angle to 0–180°.
/// `cog_deg`: vessel course over ground (0–360°).
/// `wind_dir_deg`: meteorological wind direction wind is coming FROM (0–360°).
pub fn compute_twa(cog_deg: f64, wind_dir_deg: f64) -> f64 {
    let diff = (wind_dir_deg - cog_deg).rem_euclid(360.0);
    if diff <= 180.0 { diff } else { 360.0 - diff }
}
```

- [ ] **Step 4: Fix the two sites that now fail to compile because `RouteOverlayPoint` has new fields**

In `compute_route_overlay` (around line 368), the struct literal now needs the two new fields. Add them as `None` placeholders — they'll be properly filled in Task 5. Find this block:

```rust
Some(RouteOverlayPoint {
    lat: *lat,
    lon: *lon,
    timestamp: ts.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    wind_speed_kn: interp.wind_speed_kn,
    wind_direction_deg: interp.wind_direction_deg,
    wind_gust_kn: interp.wind_gust_kn,
    wave_height_m: interp.wave_height_m,
    wave_period_s: interp.wave_period_s,
    wave_direction_deg: interp.wave_direction_deg,
    cape_j_kg: interp.cape_j_kg,
})
```

Replace with:

```rust
Some(RouteOverlayPoint {
    lat: *lat,
    lon: *lon,
    timestamp: ts.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    wind_speed_kn: interp.wind_speed_kn,
    wind_direction_deg: interp.wind_direction_deg,
    wind_gust_kn: interp.wind_gust_kn,
    wave_height_m: interp.wave_height_m,
    wave_period_s: interp.wave_period_s,
    wave_direction_deg: interp.wave_direction_deg,
    cape_j_kg: interp.cape_j_kg,
    speed_kn: None,
    twa_deg: None,
})
```

- [ ] **Step 5: Run tests**

```bash
cd /home/aboni/dev/rust_nmea_router && cargo test test_compute_twa 2>&1 | tail -10
```

Expected: all 5 `test_compute_twa_*` tests pass.

```bash
cd /home/aboni/dev/rust_nmea_router && cargo test 2>&1 | tail -5
```

Expected: all existing tests still pass.

- [ ] **Step 6: Commit**

```bash
cd /home/aboni/dev/rust_nmea_router
git add src/forecast.rs
git commit -m "feat: add compute_twa, RouteTrackPoint, and speed_kn/twa_deg fields on RouteOverlayPoint"
```

---

## Task 4 — Rewrite `generate_route_track` as a step simulation

**Files:**
- Modify: `src/forecast.rs`

This task replaces the linear interpolation approach with a wind-aware step simulation. `generate_route_track` now returns `Vec<RouteTrackPoint>` and takes `motoring_speed_kn`, optional polars, and `fetches`.

- [ ] **Step 1: Add the `nearest_forecast_wind` private helper to `src/forecast.rs`**

Add this function before `generate_route_track` (it is needed by the new implementation). This compiles cleanly on its own:

```rust
/// Finds IDW-interpolated wind speed and direction at (lat, lon, time).
/// Returns None when no forecast data is available within range.
fn nearest_forecast_wind(
    fetches: &[FetchWithHourly],
    lat: f64,
    lon: f64,
    time: DateTime<Utc>,
) -> Option<(f64, f64)> {
    let samples: Vec<(f64, f64, ForecastHourlyPoint)> = fetches
        .iter()
        .filter_map(|f| nearest_hourly(&f.hourly, time).map(|pt| (f.lat, f.lon, pt)))
        .collect();
    let interp = interpolate_idw(lat, lon, &samples)?;
    Some((interp.wind_speed_kn?, interp.wind_direction_deg?))
}
```

Run `cargo build` to verify it compiles cleanly before proceeding.

- [ ] **Step 2: Replace `generate_leg` and `generate_route_track` with the new implementations**

Remove the existing `generate_leg` function (lines 295–322 approximately) and replace `generate_route_track` entirely. The new code:

```rust
/// Generates a synthetic route track using a wind-aware step simulation.
/// At each 1-hour step the boat speed is derived from the polar diagram and
/// the forecast wind at the current position; the motoring speed is used as
/// a fallback when wind data is unavailable or the angle is too close to
/// head-to-wind (TWA < minimum polar angle) or wind too light (< minimum TWS).
///
/// Returns an empty vec for fewer than 2 waypoints.
pub fn generate_route_track(
    waypoints: &[(f64, f64)],
    departure: DateTime<Utc>,
    motoring_speed_kn: f64,
    polars: Option<&crate::polars::PolarTable>,
    fetches: &[FetchWithHourly],
) -> Vec<RouteTrackPoint> {
    if waypoints.len() < 2 || motoring_speed_kn <= 0.0 {
        return vec![];
    }

    let mut track: Vec<RouteTrackPoint> = Vec::new();
    let mut leg_start_time = departure;

    for w in waypoints.windows(2) {
        let (from_lat, from_lon) = w[0];
        let (to_lat, to_lon) = w[1];

        let mut pos = (from_lat, from_lon);
        let mut t = leg_start_time;

        // Emit the leg start point
        if track.is_empty() {
            track.push(RouteTrackPoint { lat: pos.0, lon: pos.1, time: t, speed_kn: None, twa_deg: None });
        }

        loop {
            let remaining_nm = crate::utilities::haversine_distance_nm(pos.0, pos.1, to_lat, to_lon);
            if remaining_nm < 0.1 {
                break;
            }

            let bearing = crate::utilities::haversine_heading(pos.0, pos.1, to_lat, to_lon);

            let (speed_kn, twa) = match (nearest_forecast_wind(fetches, pos.0, pos.1, t), polars) {
                (Some((wind_spd, wind_dir)), Some(p)) if wind_spd >= 5.0 => {
                    let twa = compute_twa(bearing, wind_dir);
                    let spd = p.boat_speed(twa, wind_spd).unwrap_or(motoring_speed_kn);
                    (spd, Some(twa))
                }
                _ => (motoring_speed_kn, None),
            };

            let hours_to_wp = remaining_nm / speed_kn;
            let step_hours = hours_to_wp.min(1.0);
            let dist_nm = speed_kn * step_hours;

            pos = crate::utilities::advance_position(pos.0, pos.1, bearing, dist_nm);
            t = t + Duration::seconds((step_hours * 3600.0).round() as i64);

            track.push(RouteTrackPoint { lat: pos.0, lon: pos.1, time: t, speed_kn: Some(speed_kn), twa_deg: twa });

            if hours_to_wp <= 1.0 {
                break;
            }
        }

        leg_start_time = t;
    }

    track
}
```

- [ ] **Step 3: Fix existing test call sites**

The new signature breaks five existing test calls. Find them all inside the `#[cfg(test)]` block and update each `generate_route_track(&wpts, dep, N.N)` call to `generate_route_track(&wpts, dep, N.N, None, &[])`:

- `test_generate_route_track_point_count`
- `test_generate_route_track_timestamps_advance_hourly`
- `test_generate_route_track_empty_and_single_waypoint` (two calls)
- `test_generate_route_track_two_legs`

Also update `test_compute_route_overlay_returns_points_with_coords` — but only fix the `generate_route_track` call here; leave the `compute_route_overlay` call as-is (it will be fixed in Task 5).

Run `cargo build` after this step to confirm it compiles cleanly.

- [ ] **Step 4: Write the new polar-aware test**

Add inside the `#[cfg(test)]` block:

```rust
#[test]
fn test_generate_route_track_uses_polar_speed() {
    use crate::polars::PolarTable;
    // Constant polar: 7 kn at any valid angle/speed
    let polars = PolarTable::constant_for_test(7.0);

    // Wind blowing from 180° (south) while heading north → TWA=180° → downwind
    let ts_str = "2026-06-01T06:00:00Z";
    let hourly = vec![crate::db::operations::forecast::ForecastHourlyPoint {
        timestamp: ts_str.to_string(),
        wind_speed_kn: Some(12.0),
        wind_direction_deg: Some(180.0),  // from south
        wind_gust_kn: None, wave_height_m: None, wave_period_s: None,
        wave_direction_deg: None, cape_j_kg: None,
    }];
    let fetches = vec![crate::db::operations::forecast::FetchWithHourly {
        lat: 43.5, lon: 8.0, hourly,
    }];

    let dep = chrono::DateTime::parse_from_rfc3339(ts_str).unwrap().with_timezone(&chrono::Utc);
    // 7 nm north from 43.0 → should take exactly 1 hour at 7 kn
    let wpts = vec![(43.0_f64, 8.0_f64), (43.12_f64, 8.0_f64)];
    let track = generate_route_track(&wpts, dep, 5.0, Some(&polars), &fetches);

    // At least 2 points, and speed_kn should be 7.0 (from polar, not 5.0 motoring)
    assert!(track.len() >= 2, "expected ≥2 points, got {}", track.len());
    let spd = track[1].speed_kn.expect("speed_kn should be set");
    assert!((spd - 7.0).abs() < 0.1, "expected polar speed 7.0, got {}", spd);
}

#[test]
fn test_generate_route_track_falls_back_to_motoring_no_wind() {
    // No forecast data → motoring speed used throughout
    let polars = crate::polars::PolarTable::constant_for_test(7.0);
    let dep = chrono::Utc::now();
    let wpts = vec![(43.0_f64, 8.0_f64), (43.12_f64, 8.0_f64)];
    let track = generate_route_track(&wpts, dep, 5.0, Some(&polars), &[]);

    assert!(track.len() >= 2);
    // First intermediate point has speed_kn = 5.0 (motoring)
    let spd = track[1].speed_kn.expect("speed_kn should be set");
    assert!((spd - 5.0).abs() < 0.1, "expected motoring speed 5.0, got {}", spd);
}
```

- [ ] **Step 5: Run the new tests**

```bash
cd /home/aboni/dev/rust_nmea_router && cargo test test_generate_route_track 2>&1 | tail -15
```

Expected: all `test_generate_route_track_*` tests pass.

- [ ] **Step 6: Run all tests**

```bash
cd /home/aboni/dev/rust_nmea_router && cargo test 2>&1 | tail -10
```

Expected: all tests pass (the `compute_route_overlay` tests may fail until Task 5 — if so, check the error and confirm it's only the signature mismatch).

- [ ] **Step 7: Commit**

```bash
cd /home/aboni/dev/rust_nmea_router
git add src/forecast.rs
git commit -m "feat: rewrite generate_route_track as polar-aware step simulation"
```

---

## Task 5 — Update `compute_route_overlay` to use `RouteTrackPoint`

**Files:**
- Modify: `src/forecast.rs`

- [ ] **Step 1: Update `compute_route_overlay` signature and body**

Find the current `compute_route_overlay` function (takes `track: &[(f64, f64, DateTime<Utc>)]`) and replace it entirely:

```rust
/// IDW-interpolates forecast values at each synthetic track point.
/// Carries `speed_kn` and `twa_deg` from the track point into the overlay.
/// Points for which no forecast data is available within range are omitted.
pub fn compute_route_overlay(
    track: &[RouteTrackPoint],
    fetches: &[FetchWithHourly],
) -> Vec<RouteOverlayPoint> {
    track
        .iter()
        .filter_map(|pt| {
            let samples: Vec<(f64, f64, ForecastHourlyPoint)> = fetches
                .iter()
                .filter_map(|f| {
                    nearest_hourly(&f.hourly, pt.time).map(|h| (f.lat, f.lon, h))
                })
                .collect();
            let interp = interpolate_idw(pt.lat, pt.lon, &samples)?;
            Some(RouteOverlayPoint {
                lat: pt.lat,
                lon: pt.lon,
                timestamp: pt.time.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                wind_speed_kn: interp.wind_speed_kn,
                wind_direction_deg: interp.wind_direction_deg,
                wind_gust_kn: interp.wind_gust_kn,
                wave_height_m: interp.wave_height_m,
                wave_period_s: interp.wave_period_s,
                wave_direction_deg: interp.wave_direction_deg,
                cape_j_kg: interp.cape_j_kg,
                speed_kn: pt.speed_kn,
                twa_deg: pt.twa_deg,
            })
        })
        .collect()
}
```

- [ ] **Step 2: Update `test_compute_route_overlay_returns_points_with_coords`**

This test currently builds a track from tuples. Update it to build `Vec<RouteTrackPoint>` instead, or call `generate_route_track` and use its output directly. The simplest fix is to build the track via `generate_route_track`:

```rust
#[test]
fn test_compute_route_overlay_returns_points_with_coords() {
    use crate::db::operations::forecast::{FetchWithHourly, ForecastHourlyPoint};
    let dep = chrono::Utc::now();
    let wpts = vec![(43.5_f64, 10.0_f64), (43.6_f64, 10.1_f64)];
    let track = generate_route_track(&wpts, dep, 10.0, None, &[]);

    let hourly: Vec<ForecastHourlyPoint> = track.iter().map(|pt| ForecastHourlyPoint {
        timestamp: pt.time.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        wind_speed_kn: Some(10.0),
        wind_direction_deg: Some(180.0),
        wind_gust_kn: None,
        wave_height_m: None,
        wave_period_s: None,
        wave_direction_deg: None,
        cape_j_kg: None,
    }).collect();

    let fetches = vec![FetchWithHourly { lat: 43.55, lon: 10.05, hourly }];
    let overlay = compute_route_overlay(&track, &fetches);

    assert!(!overlay.is_empty(), "expected at least one overlay point");
    for p in &overlay {
        assert!(p.lat > 43.4 && p.lat < 43.7);
        assert!(p.lon > 9.9 && p.lon < 10.2);
    }
}
```

- [ ] **Step 3: Run all tests**

```bash
cd /home/aboni/dev/rust_nmea_router && cargo test 2>&1 | tail -10
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
cd /home/aboni/dev/rust_nmea_router
git add src/forecast.rs
git commit -m "feat: update compute_route_overlay to use RouteTrackPoint with speed_kn and twa_deg"
```

---

## Task 6 — Config, AppState, and startup wiring

**Files:**
- Modify: `src/config.rs`
- Modify: `config.example.json`
- Modify: `src/web/api.rs`
- Modify: `src/web/server.rs`

- [ ] **Step 1: Add `polars_file_path` to `Config`**

In `src/config.rs`, add to the `Config` struct after the `sync` field:

```rust
/// Path to polar diagram CSV. Optional — when absent the route planner uses a fixed speed.
#[serde(default)]
pub polars_file_path: Option<String>,
```

- [ ] **Step 2: Add the example entry to `config.example.json`**

Open `config.example.json` and add (alongside the other top-level keys):

```json
"polars_file_path": "/etc/nmea_router/polars.csv"
```

- [ ] **Step 3: Add `polars` field to `AppState`**

In `src/web/api.rs`, add to the `AppState` struct:

```rust
pub polars: Option<std::sync::Arc<crate::polars::PolarTable>>,
```

Add the helper method to the `impl AppState` block:

```rust
pub fn polars(&self) -> Option<&crate::polars::PolarTable> {
    self.polars.as_deref()
}
```

- [ ] **Step 4: Update `create_test_app` in `src/web/api.rs` tests**

Search for all `AppState {` struct literals inside `#[cfg(test)]` blocks and add `polars: None` to each. There are three sites (lines ~1775, ~2778, ~3284 approximately). Each one looks like:

```rust
AppState {
    db: Arc::new(RwLock::new(...)),
    config: Arc::new(Config { ... }),
    signalk_broadcast: ...,
    backup_in_progress: ...,
    jwt_secret: ...,
    ais_cache: ...,
    poller_status: ...,
}
```

Add `polars: None,` to each.

- [ ] **Step 5: Load polars in `src/web/server.rs`**

In `start_web_server`, after building `jwt_secret` and before building `state`, add:

```rust
let polars = config.polars_file_path.as_deref().and_then(|path| {
    match crate::polars::PolarTable::from_csv(path) {
        Ok(t) => {
            tracing::info!(path, "Polar table loaded");
            Some(std::sync::Arc::new(t))
        }
        Err(e) => {
            tracing::warn!(path, error = %e, "Failed to load polar table — fixed motoring speed will be used");
            None
        }
    }
});
```

Add `polars` to the `AppState` struct literal:

```rust
let state = AppState {
    db: db.clone(),
    config,
    signalk_broadcast,
    backup_in_progress: Arc::new(AtomicBool::new(false)),
    jwt_secret,
    ais_cache,
    poller_status: poller_status.clone(),
    polars,
};
```

- [ ] **Step 6: Build and run all tests**

```bash
cd /home/aboni/dev/rust_nmea_router && cargo build 2>&1 | tail -10
cd /home/aboni/dev/rust_nmea_router && cargo test 2>&1 | tail -10
```

Expected: clean build, all tests pass.

- [ ] **Step 7: Commit**

```bash
cd /home/aboni/dev/rust_nmea_router
git add src/config.rs config.example.json src/web/api.rs src/web/server.rs
git commit -m "feat: add polars_file_path config, PolarTable in AppState, polar loading at startup"
```

---

## Task 7 — Update `get_forecast_route` handler

**Files:**
- Modify: `src/web/api.rs`

- [ ] **Step 1: Rename `speed_kn` to `motoring_speed_kn` in `ForecastRouteQuery`**

Find the struct (around line 252):

```rust
pub struct ForecastRouteQuery {
    pub trip_id: u32,
    pub waypoints: String,
    pub departure: String,
    pub speed_kn: f64,
}
```

Replace with:

```rust
pub struct ForecastRouteQuery {
    pub trip_id: u32,
    pub waypoints: String,
    pub departure: String,
    pub motoring_speed_kn: f64,
}
```

- [ ] **Step 2: Update `get_forecast_route` to pass polars and rename the field**

Find the handler (around line 1625) and replace it entirely:

```rust
pub async fn get_forecast_route(
    State(state): State<AppState>,
    Query(params): Query<ForecastRouteQuery>,
) -> Result<Json<ApiResponse<Vec<crate::forecast::RouteOverlayPoint>>>, StatusCode> {
    let departure = match chrono::DateTime::parse_from_rfc3339(&params.departure) {
        Ok(dt) => dt.with_timezone(&chrono::Utc),
        Err(_) => {
            return Ok(Json(ApiResponse::error(format!(
                "Invalid departure timestamp: {}",
                params.departure
            ))));
        }
    };
    let waypoints = match crate::forecast::parse_waypoints(&params.waypoints) {
        Ok(w) => w,
        Err(e) => return Ok(Json(ApiResponse::error(e))),
    };
    if params.motoring_speed_kn <= 0.0 {
        return Ok(Json(ApiResponse::error("motoring_speed_kn must be positive".to_string())));
    }
    let fetches = match state.db().fetch_forecast_fetches(params.trip_id) {
        Ok(f) => f,
        Err(e) => {
            error!(error = %e, trip_id = params.trip_id, "Failed to load forecast fetches for route");
            return Ok(Json(ApiResponse::error(e.to_string())));
        }
    };
    let track = crate::forecast::generate_route_track(
        &waypoints,
        departure,
        params.motoring_speed_kn,
        state.polars(),
        &fetches,
    );
    let overlay = crate::forecast::compute_route_overlay(&track, &fetches);
    Ok(Json(ApiResponse::ok(overlay)))
}
```

- [ ] **Step 3: Build and run all tests**

```bash
cd /home/aboni/dev/rust_nmea_router && cargo build 2>&1 | tail -5
cd /home/aboni/dev/rust_nmea_router && cargo test 2>&1 | tail -10
```

Expected: clean build, all tests pass.

- [ ] **Step 4: Commit**

```bash
cd /home/aboni/dev/rust_nmea_router
git add src/web/api.rs
git commit -m "feat: update get_forecast_route to use polar-aware generate_route_track"
```

---

## Task 8 — Frontend changes in `static/plan.html`

**Files:**
- Modify: `static/plan.html`

- [ ] **Step 1: Rename the speed label**

Find (around line 70):

```html
<label style="color:var(--text-secondary);">Speed:
    <input type="number" id="speedInput" value="5.5" min="1" max="20" step="0.5"
```

Replace `Speed:` with `Motoring speed:`:

```html
<label style="color:var(--text-secondary);">Motoring speed:
    <input type="number" id="speedInput" value="5.5" min="1" max="20" step="0.5"
```

- [ ] **Step 2: Update `computeRoute()` to use `motoring_speed_kn` param**

Find (around line 751):

```js
`&departure=${encodeURIComponent(departure)}&speed_kn=${speed}`;
```

Replace with:

```js
`&departure=${encodeURIComponent(departure)}&motoring_speed_kn=${speed}`;
```

- [ ] **Step 3: Add `speed_kn` and `twa_deg` to segment popups in `drawRouteLine`**

Find the popup construction inside `drawRouteLine` (around line 791):

```js
`Wind: ${(p.wind_speed_kn || 0).toFixed(1)} kn ` +
```

The full popup string is currently something like:

```js
seg.bindPopup(
    `<b>${new Date(p.timestamp).toUTCString().slice(0,22)}</b><br>` +
    `Wind: ${(p.wind_speed_kn || 0).toFixed(1)} kn ` +
    `${p.wind_direction_deg != null ? Math.round(p.wind_direction_deg) + '°' : ''}<br>` +
    `Gust: ${p.wind_gust_kn != null ? p.wind_gust_kn.toFixed(1) + ' kn' : '—'}<br>` +
    `Wave: ${p.wave_height_m != null ? p.wave_height_m.toFixed(1) + ' m' : '—'} ` +
    `/ ${p.wave_period_s != null ? p.wave_period_s.toFixed(0) + ' s' : '—'}`
);
```

Add two lines for boat speed and TWA at the end of the popup string:

```js
seg.bindPopup(
    `<b>${new Date(p.timestamp).toUTCString().slice(0,22)}</b><br>` +
    `Wind: ${(p.wind_speed_kn || 0).toFixed(1)} kn ` +
    `${p.wind_direction_deg != null ? Math.round(p.wind_direction_deg) + '°' : ''}<br>` +
    `Gust: ${p.wind_gust_kn != null ? p.wind_gust_kn.toFixed(1) + ' kn' : '—'}<br>` +
    `Wave: ${p.wave_height_m != null ? p.wave_height_m.toFixed(1) + ' m' : '—'} ` +
    `/ ${p.wave_period_s != null ? p.wave_period_s.toFixed(0) + ' s' : '—'}<br>` +
    `Est. speed: ${p.speed_kn != null ? p.speed_kn.toFixed(1) + ' kn' : '—'} · ` +
    `TWA: ${p.twa_deg != null ? Math.round(p.twa_deg) + '°' : '—'}`
);
```

**Note:** Read `static/plan.html` first to find the exact current popup string before editing, as the wording may differ slightly from this summary. Match the surrounding context precisely.

- [ ] **Step 4: Verify in browser (manual)**

Start the server if available and open `plan.html` for an active trip. Draw a route, set a departure time and motoring speed, click Compute. Verify:
- The speed label reads "Motoring speed:"
- Clicking a route segment shows "Est. speed: X.X kn · TWA: XXX°" in the popup

- [ ] **Step 5: Commit**

```bash
cd /home/aboni/dev/rust_nmea_router
git add static/plan.html
git commit -m "feat: update plan.html — motoring speed label, polar speed/TWA in route popups"
```

---

## Final verification

- [ ] **Run full test suite**

```bash
cd /home/aboni/dev/rust_nmea_router && cargo test 2>&1 | tail -15
```

Expected: all tests pass (the two known DB infrastructure flaky tests may appear — `test_populate_sample_trips` and `test_two_legs` — those are pre-existing).

- [ ] **Run release build**

```bash
cd /home/aboni/dev/rust_nmea_router && cargo build --release 2>&1 | tail -5
```

Expected: clean build, no warnings in modified files.
