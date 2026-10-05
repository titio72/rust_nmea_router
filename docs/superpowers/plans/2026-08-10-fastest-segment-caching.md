# Fastest-Segment Analytics: Fold Into Leg Cache — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the O(n²), uncached `fetch_track_analytics` (max speed + fastest 1/5/10/25nm
segments) with a per-leg, O(n), cached computation folded into the existing `trip_legs_cache`
pipeline, and eliminate the `/api/track_analytics` network round-trip entirely by computing the
result client-side from data `trip.html` already has loaded.

**Architecture:** `compute_trip_legs`'s single per-trip `vessel_status` scan (in
`src/db/operations/query.rs`) is extended to also compute, per leg: running max speed and a
single-pass (monotonic two-pointer) fastest-segment search for each of 1/5/10/25nm, scoped to that
leg only — a segment can never span a mooring stop between legs. Results ride the exact same cache
lifecycle `trip_legs_cache` already has (computed fresh while open, cached once closed, invalidated
by the same three existing triggers). Average speeds need no new storage — they're derived from
data that already exists (`trips` row for whole-trip, `trip_legs_cache`'s existing distance/time
columns per leg). The frontend combines/reads this from the `/api/trip_legs` response it already
fetches, so `/api/track_analytics` is deleted outright.

**Tech Stack:** Rust (mysql crate, chrono), vanilla JS (`static/trip.html`).

**Reference spec:** `docs/superpowers/specs/2026-08-10-fastest-segment-caching-design.md`

## Global Constraints

- Backend: Rust only. Frontend: HTML + vanilla JS (project convention, CLAUDE.md).
- `snake_case` functions, `PascalCase` structs (CLAUDE.md naming).
- SQL: parameterized queries (`params!` macro) always (CLAUDE.md).
- Never call `now()` inside business logic (CLAUDE.md) — N/A here, no new time-of-computation
  logic is added beyond what `compute_trip_legs` already does.
- Angles/distances: Haversine only (CLAUDE.md) — the new algorithm reuses the existing
  `crate::utilities::haversine_distance_nm`, already imported in `query.rs`.
- This project's CLAUDE.md normally forbids `git commit`/`git push`. The user has granted a scoped
  exception for this plan's execution only: task-level local commits (as each task's steps
  specify) are allowed so the review tooling has something to diff. **`git push` remains
  forbidden regardless** — nothing leaves this machine without the user's separate say-so.

---

## Task 1: Two-pointer fastest-segment search (pure, no DB)

**Files:**
- Modify: `src/db/operations/query.rs` — add new functions near the existing `find_fastest_segment`
  (module level, outside `impl VesselDatabase`, same as `LegRecord`/`finalize_leg` today).
- Modify: `src/db/types.rs:200` — add `Clone, PartialEq` to `FastestSegment`'s derive (needed for
  `assert_eq!` in tests below; harmless in production, no other type currently derives them so this
  is an additive, non-breaking change).
- Test: same file, inside the existing `#[cfg(test)] mod tests { ... }` block in `query.rs`.

**Interfaces:**
- Produces: `fn fastest_segment_in_leg(records: &[LegRecord], target_distance_nm: f64) -> Option<FastestSegment>`
  — used by Task 2.
- Consumes: existing `LegRecord` struct (`query.rs:90`, fields: `timestamp: String, speed_kn: f64,
  distance_nm: f64, time_ms: u64, engine_on: bool, lat: Option<f64>, lon: Option<f64>`), existing
  `FastestSegment` struct (`types.rs:200-207`), existing `haversine_distance_nm` (already imported),
  existing `find_fastest_segment` (kept until Task 5, used here only as a test oracle).

- [ ] **Step 1: Add `Clone, PartialEq` to `FastestSegment`'s derive**

In `src/db/types.rs`, change:
```rust
#[derive(Debug, serde::Serialize)]
pub struct FastestSegment {
```
to:
```rust
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct FastestSegment {
```

- [ ] **Step 2: Write the failing tests**

In `src/db/operations/query.rs`, inside the `#[cfg(test)] mod tests { ... }` block (add near the
top, after the `use` statements and before `fn setup_db`), add synthetic-data helpers and two
tests:

```rust
fn synthetic_leg_constant_speed(n: usize, speed_kn: f64) -> Vec<LegRecord> {
    let interval_s: f64 = 10.0;
    let dist_per_point = speed_kn * interval_s / 3600.0; // nm per 10s interval
    let deg_per_nm = 1.0 / 60.0; // ~1 nm per 1/60 degree of latitude
    let base = chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    (0..n)
        .map(|i| LegRecord {
            timestamp: (base + chrono::Duration::seconds(i as i64 * interval_s as i64))
                .format("%Y-%m-%dT%H:%M:%S.000Z")
                .to_string(),
            speed_kn,
            distance_nm: dist_per_point,
            time_ms: (interval_s * 1000.0) as u64,
            engine_on: false,
            lat: Some(40.0 + i as f64 * dist_per_point * deg_per_nm),
            lon: Some(2.0),
        })
        .collect()
}

fn synthetic_leg_with_engine_gap(
    n: usize,
    speed_kn: f64,
    gap_start: usize,
    gap_end: usize,
) -> Vec<LegRecord> {
    let mut records = synthetic_leg_constant_speed(n, speed_kn);
    for r in records.iter_mut().take(gap_end).skip(gap_start) {
        r.engine_on = true;
    }
    records
}

fn synthetic_leg_becalmed_stretch(
    n: usize,
    speed_kn: f64,
    becalm_start: usize,
    becalm_end: usize,
) -> Vec<LegRecord> {
    let mut records = synthetic_leg_constant_speed(n, speed_kn);
    let frozen_lat = records[becalm_start].lat;
    for r in records.iter_mut().take(becalm_end).skip(becalm_start) {
        r.lat = frozen_lat;
        r.distance_nm = 0.0;
    }
    records
}

#[test]
fn fastest_segment_in_leg_matches_old_algorithm_on_synthetic_data() {
    // Cross-check the two-pointer rewrite against the pre-existing O(n^2) algorithm across
    // constant-speed, engine-gap, and becalmed-stretch cases. Disagreement means the rewrite
    // has a bug — this is the safety net for a hand-derived algorithm change.
    let cases: Vec<Vec<LegRecord>> = vec![
        synthetic_leg_constant_speed(200, 6.0),
        synthetic_leg_with_engine_gap(200, 6.0, 50, 70),
        synthetic_leg_becalmed_stretch(300, 6.0, 100, 250),
    ];
    for (i, records) in cases.iter().enumerate() {
        for target in [1.0, 5.0, 10.0, 25.0] {
            let old_points: Vec<(String, f64, f64, f64, bool)> = records
                .iter()
                .map(|r| {
                    (
                        r.timestamp.clone(),
                        r.lat.unwrap_or(0.0),
                        r.lon.unwrap_or(0.0),
                        r.speed_kn,
                        r.engine_on,
                    )
                })
                .collect();
            let expected = find_fastest_segment(&old_points, target);
            let actual = fastest_segment_in_leg(records, target);
            assert_eq!(
                actual.as_ref().map(|s| s.average_speed_kn),
                expected.as_ref().map(|s| s.average_speed_kn),
                "case {} target {}nm: two-pointer disagrees with reference algorithm",
                i,
                target
            );
        }
    }
}

#[test]
fn fastest_segment_in_leg_is_linear_not_quadratic_on_becalmed_stretch() {
    // Regression test for the original O(n^2) blowup: a long becalmed (near-zero-distance,
    // engine-off) stretch must complete near-instantly now that the algorithm is a genuine
    // two-pointer. An accidental revert to nested-loop behavior makes this test visibly slow.
    let records = synthetic_leg_becalmed_stretch(20_000, 6.0, 100, 19_900);
    let start = std::time::Instant::now();
    let _ = fastest_segment_in_leg(&records, 25.0);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 500,
        "fastest_segment_in_leg took {:?} on 20k becalmed points — looks quadratic",
        elapsed
    );
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --bin nmea_router fastest_segment_in_leg`
Expected: FAIL — `cannot find function fastest_segment_in_leg in this scope` (compile error, not a
runtime assertion failure — confirms the test exercises code that doesn't exist yet).

- [ ] **Step 4: Implement `fastest_segment_in_leg` and its helpers**

In `src/db/operations/query.rs`, add near the existing `find_fastest_segment` function (module
level):

```rust
/// Find the fastest continuous segment of at least `target_distance_nm` within a single leg's
/// records, considering only maximal runs where `engine_on` is false — a segment can never
/// include a motoring point, matching the semantics of the original per-trip algorithm. O(leg
/// length) per target distance via a monotonic two-pointer within each run: the window's start
/// and end indices only ever advance, never reset backward.
fn fastest_segment_in_leg(records: &[LegRecord], target_distance_nm: f64) -> Option<FastestSegment> {
    let mut best: Option<FastestSegment> = None;
    let mut run_start = 0;
    while run_start < records.len() {
        if records[run_start].engine_on {
            run_start += 1;
            continue;
        }
        let mut run_end = run_start;
        while run_end < records.len() && !records[run_end].engine_on {
            run_end += 1;
        }
        if let Some(candidate) = fastest_in_run(&records[run_start..run_end], target_distance_nm) {
            let better = best
                .as_ref()
                .map(|b| candidate.average_speed_kn > b.average_speed_kn)
                .unwrap_or(true);
            if better {
                best = Some(candidate);
            }
        }
        run_start = run_end;
    }
    best
}

/// Two-pointer scan within a single engine-off run: for each `right`, shrink `left` as far as
/// possible while the window still covers `target_distance_nm`. `left` only ever advances across
/// the whole run, so this is O(run length), not O(run length^2).
fn fastest_in_run(run: &[LegRecord], target_distance_nm: f64) -> Option<FastestSegment> {
    if run.len() < 2 {
        return None;
    }
    let edge_dist: Vec<f64> = (0..run.len() - 1)
        .map(|i| {
            haversine_distance_nm(
                run[i].lat.unwrap_or(0.0),
                run[i].lon.unwrap_or(0.0),
                run[i + 1].lat.unwrap_or(0.0),
                run[i + 1].lon.unwrap_or(0.0),
            )
        })
        .collect();

    let mut best: Option<FastestSegment> = None;
    let mut left = 0usize;
    let mut window_dist = 0.0;

    for right in 1..run.len() {
        window_dist += edge_dist[right - 1];
        while left < right && window_dist - edge_dist[left] >= target_distance_nm {
            window_dist -= edge_dist[left];
            left += 1;
        }
        if window_dist >= target_distance_nm {
            if let Some(candidate) = segment_from_window(run, left, right, window_dist) {
                let better = best
                    .as_ref()
                    .map(|b| candidate.average_speed_kn > b.average_speed_kn)
                    .unwrap_or(true);
                if better {
                    best = Some(candidate);
                }
            }
        }
    }
    best
}

fn segment_from_window(
    run: &[LegRecord],
    left: usize,
    right: usize,
    distance_nm: f64,
) -> Option<FastestSegment> {
    let start_time = chrono::NaiveDateTime::parse_from_str(
        &run[left].timestamp.replace('Z', ""),
        "%Y-%m-%dT%H:%M:%S%.f",
    )
    .ok()?;
    let end_time = chrono::NaiveDateTime::parse_from_str(
        &run[right].timestamp.replace('Z', ""),
        "%Y-%m-%dT%H:%M:%S%.f",
    )
    .ok()?;
    let duration_ms = (end_time - start_time).num_milliseconds().max(0) as u64;
    if duration_ms == 0 {
        return None;
    }
    let average_speed_kn = distance_nm / (duration_ms as f64 / 1000.0 / 3600.0);
    Some(FastestSegment {
        distance_nm,
        average_speed_kn,
        duration_ms,
        start_timestamp: run[left].timestamp.clone(),
        end_timestamp: run[right].timestamp.clone(),
    })
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --bin nmea_router fastest_segment_in_leg`
Expected: PASS (2 tests).

- [ ] **Step 6: Run the full non-DB suite to check nothing else broke**

Run: `cargo test --bin nmea_router`
Expected: same pass count as before this task plus 2 (pre-existing DB-lock-timeout flakiness in
`test_infrastructure_examples::*`, if present, is unrelated — see prior session notes; re-run a
failing test alone to confirm it passes standalone before treating it as a real regression).

- [ ] **Step 7: Commit**

```bash
git add src/db/types.rs src/db/operations/query.rs
git commit -m "Add O(n) two-pointer fastest-segment search, scoped per leg"
```

---

## Task 2: Wire the search into `finalize_leg` + extend `TripLeg`

**Files:**
- Modify: `src/db/types.rs:135-158` — add new fields to `TripLeg`.
- Modify: `src/db/operations/query.rs:126-214` — extend `finalize_leg` to populate them.
- Test: same `mod tests` block in `query.rs`.

**Interfaces:**
- Consumes: `fastest_segment_in_leg` from Task 1.
- Produces: `TripLeg` now carries `max_speed_kn: Option<f64>`, `max_speed_timestamp: Option<String>`,
  `fastest_1nm/5nm/10nm/25nm: Option<FastestSegment>` — used by Task 3 (schema/cache) and Task 4
  (frontend, via the JSON these fields serialize to).

- [ ] **Step 1: Write the failing test**

In `query.rs`'s `mod tests`, add (reuses `synthetic_leg_constant_speed` from Task 1):

```rust
#[test]
fn finalize_leg_populates_speed_records() {
    let records = synthetic_leg_constant_speed(400, 6.0); // 400 * 10s = ~1.1h, ~2.5nm total
    let leg = finalize_leg(&records, 1, records[0].lat, records[0].lon)
        .expect("leg should finalize — total distance exceeds the 0.5nm minimum");

    assert!(leg.max_speed_kn.is_some());
    assert!((leg.max_speed_kn.unwrap() - 6.0).abs() < 0.01);
    assert!(leg.max_speed_timestamp.is_some());

    // Total leg distance is ~2.5nm (400 * 6.0 * 10/3600), so a 1nm segment must exist...
    assert!(leg.fastest_1nm.is_some());
    let seg = leg.fastest_1nm.as_ref().unwrap();
    assert!((seg.average_speed_kn - 6.0).abs() < 0.1);
    // ...but 25nm never fits in a 2.5nm leg.
    assert!(leg.fastest_25nm.is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bin nmea_router finalize_leg_populates_speed_records`
Expected: FAIL — `no field max_speed_kn on type TripLeg` (compile error).

- [ ] **Step 3: Add the new fields to `TripLeg`**

In `src/db/types.rs`, change:
```rust
#[derive(Debug, serde::Serialize)]
pub struct TripLeg {
    pub leg_number: u32,
    pub start_timestamp: String,
    pub end_timestamp: String,
    pub total_distance_nm: f64,
    pub sailing_distance_nm: f64,
    pub motoring_distance_nm: f64,
    pub sailing_time_ms: u64,
    pub motoring_time_ms: u64,
    pub sailing_time_formatted: String,
    pub motoring_time_formatted: String,
    pub start_lat: Option<f64>,
    pub start_lon: Option<f64>,
    pub end_lat: Option<f64>,
    pub end_lon: Option<f64>,
    /// Timestamp when pure navigation begins (first engine-off, or first point ≥ 4 kn)
    pub nav_start_timestamp: Option<String>,
    /// Timestamp when pure navigation ends (last engine-off before final engine-on, or last point ≥ 4 kn)
    pub nav_end_timestamp: Option<String>,
    pub nav_distance_nm: f64,
    pub nav_time_ms: u64,
    /// How the nav window was detected: "engine_transition", "speed_fallback", or null
    pub nav_detection_method: Option<String>,
}
```
to (adding the six new fields at the end):
```rust
#[derive(Debug, serde::Serialize)]
pub struct TripLeg {
    pub leg_number: u32,
    pub start_timestamp: String,
    pub end_timestamp: String,
    pub total_distance_nm: f64,
    pub sailing_distance_nm: f64,
    pub motoring_distance_nm: f64,
    pub sailing_time_ms: u64,
    pub motoring_time_ms: u64,
    pub sailing_time_formatted: String,
    pub motoring_time_formatted: String,
    pub start_lat: Option<f64>,
    pub start_lon: Option<f64>,
    pub end_lat: Option<f64>,
    pub end_lon: Option<f64>,
    /// Timestamp when pure navigation begins (first engine-off, or first point ≥ 4 kn)
    pub nav_start_timestamp: Option<String>,
    /// Timestamp when pure navigation ends (last engine-off before final engine-on, or last point ≥ 4 kn)
    pub nav_end_timestamp: Option<String>,
    pub nav_distance_nm: f64,
    pub nav_time_ms: u64,
    /// How the nav window was detected: "engine_transition", "speed_fallback", or null
    pub nav_detection_method: Option<String>,
    /// Highest recorded speed while sailing (engine off) within this leg.
    pub max_speed_kn: Option<f64>,
    pub max_speed_timestamp: Option<String>,
    pub fastest_1nm: Option<FastestSegment>,
    pub fastest_5nm: Option<FastestSegment>,
    pub fastest_10nm: Option<FastestSegment>,
    pub fastest_25nm: Option<FastestSegment>,
}
```

- [ ] **Step 4: Populate the new fields in `finalize_leg`**

In `src/db/operations/query.rs`, `finalize_leg` currently ends with:
```rust
    let start_timestamp = records
        .first()
        .map(|r| r.timestamp.clone())
        .unwrap_or_default();
    let end_timestamp = records
        .last()
        .map(|r| r.timestamp.clone())
        .unwrap_or_default();

    Some(TripLeg {
        leg_number,
        start_timestamp,
        end_timestamp,
        total_distance_nm: total_distance,
        sailing_distance_nm: sailing_distance,
        motoring_distance_nm: motoring_distance,
        sailing_time_ms: sailing_time,
        motoring_time_ms: motoring_time,
        sailing_time_formatted: format_duration_ms(sailing_time),
        motoring_time_formatted: format_duration_ms(motoring_time),
        start_lat,
        start_lon,
        end_lat,
        end_lon,
        nav_start_timestamp,
        nav_end_timestamp,
        nav_distance_nm,
        nav_time_ms,
        nav_detection_method,
    })
}
```
Change to:
```rust
    let start_timestamp = records
        .first()
        .map(|r| r.timestamp.clone())
        .unwrap_or_default();
    let end_timestamp = records
        .last()
        .map(|r| r.timestamp.clone())
        .unwrap_or_default();

    // Every record in `records` already belongs to a non-moored stretch (compute_trip_legs only
    // pushes here when !is_moored), so no separate moored filter is needed — unlike the old
    // whole-trip algorithm, which only excluded moored points from the distance/time sums, not
    // from max-speed tracking.
    let mut max_speed_kn: Option<f64> = None;
    let mut max_speed_timestamp: Option<String> = None;
    for r in records {
        if !r.engine_on && (max_speed_kn.is_none() || r.speed_kn > max_speed_kn.unwrap()) {
            max_speed_kn = Some(r.speed_kn);
            max_speed_timestamp = Some(r.timestamp.clone());
        }
    }
    let fastest_1nm = fastest_segment_in_leg(records, 1.0);
    let fastest_5nm = fastest_segment_in_leg(records, 5.0);
    let fastest_10nm = fastest_segment_in_leg(records, 10.0);
    let fastest_25nm = fastest_segment_in_leg(records, 25.0);

    Some(TripLeg {
        leg_number,
        start_timestamp,
        end_timestamp,
        total_distance_nm: total_distance,
        sailing_distance_nm: sailing_distance,
        motoring_distance_nm: motoring_distance,
        sailing_time_ms: sailing_time,
        motoring_time_ms: motoring_time,
        sailing_time_formatted: format_duration_ms(sailing_time),
        motoring_time_formatted: format_duration_ms(motoring_time),
        start_lat,
        start_lon,
        end_lat,
        end_lon,
        nav_start_timestamp,
        nav_end_timestamp,
        nav_distance_nm,
        nav_time_ms,
        nav_detection_method,
        max_speed_kn,
        max_speed_timestamp,
        fastest_1nm,
        fastest_5nm,
        fastest_10nm,
        fastest_25nm,
    })
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --bin nmea_router finalize_leg_populates_speed_records`
Expected: PASS.

- [ ] **Step 6: Run the full non-DB suite**

Run: `cargo test --bin nmea_router`
Expected: all passing as in Task 1's Step 6, plus this new test. `cargo build --release` should
also succeed — other constructors of `TripLeg` (if any exist outside `finalize_leg`, e.g. in
`src/db/test_helpers.rs`) will fail to compile until updated; check with `cargo build --release`
and fix any such struct-literal call sites by adding the six new fields (`None` is a safe default
for hand-built test fixtures that don't care about speed records).

- [ ] **Step 7: Commit**

```bash
git add src/db/types.rs src/db/operations/query.rs
git commit -m "Compute max speed and fastest segments per leg in finalize_leg"
```

---

## Task 3: Persist the new fields in `trip_legs_cache`

**Files:**
- Modify: `src/db/operations/query.rs` — `get_cached_trip_legs` (~line 1196), `save_trip_legs_to_cache`
  (~line 1330), `invalidate_trip_legs_cache` (~line 1374).
- Test: DB-backed `#[ignore]` test in the same `mod tests` block, following the existing
  `setup_db()` / `add_test_trip` / `add_test_vessel_status` pattern used by `test_get_track`.

**Interfaces:**
- Consumes: `TripLeg`'s new fields from Task 2.
- Produces: closed trips now cache and return the new fields via `fetch_trip_legs` — used by
  Task 4 (frontend reads them from the `/api/trip_legs` JSON response, no API contract change
  needed since `TripLeg` already serializes whatever fields it has).

- [ ] **Step 1: Write the failing DB test**

In `query.rs`'s `mod tests`, add (mirrors `test_get_track`'s structure):

```rust
#[test]
#[ignore]
fn test_trip_legs_cache_round_trips_speed_records() {
    let db = setup_db();
    const ONE_HOUR_S: u64 = 3600;

    let start_time = SystemTime::now().add(Duration::from_secs(48 * ONE_HOUR_S));
    let end_time = start_time.add(Duration::from_secs(2 * ONE_HOUR_S));

    let trip_id = add_test_trip(
        &db,
        "Speed Record Cache Test".to_string(),
        start_time,
        end_time,
        10.5,
        2.3,
        3600000,
        600000,
        0,
    )
    .expect("Failed to insert test trip");

    // A steady 6kn run for 2 hours (~12nm) — long enough that fastest_1nm/5nm/10nm all exist,
    // fastest_25nm does not. add_test_vessel_status's own signature: (db, timestamp, latitude,
    // longitude, average_speed_kn, max_speed_kn, average_wind_speed_kn, average_wind_angle_deg,
    // is_moored, engine_on, total_distance_nm, total_time_ms, cog_deg, average_heading_deg).
    let mut current_time = start_time;
    let mut lat = 41.0;
    let interval_s = 30u64;
    let dist_per_interval_nm = 6.0 * interval_s as f64 / 3600.0; // 0.05nm per 30s at 6kn
    while current_time < end_time {
        add_test_vessel_status(
            &db,
            current_time,
            lat,
            2.0,
            6.0,
            6.0,
            None,
            None,
            false,
            EngineStatus::Off,
            dist_per_interval_nm,
            interval_s * 1000,
            None,
            None,
        )
        .expect("Failed to insert vessel status");
        current_time = current_time.add(Duration::from_secs(interval_s));
        lat += dist_per_interval_nm / 60.0; // ~1 nm per 1/60 degree of latitude
    }

    // fetch_trip_legs always computes fresh regardless of is_closed — only the caching step is
    // conditional — so this exercises finalize_leg's new fields without depending on wall-clock
    // trip closure timing.
    let legs_data = db.fetch_trip_legs(trip_id).expect("fetch_trip_legs failed");
    assert!(!legs_data.legs.is_empty(), "expected at least one leg");
    let leg = &legs_data.legs[0];
    assert!(leg.max_speed_kn.is_some(), "max_speed_kn should be populated");
    assert!(leg.fastest_1nm.is_some(), "fastest_1nm should be populated for a 12nm run");
    assert!(leg.fastest_5nm.is_some(), "fastest_5nm should be populated for a 12nm run");
    assert!(leg.fastest_25nm.is_none(), "fastest_25nm should be absent for a 12nm run");

    let fastest_1nm_before = leg.fastest_1nm.clone();
    let max_speed_before = leg.max_speed_kn;

    // Exercise the cache write/read path directly (mirrors how get_cached_trip_legs is reached
    // for closed trips) via the #[cfg(test)] wrappers added below.
    db.save_trip_legs_to_cache_for_test(trip_id, &legs_data.legs)
        .expect("save_trip_legs_to_cache failed");
    let cached = db
        .get_cached_trip_legs_for_test(trip_id)
        .expect("get_cached_trip_legs failed")
        .expect("expected a cached row");
    assert_eq!(cached.legs[0].fastest_1nm, fastest_1nm_before);
    assert_eq!(cached.legs[0].max_speed_kn, max_speed_before);
}
```

This test calls two `#[cfg(test)]`-only wrapper methods (`save_trip_legs_to_cache_for_test`,
`get_cached_trip_legs_for_test`) because `save_trip_legs_to_cache`/`get_cached_trip_legs` are
private (`fn`, not `pub fn`) — add these thin wrappers right after `impl VesselDatabase {` opens
(near `fetch_trip`), guarded by `#[cfg(test)]`:

```rust
    #[cfg(test)]
    pub fn save_trip_legs_to_cache_for_test(
        &self,
        trip_id: u32,
        legs: &[crate::db::types::TripLeg],
    ) -> Result<(), AppError> {
        let mut conn = self.pool.get_conn()?;
        self.save_trip_legs_to_cache(&mut conn, trip_id, legs)
    }

    #[cfg(test)]
    pub fn get_cached_trip_legs_for_test(
        &self,
        trip_id: u32,
    ) -> Result<Option<crate::db::types::TripLegsData>, AppError> {
        let mut conn = self.pool.get_conn()?;
        self.get_cached_trip_legs(&mut conn, trip_id)
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --bin nmea_router test_trip_legs_cache_round_trips_speed_records -- --ignored --test-threads=1`
Expected: FAIL — either a compile error (new test-only wrapper methods reference fields/columns
that don't exist in the cache table yet) or a runtime assertion failure (`fastest_1nm` is `None`
after round-tripping through the cache, because the SELECT/INSERT don't include the new columns
yet).

- [ ] **Step 3: Add the new columns to both `CREATE TABLE trip_legs_cache` statements**

In `get_cached_trip_legs` (`query.rs` ~line 1201), change the `CREATE TABLE IF NOT EXISTS
trip_legs_cache (...)` block's column list from:
```sql
                nav_distance_nm      DOUBLE          NOT NULL DEFAULT 0,
                nav_time_ms          BIGINT UNSIGNED NOT NULL DEFAULT 0,
                nav_detection_method VARCHAR(20)     NULL,
                PRIMARY KEY (trip_id, leg_number)
```
to:
```sql
                nav_distance_nm      DOUBLE          NOT NULL DEFAULT 0,
                nav_time_ms          BIGINT UNSIGNED NOT NULL DEFAULT 0,
                nav_detection_method VARCHAR(20)     NULL,
                max_speed_kn                 DOUBLE          NULL,
                max_speed_timestamp          VARCHAR(30)     NULL,
                fastest_1nm_distance_nm      DOUBLE          NULL,
                fastest_1nm_avg_speed_kn     DOUBLE          NULL,
                fastest_1nm_duration_ms      BIGINT UNSIGNED NULL,
                fastest_1nm_start_timestamp  VARCHAR(30)     NULL,
                fastest_1nm_end_timestamp    VARCHAR(30)     NULL,
                fastest_5nm_distance_nm      DOUBLE          NULL,
                fastest_5nm_avg_speed_kn     DOUBLE          NULL,
                fastest_5nm_duration_ms      BIGINT UNSIGNED NULL,
                fastest_5nm_start_timestamp  VARCHAR(30)     NULL,
                fastest_5nm_end_timestamp    VARCHAR(30)     NULL,
                fastest_10nm_distance_nm     DOUBLE          NULL,
                fastest_10nm_avg_speed_kn    DOUBLE          NULL,
                fastest_10nm_duration_ms     BIGINT UNSIGNED NULL,
                fastest_10nm_start_timestamp VARCHAR(30)     NULL,
                fastest_10nm_end_timestamp   VARCHAR(30)     NULL,
                fastest_25nm_distance_nm     DOUBLE          NULL,
                fastest_25nm_avg_speed_kn    DOUBLE          NULL,
                fastest_25nm_duration_ms     BIGINT UNSIGNED NULL,
                fastest_25nm_start_timestamp VARCHAR(30)     NULL,
                fastest_25nm_end_timestamp   VARCHAR(30)     NULL,
                PRIMARY KEY (trip_id, leg_number)
```

Then extend the "best-effort migrations" `ALTER TABLE` list right below it (add these 22 new
strings to the existing `&[...]` array in the same function):
```rust
            "ALTER TABLE trip_legs_cache ADD COLUMN max_speed_kn DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN max_speed_timestamp VARCHAR(30) NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_1nm_distance_nm DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_1nm_avg_speed_kn DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_1nm_duration_ms BIGINT UNSIGNED NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_1nm_start_timestamp VARCHAR(30) NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_1nm_end_timestamp VARCHAR(30) NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_5nm_distance_nm DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_5nm_avg_speed_kn DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_5nm_duration_ms BIGINT UNSIGNED NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_5nm_start_timestamp VARCHAR(30) NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_5nm_end_timestamp VARCHAR(30) NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_10nm_distance_nm DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_10nm_avg_speed_kn DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_10nm_duration_ms BIGINT UNSIGNED NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_10nm_start_timestamp VARCHAR(30) NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_10nm_end_timestamp VARCHAR(30) NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_25nm_distance_nm DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_25nm_avg_speed_kn DOUBLE NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_25nm_duration_ms BIGINT UNSIGNED NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_25nm_start_timestamp VARCHAR(30) NULL",
            "ALTER TABLE trip_legs_cache ADD COLUMN fastest_25nm_end_timestamp VARCHAR(30) NULL",
```

Do the same column additions to `invalidate_trip_legs_cache`'s `CREATE TABLE IF NOT EXISTS
trip_legs_cache (...)` block (~line 1377) for consistency — it's a smaller/older copy of the same
table definition (already missing the `nav_*` columns `get_cached_trip_legs`'s copy has; leave
that pre-existing drift alone, just add the same 22 new columns before its `PRIMARY KEY` line).

- [ ] **Step 4: Add a `fastest_segment_from_row` helper and use it in `get_cached_trip_legs`'s SELECT**

Add this helper function at module level in `query.rs` (near `get_or_log`):
```rust
/// Reconstruct an `Option<FastestSegment>` from a `trip_legs_cache` row's `{prefix}_*` columns.
/// All five columns are written together (see `save_trip_legs_to_cache`) so partial-NULL rows
/// only occur pre-migration; treat any missing field as "no segment" rather than panicking.
fn fastest_segment_from_row(row: &mysql::Row, prefix: &str) -> Option<FastestSegment> {
    let distance_nm: Option<f64> = row
        .get_opt(format!("{prefix}_distance_nm").as_str())
        .and_then(|v| v.ok());
    let average_speed_kn: Option<f64> = row
        .get_opt(format!("{prefix}_avg_speed_kn").as_str())
        .and_then(|v| v.ok());
    let duration_ms: Option<u64> = row
        .get_opt(format!("{prefix}_duration_ms").as_str())
        .and_then(|v| v.ok());
    let start_timestamp: Option<String> = row
        .get_opt(format!("{prefix}_start_timestamp").as_str())
        .and_then(|v: Result<Option<String>, _>| v.ok())
        .flatten();
    let end_timestamp: Option<String> = row
        .get_opt(format!("{prefix}_end_timestamp").as_str())
        .and_then(|v: Result<Option<String>, _>| v.ok())
        .flatten();
    match (distance_nm, average_speed_kn, duration_ms, start_timestamp, end_timestamp) {
        (Some(distance_nm), Some(average_speed_kn), Some(duration_ms), Some(start_timestamp), Some(end_timestamp)) => {
            Some(FastestSegment {
                distance_nm,
                average_speed_kn,
                duration_ms,
                start_timestamp,
                end_timestamp,
            })
        }
        _ => None,
    }
}
```

In `get_cached_trip_legs`'s `SELECT` statement, change:
```rust
        let rows: Vec<mysql::Row> = conn.exec(
            r"SELECT leg_number, start_timestamp, end_timestamp,
                         total_distance_nm, sailing_distance_nm, motoring_distance_nm,
                         sailing_time_ms, motoring_time_ms,
                         start_lat, start_lon, end_lat, end_lon,
                         nav_start_timestamp, nav_end_timestamp,
                         nav_distance_nm, nav_time_ms, nav_detection_method
                  FROM trip_legs_cache
                  WHERE trip_id = :trip_id
                  ORDER BY leg_number",
            mysql::params! { "trip_id" => trip_id },
        )?;
```
to:
```rust
        let rows: Vec<mysql::Row> = conn.exec(
            r"SELECT leg_number, start_timestamp, end_timestamp,
                         total_distance_nm, sailing_distance_nm, motoring_distance_nm,
                         sailing_time_ms, motoring_time_ms,
                         start_lat, start_lon, end_lat, end_lon,
                         nav_start_timestamp, nav_end_timestamp,
                         nav_distance_nm, nav_time_ms, nav_detection_method,
                         max_speed_kn, max_speed_timestamp,
                         fastest_1nm_distance_nm, fastest_1nm_avg_speed_kn, fastest_1nm_duration_ms,
                         fastest_1nm_start_timestamp, fastest_1nm_end_timestamp,
                         fastest_5nm_distance_nm, fastest_5nm_avg_speed_kn, fastest_5nm_duration_ms,
                         fastest_5nm_start_timestamp, fastest_5nm_end_timestamp,
                         fastest_10nm_distance_nm, fastest_10nm_avg_speed_kn, fastest_10nm_duration_ms,
                         fastest_10nm_start_timestamp, fastest_10nm_end_timestamp,
                         fastest_25nm_distance_nm, fastest_25nm_avg_speed_kn, fastest_25nm_duration_ms,
                         fastest_25nm_start_timestamp, fastest_25nm_end_timestamp
                  FROM trip_legs_cache
                  WHERE trip_id = :trip_id
                  ORDER BY leg_number",
            mysql::params! { "trip_id" => trip_id },
        )?;
```

In the row-mapping closure right below (the `rows.iter().map(|row| { ... TripLeg { ... } })`),
change the closing part from:
```rust
                    nav_time_ms: get_or_log(row, "nav_time_ms", 0u64, "get_cached_trip_legs"),
                    nav_detection_method: row
                        .get_opt("nav_detection_method")
                        .and_then(|v: Result<Option<String>, _>| v.ok())
                        .flatten(),
                }
```
to:
```rust
                    nav_time_ms: get_or_log(row, "nav_time_ms", 0u64, "get_cached_trip_legs"),
                    nav_detection_method: row
                        .get_opt("nav_detection_method")
                        .and_then(|v: Result<Option<String>, _>| v.ok())
                        .flatten(),
                    max_speed_kn: row.get_opt("max_speed_kn").and_then(|v| v.ok()),
                    max_speed_timestamp: row
                        .get_opt("max_speed_timestamp")
                        .and_then(|v: Result<Option<String>, _>| v.ok())
                        .flatten(),
                    fastest_1nm: fastest_segment_from_row(row, "fastest_1nm"),
                    fastest_5nm: fastest_segment_from_row(row, "fastest_5nm"),
                    fastest_10nm: fastest_segment_from_row(row, "fastest_10nm"),
                    fastest_25nm: fastest_segment_from_row(row, "fastest_25nm"),
                }
```

- [ ] **Step 5: Extend `save_trip_legs_to_cache`'s INSERT**

Change:
```rust
    fn save_trip_legs_to_cache(
        &self,
        conn: &mut mysql::PooledConn,
        trip_id: u32,
        legs: &[TripLeg],
    ) -> Result<(), AppError> {
        if legs.is_empty() {
            return Ok(());
        }
        conn.exec_batch(
            r"INSERT IGNORE INTO trip_legs_cache
                (trip_id, leg_number, start_timestamp, end_timestamp,
                 total_distance_nm, sailing_distance_nm, motoring_distance_nm,
                 sailing_time_ms, motoring_time_ms,
                 start_lat, start_lon, end_lat, end_lon,
                 nav_start_timestamp, nav_end_timestamp,
                 nav_distance_nm, nav_time_ms, nav_detection_method)
              VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            legs.iter().map(|leg| -> Vec<mysql::Value> {
                vec![
                    trip_id.into(),
                    leg.leg_number.into(),
                    leg.start_timestamp.as_str().into(),
                    leg.end_timestamp.as_str().into(),
                    leg.total_distance_nm.into(),
                    leg.sailing_distance_nm.into(),
                    leg.motoring_distance_nm.into(),
                    leg.sailing_time_ms.into(),
                    leg.motoring_time_ms.into(),
                    leg.start_lat.into(),
                    leg.start_lon.into(),
                    leg.end_lat.into(),
                    leg.end_lon.into(),
                    leg.nav_start_timestamp.as_deref().into(),
                    leg.nav_end_timestamp.as_deref().into(),
                    leg.nav_distance_nm.into(),
                    leg.nav_time_ms.into(),
                    leg.nav_detection_method.as_deref().into(),
                ]
            }),
        )?;
        Ok(())
    }
```
to:
```rust
    fn save_trip_legs_to_cache(
        &self,
        conn: &mut mysql::PooledConn,
        trip_id: u32,
        legs: &[TripLeg],
    ) -> Result<(), AppError> {
        if legs.is_empty() {
            return Ok(());
        }
        conn.exec_batch(
            r"INSERT IGNORE INTO trip_legs_cache
                (trip_id, leg_number, start_timestamp, end_timestamp,
                 total_distance_nm, sailing_distance_nm, motoring_distance_nm,
                 sailing_time_ms, motoring_time_ms,
                 start_lat, start_lon, end_lat, end_lon,
                 nav_start_timestamp, nav_end_timestamp,
                 nav_distance_nm, nav_time_ms, nav_detection_method,
                 max_speed_kn, max_speed_timestamp,
                 fastest_1nm_distance_nm, fastest_1nm_avg_speed_kn, fastest_1nm_duration_ms,
                 fastest_1nm_start_timestamp, fastest_1nm_end_timestamp,
                 fastest_5nm_distance_nm, fastest_5nm_avg_speed_kn, fastest_5nm_duration_ms,
                 fastest_5nm_start_timestamp, fastest_5nm_end_timestamp,
                 fastest_10nm_distance_nm, fastest_10nm_avg_speed_kn, fastest_10nm_duration_ms,
                 fastest_10nm_start_timestamp, fastest_10nm_end_timestamp,
                 fastest_25nm_distance_nm, fastest_25nm_avg_speed_kn, fastest_25nm_duration_ms,
                 fastest_25nm_start_timestamp, fastest_25nm_end_timestamp)
              VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
                      ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            legs.iter().map(|leg| -> Vec<mysql::Value> {
                let mut values: Vec<mysql::Value> = vec![
                    trip_id.into(),
                    leg.leg_number.into(),
                    leg.start_timestamp.as_str().into(),
                    leg.end_timestamp.as_str().into(),
                    leg.total_distance_nm.into(),
                    leg.sailing_distance_nm.into(),
                    leg.motoring_distance_nm.into(),
                    leg.sailing_time_ms.into(),
                    leg.motoring_time_ms.into(),
                    leg.start_lat.into(),
                    leg.start_lon.into(),
                    leg.end_lat.into(),
                    leg.end_lon.into(),
                    leg.nav_start_timestamp.as_deref().into(),
                    leg.nav_end_timestamp.as_deref().into(),
                    leg.nav_distance_nm.into(),
                    leg.nav_time_ms.into(),
                    leg.nav_detection_method.as_deref().into(),
                    leg.max_speed_kn.into(),
                    leg.max_speed_timestamp.as_deref().into(),
                ];
                for segment in [&leg.fastest_1nm, &leg.fastest_5nm, &leg.fastest_10nm, &leg.fastest_25nm] {
                    match segment {
                        Some(s) => {
                            values.push(s.distance_nm.into());
                            values.push(s.average_speed_kn.into());
                            values.push(s.duration_ms.into());
                            values.push(s.start_timestamp.as_str().into());
                            values.push(s.end_timestamp.as_str().into());
                        }
                        None => {
                            for _ in 0..5 {
                                values.push(mysql::Value::NULL);
                            }
                        }
                    }
                }
                values
            }),
        )?;
        Ok(())
    }
```

- [ ] **Step 6: Run the DB test to verify it passes**

Requires a live MariaDB configured in `test_config.json` (per CLAUDE.md testing section).
Run: `cargo test --bin nmea_router test_trip_legs_cache_round_trips_speed_records -- --ignored --test-threads=1`
Expected: PASS.

- [ ] **Step 7: Confirm the existing invalidation triggers already cover the new fields — no code change needed**

`invalidate_trip_legs_cache` (called from `delete_trip`, `trim_trip`, and `correct_engine_status`
in `src/db/operations/trip.rs`) runs `DELETE FROM trip_legs_cache WHERE trip_id = :trip_id` — this
removes the *entire* row per leg, new speed-record columns included, not just the original
leg-boundary columns. So all three existing triggers already invalidate the new fields correctly
with zero additional code. Confirm this by reading `src/db/operations/trip.rs:90-92,134-136,346-348`
(the three `invalidate_trip_legs_cache` call sites) — no edit needed here, this step is a
verification checkpoint, not a code change.

- [ ] **Step 8: One-time cache reset for trips cached before this change**

Trips already cached under the old schema have `NULL` in the new columns (the `ALTER TABLE ADD
COLUMN` migration from Step 3 adds them as `NULL`-default, it doesn't backfill data). Per the
design doc, don't write NULL-detection logic to paper over this — just clear the cache once so
every closed trip recomputes (and re-caches, now with the new fields) on its next visit, the same
as any other cache miss:
```bash
mysql -u <user> -p <database_name> -e "DELETE FROM trip_legs_cache"
```
Run this once against whichever database this plan's changes are deployed to (for local dev, the
database `config.json`/`test_config.json` points at). Not needed in CI/fresh databases — the table
starts empty there.

- [ ] **Step 9: Run the full test suite (including ignored DB tests) once, serially**

Run: `cargo test --bin nmea_router -- --test-threads=1 --include-ignored`
Expected: all pass (this is slower — only run once at the end of this task, not after every step).

- [ ] **Step 10: Commit**

```bash
git add src/db/operations/query.rs
git commit -m "Persist per-leg max speed and fastest segments in trip_legs_cache"
```

---

## Task 4: Frontend — compute analytics client-side from already-loaded legs

**Files:**
- Modify: `static/trip.html` — replace `renderAnalytics()`'s fetch with a synchronous computation,
  update its call site in `loadTripDetails`.

**Interfaces:**
- Consumes: `currentTrip` (global, from `/api/trip`), `currentLegs` (global, from
  `/api/trip_legs` — now carries the new fields from Task 3), `selectedLeg` (the
  `loadTripDetails` parameter — a `TripLeg`-shaped object, or `null` for full-trip view).
- Produces: same `analyticsData` shape `renderAnalyticsCards` and `handleSegmentClick` already
  consume (`max_speed_kn`, `max_speed_timestamp`, `fastest_1nm/5nm/10nm/25nm`, `average_speed_kn`,
  `average_speed_sailing_kn`, `average_speed_motoring_kn`) — **no change** to
  `renderAnalyticsCards` or `handleSegmentClick` themselves.

- [ ] **Step 1: Add `averageSpeed` and `buildAnalyticsFromLegs` helpers**

In `static/trip.html`, add these two functions right before `async function renderAnalytics(...)`
(current line 913 — they'll replace it in Step 2):

```js
function averageSpeed(distanceNm, timeMs) {
    if (!timeMs || timeMs <= 0) return null;
    return distanceNm / (timeMs / 3600000);
}

// Builds the same shape /api/track_analytics used to return, from data already loaded via
// /api/trip and /api/trip_legs — no network request needed. selectedLeg is the TripLeg object
// for a single-leg view, or null for the full-trip view (best-of across all legs).
function buildAnalyticsFromLegs(trip, legs, selectedLeg) {
    if (selectedLeg) {
        const sailingMs = selectedLeg.sailing_time_ms || 0;
        const motoringMs = selectedLeg.motoring_time_ms || 0;
        return {
            max_speed_kn: selectedLeg.max_speed_kn ?? null,
            max_speed_timestamp: selectedLeg.max_speed_timestamp ?? null,
            fastest_1nm: selectedLeg.fastest_1nm ?? null,
            fastest_5nm: selectedLeg.fastest_5nm ?? null,
            fastest_10nm: selectedLeg.fastest_10nm ?? null,
            fastest_25nm: selectedLeg.fastest_25nm ?? null,
            average_speed_kn: averageSpeed(
                (selectedLeg.sailing_distance_nm || 0) + (selectedLeg.motoring_distance_nm || 0),
                sailingMs + motoringMs
            ),
            average_speed_sailing_kn: averageSpeed(selectedLeg.sailing_distance_nm || 0, sailingMs),
            average_speed_motoring_kn: averageSpeed(selectedLeg.motoring_distance_nm || 0, motoringMs)
        };
    }

    let maxSpeedKn = null;
    let maxSpeedTimestamp = null;
    const best = { fastest_1nm: null, fastest_5nm: null, fastest_10nm: null, fastest_25nm: null };
    for (const leg of legs) {
        if (leg.max_speed_kn !== null && leg.max_speed_kn !== undefined &&
            (maxSpeedKn === null || leg.max_speed_kn > maxSpeedKn)) {
            maxSpeedKn = leg.max_speed_kn;
            maxSpeedTimestamp = leg.max_speed_timestamp;
        }
        for (const key of ['fastest_1nm', 'fastest_5nm', 'fastest_10nm', 'fastest_25nm']) {
            const candidate = leg[key];
            if (candidate && (!best[key] || candidate.average_speed_kn > best[key].average_speed_kn)) {
                best[key] = candidate;
            }
        }
    }

    const sailingMs = trip.sailing_time_ms || 0;
    const motoringMs = trip.motoring_time_ms || 0;
    return {
        max_speed_kn: maxSpeedKn,
        max_speed_timestamp: maxSpeedTimestamp,
        fastest_1nm: best.fastest_1nm,
        fastest_5nm: best.fastest_5nm,
        fastest_10nm: best.fastest_10nm,
        fastest_25nm: best.fastest_25nm,
        average_speed_kn: averageSpeed(
            (trip.sailing_distance_nm || 0) + (trip.motoring_distance_nm || 0),
            sailingMs + motoringMs
        ),
        average_speed_sailing_kn: averageSpeed(trip.sailing_distance_nm || 0, sailingMs),
        average_speed_motoring_kn: averageSpeed(trip.motoring_distance_nm || 0, motoringMs)
    };
}
```

- [ ] **Step 2: Remove the old `renderAnalytics` function**

Delete the entire existing function (current lines 913-936):
```js
async function renderAnalytics(startTime, endTime) {
    markTime('analytics_start');
    let analyticsQuery;
    const formattedStart = encodeURIComponent(startTime.split('.')[0].replace('T', ' '));
    const formattedEnd = encodeURIComponent(endTime.split('.')[0].replace('T', ' '));
    analyticsQuery = '/api/track_analytics?start=' + formattedStart + '&end=' + formattedEnd;
    try {
        const analyticsResponse = await fetch(analyticsQuery);
        let analyticsData = null;
        if (analyticsResponse.ok) {
            const result = await analyticsResponse.json();
            if (result.status === 'ok' && result.data) {
                analyticsData = result.data;
            }
        }
        markTime('analytics_fetch_resolved');
        // Save analytics data globally for segment marker handling
        currentAnalyticsData = analyticsData;
        renderAnalyticsCards(analyticsData);
        markTime('analytics_rendered');
    } catch (error) {
        console.error('Failed to fetch analytics data:', error);
    }
}
```
(This is fully replaced by `buildAnalyticsFromLegs` + the call-site change in Step 3 — nothing
else in the file calls `renderAnalytics`, confirmed by its only other reference being the call
site being changed next.)

- [ ] **Step 3: Update the call site in `loadTripDetails`**

Change:
```js
                renderAnalytics(startTime, endTime).catch(error => {
                    console.error('Failed to load analytics:', error);
                });
```
to:
```js
                currentAnalyticsData = buildAnalyticsFromLegs(currentTrip, currentLegs, selectedLeg);
                renderAnalyticsCards(currentAnalyticsData);
                markTime('analytics_rendered_client_side');
```

- [ ] **Step 4: Verify JS syntax**

Run:
```bash
awk '/^<script>$/{c++} c==1 && /^<script>$/{start=NR} /^<\/script>$/{if(c==1){print NR; exit}}' static/trip.html
```
(or simply locate the inline `<script>...</script>` block's line range with
`grep -n "<script>$\|</script>$" static/trip.html`, extract that range with `sed`, and run
`node --check` on it — same approach used earlier in this project's session for this exact file.)
Expected: no syntax errors.

- [ ] **Step 5: Verify end-to-end against a running dev server**

This repo has no JS test framework (confirmed earlier in this project). Verify manually:
1. Ensure a dev server is running against a database with at least one closed trip that has
   sailing data (`./target/debug/nmea_router` or `./target/release/nmea_router` with a valid
   `config.json`).
2. Load `http://<host>:<port>/trip.html?id=<a real trip id>` in a browser (or via the
   headless-Chrome CDP capture script used earlier this session, if still present in the
   scratchpad) and open the browser console.
3. Confirm: no request to `/api/track_analytics` appears in the Network tab; the analytics cards
   (Max Speed, Fastest 1/5/10/25 NM, Average Speed) render with plausible non-error values; a
   `[timing] analytics_rendered_client_side` console line appears with a near-zero delta (no
   network round-trip).
4. Select an individual leg from the leg selector and repeat — confirm the analytics cards update
   to that leg's own values (not the whole-trip best-of).

- [ ] **Step 6: Commit**

```bash
git add static/trip.html
git commit -m "Compute trip analytics client-side from already-loaded leg data"
```

---

## Task 5: Delete the now-unused `/api/track_analytics` endpoint and O(n²) algorithm

**Files:**
- Modify: `src/web/api.rs` — remove `get_track_analytics`, its route registration, and
  `TimeRangeRequiredQuery`.
- Modify: `src/db/operations/query.rs` — remove `fetch_track_analytics`, the old
  `find_fastest_segment`, and the differential test from Task 1 that depended on it.
- Modify: `src/db/types.rs` — remove `TrackAnalytics`.
- Modify: `src/db/mod.rs` — remove `TrackAnalytics` from the re-export list.
- Modify: `src/bin/trip_timing.rs` — remove the `fetch_track_analytics` diagnostic call (this
  binary was built earlier in this project's session as a DB-layer timing tool; it must keep
  compiling).

**Interfaces:** none — this task only removes code confirmed unused by every other task.

- [ ] **Step 1: Confirm nothing else references the code being removed**

Run:
```bash
grep -rln "TrackAnalytics\|fetch_track_analytics\|find_fastest_segment" src/
```
Expected output: exactly `src/bin/trip_timing.rs`, `src/db/mod.rs`, `src/db/operations/query.rs`,
`src/db/types.rs`, `src/web/api.rs` — the five files below. If anything else shows up, stop and
investigate before deleting (something added later in this plan may have started depending on the
old code).

- [ ] **Step 2: Remove the differential test's dependency on the old algorithm**

In `query.rs`'s `mod tests`, the test `fastest_segment_in_leg_matches_old_algorithm_on_synthetic_data`
(added in Task 1) calls `find_fastest_segment` as a reference oracle. Now that the two-pointer
rewrite has been running in production-shaped code since Task 2-3 (and this differential test has
already served its purpose of catching rewrite bugs during development), delete this test along
with the function it references:
```rust
#[test]
fn fastest_segment_in_leg_matches_old_algorithm_on_synthetic_data() {
    // ... (entire test body from Task 1)
}
```
Keep `fastest_segment_in_leg_is_linear_not_quadratic_on_becalmed_stretch` — it has no dependency
on the old algorithm and remains a valuable regression guard.

- [ ] **Step 3: Remove `find_fastest_segment` and `fetch_track_analytics` from `query.rs`**

Delete the `fetch_track_analytics` method (inside `impl VesselDatabase`, spans from
`pub fn fetch_track_analytics(` through its closing `}` — includes the `log_timing` calls added
earlier this session) and the standalone `find_fastest_segment` function (module level, after the
`impl VesselDatabase` block closes). Locate exact boundaries with:
```bash
grep -n "pub fn fetch_track_analytics\|^fn find_fastest_segment" src/db/operations/query.rs
```
and delete from each start line through its matching closing brace.

- [ ] **Step 4: Remove `TrackAnalytics` from `types.rs` and `mod.rs`**

In `src/db/types.rs`, delete:
```rust
#[derive(Debug, serde::Serialize)]
pub struct TrackAnalytics {
    pub max_speed_kn: Option<f64>,
    pub max_speed_timestamp: Option<String>,
    pub average_speed_kn: Option<f64>,
    pub average_speed_sailing_kn: Option<f64>,
    pub average_speed_motoring_kn: Option<f64>,
    pub fastest_1nm: Option<FastestSegment>,
    pub fastest_5nm: Option<FastestSegment>,
    pub fastest_10nm: Option<FastestSegment>,
    pub fastest_25nm: Option<FastestSegment>,
}
```
(Leave `FastestSegment` itself — `TripLeg` now uses it.)

In `src/db/mod.rs:21`, remove `TrackAnalytics` from the re-export list (comma-separated import —
delete just that identifier, keep the rest of the list intact).

- [ ] **Step 5: Remove the endpoint and its test from `api.rs`**

Delete the `get_track_analytics` handler (`src/web/api.rs:564-581`):
```rust
pub async fn get_track_analytics(
    State(state): State<AppState>,
    Query(params): Query<TimeRangeRequiredQuery>,
) -> Result<Json<ApiResponse<TrackAnalytics>>, StatusCode> {
    let start = parse_required_datetime(&params.start)?;
    let end = parse_required_datetime(&params.end)?;
    match state.db().fetch_track_analytics(start, end) {
        Ok(analytics) => Ok(Json(ApiResponse::ok(analytics))),
        Err(e) => {
            error!(error = %e, "Failed to fetch track analytics");
            {
                let bt = Backtrace::force_capture();
                error!(?bt, "Backtrace for error");
                Ok(Json(ApiResponse::error(e.to_string())))
            }
        }
    }
}
```

Delete the route registration (find with
`grep -n '"/track_analytics"' src/web/api.rs`):
```rust
        .route("/track_analytics", get(get_track_analytics))
```

Delete the now-unused query struct:
```rust
#[derive(Debug, Deserialize)]
pub struct TimeRangeRequiredQuery {
    pub start: String,
    pub end: String,
}
```
(`parse_required_datetime` itself stays — it's also used by `correct_engine_status`'s handler,
confirmed via `grep -n parse_required_datetime src/web/api.rs`.)

Delete the test (find with `grep -n "async fn test_track_analytics" src/web/api.rs`):
```rust
    #[tokio::test]
    async fn test_track_analytics() {
        // ... (entire test body)
    }
```

Remove `TrackAnalytics` from the `use crate::db::{ ... }` import list at the top of `api.rs`
(comma-separated — delete just that identifier).

- [ ] **Step 6: Update `trip_timing.rs`**

In `src/bin/trip_timing.rs`, remove:
```rust
    // 4. GET /api/track_analytics?start=<trip.start_date>&end=<trip.end_date>
    // (fired without awaiting in the browser, but it's real server work either way)
    let _analytics = db.fetch_track_analytics(start, end);
```
and the now-unused `start`/`end` variables if nothing else in the file uses them (check with
`grep -n "start\b\|end\b" src/bin/trip_timing.rs` — `fetch_metrics_batch` a few lines below also
takes `start`/`end`, so they likely stay; only remove them if that call is gone too, which it
isn't in this plan).

- [ ] **Step 7: Build and run the full suite**

Run: `cargo build --release`
Expected: clean build, no errors (unused-import warnings if any leftover — fix by removing the
specific unused import).

Run: `cargo test --bin nmea_router`
Expected: same pass count as Task 3's Step 6 minus the one deleted differential test, minus
`test_track_analytics` (deleted), no new failures.

- [ ] **Step 8: Manual smoke test**

Restart the dev server (rebuild + relaunch, as done earlier this session) and load
`http://<host>:<port>/trip.html?id=<a real trip id>` — confirm the page loads, analytics cards
populate, and `curl -s -o /dev/null -w "%{http_code}\n" http://<host>:<port>/api/track_analytics`
returns `404` (route no longer exists).

- [ ] **Step 9: Commit**

```bash
git add src/web/api.rs src/db/operations/query.rs src/db/types.rs src/db/mod.rs src/bin/trip_timing.rs
git commit -m "Delete the now-unused /api/track_analytics endpoint and O(n^2) algorithm"
```
