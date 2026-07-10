# Minimum True Wind Angle (TWA) Constraint Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a user-configurable `min_twa_deg` parameter (default 60°) that forces the router to motor instead of sail whenever a candidate heading's true wind angle is tighter than this threshold, regardless of what the polar table itself would allow.

**Architecture:** The gate is applied at the two places headings are evaluated against the polar table (`run_isochrone` in `src/routing.rs`, `generate_route_track` in `src/forecast.rs`), plus the one place TWA is recomputed for display after the fact (`get_optimal_route` in `src/web/api.rs`). Threaded through both HTTP query structs and the `plan.html` UI the same way `min_sail_speed_kn` already is.

**Tech Stack:** Rust (routing/forecast/API layers), vanilla JS (`static/plan.html`). No new dependencies.

## Global Constraints

- Backend: Rust only. Frontend: HTML + vanilla JavaScript (CLAUDE.md).
- Do NOT run `git commit` or `git push` — per this repo's CLAUDE.md, stop after code changes; the user commits.
- `compute_twa` (`src/forecast.rs:138`) returns TWA in 0–180°, 0° = wind dead ahead, 180° = wind dead astern. `min_twa_deg` only restricts the upwind/close-hauled side (`TWA < min_twa_deg` is rejected); the downwind side is never restricted by this parameter.
- Default value: `60.0`. Valid range: `[0, 180]`.
- Existing test call sites must keep passing `0.0` for the new parameter to preserve their original, unrestricted intent — this is not a behavior change for those tests, only a signature change.

---

### Task 1: Add `min_twa_deg` gate to `run_isochrone`

**Files:**
- Modify: `src/routing.rs:36-47` (signature), `src/routing.rs:100-116` (heading evaluation loop)
- Test: `src/routing.rs` (existing `mod tests`, update 5 existing call sites, add 1 new test)

**Interfaces:**
- Produces: `run_isochrone`'s new parameter `min_twa_deg: f64`, inserted immediately after `min_sail_speed_kn` and before `sail_weight_kn` in the signature: `(from, to, departure, motoring_speed_kn, polar_efficiency, min_sail_speed_kn, min_twa_deg, sail_weight_kn, polars, fetches, land_mask)`. This is the signature Task 3 (API layer) calls.

- [ ] **Step 1: Update the 5 existing test call sites to pass `0.0` for the new parameter**

In `src/routing.rs`, change (current line 288):
```rust
        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, &polars, &[], None);
```
to:
```rust
        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, 0.0, &polars, &[], None);
```

Change (current line 306, identical text to line 288 — this is inside `test_backtrack_produces_monotonic_timestamps`):
```rust
        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, &polars, &[], None);
```
to:
```rust
        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, 0.0, &polars, &[], None);
```

Change (current lines 390-401, inside `test_land_mask_blocks_candidate`):
```rust
        let result = run_isochrone(
            from,
            to,
            departure,
            6.0,
            1.0,
            0.0,
            0.0,
            &polars,
            &[],
            Some(&mask),
        );
```
to:
```rust
        let result = run_isochrone(
            from,
            to,
            departure,
            6.0,
            1.0,
            0.0,
            0.0,
            0.0,
            &polars,
            &[],
            Some(&mask),
        );
```

Change (current lines 448-459, inside `test_arrival_prefers_tacking_over_forced_motor`):
```rust
        let result = run_isochrone(
            from,
            to,
            departure,
            motoring_speed_kn,
            1.0,
            0.0,
            0.0,
            &polars,
            &fetches,
            None,
        );
```
to:
```rust
        let result = run_isochrone(
            from,
            to,
            departure,
            motoring_speed_kn,
            1.0,
            0.0,
            0.0,
            0.0,
            &polars,
            &fetches,
            None,
        );
```

Change (current line 484, inside `test_frontiers_exclude_seed_and_respect_sector_count`):
```rust
        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, &polars, &[], None);
```
to:
```rust
        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, 0.0, &polars, &[], None);
```

- [ ] **Step 2: Write the new failing test for the gate**

Add this test at the end of `mod tests` in `src/routing.rs`, immediately before the module's closing `}` (currently after `test_frontiers_exclude_seed_and_respect_sector_count`, which ends at line 504):

```rust
    #[test]
    fn test_min_twa_deg_forces_motor_below_threshold() {
        // Same scenario as test_arrival_prefers_tacking_over_forced_motor: the polar (min
        // sailable angle 42°) would happily sail at the ~45° tacking angle needed to make
        // progress upwind at 6 kn, well above the 3 kn motoring speed. But with
        // min_twa_deg = 60.0, 45° is tighter than the user's allowed minimum, so the router
        // must not treat it as sailable — the tacking speed advantage must disappear, leaving
        // the boat no faster than pure motoring for this dead-upwind destination.
        let from = (43.0, 8.0);
        let to = (43.2, 8.0); // ~12 nm due north
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let motoring_speed_kn = 3.0;
        let polars = upwind_polars(6.0);

        let fetches = vec![crate::db::operations::forecast::FetchWithHourly {
            lat: 43.0,
            lon: 8.0,
            model: "ecmwf".to_string(),
            hourly: (0..48)
                .map(|h| crate::db::operations::forecast::ForecastHourlyPoint {
                    timestamp: (departure + chrono::Duration::hours(h))
                        .format("%Y-%m-%dT%H:%M:%SZ")
                        .to_string(),
                    wind_speed_kn: Some(10.0),
                    wind_direction_deg: Some(0.0), // wind from the north, blowing toward the south
                    wind_gust_kn: None,
                    wave_height_m: None,
                    wave_period_s: None,
                    wave_direction_deg: None,
                    cape_j_kg: None,
                })
                .collect(),
        }];

        let result = run_isochrone(
            from,
            to,
            departure,
            motoring_speed_kn,
            1.0,
            0.0,
            60.0,
            0.0,
            &polars,
            &fetches,
            None,
        );
        assert!(
            result.reached_destination,
            "should still reach destination by motoring"
        );

        let total_hours =
            (result.track.last().unwrap().2 - departure).num_seconds() as f64 / 3600.0;
        let motoring_only_hours =
            haversine_distance_nm(from.0, from.1, to.0, to.1) / motoring_speed_kn;
        assert!(
            total_hours >= motoring_only_hours - 0.01,
            "expected no faster than pure motoring ({:.2}h) once min_twa_deg=60 excludes the \
             45° tacking angle, got {:.2}h",
            motoring_only_hours,
            total_hours
        );
    }
```

- [ ] **Step 3: Run the tests to verify they fail to compile**

Run: `cargo test --lib routing::tests`
Expected: FAIL to compile — `this function takes 11 arguments but 10 arguments were supplied` (the signature doesn't have `min_twa_deg` yet).

- [ ] **Step 4: Add `min_twa_deg` to the signature**

In `src/routing.rs`, change (current lines 36-47):
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
to:
```rust
pub fn run_isochrone(
    from: (f64, f64),
    to: (f64, f64),
    departure: DateTime<Utc>,
    motoring_speed_kn: f64,
    polar_efficiency: f64,
    min_sail_speed_kn: f64,
    min_twa_deg: f64,
    sail_weight_kn: f64,
    polars: &crate::polars::PolarTable,
    fetches: &[FetchWithHourly],
    land_mask: Option<&crate::land_mask::LandMask>,
) -> IsochroneResult {
```

- [ ] **Step 5: Apply the gate in the heading evaluation loop**

In `src/routing.rs`, change (current lines 100-116):
```rust
                let (speed_kn, was_sailing) = match wind {
                    Some((wind_spd, wind_dir)) if wind_spd > 0.0 => {
                        let twa = compute_twa(heading, wind_dir);
                        match polars.boat_speed(twa, wind_spd).filter(|&s| s > 0.0) {
                            Some(raw) => {
                                let eff = raw * polar_efficiency;
                                if eff >= min_sail_speed_kn {
                                    (eff, true)
                                } else {
                                    (motoring_speed_kn, false)
                                }
                            }
                            None => (motoring_speed_kn, false),
                        }
                    }
                    _ => (motoring_speed_kn, false),
                };
```
to:
```rust
                let (speed_kn, was_sailing) = match wind {
                    Some((wind_spd, wind_dir)) if wind_spd > 0.0 => {
                        let twa = compute_twa(heading, wind_dir);
                        if twa < min_twa_deg {
                            (motoring_speed_kn, false)
                        } else {
                            match polars.boat_speed(twa, wind_spd).filter(|&s| s > 0.0) {
                                Some(raw) => {
                                    let eff = raw * polar_efficiency;
                                    if eff >= min_sail_speed_kn {
                                        (eff, true)
                                    } else {
                                        (motoring_speed_kn, false)
                                    }
                                }
                                None => (motoring_speed_kn, false),
                            }
                        }
                    }
                    _ => (motoring_speed_kn, false),
                };
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --lib routing::tests`
Expected: PASS — all tests in `routing::tests`, including the new `test_min_twa_deg_forces_motor_below_threshold`.

- [ ] **Step 7: Do not commit**

Per this repo's CLAUDE.md, leave the change in the working tree for the user to review and commit themselves. Do not run `git add` or `git commit`.

---

### Task 2: Add `min_twa_deg` gate to `generate_route_track`

**Files:**
- Modify: `src/forecast.rs:356-364` (signature), `src/forecast.rs:393-409` (heading evaluation)
- Test: `src/forecast.rs` (existing `mod tests`, update 8 existing call sites, add 1 new test)

**Interfaces:**
- Produces: `generate_route_track`'s new parameter `min_twa_deg: f64`, inserted immediately after `min_sail_speed_kn` and before `polars`: `(waypoints, departure, motoring_speed_kn, polar_efficiency, min_sail_speed_kn, min_twa_deg, polars, fetches)`. This is the signature Task 3 (API layer) calls.

- [ ] **Step 1: Update the 8 existing test call sites to pass `0.0` for the new parameter**

In `src/forecast.rs`, change (current line 724, inside `test_generate_route_track_point_count`):
```rust
        let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, None, &[]);
```
to:
```rust
        let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, 0.0, None, &[]);
```

Change (current line 738, inside `test_generate_route_track_timestamps_advance_hourly` — identical text to line 724):
```rust
        let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, None, &[]);
```
to:
```rust
        let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, 0.0, None, &[]);
```

Change (current line 753, inside `test_compute_route_overlay_returns_points_with_coords`):
```rust
        let track = generate_route_track(&wpts, dep, 10.0, 1.0, 0.0, None, &[]);
```
to:
```rust
        let track = generate_route_track(&wpts, dep, 10.0, 1.0, 0.0, 0.0, None, &[]);
```

Change (current lines 786-788, inside `test_generate_route_track_empty_and_single_waypoint`):
```rust
        assert!(generate_route_track(&[], dep, 5.0, 1.0, 0.0, None, &[]).is_empty());
        // 1 waypoint → empty track (no pair to form a leg)
        assert!(generate_route_track(&[(43.55, 10.29)], dep, 5.0, 1.0, 0.0, None, &[]).is_empty());
```
to:
```rust
        assert!(generate_route_track(&[], dep, 5.0, 1.0, 0.0, 0.0, None, &[]).is_empty());
        // 1 waypoint → empty track (no pair to form a leg)
        assert!(generate_route_track(&[(43.55, 10.29)], dep, 5.0, 1.0, 0.0, 0.0, None, &[]).is_empty());
```

Change (current line 796, inside `test_generate_route_track_two_legs`):
```rust
        let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, None, &[]);
```
to:
```rust
        let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, 0.0, None, &[]);
```

Change (current line 888, inside `test_generate_route_track_uses_polar_speed`):
```rust
        let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, Some(&polars), &fetches);
```
to:
```rust
        let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, 0.0, Some(&polars), &fetches);
```

Change (current line 900, inside `test_generate_route_track_falls_back_to_motoring_no_wind`):
```rust
        let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, Some(&polars), &[]);
```
to:
```rust
        let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, 0.0, Some(&polars), &[]);
```

- [ ] **Step 2: Write the new failing test for the gate**

Add this test at the end of `mod tests` in `src/forecast.rs`, immediately before the module's closing `}` (currently after `test_generate_route_track_falls_back_to_motoring_no_wind`, which ends at line 905):

```rust
    #[test]
    fn test_generate_route_track_min_twa_deg_forces_motor() {
        use crate::polars::PolarTable;
        let polars = PolarTable::constant_for_test(7.0);

        let ts_str = "2026-06-01T06:00:00Z";
        let hourly = vec![crate::db::operations::forecast::ForecastHourlyPoint {
            timestamp: ts_str.to_string(),
            wind_speed_kn: Some(12.0),
            // Leg bearing is due north (0°); wind from 45° → TWA 45°, which the polar itself
            // happily sails (its minimum is 42°) but which min_twa_deg = 60.0 must reject.
            wind_direction_deg: Some(45.0),
            wind_gust_kn: None, wave_height_m: None, wave_period_s: None,
            wave_direction_deg: None, cape_j_kg: None,
        }];
        let fetches = vec![crate::db::operations::forecast::FetchWithHourly {
            lat: 43.0, lon: 8.0, model: "ecmwf".to_string(), hourly,
        }];

        let dep = chrono::DateTime::parse_from_rfc3339(ts_str).unwrap().with_timezone(&chrono::Utc);
        let wpts = vec![(43.0_f64, 8.0_f64), (43.12_f64, 8.0_f64)];
        let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, 60.0, Some(&polars), &fetches);

        assert!(track.len() >= 2, "expected ≥2 points, got {}", track.len());
        assert_eq!(
            track[1].twa_deg, None,
            "TWA 45° is below min_twa_deg=60°, should not count as sailing"
        );
        let spd = track[1].speed_kn.expect("speed_kn should be set");
        assert!(
            (spd - 5.0).abs() < 0.1,
            "expected motoring speed 5.0 once min_twa_deg excludes this TWA, got {}",
            spd
        );
    }
```

- [ ] **Step 3: Run the tests to verify they fail to compile**

Run: `cargo test --lib forecast::tests`
Expected: FAIL to compile — argument count mismatch on `generate_route_track` (the signature doesn't have `min_twa_deg` yet).

- [ ] **Step 4: Add `min_twa_deg` to the signature**

In `src/forecast.rs`, change (current lines 356-364):
```rust
pub fn generate_route_track(
    waypoints: &[(f64, f64)],
    departure: DateTime<Utc>,
    motoring_speed_kn: f64,
    polar_efficiency: f64,
    min_sail_speed_kn: f64,
    polars: Option<&crate::polars::PolarTable>,
    fetches: &[FetchWithHourly],
) -> Vec<RouteTrackPoint> {
```
to:
```rust
pub fn generate_route_track(
    waypoints: &[(f64, f64)],
    departure: DateTime<Utc>,
    motoring_speed_kn: f64,
    polar_efficiency: f64,
    min_sail_speed_kn: f64,
    min_twa_deg: f64,
    polars: Option<&crate::polars::PolarTable>,
    fetches: &[FetchWithHourly],
) -> Vec<RouteTrackPoint> {
```

- [ ] **Step 5: Apply the gate in the heading evaluation**

In `src/forecast.rs`, change (current lines 393-409):
```rust
            let (speed_kn, twa) = match (nearest_forecast_wind(&parsed, pos.0, pos.1, t), polars) {
                (Some((wind_spd, wind_dir)), Some(p)) if wind_spd > 0.0 => {
                    let twa = compute_twa(bearing, wind_dir);
                    match p.boat_speed(twa, wind_spd).filter(|&s| s > 0.0) {
                        Some(raw) => {
                            let eff = raw * efficiency;
                            if eff >= min_sail_speed_kn {
                                (eff, Some(twa))
                            } else {
                                (motoring_speed_kn, None) // polar speed too low — motor
                            }
                        }
                        None => (motoring_speed_kn, None), // TWA below polar minimum — motor
                    }
                }
                _ => (motoring_speed_kn, None),
            };
```
to:
```rust
            let (speed_kn, twa) = match (nearest_forecast_wind(&parsed, pos.0, pos.1, t), polars) {
                (Some((wind_spd, wind_dir)), Some(p)) if wind_spd > 0.0 => {
                    let twa = compute_twa(bearing, wind_dir);
                    if twa < min_twa_deg {
                        (motoring_speed_kn, None) // TWA below the user's minimum — motor
                    } else {
                        match p.boat_speed(twa, wind_spd).filter(|&s| s > 0.0) {
                            Some(raw) => {
                                let eff = raw * efficiency;
                                if eff >= min_sail_speed_kn {
                                    (eff, Some(twa))
                                } else {
                                    (motoring_speed_kn, None) // polar speed too low — motor
                                }
                            }
                            None => (motoring_speed_kn, None), // TWA below polar minimum — motor
                        }
                    }
                }
                _ => (motoring_speed_kn, None),
            };
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --lib forecast::tests`
Expected: PASS — all tests in `forecast::tests`, including the new `test_generate_route_track_min_twa_deg_forces_motor`.

- [ ] **Step 7: Do not commit**

Per this repo's CLAUDE.md, leave the change in the working tree for the user to review and commit themselves. Do not run `git add` or `git commit`.

---

### Task 3: Wire `min_twa_deg` through the API and frontend

**Files:**
- Modify: `src/web/api.rs:251-262` (`ForecastRouteQuery`), `src/web/api.rs:266-284` (`OptimalRouteQuery`), `src/web/api.rs:1654-1692` (`get_forecast_route`), `src/web/api.rs:1700-1784` (`get_optimal_route`)
- Modify: `static/plan.html:125-130` (input markup), `static/plan.html:292-299` (localStorage restore), `static/plan.html:774-786` (event listener), `static/plan.html:787-829` (`computeRoute`), `static/plan.html:831-864` (`optimizeRoute`)

**Interfaces:**
- Consumes: `crate::routing::run_isochrone(..., min_sail_speed_kn, min_twa_deg, sail_weight_kn, ...)` and `crate::forecast::generate_route_track(..., min_sail_speed_kn, min_twa_deg, polars, ...)` — the exact signatures from Task 1 and Task 2.
- Produces: `min_twa_deg` as a query parameter on both `/api/forecast/route` and `/api/forecast/optimal-route`, and a `minTwaInput` field in `plan.html` sent as `min_twa_deg` on both endpoints' requests.

- [ ] **Step 1: Add `min_twa_deg` to `ForecastRouteQuery` with validation in `get_forecast_route`**

In `src/web/api.rs`, change (current lines 251-262):
```rust
#[derive(Debug, Deserialize)]
pub struct ForecastRouteQuery {
    pub waypoints: String,   // "lat1,lon1;lat2,lon2;…" — at least 2 pairs
    pub departure: String,
    pub motoring_speed_kn: f64,
    /// Fraction of raw polar speed to use (0–1). Default 1.0 (full polar speed).
    #[serde(default = "default_polar_efficiency")]
    pub polar_efficiency: f64,
    /// Motor instead of sail when effective polar speed is below this threshold (kn). Default 0.
    #[serde(default)]
    pub min_sail_speed_kn: f64,
}

fn default_polar_efficiency() -> f64 { 1.0 }
```
to:
```rust
#[derive(Debug, Deserialize)]
pub struct ForecastRouteQuery {
    pub waypoints: String,   // "lat1,lon1;lat2,lon2;…" — at least 2 pairs
    pub departure: String,
    pub motoring_speed_kn: f64,
    /// Fraction of raw polar speed to use (0–1). Default 1.0 (full polar speed).
    #[serde(default = "default_polar_efficiency")]
    pub polar_efficiency: f64,
    /// Motor instead of sail when effective polar speed is below this threshold (kn). Default 0.
    #[serde(default)]
    pub min_sail_speed_kn: f64,
    /// Motor instead of sail when the true wind angle is tighter (closer to the wind) than
    /// this, regardless of what the polar table would otherwise report. Default 60°.
    #[serde(default = "default_min_twa_deg")]
    pub min_twa_deg: f64,
}

fn default_polar_efficiency() -> f64 { 1.0 }
fn default_min_twa_deg() -> f64 { 60.0 }
```

- [ ] **Step 2: Add the same field and validation to `get_forecast_route`**

In `src/web/api.rs`, change (current lines 1671-1673):
```rust
    if params.motoring_speed_kn <= 0.0 {
        return Ok(Json(ApiResponse::error("motoring_speed_kn must be positive".to_string())));
    }
```
to:
```rust
    if params.motoring_speed_kn <= 0.0 {
        return Ok(Json(ApiResponse::error("motoring_speed_kn must be positive".to_string())));
    }
    if !(0.0..=180.0).contains(&params.min_twa_deg) {
        return Ok(Json(ApiResponse::error("min_twa_deg must be between 0 and 180".to_string())));
    }
```

Then change (current lines 1681-1689):
```rust
    let track = crate::forecast::generate_route_track(
        &waypoints,
        departure,
        params.motoring_speed_kn,
        params.polar_efficiency,
        params.min_sail_speed_kn,
        state.polars(),
        &fetches,
    );
```
to:
```rust
    let track = crate::forecast::generate_route_track(
        &waypoints,
        departure,
        params.motoring_speed_kn,
        params.polar_efficiency,
        params.min_sail_speed_kn,
        params.min_twa_deg,
        state.polars(),
        &fetches,
    );
```

- [ ] **Step 3: Add `min_twa_deg` to `OptimalRouteQuery` with validation in `get_optimal_route`**

In `src/web/api.rs`, change (current lines 266-284):
```rust
#[derive(Debug, Deserialize)]
pub struct OptimalRouteQuery {
    pub from_lat: f64,
    pub from_lon: f64,
    pub to_lat: f64,
    pub to_lon: f64,
    pub departure: String,          // ISO 8601 UTC, e.g. "2026-06-01T06:00:00Z"
    pub motoring_speed_kn: f64,
    #[serde(default = "default_polar_efficiency")]
    pub polar_efficiency: f64,
    #[serde(default)]
    pub min_sail_speed_kn: f64,
    #[serde(default)]
    pub sail_weight_kn: f64,
    /// Whether to route around land/islands using the configured land mask. Default true.
    /// Omitted from older clients, so it must default on to preserve existing behavior.
    #[serde(default = "default_avoid_land")]
    pub avoid_land: bool,
}
```
to:
```rust
#[derive(Debug, Deserialize)]
pub struct OptimalRouteQuery {
    pub from_lat: f64,
    pub from_lon: f64,
    pub to_lat: f64,
    pub to_lon: f64,
    pub departure: String,          // ISO 8601 UTC, e.g. "2026-06-01T06:00:00Z"
    pub motoring_speed_kn: f64,
    #[serde(default = "default_polar_efficiency")]
    pub polar_efficiency: f64,
    #[serde(default)]
    pub min_sail_speed_kn: f64,
    /// Motor instead of sail when the true wind angle is tighter (closer to the wind) than
    /// this, regardless of what the polar table would otherwise report. Default 60°.
    #[serde(default = "default_min_twa_deg")]
    pub min_twa_deg: f64,
    #[serde(default)]
    pub sail_weight_kn: f64,
    /// Whether to route around land/islands using the configured land mask. Default true.
    /// Omitted from older clients, so it must default on to preserve existing behavior.
    #[serde(default = "default_avoid_land")]
    pub avoid_land: bool,
}
```

- [ ] **Step 4: Validate and thread `min_twa_deg` through `get_optimal_route`**

In `src/web/api.rs`, change (current lines 1722-1724):
```rust
    if params.sail_weight_kn < 0.0 {
        return Ok(Json(ApiResponse::error("sail_weight_kn must be non-negative".to_string())));
    }
```
to:
```rust
    if params.sail_weight_kn < 0.0 {
        return Ok(Json(ApiResponse::error("sail_weight_kn must be non-negative".to_string())));
    }

    if !(0.0..=180.0).contains(&params.min_twa_deg) {
        return Ok(Json(ApiResponse::error("min_twa_deg must be between 0 and 180".to_string())));
    }
```

Then change (current lines 1740-1751):
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
        if params.avoid_land { state.land_mask() } else { None },
    );
```
to:
```rust
    let result = crate::routing::run_isochrone(
        (params.from_lat, params.from_lon),
        (params.to_lat, params.to_lon),
        departure,
        params.motoring_speed_kn,
        params.polar_efficiency,
        params.min_sail_speed_kn,
        params.min_twa_deg,
        params.sail_weight_kn,
        polars,
        &fetches,
        if params.avoid_land { state.land_mask() } else { None },
    );
```

Then change the post-hoc TWA recomputation (current lines 1768-1775):
```rust
        let twa_deg = crate::forecast::nearest_forecast_wind(&parsed_fetches, prev_lat, prev_lon, prev_time)
            .filter(|(ws, _)| *ws > 0.0)
            .and_then(|(ws, wd)| {
                let twa = crate::forecast::compute_twa(bearing, wd);
                polars.boat_speed(twa, ws)
                    .filter(|&raw| raw * params.polar_efficiency >= params.min_sail_speed_kn)
                    .map(|_| twa)
            });
```
to:
```rust
        let twa_deg = crate::forecast::nearest_forecast_wind(&parsed_fetches, prev_lat, prev_lon, prev_time)
            .filter(|(ws, _)| *ws > 0.0)
            .and_then(|(ws, wd)| {
                let twa = crate::forecast::compute_twa(bearing, wd);
                if twa < params.min_twa_deg {
                    return None;
                }
                polars.boat_speed(twa, ws)
                    .filter(|&raw| raw * params.polar_efficiency >= params.min_sail_speed_kn)
                    .map(|_| twa)
            });
```

- [ ] **Step 5: Verify the backend compiles and existing tests still pass**

Run: `cargo build`
Expected: builds with no errors.

Run: `cargo test --lib routing::tests forecast::tests`
Expected: PASS — includes both new tests from Task 1 and Task 2.

- [ ] **Step 6: Add the `minTwaInput` field to the `plan.html` UI**

In `static/plan.html`, change (current lines 125-130):
```html
                <label style="color:var(--text-secondary);" title="Motor if expected sail speed drops below this">Min sail speed:
                    <input type="number" id="minSailInput" value="3.0" min="0" max="10" step="0.5"
                        style="width:52px; margin-left:6px; background:var(--bg-secondary);
                               border:1px solid var(--border-color); border-radius:4px;
                               padding:3px 8px; color:var(--text-primary); font-size:13px;"> kn
                </label>
```
to:
```html
                <label style="color:var(--text-secondary);" title="Motor if expected sail speed drops below this">Min sail speed:
                    <input type="number" id="minSailInput" value="3.0" min="0" max="10" step="0.5"
                        style="width:52px; margin-left:6px; background:var(--bg-secondary);
                               border:1px solid var(--border-color); border-radius:4px;
                               padding:3px 8px; color:var(--text-primary); font-size:13px;"> kn
                </label>
                <label style="color:var(--text-secondary);" title="Motor instead of sail when the wind is tighter than this off the bow">Min wind angle:
                    <input type="number" id="minTwaInput" value="60" min="0" max="180" step="1"
                        style="width:52px; margin-left:6px; background:var(--bg-secondary);
                               border:1px solid var(--border-color); border-radius:4px;
                               padding:3px 8px; color:var(--text-primary); font-size:13px;">°
                </label>
```

- [ ] **Step 7: Restore the saved value on init**

In `static/plan.html`, change (current lines 294-295):
```javascript
            const savedMinSail = localStorage.getItem('plan_min_sail_speed');
            if (savedMinSail) document.getElementById('minSailInput').value = savedMinSail;
```
to:
```javascript
            const savedMinSail = localStorage.getItem('plan_min_sail_speed');
            if (savedMinSail) document.getElementById('minSailInput').value = savedMinSail;
            const savedMinTwa = localStorage.getItem('plan_min_twa_deg');
            if (savedMinTwa) document.getElementById('minTwaInput').value = savedMinTwa;
```

- [ ] **Step 8: Persist changes to localStorage**

In `static/plan.html`, change (current lines 777-779):
```javascript
        document.getElementById('minSailInput').addEventListener('input', function () {
            localStorage.setItem('plan_min_sail_speed', this.value);
        });
```
to:
```javascript
        document.getElementById('minSailInput').addEventListener('input', function () {
            localStorage.setItem('plan_min_sail_speed', this.value);
        });
        document.getElementById('minTwaInput').addEventListener('input', function () {
            localStorage.setItem('plan_min_twa_deg', this.value);
        });
```

- [ ] **Step 9: Send `min_twa_deg` on the "Compute" request**

In `static/plan.html`, change (current lines 790-791):
```javascript
            const efficiency = parseFloat(document.getElementById('efficiencyInput').value) / 100;
            const minSail    = parseFloat(document.getElementById('minSailInput').value);
```
to:
```javascript
            const efficiency = parseFloat(document.getElementById('efficiencyInput').value) / 100;
            const minSail    = parseFloat(document.getElementById('minSailInput').value);
            const minTwa     = parseFloat(document.getElementById('minTwaInput').value);
```

Then change (current lines 803-807):
```javascript
            const url = `/api/forecast/route?waypoints=${encodeURIComponent(waypointsParam)}` +
                `&departure=${encodeURIComponent(departure)}` +
                `&motoring_speed_kn=${speed}` +
                `&polar_efficiency=${efficiency.toFixed(3)}` +
                `&min_sail_speed_kn=${minSail}`;
```
to:
```javascript
            const url = `/api/forecast/route?waypoints=${encodeURIComponent(waypointsParam)}` +
                `&departure=${encodeURIComponent(departure)}` +
                `&motoring_speed_kn=${speed}` +
                `&polar_efficiency=${efficiency.toFixed(3)}` +
                `&min_sail_speed_kn=${minSail}` +
                `&min_twa_deg=${minTwa}`;
```

- [ ] **Step 10: Send `min_twa_deg` on the "Optimize" request**

In `static/plan.html`, change (current lines 849-852):
```javascript
            const efficiency = parseFloat(document.getElementById('efficiencyInput').value) / 100;
            const minSail    = parseFloat(document.getElementById('minSailInput').value);
            const sailWeight = parseFloat(document.getElementById('sailWeightInput').value) || 0;
            const avoidLand  = document.getElementById('avoidLandInput').checked;
```
to:
```javascript
            const efficiency = parseFloat(document.getElementById('efficiencyInput').value) / 100;
            const minSail    = parseFloat(document.getElementById('minSailInput').value);
            const minTwa     = parseFloat(document.getElementById('minTwaInput').value);
            const sailWeight = parseFloat(document.getElementById('sailWeightInput').value) || 0;
            const avoidLand  = document.getElementById('avoidLandInput').checked;
```

Then change (current lines 855-861):
```javascript
                const url = `/api/forecast/optimal-route?from_lat=${from.lat.toFixed(6)}&from_lon=${from.lng.toFixed(6)}`
                    + `&to_lat=${to.lat.toFixed(6)}&to_lon=${to.lng.toFixed(6)}`
                    + `&departure=${encodeURIComponent(departure)}`
                    + `&motoring_speed_kn=${speed}`
                    + `&polar_efficiency=${efficiency.toFixed(3)}`
                    + `&min_sail_speed_kn=${minSail}`
                    + `&sail_weight_kn=${sailWeight.toFixed(1)}`
```
to:
```javascript
                const url = `/api/forecast/optimal-route?from_lat=${from.lat.toFixed(6)}&from_lon=${from.lng.toFixed(6)}`
                    + `&to_lat=${to.lat.toFixed(6)}&to_lon=${to.lng.toFixed(6)}`
                    + `&departure=${encodeURIComponent(departure)}`
                    + `&motoring_speed_kn=${speed}`
                    + `&polar_efficiency=${efficiency.toFixed(3)}`
                    + `&min_sail_speed_kn=${minSail}`
                    + `&min_twa_deg=${minTwa}`
                    + `&sail_weight_kn=${sailWeight.toFixed(1)}`
```

- [ ] **Step 11: Verify the file is well-formed**

Run: `node --check <(sed -n '/<script>/,/<\/script>/p' static/plan.html | sed '1d;$d')`
Expected: no output (syntax OK). If the process-substitution syntax doesn't work in your shell, extract the `<script>...</script>` body to a temp `.js` file first and run `node --check` on that file instead.

- [ ] **Step 12: Manually verify in the browser**

Run the project's existing app-launch process with a `config.json` that has a polar table and forecast data configured.

In the browser, open `plan.html`, plan a route, and confirm:
- A "Min wind angle" input appears next to "Min sail speed", defaulting to 60.
- Changing it and reloading the page preserves the value (localStorage persistence).
- Clicking "Compute" and "Optimize" both succeed with the new parameter included in the request (check browser dev tools network tab for `min_twa_deg` in the request URL).
- Setting "Min wind angle" very high (e.g. 170) on a route that would otherwise sail close-hauled causes the route to motor instead, visible as a dashed segment in the rendered route line.

- [ ] **Step 13: Do not commit**

Per this repo's CLAUDE.md, leave the change in the working tree for the user to review and commit themselves. Do not run `git add` or `git commit`.

---

## Self-Review Notes

- **Spec coverage:** All three call sites from the spec are covered (Task 1: `run_isochrone`; Task 2: `generate_route_track`; Task 3: API validation/threading for both endpoints, plus the post-hoc TWA recomputation in `get_optimal_route`, plus frontend wiring). Default value 60.0 and range validation `[0, 180]` both included in Task 3.
- **Placeholder scan:** No TBDs; every code block is complete and copy-pasteable. The spec's prose said "7 existing `generate_route_track` calls" but actually listed 8 line numbers (724, 738, 753, 786, 788, 796, 888, 900) — this plan's Task 2 Step 1 correctly enumerates and updates all 8 call sites (note lines 786 and 788 are two separate calls inside the same test, handled together in one step since they're adjacent).
- **Type consistency:** `min_twa_deg: f64` is used identically across `run_isochrone` (Task 1), `generate_route_track` (Task 2), both query structs, and both handlers (Task 3). Parameter position is consistent: always immediately after `min_sail_speed_kn`.
