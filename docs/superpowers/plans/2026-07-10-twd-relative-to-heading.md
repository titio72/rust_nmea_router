# TWD Relative-to-Heading Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the "TWD" column in both of `static/plan.html`'s step-by-step tables (the inline
Route Summary report and the alt-route-modal shown for alternative frontier routes) show the angle
between the boat's heading and the true wind direction, instead of the wind's absolute compass
bearing.

**Architecture:** Add a new `Option<f64>` field, `relative_wind_deg`, computed via the existing
`compute_twa(heading, wind_dir)` helper, unconditionally (regardless of the sail/motor decision) —
parallel to but independent of the existing `twa_deg` field, which is `None` whenever the router
chose to motor and can't be reused for this purpose. It's threaded through two independent data
paths: `RouteTrackPoint`/`RouteOverlayPoint` (`src/forecast.rs`, `src/web/api.rs`) for the main
Compute/Optimize/restored route, and `IsochronePoint`/`FrontierPoint` (`src/routing.rs`) for
alternative frontier routes. Both paths terminate in `static/plan.html`, which switches its two
"TWD" columns from reading `wind_direction_deg`/`wind_dir_deg` to reading `relative_wind_deg`.

**Tech Stack:** Rust (backend, `cargo test`), vanilla JavaScript (frontend, no test harness —
manual browser verification, per established project convention for `plan.html`).

## Global Constraints

- `relative_wind_deg` means: angle (0–180°) between the boat's heading and the true wind direction
  at a point, computed via the existing `compute_twa(cog_deg, wind_dir_deg)` in `src/forecast.rs` —
  do not modify `compute_twa` itself.
- Computed independently of `twa_deg`/the sail-vs-motor decision — `None` only when there's no
  heading yet (the departure/seed point) or no wind data at all.
- No change to `min_twa_deg` gating or any existing routing/sailing decision logic.
- No new API endpoint — this extends existing response shapes (`RouteOverlayPoint`,
  `FrontierPoint`), both already `Serialize`.
- Design source: [docs/superpowers/specs/2026-07-10-twd-relative-to-heading-design.md](../specs/2026-07-10-twd-relative-to-heading-design.md).
- Do not run `git add`/`git commit`/`git push` (project CLAUDE.md — the user reviews and commits
  everything themselves).

---

### Task 1: `heading_deg` / `relative_wind_deg` for the main route report

**Files:**
- Modify: `src/forecast.rs` (`RouteTrackPoint` struct at line 37-43, `RouteOverlayPoint` struct at
  line 19-34, `generate_route_track` pushes at line 383 and 423, `compute_route_overlay` at line
  440-476)
- Modify: `src/web/api.rs` (`get_optimal_route`'s manual `RouteTrackPoint` pushes at line 1862 and
  1893)
- Test: `src/forecast.rs` (`#[cfg(test)] mod tests` block, alongside existing tests like
  `test_compute_route_overlay_returns_points_with_coords` at line 752 and
  `test_generate_route_track_uses_polar_speed` at line 875)

**Interfaces:**
- Produces: `RouteTrackPoint.heading_deg: Option<f64>` (bearing of the leg into this point; `None`
  on the departure point). `RouteOverlayPoint.relative_wind_deg: Option<f64>`. Task 3 (frontend)
  reads `relative_wind_deg` off every point in the `route` array returned by
  `/api/forecast/route` and `/api/forecast/optimal-route`.

- [ ] **Step 1: Write the failing tests**

Add to `src/forecast.rs`'s `#[cfg(test)] mod tests` block (near the existing
`test_generate_route_track_uses_polar_speed` test, end of the file):

```rust
    #[test]
    fn test_generate_route_track_heading_deg() {
        use chrono::TimeZone;
        let dep = Utc.with_ymd_and_hms(2026, 5, 14, 6, 0, 0).unwrap();
        let wpts = vec![(43.0_f64, 8.0_f64), (43.0, 8.5)];
        let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, 0.0, None, &[]);
        assert!(track.len() >= 2);
        assert_eq!(track[0].heading_deg, None, "departure point has no incoming heading");
        let bearing = track[1].heading_deg.expect("heading_deg should be set for a traveled leg");
        let expected = crate::utilities::haversine_heading(track[0].lat, track[0].lon, track[1].lat, track[1].lon);
        assert!((bearing - expected).abs() < 0.01, "expected {}, got {}", expected, bearing);
    }

    #[test]
    fn test_compute_route_overlay_relative_wind_deg() {
        use chrono::TimeZone;
        use crate::db::operations::forecast::{FetchWithHourly, ForecastHourlyPoint};

        let dep = Utc.with_ymd_and_hms(2026, 5, 14, 9, 0, 0).unwrap();
        let wpts = vec![(43.5_f64, 9.0_f64), (43.5, 9.5)]; // eastward leg
        let track = generate_route_track(&wpts, dep, 10.0, 1.0, 0.0, 0.0, None, &[]);
        let hourly: Vec<ForecastHourlyPoint> = track.iter().map(|pt| ForecastHourlyPoint {
            timestamp: pt.time.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            wind_speed_kn: Some(12.0),
            wind_direction_deg: Some(180.0), // wind from due south
            wind_gust_kn: Some(15.0),
            wave_height_m: Some(1.0),
            wave_period_s: Some(6.0),
            wave_direction_deg: Some(185.0),
            cape_j_kg: Some(0.0),
        }).collect();
        let fetches = vec![FetchWithHourly {
            lat: 43.5, lon: 9.25,
            model: "ecmwf".to_string(),
            hourly,
        }];
        let overlay = compute_route_overlay(&track, &fetches);
        assert!(overlay.len() >= 2);
        assert_eq!(overlay[0].relative_wind_deg, None, "departure point has no heading yet");
        // Heading ~east (90°) with wind from due south (180°) → TWA = compute_twa(90, 180) = 90
        let twd = overlay[1].relative_wind_deg
            .expect("relative_wind_deg should be set once heading and wind are both known");
        assert!((twd - 90.0).abs() < 1.0, "expected ~90° relative wind angle, got {}", twd);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib forecast::tests::test_generate_route_track_heading_deg forecast::tests::test_compute_route_overlay_relative_wind_deg`
Expected: FAIL to compile — `no field \`heading_deg\` on type \`RouteTrackPoint\`` and `no field
\`relative_wind_deg\` on type \`RouteOverlayPoint\`` (neither field exists yet).

- [ ] **Step 3: Add the fields and wire them through**

In `src/forecast.rs`, change the `RouteTrackPoint` struct (currently at line 36-43):

```rust
#[derive(Debug, Clone)]
pub struct RouteTrackPoint {
    pub lat: f64,
    pub lon: f64,
    pub time: DateTime<Utc>,
    pub speed_kn: Option<f64>,
    pub twa_deg: Option<f64>,
    pub heading_deg: Option<f64>,
}
```

Change the `RouteOverlayPoint` struct (currently at line 19-34):

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
    pub speed_kn: Option<f64>,
    pub twa_deg: Option<f64>,
    pub wind_model: Option<String>,
    pub relative_wind_deg: Option<f64>,
}
```

In `generate_route_track`, find the departure-point push (currently at line 383):

```rust
            track.push(RouteTrackPoint { lat: pos.0, lon: pos.1, time: t, speed_kn: None, twa_deg: None });
```

Replace with:

```rust
            track.push(RouteTrackPoint { lat: pos.0, lon: pos.1, time: t, speed_kn: None, twa_deg: None, heading_deg: None });
```

Find the per-step push (currently at line 423):

```rust
            track.push(RouteTrackPoint { lat: pos.0, lon: pos.1, time: t, speed_kn: Some(speed_kn), twa_deg: twa });
```

Replace with:

```rust
            track.push(RouteTrackPoint { lat: pos.0, lon: pos.1, time: t, speed_kn: Some(speed_kn), twa_deg: twa, heading_deg: Some(bearing) });
```

In `compute_route_overlay`, find the point construction (currently at line 458-473):

```rust
            let interp = interpolate_blended(pt.lat, pt.lon, &arome, &ecmwf)?;
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
                wind_model: interp.wind_model,
            })
```

Replace with:

```rust
            let interp = interpolate_blended(pt.lat, pt.lon, &arome, &ecmwf)?;
            let relative_wind_deg = pt.heading_deg
                .zip(interp.wind_direction_deg)
                .map(|(h, wd)| compute_twa(h, wd));
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
                wind_model: interp.wind_model,
                relative_wind_deg,
            })
```

In `src/web/api.rs`, find the `i == 0` push inside `get_optimal_route` (currently at line
1861-1868):

```rust
        if i == 0 {
            route_points.push(crate::forecast::RouteTrackPoint {
                lat,
                lon,
                time,
                speed_kn: None,
                twa_deg: None,
            });
            continue;
        }
```

Replace with:

```rust
        if i == 0 {
            route_points.push(crate::forecast::RouteTrackPoint {
                lat,
                lon,
                time,
                speed_kn: None,
                twa_deg: None,
                heading_deg: None,
            });
            continue;
        }
```

Find the subsequent push (currently at line 1893-1899 — note `bearing` is already computed above
it at line 1879):

```rust
        route_points.push(crate::forecast::RouteTrackPoint {
            lat,
            lon,
            time,
            speed_kn: Some(speed_kn),
            twa_deg,
        });
```

Replace with:

```rust
        route_points.push(crate::forecast::RouteTrackPoint {
            lat,
            lon,
            time,
            speed_kn: Some(speed_kn),
            twa_deg,
            heading_deg: Some(bearing),
        });
```

- [ ] **Step 4: Run tests to verify they pass, and that the whole crate still builds**

Run: `cargo test --lib forecast::tests::test_generate_route_track_heading_deg forecast::tests::test_compute_route_overlay_relative_wind_deg`
Expected: PASS (2 passed)

Run: `cargo build`
Expected: builds cleanly — this also confirms `src/web/api.rs`'s two `RouteTrackPoint` literals
compile with the new field.

- [ ] **Step 5: Run the full non-DB test suite**

Run: `cargo test`
Expected: all non-`#[ignore]`d tests pass (no DB required for this task's tests).

- [ ] **Step 6: Stop for review**

Per this project's CLAUDE.md, do not run `git add`, `git commit`, or `git push` — leave
`src/forecast.rs` and `src/web/api.rs` unstaged for the user to review and commit themselves.

---

### Task 2: `relative_wind_deg` for frontier points (alt-route-modal)

**Files:**
- Modify: `src/routing.rs` (`IsochronePoint` struct at line 19-31, `FrontierPoint` struct at line
  33-42, seed construction at line 85-95, candidate loop at line 108-178, `frontiers` mapping at
  line 206-222, existing test constructors at line 300-327 and 366-409)
- Test: `src/routing.rs` (`#[cfg(test)] mod tests` block)

**Interfaces:**
- Consumes: `compute_twa(cog_deg: f64, wind_dir_deg: f64) -> f64` from `crate::forecast` (already
  imported in `src/routing.rs` at line 2 — no new import needed).
- Produces: `FrontierPoint.relative_wind_deg: Option<f64>`. Task 3 (frontend) reads this off each
  point in `OptimalRouteResponse.frontiers` (`lastFrontiers` client-side).

- [ ] **Step 1: Write the failing test**

Add to `src/routing.rs`'s `#[cfg(test)] mod tests` block (near the existing
`test_frontier_parent_idx_chains_to_origin` test):

```rust
    #[test]
    fn test_frontier_relative_wind_deg_matches_heading_and_wind() {
        use crate::db::operations::forecast::{FetchWithHourly, ForecastHourlyPoint};

        let from = (43.0, 8.0);
        let to = (43.29, 8.0); // ~20 nm north
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let polars = dummy_polars();

        let hourly = vec![ForecastHourlyPoint {
            timestamp: departure.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            wind_speed_kn: Some(12.0),
            wind_direction_deg: Some(90.0), // wind from due east
            wind_gust_kn: None, wave_height_m: None, wave_period_s: None,
            wave_direction_deg: None, cape_j_kg: None,
        }];
        let fetches = vec![FetchWithHourly { lat: 43.0, lon: 8.0, model: "ecmwf".to_string(), hourly }];

        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, 0.0, &polars, &fetches, None);
        assert!(!result.frontiers.is_empty());

        let first_step = &result.frontiers[0];
        let mut checked_any = false;
        for pt in first_step {
            if let (Some(rel), Some(wind_dir)) = (pt.relative_wind_deg, pt.wind_dir_deg) {
                let bearing = haversine_heading(from.0, from.1, pt.lat, pt.lon);
                let expected = crate::forecast::compute_twa(bearing, wind_dir);
                assert!(
                    (rel - expected).abs() < 1.0,
                    "relative_wind_deg {} doesn't match compute_twa(heading, wind_dir) {}",
                    rel, expected
                );
                checked_any = true;
            }
        }
        assert!(checked_any, "expected at least one candidate with wind data and relative_wind_deg set");
    }
```

Also extend the existing `test_frontiers_exclude_seed_and_respect_sector_count` test (currently at
line 536-565), which supplies no wind fetches (`&[]`), to assert `relative_wind_deg` stays `None`
without wind data. Find this loop inside that test:

```rust
        for frontier in &result.frontiers {
            assert!(
                frontier.len() <= SECTOR_COUNT,
                "frontier has {} points, expected <= {}",
                frontier.len(),
                SECTOR_COUNT
            );
            assert!(!frontier.is_empty(), "frontier must not be empty");
        }
```

Replace with:

```rust
        for frontier in &result.frontiers {
            assert!(
                frontier.len() <= SECTOR_COUNT,
                "frontier has {} points, expected <= {}",
                frontier.len(),
                SECTOR_COUNT
            );
            assert!(!frontier.is_empty(), "frontier must not be empty");
            assert!(
                frontier.iter().all(|p| p.relative_wind_deg.is_none()),
                "no wind data supplied, relative_wind_deg should stay None"
            );
        }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib routing::tests::test_frontier_relative_wind_deg_matches_heading_and_wind routing::tests::test_frontiers_exclude_seed_and_respect_sector_count`
Expected: FAIL to compile — `no field \`relative_wind_deg\` on type \`FrontierPoint\`` (the field
doesn't exist yet).

- [ ] **Step 3: Add the field and wire it through**

In `src/routing.rs`, change the `IsochronePoint` struct (currently at line 19-31):

```rust
#[derive(Clone)]
struct IsochronePoint {
    lat: f64,
    lon: f64,
    time: DateTime<Utc>,
    sailed_hours: f64,
    parent_idx: Option<usize>,
    // Leg data for the hop from parent -> this point; meaningless on the seed point.
    speed_kn: f64,
    motoring: bool,
    wind_speed_kn: Option<f64>,
    wind_dir_deg: Option<f64>,
    relative_wind_deg: Option<f64>,
}
```

Change the `FrontierPoint` struct (currently at line 33-42):

```rust
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct FrontierPoint {
    pub lat: f64,
    pub lon: f64,
    pub parent_idx: usize,
    pub speed_kn: f64,
    pub motoring: bool,
    pub wind_speed_kn: Option<f64>,
    pub wind_dir_deg: Option<f64>,
    pub relative_wind_deg: Option<f64>,
}
```

Find the seed construction (currently at line 85-95):

```rust
    let seed = IsochronePoint {
        lat: from.0,
        lon: from.1,
        time: departure,
        sailed_hours: 0.0,
        parent_idx: None,
        speed_kn: 0.0,
        motoring: false,
        wind_speed_kn: None,
        wind_dir_deg: None,
    };
```

Replace with:

```rust
    let seed = IsochronePoint {
        lat: from.0,
        lon: from.1,
        time: departure,
        sailed_hours: 0.0,
        parent_idx: None,
        speed_kn: 0.0,
        motoring: false,
        wind_speed_kn: None,
        wind_dir_deg: None,
        relative_wind_deg: None,
    };
```

In the candidate loop, find this line (currently at line 119):

```rust
                let heading = h as f64 * HEADING_STEP_DEG;
```

Add immediately after it:

```rust
                let heading = h as f64 * HEADING_STEP_DEG;
                let relative_wind_deg = wind.map(|(_, wind_dir)| compute_twa(heading, wind_dir));
```

Find the candidate push (currently at line 167-177):

```rust
                candidates.push(IsochronePoint {
                    lat: new_pos.0,
                    lon: new_pos.1,
                    time: parent.time + chrono::Duration::minutes((STEP_HOURS * 60.0) as i64),
                    sailed_hours: parent.sailed_hours + if was_sailing { STEP_HOURS } else { 0.0 },
                    parent_idx: Some(parent_idx),
                    speed_kn,
                    motoring: !was_sailing,
                    wind_speed_kn: wind.map(|(spd, _)| spd),
                    wind_dir_deg: wind.map(|(_, dir)| dir),
                });
```

Replace with:

```rust
                candidates.push(IsochronePoint {
                    lat: new_pos.0,
                    lon: new_pos.1,
                    time: parent.time + chrono::Duration::minutes((STEP_HOURS * 60.0) as i64),
                    sailed_hours: parent.sailed_hours + if was_sailing { STEP_HOURS } else { 0.0 },
                    parent_idx: Some(parent_idx),
                    speed_kn,
                    motoring: !was_sailing,
                    wind_speed_kn: wind.map(|(spd, _)| spd),
                    wind_dir_deg: wind.map(|(_, dir)| dir),
                    relative_wind_deg,
                });
```

Find the `frontiers` mapping (currently at line 206-222):

```rust
    let frontiers: Vec<Vec<FrontierPoint>> = isochrones[1..]
        .iter()
        .map(|frontier| {
            frontier
                .iter()
                .map(|p| FrontierPoint {
                    lat: p.lat,
                    lon: p.lon,
                    parent_idx: p.parent_idx.unwrap(),
                    speed_kn: p.speed_kn,
                    motoring: p.motoring,
                    wind_speed_kn: p.wind_speed_kn,
                    wind_dir_deg: p.wind_dir_deg,
                })
                .collect()
        })
        .collect();
```

Replace with:

```rust
    let frontiers: Vec<Vec<FrontierPoint>> = isochrones[1..]
        .iter()
        .map(|frontier| {
            frontier
                .iter()
                .map(|p| FrontierPoint {
                    lat: p.lat,
                    lon: p.lon,
                    parent_idx: p.parent_idx.unwrap(),
                    speed_kn: p.speed_kn,
                    motoring: p.motoring,
                    wind_speed_kn: p.wind_speed_kn,
                    wind_dir_deg: p.wind_dir_deg,
                    relative_wind_deg: p.relative_wind_deg,
                })
                .collect()
        })
        .collect();
```

Two existing test constructors also need the new field or the crate won't compile. In
`test_prune_retains_at_most_72_points` (currently at line 308-318), find:

```rust
                IsochronePoint {
                    lat,
                    lon,
                    time: chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
                    sailed_hours: 0.0,
                    parent_idx: Some(0),
                    speed_kn: 0.0,
                    motoring: false,
                    wind_speed_kn: None,
                    wind_dir_deg: None,
                }
```

Replace with:

```rust
                IsochronePoint {
                    lat,
                    lon,
                    time: chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
                    sailed_hours: 0.0,
                    parent_idx: Some(0),
                    speed_kn: 0.0,
                    motoring: false,
                    wind_speed_kn: None,
                    wind_dir_deg: None,
                    relative_wind_deg: None,
                }
```

In `test_sail_weight_prefers_sailing_candidate` (currently at line 377-398), find:

```rust
        let motoring = IsochronePoint {
            lat: motoring_pos.0,
            lon: motoring_pos.1,
            time: t,
            sailed_hours: 0.0,
            parent_idx: None,
            speed_kn: 0.0,
            motoring: true,
            wind_speed_kn: None,
            wind_dir_deg: None,
        };
        let sailing = IsochronePoint {
            lat: sailing_pos.0,
            lon: sailing_pos.1,
            time: t,
            sailed_hours: 1.0,
            parent_idx: None,
            speed_kn: 0.0,
            motoring: false,
            wind_speed_kn: None,
            wind_dir_deg: None,
        };
```

Replace with:

```rust
        let motoring = IsochronePoint {
            lat: motoring_pos.0,
            lon: motoring_pos.1,
            time: t,
            sailed_hours: 0.0,
            parent_idx: None,
            speed_kn: 0.0,
            motoring: true,
            wind_speed_kn: None,
            wind_dir_deg: None,
            relative_wind_deg: None,
        };
        let sailing = IsochronePoint {
            lat: sailing_pos.0,
            lon: sailing_pos.1,
            time: t,
            sailed_hours: 1.0,
            parent_idx: None,
            speed_kn: 0.0,
            motoring: false,
            wind_speed_kn: None,
            wind_dir_deg: None,
            relative_wind_deg: None,
        };
```

- [ ] **Step 4: Run tests to verify they pass, and that the whole crate still builds**

Run: `cargo test --lib routing::tests::`
Expected: all tests in `routing::tests` PASS, including the two new/modified ones.

Run: `cargo build`
Expected: builds cleanly.

- [ ] **Step 5: Run the full non-DB test suite**

Run: `cargo test`
Expected: all non-`#[ignore]`d tests pass.

- [ ] **Step 6: Stop for review**

Per this project's CLAUDE.md, do not run `git add`, `git commit`, or `git push` — leave
`src/routing.rs` unstaged for the user to review and commit themselves.

---

### Task 3: Frontend — both TWD columns read `relative_wind_deg`

**Files:**
- Modify: `static/plan.html` (`renderRouteReportTable` at line 1451-1463,
  `backtrackFrontierReport` at line 1128-1156, `showFrontierReport` at line 1164-1191,
  `selectFrontierRoute` at line 1198-1220)

**Interfaces:**
- Consumes: `relative_wind_deg: Option<f64>` — now present on every point in the `route` array from
  `/api/forecast/route`/`/api/forecast/optimal-route` (Task 1) and on every `FrontierPoint` in
  `OptimalRouteResponse.frontiers` (Task 2). No other new interfaces — this task only changes which
  field these four existing functions read.

This task has no automated test harness (project convention for `plan.html` — verification is
manual in a browser). There is no TDD red/green cycle here; each step below is a direct edit
followed by a manual check.

- [ ] **Step 1: Simplify `renderRouteReportTable` back to a plain-null check on `relative_wind_deg`**

Find (currently at line 1451-1463):

```javascript
        function renderRouteReportTable(pts) {
            const body = document.getElementById('routeReportTableBody');
            body.innerHTML = pts.map((p, i) => {
                const t = new Date(p.timestamp);
                const time = t.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', timeZone: 'UTC' })
                    + ' UTC ' + t.toLocaleDateString('en-GB', { day: '2-digit', month: 'short', timeZone: 'UTC' });
                const spd = p.speed_kn != null ? p.speed_kn.toFixed(1) + ' kn' : '—';
                const eng = p.speed_kn == null ? '—' : (p.twa_deg === null ? '⚙ Motoring' : '⛵ Sailing');
                const tws = p.wind_speed_kn != null ? p.wind_speed_kn.toFixed(1) + ' kn' : '—';
                const twd = i === 0 ? '—' : (p.wind_direction_deg != null ? p.wind_direction_deg.toFixed(0) + '°' : '—');
                return `<tr><td>${time}</td><td>${spd}</td><td>${eng}</td><td>${tws}</td><td>${twd}</td></tr>`;
            }).join('');
        }
```

Replace with:

```javascript
        function renderRouteReportTable(pts) {
            const body = document.getElementById('routeReportTableBody');
            body.innerHTML = pts.map(p => {
                const t = new Date(p.timestamp);
                const time = t.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', timeZone: 'UTC' })
                    + ' UTC ' + t.toLocaleDateString('en-GB', { day: '2-digit', month: 'short', timeZone: 'UTC' });
                const spd = p.speed_kn != null ? p.speed_kn.toFixed(1) + ' kn' : '—';
                const eng = p.speed_kn == null ? '—' : (p.twa_deg === null ? '⚙ Motoring' : '⛵ Sailing');
                const tws = p.wind_speed_kn != null ? p.wind_speed_kn.toFixed(1) + ' kn' : '—';
                const twd = p.relative_wind_deg != null ? p.relative_wind_deg.toFixed(0) + '°' : '—';
                return `<tr><td>${time}</td><td>${spd}</td><td>${eng}</td><td>${tws}</td><td>${twd}</td></tr>`;
            }).join('');
        }
```

- [ ] **Step 2: Thread `relative_wind_deg` through `backtrackFrontierReport`**

Find (currently at line 1128-1156):

```javascript
        function backtrackFrontierReport(stepIdx, ptIdx) {
            const rows = [];
            let s = stepIdx, i = ptIdx;
            const clickedPt = lastFrontiers[stepIdx][ptIdx];
            while (s >= 0) {
                const pt = lastFrontiers[s][i];
                rows.push({
                    time: new Date(new Date(lastFrontiersDeparture).getTime()
                        + (s + 1) * FRONTIER_STEP_HOURS * 3600000),
                    speed_kn: pt.speed_kn,
                    motoring: pt.motoring,
                    wind_speed_kn: pt.wind_speed_kn,
                    wind_dir_deg: pt.wind_dir_deg
                });
                i = pt.parent_idx;
                s -= 1;
            }
            rows.push({ time: new Date(lastFrontiersDeparture), speed_kn: null, motoring: null,
                        wind_speed_kn: null, wind_dir_deg: null });
            rows.reverse();

            const lastTime = rows[rows.length - 1].time;
            const leg = frontierDestinationLeg(clickedPt.lat, clickedPt.lon, lastTime, clickedPt.speed_kn);
            if (leg) {
                rows.push({ time: leg.time, speed_kn: clickedPt.speed_kn, motoring: clickedPt.motoring,
                            wind_speed_kn: null, wind_dir_deg: null });
            }
            return rows;
        }
```

Replace with:

```javascript
        function backtrackFrontierReport(stepIdx, ptIdx) {
            const rows = [];
            let s = stepIdx, i = ptIdx;
            const clickedPt = lastFrontiers[stepIdx][ptIdx];
            while (s >= 0) {
                const pt = lastFrontiers[s][i];
                rows.push({
                    time: new Date(new Date(lastFrontiersDeparture).getTime()
                        + (s + 1) * FRONTIER_STEP_HOURS * 3600000),
                    speed_kn: pt.speed_kn,
                    motoring: pt.motoring,
                    wind_speed_kn: pt.wind_speed_kn,
                    wind_dir_deg: pt.wind_dir_deg,
                    relative_wind_deg: pt.relative_wind_deg
                });
                i = pt.parent_idx;
                s -= 1;
            }
            rows.push({ time: new Date(lastFrontiersDeparture), speed_kn: null, motoring: null,
                        wind_speed_kn: null, wind_dir_deg: null, relative_wind_deg: null });
            rows.reverse();

            const lastTime = rows[rows.length - 1].time;
            const leg = frontierDestinationLeg(clickedPt.lat, clickedPt.lon, lastTime, clickedPt.speed_kn);
            if (leg) {
                rows.push({ time: leg.time, speed_kn: clickedPt.speed_kn, motoring: clickedPt.motoring,
                            wind_speed_kn: null, wind_dir_deg: null, relative_wind_deg: null });
            }
            return rows;
        }
```

- [ ] **Step 3: Switch `showFrontierReport`'s TWD column to `relative_wind_deg`**

Find (currently at line 1179-1188):

```javascript
            const body = document.getElementById('altRouteTableBody');
            body.innerHTML = rows.map(r => {
                const t = r.time.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', timeZone: 'UTC' })
                    + ' UTC ' + r.time.toLocaleDateString('en-GB', { day: '2-digit', month: 'short', timeZone: 'UTC' });
                const spd = r.speed_kn != null ? r.speed_kn.toFixed(1) + ' kn' : '—';
                const eng = r.motoring == null ? '—' : (r.motoring ? '⚙ Motoring' : '⛵ Sailing');
                const tws = r.wind_speed_kn != null ? r.wind_speed_kn.toFixed(1) + ' kn' : '—';
                const twd = r.wind_dir_deg != null ? r.wind_dir_deg.toFixed(0) + '°' : '—';
                return `<tr><td>${t}</td><td>${spd}</td><td>${eng}</td><td>${tws}</td><td>${twd}</td></tr>`;
            }).join('');
```

Replace with:

```javascript
            const body = document.getElementById('altRouteTableBody');
            body.innerHTML = rows.map(r => {
                const t = r.time.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', timeZone: 'UTC' })
                    + ' UTC ' + r.time.toLocaleDateString('en-GB', { day: '2-digit', month: 'short', timeZone: 'UTC' });
                const spd = r.speed_kn != null ? r.speed_kn.toFixed(1) + ' kn' : '—';
                const eng = r.motoring == null ? '—' : (r.motoring ? '⚙ Motoring' : '⛵ Sailing');
                const tws = r.wind_speed_kn != null ? r.wind_speed_kn.toFixed(1) + ' kn' : '—';
                const twd = r.relative_wind_deg != null ? r.relative_wind_deg.toFixed(0) + '°' : '—';
                return `<tr><td>${t}</td><td>${spd}</td><td>${eng}</td><td>${tws}</td><td>${twd}</td></tr>`;
            }).join('');
```

- [ ] **Step 4: Carry `relative_wind_deg` into the promoted route in `selectFrontierRoute`**

Find (currently at line 1198-1220):

```javascript
        function selectFrontierRoute() {
            if (openFrontierReportStepIdx == null || openFrontierReportPtIdx == null) return;
            const path = backtrackFrontierPath(openFrontierReportStepIdx, openFrontierReportPtIdx);
            const rows = backtrackFrontierReport(openFrontierReportStepIdx, openFrontierReportPtIdx);
            const pts = rows.map((r, i) => ({
                lat: path[i][0],
                lon: path[i][1],
                timestamp: r.time.toISOString(),
                speed_kn: r.speed_kn,
                twa_deg: r.motoring == null ? null : (r.motoring ? null : 0),
                wind_speed_kn: r.wind_speed_kn,
                wind_direction_deg: r.wind_dir_deg,
                wind_gust_kn: null,
                wave_height_m: null,
                wave_period_s: null
            }));

            hideFrontierAlternative();

            drawRouteLine(pts);
            drawRouteSummary(pts);
            closeFrontierReport();
        }
```

Replace with:

```javascript
        function selectFrontierRoute() {
            if (openFrontierReportStepIdx == null || openFrontierReportPtIdx == null) return;
            const path = backtrackFrontierPath(openFrontierReportStepIdx, openFrontierReportPtIdx);
            const rows = backtrackFrontierReport(openFrontierReportStepIdx, openFrontierReportPtIdx);
            const pts = rows.map((r, i) => ({
                lat: path[i][0],
                lon: path[i][1],
                timestamp: r.time.toISOString(),
                speed_kn: r.speed_kn,
                twa_deg: r.motoring == null ? null : (r.motoring ? null : 0),
                wind_speed_kn: r.wind_speed_kn,
                wind_direction_deg: r.wind_dir_deg,
                relative_wind_deg: r.relative_wind_deg,
                wind_gust_kn: null,
                wave_height_m: null,
                wave_period_s: null
            }));

            hideFrontierAlternative();

            drawRouteLine(pts);
            drawRouteSummary(pts);
            closeFrontierReport();
        }
```

- [ ] **Step 5: Manual verification — main report (Compute/Optimize)**

Start the server and open `plan.html`. Place two waypoints forming an eastward leg inside a
forecast area with a steady, clearly non-north/non-east wind (check the area's actual forecast
wind direction via the existing wind-particle overlay or hourly data first, so you know what to
expect), set departure/speed, and click "Compute".

Expected: the "Step-by-step" table's TWD column shows values that are NOT the same as the area's
absolute wind direction (unless heading happens to equal 0° or 180° relative to it) — e.g. if the
wind is blowing from a compass bearing that is 60° away from the boat's heading on a given leg, TWD
should read close to 60°, not the compass bearing itself. Turn the route roughly 90° (add a
waypoint that bends the course) and confirm TWD changes between the two legs even though the wind
direction hasn't changed.

- [ ] **Step 6: Manual verification — alt-route-modal and Select-this-route**

Click "Optimize", then hover and click a frontier line to open the alt-route-modal. Confirm its
"TWD" column also shows a heading-relative angle (not the raw compass wind direction), consistent
with the same wind/heading check as Step 5. Click "Select this route" and confirm the inline
Step-by-step report's TWD column now matches the values that were just shown in the modal for that
same path.

- [ ] **Step 7: Stop for review**

Per this project's CLAUDE.md, do not run `git add`, `git commit`, or `git push` — leave
`static/plan.html` unstaged for the user to review and commit themselves.
