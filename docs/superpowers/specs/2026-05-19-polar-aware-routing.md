# Polar-Aware Route Planner — Design Spec

**Date:** 2026-05-19
**Status:** Approved

---

## Summary

Extend the existing polyline route planner in `plan.html` with polar diagram support. The user continues to draw a route and set a departure time, but boat speed at each step is now derived from the forecast wind conditions and the vessel's polar table rather than a single fixed speed. The fixed speed input becomes a **motoring speed** fallback used when the wind drops below 5 kn or the required heading is too close to the wind (TWA < 42°). ETAs and per-segment speed estimates reflect actual passage conditions.

This spec is a prerequisite for the isochrone weather routing spec.

---

## Files Changed

| File | Change |
|---|---|
| `src/polars.rs` | New — `PolarTable` struct, CSV parser, bilinear interpolation |
| `src/config.rs` | Add `polars_file_path: Option<String>` to `Config` |
| `config.example.json` | Add `polars_file_path` example entry |
| `src/main.rs` | Load polar at startup, add to `AppState` |
| `src/web/api.rs` | Add `polars` field to `AppState`; thread into route handler |
| `src/forecast.rs` | Replace linear interpolation in `generate_route_track` with step simulation using polars |
| `static/plan.html` | Rename "Speed" label; add per-leg speed and TWA to segment popups |

---

## Polar table (`src/polars.rs`)

### CSV format

The CSV has the following structure (from `dufour40.csv`):

```
,wind in knots,speed in knots,,,,,,,,,,,,,,,,,,
angle,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20
42,,,,,, 3.31 ,, 4.02 ,, 4.57 ,, 5.05 ,, 5.32 ,, 5.44 ,,,, 5.49
...
180,,,,,, 3.53 ,, 4.48 ,, 5.30 ,, 6.06 ,, 6.73 ,, 7.33 ,,,, 8.04
```

- Row 0: ignored (header comment)
- Row 1: column headers — first column is `"angle"`, remaining are integer TWS values
- Rows 2+: first column is TWA in degrees (0–180), remaining columns are boat speeds (some empty)

### Data structures

```rust
pub struct PolarTable {
    twa_breakpoints: Vec<f64>,          // sorted TWA values, e.g. [42.0, 52.0, ..., 180.0]
    tws_breakpoints: Vec<f64>,          // sorted TWS values with at least one speed entry, e.g. [6.0, 8.0, ..., 20.0]
    speeds: Vec<Vec<Option<f64>>>,      // speeds[twa_idx][tws_idx]
}
```

### Public API

```rust
impl PolarTable {
    /// Parse polar CSV from file path. Returns error if file not found or unparseable.
    pub fn from_csv(path: &str) -> Result<Self, Box<dyn std::error::Error>>

    /// Look up boat speed (kn) for given true wind angle (degrees) and true wind speed (kn).
    /// Returns None when:
    ///   - twa_deg < minimum TWA in table (boat cannot point that high — use motoring speed)
    ///   - tws_kn < minimum populated TWS column (too little wind — use motoring speed)
    /// Clamps twa_deg at 180° (symmetric — caller normalises to 0–180° before calling).
    /// Clamps tws_kn at the maximum populated TWS column.
    /// Uses bilinear interpolation over the two nearest TWA and TWS breakpoints.
    pub fn boat_speed(&self, twa_deg: f64, tws_kn: f64) -> Option<f64>
}
```

### Parsing notes

- Skip rows until a row whose first cell parses as a non-negative float (first data row)
- For each data row: `twa = row[0].parse::<f64>()`; iterate remaining cells and record `(tws, speed)` for non-empty cells
- `tws_breakpoints` is the union of all TWS columns that have at least one non-empty entry anywhere in the table
- Empty cells within the populated TWS range: fill via linear interpolation between the two nearest populated cells in the same TWA row before storing

### Interpolation

```rust
fn boat_speed(&self, twa_deg: f64, tws_kn: f64) -> Option<f64> {
    let twa = twa_deg.clamp(0.0, 180.0);
    let tws = tws_kn.clamp(0.0, *self.tws_breakpoints.last().unwrap());

    // Below minimum populated TWS or below minimum TWA → caller uses motoring speed
    if tws < self.tws_breakpoints[0] { return None; }
    if twa < self.twa_breakpoints[0] { return None; }

    // Find bracketing TWA indices
    let ti = self.twa_breakpoints.partition_point(|&v| v <= twa).saturating_sub(1);
    let ti_hi = (ti + 1).min(self.twa_breakpoints.len() - 1);

    // Find bracketing TWS indices
    let si = self.tws_breakpoints.partition_point(|&v| v <= tws).saturating_sub(1);
    let si_hi = (si + 1).min(self.tws_breakpoints.len() - 1);

    // Bilinear interpolation
    let t_frac = if ti == ti_hi { 0.0 }
        else { (twa - self.twa_breakpoints[ti]) / (self.twa_breakpoints[ti_hi] - self.twa_breakpoints[ti]) };
    let s_frac = if si == si_hi { 0.0 }
        else { (tws - self.tws_breakpoints[si]) / (self.tws_breakpoints[si_hi] - self.tws_breakpoints[si]) };

    let v00 = self.speeds[ti][si]?;
    let v10 = self.speeds[ti_hi][si].unwrap_or(v00);
    let v01 = self.speeds[ti][si_hi].unwrap_or(v00);
    let v11 = self.speeds[ti_hi][si_hi].unwrap_or(v00);

    let v = v00 * (1.0 - t_frac) * (1.0 - s_frac)
          + v10 * t_frac         * (1.0 - s_frac)
          + v01 * (1.0 - t_frac) * s_frac
          + v11 * t_frac         * s_frac;
    Some(v)
}
```

---

## Config (`src/config.rs`)

Add to the `Config` struct:

```rust
/// Optional path to polar diagram CSV file.
#[serde(default)]
pub polars_file_path: Option<String>,
```

Add to `config.example.json`:

```json
"polars_file_path": "/etc/nmea_router/polars.csv"
```

---

## AppState (`src/web/api.rs`)

Add field:

```rust
pub polars: Option<Arc<crate::polars::PolarTable>>,
```

Add helper:

```rust
pub fn polars(&self) -> Option<&crate::polars::PolarTable> {
    self.polars.as_deref()
}
```

---

## Startup (`src/main.rs`)

After loading config, before building AppState:

```rust
let polars = config.polars_file_path.as_deref().and_then(|path| {
    match crate::polars::PolarTable::from_csv(path) {
        Ok(t) => {
            info!(path, "Polar table loaded");
            Some(Arc::new(t))
        }
        Err(e) => {
            warn!(path, error = %e, "Failed to load polar table — fixed speed will be used");
            None
        }
    }
});
```

Pass `polars` into `AppState { ..., polars }`.

All existing `AppState` construction sites in `api.rs` tests: add `polars: None`.

---

## Route computation (`src/forecast.rs`)

### TWA helper (new, public for use in routing.rs)

```rust
/// Normalise true wind angle to 0–180°.
/// cog_deg: vessel course over ground (0–360°)
/// wind_dir_deg: meteorological wind direction the wind is coming FROM (0–360°)
pub fn compute_twa(cog_deg: f64, wind_dir_deg: f64) -> f64 {
    let diff = (wind_dir_deg - cog_deg).rem_euclid(360.0);
    if diff <= 180.0 { diff } else { 360.0 - diff }
}
```

### Updated `generate_route_track` signature

```rust
pub fn generate_route_track(
    waypoints: &[(f64, f64)],
    departure: DateTime<Utc>,
    motoring_speed_kn: f64,
    polars: Option<&crate::polars::PolarTable>,
    forecast_inputs: &TripForecastInputs,
) -> Vec<(f64, f64, DateTime<Utc>)>
```

The `TripForecastInputs` is already loaded by the route handler before calling this function (it contains the grid point forecasts for the trip). Passing it in avoids a second DB round-trip.

### Step simulation (replaces linear interpolation)

```rust
pub fn generate_route_track(...) -> Vec<(f64, f64, DateTime<Utc>)> {
    let mut track = Vec::new();
    let mut leg_start_time = departure;

    for w in waypoints.windows(2) {
        let (from_lat, from_lon) = w[0];
        let (to_lat, to_lon) = w[1];

        let mut pos = (from_lat, from_lon);
        let mut t = leg_start_time;
        track.push((pos.0, pos.1, t));

        loop {
            let bearing = crate::utilities::haversine_bearing(pos.0, pos.1, to_lat, to_lon);
            let remaining_nm = crate::utilities::haversine_distance_nm(pos.0, pos.1, to_lat, to_lon);
            if remaining_nm < 0.1 { break; }

            // Look up forecast at current position and time
            let wind = nearest_forecast_wind(&forecast_inputs.fetches, pos.0, pos.1, t);

            let speed_kn = match (wind, polars) {
                (Some(w), Some(p)) if w.speed_kn >= 5.0 => {
                    let twa = compute_twa(bearing, w.direction_deg);
                    p.boat_speed(twa, w.speed_kn).unwrap_or(motoring_speed_kn)
                }
                _ => motoring_speed_kn,
            };

            // Advance one hour (or less if we reach the waypoint sooner)
            let hours_to_wp = remaining_nm / speed_kn;
            let step_hours = hours_to_wp.min(1.0);
            let dist_nm = speed_kn * step_hours;

            pos = crate::utilities::advance_position(pos.0, pos.1, bearing, dist_nm);
            t = t + chrono::Duration::seconds((step_hours * 3600.0) as i64);
            track.push((pos.0, pos.1, t));

            if hours_to_wp <= 1.0 { break; }
        }

        leg_start_time = t;
    }

    track
}
```

`nearest_forecast_wind` is a private helper that finds the spatially and temporally nearest forecast entry in `TripForecastInputs` for a given position and time. It returns `Option<WindSample>` where:

```rust
struct WindSample { speed_kn: f64, direction_deg: f64 }
```

`advance_position` is a new utility (in `src/utilities.rs`) that moves a lat/lon point by `dist_nm` nautical miles along a bearing, using the Haversine formula.

### Route handler update (`src/web/api.rs`)

In `get_forecast_route`: load `TripForecastInputs` first (already done today), then pass `state.polars()` and `motoring_speed_kn` to `generate_route_track`.

Rename `speed_kn` query parameter to `motoring_speed_kn` in `ForecastRouteQuery` to make its role explicit. The frontend field label changes to match.

---

## Frontend (`static/plan.html`)

- Rename speed input label from `"Speed (kn)"` to `"Motoring speed (kn)"`
- In `drawRouteLine`, each segment popup currently shows wind speed, direction, gust, and wave data. Add two lines:
  - `Est. speed: X.X kn` — derived from the `RouteOverlayPoint.speed_kn` field (add this field to the response struct)
  - `TWA: XXX°` — derived from `RouteOverlayPoint.twa_deg` (add this field too)
- Rename the `speed_kn` query parameter sent by `computeRoute()` to `motoring_speed_kn`

### New fields on `RouteOverlayPoint` (Rust)

```rust
pub struct RouteOverlayPoint {
    // existing fields ...
    pub speed_kn: Option<f64>,      // actual speed used at this point (polar or motoring)
    pub twa_deg: Option<f64>,       // true wind angle at this point
}
```

---

## Tests

### `src/polars.rs`

```rust
#[test]
fn test_polar_loads_dufour40() {
    let p = PolarTable::from_csv("tests/fixtures/dufour40.csv").unwrap();
    // TWA=90°, TWS=10 kn → 7.44 kn from table
    let spd = p.boat_speed(90.0, 10.0).unwrap();
    assert!((spd - 7.44).abs() < 0.1, "got {}", spd);
}

#[test]
fn test_polar_returns_none_below_min_tws() {
    let p = PolarTable::from_csv("tests/fixtures/dufour40.csv").unwrap();
    assert!(p.boat_speed(90.0, 3.0).is_none());
}

#[test]
fn test_polar_returns_none_below_min_twa() {
    let p = PolarTable::from_csv("tests/fixtures/dufour40.csv").unwrap();
    assert!(p.boat_speed(30.0, 10.0).is_none());
}

#[test]
fn test_polar_interpolates_between_tws() {
    let p = PolarTable::from_csv("tests/fixtures/dufour40.csv").unwrap();
    // TWS=9 is between 8 (6.63) and 10 (7.44) at TWA=90 — should be ~7.0
    let spd = p.boat_speed(90.0, 9.0).unwrap();
    assert!(spd > 6.63 && spd < 7.44, "got {}", spd);
}

#[test]
fn test_compute_twa_upwind() {
    // COG north (0°), wind from north (0°) → TWA = 0°
    assert!((compute_twa(0.0, 0.0) - 0.0).abs() < 0.01);
}

#[test]
fn test_compute_twa_beam_reach() {
    // COG north (0°), wind from east (90°) → TWA = 90°
    assert!((compute_twa(0.0, 90.0) - 90.0).abs() < 0.01);
}

#[test]
fn test_compute_twa_downwind() {
    // COG north (0°), wind from south (180°) → TWA = 180°
    assert!((compute_twa(0.0, 180.0) - 180.0).abs() < 0.01);
}
```

Copy `dufour40.csv` to `tests/fixtures/dufour40.csv` as the test fixture.

### `src/forecast.rs`

Update existing `test_generate_route_track_*` tests to pass `motoring_speed_kn` and `polars: None` (no polar, so behaviour is identical to today — fixed speed, linear steps).

---

## Constraints

- All speeds in knots, distances in nautical miles, angles in degrees.
- `advance_position` uses the Haversine formula (project rule).
- No new Rust dependencies.
- `PolarTable::from_csv` is infallible after parsing — any CSV error returns `Err` at startup, not at request time.
- When `polars` is `None` in `AppState`, the route planner works exactly as today (fixed speed throughout). Existing behaviour is preserved for deployments without a polar file.
