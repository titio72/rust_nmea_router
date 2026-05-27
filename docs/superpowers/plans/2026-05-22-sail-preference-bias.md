# Sail-Preference Bias in Isochrone Routing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `sail_weight_kn` parameter that biases the isochrone pruning algorithm toward sailing routes, so users can trade journey time for a better sail/motor ratio.

**Architecture:** Each `IsochronePoint` tracks cumulative `sailed_hours`. Sector pruning scores candidates as `dist_from_origin + sail_weight_kn × sailed_hours` instead of raw distance, so sailing paths gain a progressive advantage. `sail_weight_kn = 0.0` is the default and preserves existing behaviour exactly.

**Tech Stack:** Rust (routing.rs, api.rs), vanilla JS (plan.html)

---

## File Map

- **Modify:** `src/routing.rs` — add `sailed_hours` to struct, update candidate generation, update `prune_isochrone`, update `run_isochrone` signature
- **Modify:** `src/web/api.rs` — add `sail_weight_kn` field to `OptimalRouteQuery`, pass through to `run_isochrone`
- **Modify:** `static/plan.html` — add "Sail preference" slider, localStorage persistence, include in `optimizeRoute()` URL

---

## Task 1: Add `sailed_hours` to `IsochronePoint` and update candidate generation

**Files:**
- Modify: `src/routing.rs:15-21` (struct), `src/routing.rs:44` (seed), `src/routing.rs:57-77` (candidate loop)

- [ ] **Step 1: Add `sailed_hours: f64` to `IsochronePoint`**

Replace the struct at lines 15–21 of `src/routing.rs`:

```rust
#[derive(Clone)]
struct IsochronePoint {
    lat: f64,
    lon: f64,
    time: DateTime<Utc>,
    sailed_hours: f64,
    parent_idx: Option<usize>,
}
```

- [ ] **Step 2: Update the seed point (line 44)**

```rust
let seed = IsochronePoint { lat: from.0, lon: from.1, time: departure, sailed_hours: 0.0, parent_idx: None };
```

- [ ] **Step 3: Update the candidate loop to track sailed/motored and set `sailed_hours`**

Replace the speed block and `candidates.push` at lines 57–77:

```rust
                let (speed_kn, was_sailing) = match wind {
                    Some((wind_spd, wind_dir)) if wind_spd >= 5.0 => {
                        let twa = compute_twa(heading, wind_dir);
                        match polars.boat_speed(twa, wind_spd).filter(|&s| s > 0.0) {
                            Some(raw) => {
                                let eff = raw * polar_efficiency;
                                if eff >= min_sail_speed_kn { (eff, true) } else { (motoring_speed_kn, false) }
                            }
                            None => (motoring_speed_kn, false),
                        }
                    }
                    _ => (motoring_speed_kn, false),
                };

                let new_pos = advance_position(parent.lat, parent.lon, heading, speed_kn);
                candidates.push(IsochronePoint {
                    lat: new_pos.0,
                    lon: new_pos.1,
                    time: parent.time + chrono::Duration::hours(1),
                    sailed_hours: parent.sailed_hours + if was_sailing { 1.0 } else { 0.0 },
                    parent_idx: Some(parent_idx),
                });
```

- [ ] **Step 4: Update `test_prune_retains_at_most_72_points` — its candidate construction uses `IsochronePoint` directly**

In the test at line 176, the map closure constructs `IsochronePoint` without `sailed_hours`. Add it:

```rust
IsochronePoint { lat, lon, time: chrono::Utc::now(), sailed_hours: 0.0, parent_idx: Some(0) }
```

- [ ] **Step 5: Build to confirm it compiles**

```bash
cargo build 2>&1
```

Expected: compile error on `prune_isochrone` call (line 81) because we haven't updated the signature yet. The struct and candidate push should be clean.

---

## Task 2: Update `prune_isochrone` and `run_isochrone` to accept `sail_weight_kn`

**Files:**
- Modify: `src/routing.rs:30-39` (`run_isochrone` signature), `src/routing.rs:81` (call to `prune_isochrone`), `src/routing.rs:114-132` (`prune_isochrone`)
- Modify: `src/routing.rs:189,204` (existing test call sites)

- [ ] **Step 1: Add `sail_weight_kn` parameter to `run_isochrone`**

Replace lines 30–39:

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
) -> IsochroneResult {
```

- [ ] **Step 2: Pass `sail_weight_kn` to `prune_isochrone` at the call site (line 81)**

```rust
        let pruned = prune_isochrone(candidates, from, sail_weight_kn);
```

- [ ] **Step 3: Update `prune_isochrone` to use composite score**

Replace lines 114–132:

```rust
fn prune_isochrone(
    candidates: Vec<IsochronePoint>,
    origin: (f64, f64),
    sail_weight_kn: f64,
) -> Vec<IsochronePoint> {
    let mut sectors: Vec<Option<IsochronePoint>> = vec![None; SECTOR_COUNT];
    let mut sector_score: Vec<f64> = vec![0.0; SECTOR_COUNT];

    for pt in candidates {
        let bearing = haversine_heading(origin.0, origin.1, pt.lat, pt.lon);
        let sector = ((bearing / (360.0 / SECTOR_COUNT as f64)) as usize) % SECTOR_COUNT;
        let dist = haversine_distance_nm(origin.0, origin.1, pt.lat, pt.lon);
        let score = dist + sail_weight_kn * pt.sailed_hours;
        if score > sector_score[sector] {
            sector_score[sector] = score;
            sectors[sector] = Some(pt);
        }
    }

    sectors.into_iter().flatten().collect()
}
```

- [ ] **Step 4: Update existing test call sites to add `sail_weight_kn: 0.0`**

Line 189 (inside `test_isochrone_reaches_nearby_destination`):

```rust
        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, &polars, &[]);
```

Line 204 (inside `test_backtrack_produces_monotonic_timestamps`):

```rust
        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, &polars, &[]);
```

- [ ] **Step 5: Build and run all routing tests**

```bash
cargo test routing 2>&1
```

Expected output:
```
test routing::tests::test_prune_retains_at_most_72_points ... ok
test routing::tests::test_isochrone_reaches_nearby_destination ... ok
test routing::tests::test_backtrack_produces_monotonic_timestamps ... ok

test result: ok. 3 passed; 0 failed; 0 ignored
```

---

## Task 3: Write and run new sail-preference test

**Files:**
- Modify: `src/routing.rs` — add test to the `tests` module

- [ ] **Step 1: Add `test_sail_weight_prefers_sailing_candidate` to the tests module**

Append inside `mod tests { … }`, after the last existing test:

```rust
    #[test]
    fn test_sail_weight_prefers_sailing_candidate() {
        let origin = (43.0, 8.0);

        // Two candidates in the same sector (both heading north, 0°).
        // Motoring candidate: 10 nm from origin, no sail history.
        // Sailing candidate:   9 nm from origin, 1 hour of sail history.
        let motoring_pos = advance_position(origin.0, origin.1, 0.0, 10.0);
        let sailing_pos  = advance_position(origin.0, origin.1, 0.0, 9.0);
        let t = chrono::Utc::now();

        let motoring = IsochronePoint { lat: motoring_pos.0, lon: motoring_pos.1, time: t, sailed_hours: 0.0, parent_idx: None };
        let sailing  = IsochronePoint { lat: sailing_pos.0,  lon: sailing_pos.1,  time: t, sailed_hours: 1.0, parent_idx: None };

        // sail_weight_kn = 0 → raw distance wins → motoring (10 nm) beats sailing (9 nm)
        let result_zero = prune_isochrone(vec![motoring.clone(), sailing.clone()], origin, 0.0);
        assert_eq!(result_zero.len(), 1);
        let d = haversine_distance_nm(result_zero[0].lat, result_zero[0].lon, origin.0, origin.1);
        assert!((d - 10.0).abs() < 0.5, "expected motoring winner at ~10 nm, got {:.2}", d);

        // sail_weight_kn = 2.0 → sailing score = 9 + 2×1 = 11 > motoring score = 10 + 2×0 = 10
        let result_biased = prune_isochrone(vec![motoring, sailing], origin, 2.0);
        assert_eq!(result_biased.len(), 1);
        let d = haversine_distance_nm(result_biased[0].lat, result_biased[0].lon, origin.0, origin.1);
        assert!((d - 9.0).abs() < 0.5, "expected sailing winner at ~9 nm, got {:.2}", d);
    }
```

- [ ] **Step 2: Run the test to confirm it fails (function exists but logic not proven yet)**

```bash
cargo test routing::tests::test_sail_weight_prefers_sailing_candidate -- --nocapture 2>&1
```

Expected: PASS (the implementation is already in place from Task 2).

- [ ] **Step 3: Run all routing tests**

```bash
cargo test routing 2>&1
```

Expected: 4 passed, 0 failed.

---

## Task 4: Update the API to accept and forward `sail_weight_kn`

**Files:**
- Modify: `src/web/api.rs:274-282` (`OptimalRouteQuery` struct), `src/web/api.rs:1725-1732` (`run_isochrone` call)

- [ ] **Step 1: Add `sail_weight_kn` to `OptimalRouteQuery`**

The struct already has `polar_efficiency` and `min_sail_speed_kn` (added earlier). Append one more field after `min_sail_speed_kn`:

```rust
    #[serde(default)]
    pub sail_weight_kn: f64,
```

The full struct becomes:

```rust
#[derive(Debug, Deserialize)]
pub struct OptimalRouteQuery {
    pub trip_id: u32,
    pub from_lat: f64,
    pub from_lon: f64,
    pub to_lat: f64,
    pub to_lon: f64,
    pub departure: String,
    pub motoring_speed_kn: f64,
    #[serde(default = "default_polar_efficiency")]
    pub polar_efficiency: f64,
    #[serde(default)]
    pub min_sail_speed_kn: f64,
    #[serde(default)]
    pub sail_weight_kn: f64,
}
```

- [ ] **Step 2: Pass `sail_weight_kn` to `run_isochrone`**

The current call at ~line 1725 passes 6 positional arguments. Add `params.sail_weight_kn` after `params.min_sail_speed_kn`:

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
    );
```

- [ ] **Step 3: Build to confirm no compile errors**

```bash
cargo build 2>&1
```

Expected: `Finished` with no errors.

- [ ] **Step 4: Run all non-DB tests to confirm nothing regressed**

```bash
cargo test 2>&1
```

Expected: all tests pass (the 2 ignored DB tests may show as ignored — that is expected).

---

## Task 5: Add "Sail preference" slider to the UI

**Files:**
- Modify: `static/plan.html` — three locations: input HTML (~line 87), localStorage restore (~line 252), event listener (~line 820), `optimizeRoute()` URL (~line 883)

- [ ] **Step 1: Add the slider HTML after `minSailInput` (after line 87)**

Insert after the closing `</label>` of `minSailInput`:

```html
                <label style="color:var(--text-secondary);" title="Bonus score per sailed hour during route optimisation. 0 = fastest route, higher values favour more sailing.">Sail preference:
                    <input type="number" id="sailWeightInput" value="0" min="0" max="10" step="0.5"
                        style="width:52px; margin-left:6px; background:var(--bg-secondary);
                               border:1px solid var(--border-color); border-radius:4px;
                               padding:3px 8px; color:var(--text-primary); font-size:13px;"> kn
                </label>
```

- [ ] **Step 2: Restore value from localStorage on page load (after line 252)**

After `if (savedMinSail) document.getElementById('minSailInput').value = savedMinSail;`, add:

```js
            const savedSailWeight = localStorage.getItem('plan_sail_weight');
            if (savedSailWeight) document.getElementById('sailWeightInput').value = savedSailWeight;
```

- [ ] **Step 3: Add event listener to persist value (after line 821)**

After the `minSailInput` listener block, add:

```js
        document.getElementById('sailWeightInput').addEventListener('input', function () {
            localStorage.setItem('plan_sail_weight', this.value);
        });
```

- [ ] **Step 4: Include `sail_weight_kn` in the `optimizeRoute()` URL**

Inside `optimizeRoute()`, after reading `minSail`, add:

```js
            const sailWeight = parseFloat(document.getElementById('sailWeightInput').value) || 0;
```

Then append to the URL string (after `&min_sail_speed_kn=${minSail}`):

```js
                    + `&sail_weight_kn=${sailWeight.toFixed(1)}`;
```

The full URL block becomes:

```js
                const url = `/api/forecast/optimal-route?trip_id=${tripId}`
                    + `&from_lat=${from.lat.toFixed(6)}&from_lon=${from.lng.toFixed(6)}`
                    + `&to_lat=${to.lat.toFixed(6)}&to_lon=${to.lng.toFixed(6)}`
                    + `&departure=${encodeURIComponent(departure)}`
                    + `&motoring_speed_kn=${speed}`
                    + `&polar_efficiency=${efficiency.toFixed(3)}`
                    + `&min_sail_speed_kn=${minSail}`
                    + `&sail_weight_kn=${sailWeight.toFixed(1)}`;
```

- [ ] **Step 5: Final build and test**

```bash
cargo build 2>&1
```

Expected: `Finished` with no errors.

```bash
cargo test routing 2>&1
```

Expected: 4 passed, 0 failed.
