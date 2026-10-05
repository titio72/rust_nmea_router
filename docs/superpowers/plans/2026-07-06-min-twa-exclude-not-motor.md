# Min TWA: Exclude Heading Instead Of Motoring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix `run_isochrone` so that a candidate heading tighter than `min_twa_deg` is excluded from the search entirely, instead of being replaced with a motoring candidate — the router should route around a headwind via a longer sailing path, never turn the engine on because of it.

**Architecture:** Single gate-logic change inside `run_isochrone`'s per-heading candidate loop (`src/routing.rs`): replace the `if twa < min_twa_deg { (motoring_speed_kn, false) }` branch with `if twa < min_twa_deg { continue; }`, so no candidate (sailing or motoring) is generated for that heading at all. No signature change, no other files touched.

**Tech Stack:** Rust (`src/routing.rs`).

## Global Constraints

- Backend: Rust only (CLAUDE.md).
- Do NOT run `git commit` or `git push` — per this repo's CLAUDE.md, stop after code changes; the user commits.
- Only `run_isochrone` changes. `generate_route_track` (`src/forecast.rs`), the API query structs/validation (`src/web/api.rs`), and the frontend (`static/plan.html`) are confirmed unchanged — their existing `min_twa_deg` wiring (parameter name, default 60°, range `[0,180]`) stays exactly as already shipped.
- The `_ => (motoring_speed_kn, false)` fallback for "no forecast wind data at this point/time, or wind speed ≤ 0" is untouched — that is a genuinely different situation (nothing to sail on at all), not a headwind exclusion.
- No changes to `MAX_STEPS`, `prune_isochrone`'s pruning/scoring, or stagnation detection.
- `run_isochrone`'s signature is unchanged — this is purely an internal gate-logic fix, not a parameter change.

---

### Task 1: Exclude headwind headings instead of motoring through them

**Files:**
- Modify: `src/routing.rs:101-121` (heading evaluation loop)
- Test: `src/routing.rs` (existing `mod tests`, replace `test_min_twa_deg_forces_motor_below_threshold` at lines 513-576)

**Interfaces:**
- No signature or interface changes — `run_isochrone`'s parameter list and `IsochroneResult` are unchanged. This task only changes internal gate behavior.

- [ ] **Step 1: Replace the existing test with one that discriminates old vs. new behavior**

In `src/routing.rs`, delete the existing test (current lines 513-576):

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

Replace it with:

```rust
    #[test]
    fn test_min_twa_deg_excludes_heading_never_motors_through_headwind() {
        // Motoring is fast (5 kn); the only sailable headings once min_twa_deg = 80.0 excludes
        // anything closer to the wind have a poor VMG toward this dead-upwind destination
        // (6 kn boat speed * cos(80°) ≈ 1.04 kn) — much slower than motoring. If the router
        // ever substituted "motor" for an excluded heading (the bug this test guards against),
        // it would simply motor the direct heading and reach in ~motoring_only_hours. The
        // correct behavior is to never offer that shortcut: the excluded heading must simply
        // not exist as a candidate, forcing a much slower sail-only route around the headwind.
        let from = (43.0, 8.0);
        let to = (43.2, 8.0); // ~12 nm due north
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let motoring_speed_kn = 5.0;
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
            80.0,
            0.0,
            &polars,
            &fetches,
            None,
        );
        assert!(
            result.reached_destination,
            "should still reach destination via slow tacking"
        );

        let total_hours =
            (result.track.last().unwrap().2 - departure).num_seconds() as f64 / 3600.0;
        let motoring_only_hours =
            haversine_distance_nm(from.0, from.1, to.0, to.1) / motoring_speed_kn;
        assert!(
            total_hours > motoring_only_hours * 1.5,
            "expected much slower than pure motoring ({:.2}h) since min_twa_deg=80 must exclude \
             the headwind heading rather than motor through it, got {:.2}h",
            motoring_only_hours,
            total_hours
        );
    }
```

- [ ] **Step 2: Run the test to verify it fails against the current (unfixed) code**

Run: `cargo test --bin nmea_router routing::tests::test_min_twa_deg_excludes_heading_never_motors_through_headwind`
Expected: FAIL — the assertion `total_hours > motoring_only_hours * 1.5` does not hold, because the current code still substitutes `(motoring_speed_kn, false)` for the excluded heading, so the router simply motors the direct 12 nm heading in ~`12/5 = 2.4` hours, nowhere near 1.5× that.

- [ ] **Step 3: Fix the gate — exclude the heading instead of motoring**

In `src/routing.rs`, change (current lines 101-121):

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

to:

```rust
                let (speed_kn, was_sailing) = match wind {
                    Some((wind_spd, wind_dir)) if wind_spd > 0.0 => {
                        let twa = compute_twa(heading, wind_dir);
                        if twa < min_twa_deg {
                            continue;
                        }
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

`continue` skips to the next sampled heading in the enclosing `for h in 0..SECTOR_COUNT` loop — no `IsochronePoint` is ever pushed for this heading, so it contributes no candidate (sailing or motoring) to this step's frontier.

- [ ] **Step 4: Run the full routing test module to verify everything passes**

Run: `cargo test --bin nmea_router routing::tests`
Expected: PASS — all tests in `routing::tests`, including the new `test_min_twa_deg_excludes_heading_never_motors_through_headwind`. This also re-confirms the other existing tests (e.g. `test_arrival_prefers_tacking_over_forced_motor`, which passes `min_twa_deg = 0.0` and is therefore unaffected by this change) still pass unchanged.

- [ ] **Step 5: Do not commit**

Per this repo's CLAUDE.md, leave the change in the working tree for the user to review and commit themselves. Do not run `git add` or `git commit`.

---

## Self-Review Notes

- **Spec coverage:** The spec's single behavior change (exclude, don't motor) and single test replacement are both covered by this one task. The spec's explicit non-goals (no change to `generate_route_track`, `api.rs`, `plan.html`, `MAX_STEPS`, `prune_isochrone`, stagnation detection, or `run_isochrone`'s signature) require no tasks — there's nothing to do for them, which is the point.
- **Placeholder scan:** No TBDs; both the deleted and replacement test code, and the gate-logic diff, are complete and copy-pasteable.
- **Type consistency:** No signature or type changes anywhere in this task — `run_isochrone`'s call in the new test uses the exact same 11-argument positional call already established by the prior plan (`min_twa_deg` as the 7th positional argument, between `min_sail_speed_kn` and `sail_weight_kn`).
