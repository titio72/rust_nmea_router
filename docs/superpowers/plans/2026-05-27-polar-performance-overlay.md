# Polar Performance Overlay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Overlay a polar performance ratio (actual speed / polar speed × 100%) as a dashed green line on a second Y-axis of the existing speed chart in `trip.html`, computed server-side per track point.

**Architecture:** Add `polar_speed_kn` and `polar_ratio` optional fields to `TrackPoint`. The `get_track` handler post-processes the fetched points using the polar table from `AppState` — converting TWA from 0–360° to 0–180° and calling `polars.boat_speed()`. The frontend detects non-null `polar_ratio` values and conditionally adds a second Y-axis and two datasets (100% reference + ratio line) to the existing speed chart.

**Tech Stack:** Rust / Axum (backend), Chart.js v3 (frontend), vanilla JS (no new dependencies).

---

## File Map

| File | Change |
|------|--------|
| `src/db/types.rs` | Add `polar_speed_kn` and `polar_ratio` fields to `TrackPoint` |
| `src/web/api.rs` | Post-process track points with polar computation in `get_track`; add test helper and tests |
| `static/trip.html` | Extend `createSpeedChart` with second axis, polar datasets, and tooltip |

---

### Task 1: Extend `TrackPoint` with polar fields

**Files:**
- Modify: `src/db/types.rs:88-102`

- [ ] **Step 1: Add the two fields to `TrackPoint`**

In `src/db/types.rs`, extend the struct (after `average_heading_deg`):

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct TrackPoint {
    pub timestamp: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub avg_speed_kn: Option<f64>,
    pub max_speed_kn: Option<f64>,
    pub moored: bool,
    pub engine_on: u8,
    pub total_distance_nm: Option<f64>,
    pub total_time_ms: u64,
    pub average_wind_speed_kn: Option<f64>,
    pub average_wind_angle_deg: Option<f64>,
    pub cog_deg: Option<f64>,
    pub average_heading_deg: Option<f64>,
    pub polar_speed_kn: Option<f64>,
    pub polar_ratio: Option<f64>,
}
```

- [ ] **Step 2: Find all places that construct a `TrackPoint` literal and add the new fields**

Run:
```bash
grep -rn "TrackPoint {" src/
```

Each site must add `polar_speed_kn: None, polar_ratio: None`. The main construction site is in `src/db/operations/query.rs` inside `fetch_track`. Update that `TrackPoint { ... }` literal to include:

```rust
polar_speed_kn: None,
polar_ratio: None,
```

- [ ] **Step 3: Build to confirm no compile errors**

```bash
cargo build 2>&1 | head -40
```

Expected: compiles cleanly (zero errors).

- [ ] **Step 4: Run existing tests to confirm no regressions**

```bash
cargo test -- --test-threads=1
```

Expected: all non-ignored tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/db/types.rs src/db/operations/query.rs
git commit -m "feat: add polar_speed_kn and polar_ratio fields to TrackPoint"
```

---

### Task 2: Compute polar fields in the `get_track` handler

**Files:**
- Modify: `src/web/api.rs:377-399` (handler), `src/web/api.rs:2854-2870` (test helper area)

- [ ] **Step 1: Write the failing test — polar fields populated when polar table is configured**

Add this test near the existing `test_get_track_seeded` test (after line ~3122 in `src/web/api.rs`):

```rust
#[tokio::test]
#[ignore]
async fn test_get_track_polar_fields_populated() {
    use crate::db::test_helpers::{add_test_trip, add_test_vessel_status};
    use crate::utilities::EngineStatus;
    use std::ops::Add;
    use std::time::{Duration, SystemTime};

    // Build app state with the real polar fixture
    let polars = crate::polars::PolarTable::from_csv("tests/fixtures/dufour40.csv")
        .ok()
        .map(std::sync::Arc::new);
    let (app, db) = create_test_app_with_polars(polars);

    let now = SystemTime::now();
    let trip_id = {
        let db = db.read().unwrap();
        let tid = add_test_trip(
            &db,
            "Polar Test".to_string(),
            now,
            now.add(Duration::from_secs(3600)),
            5.0,
            0.0,
            3600000,
            0,
            0,
        )
        .unwrap();
        // TWA=90°, TWS=10 kn, actual=6.0 kn → polar at TWA=90,TWS=10 is 7.44 kn → ratio ≈ 80.6%
        add_test_vessel_status(
            &db,
            now.add(Duration::from_secs(60)),
            51.5,
            -0.1,
            6.0,   // average_speed_kn
            7.0,   // max_speed_kn
            Some(10.0),  // average_wind_speed_kn (TWS)
            Some(90.0),  // average_wind_angle_deg (TWA 0-360)
            false,
            EngineStatus::Off,
            1.0,
            600000,
            None,
            None,
        )
        .unwrap();
        tid
    };

    let (status, json) = call_api(
        app,
        axum::http::Request::builder()
            .uri(format!("/track?trip_id={}", trip_id))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "ok");
    let pt = &json["data"][0];
    let polar_spd = pt["polar_speed_kn"].as_f64().expect("polar_speed_kn should be set");
    let ratio = pt["polar_ratio"].as_f64().expect("polar_ratio should be set");
    assert!((polar_spd - 7.44).abs() < 0.1, "expected polar ~7.44, got {polar_spd}");
    assert!((ratio - 80.6).abs() < 1.0, "expected ratio ~80.6%, got {ratio}");
}

#[tokio::test]
#[ignore]
async fn test_get_track_polar_fields_null_when_no_polars() {
    use crate::db::test_helpers::{add_test_trip, add_test_vessel_status};
    use crate::utilities::EngineStatus;
    use std::ops::Add;
    use std::time::{Duration, SystemTime};

    let (app, db) = create_clean_test_app(); // polars: None
    let now = SystemTime::now();
    let trip_id = {
        let db = db.read().unwrap();
        let tid = add_test_trip(&db, "No Polar".to_string(), now,
            now.add(Duration::from_secs(3600)), 5.0, 0.0, 3600000, 0, 0).unwrap();
        add_test_vessel_status(&db, now.add(Duration::from_secs(60)),
            51.5, -0.1, 6.0, 7.0, Some(10.0), Some(90.0),
            false, EngineStatus::Off, 1.0, 600000, None, None).unwrap();
        tid
    };
    let (status, json) = call_api(app,
        axum::http::Request::builder()
            .uri(format!("/track?trip_id={}", trip_id))
            .body(Body::empty()).unwrap(),
    ).await;
    assert_eq!(status, StatusCode::OK);
    assert!(json["data"][0]["polar_speed_kn"].is_null(), "should be null with no polar table");
    assert!(json["data"][0]["polar_ratio"].is_null(), "should be null with no polar table");
}

#[tokio::test]
#[ignore]
async fn test_get_track_polar_fields_null_when_no_wind_data() {
    use crate::db::test_helpers::{add_test_trip, add_test_vessel_status};
    use crate::utilities::EngineStatus;
    use std::ops::Add;
    use std::time::{Duration, SystemTime};

    let polars = crate::polars::PolarTable::from_csv("tests/fixtures/dufour40.csv")
        .ok().map(std::sync::Arc::new);
    let (app, db) = create_test_app_with_polars(polars);
    let now = SystemTime::now();
    let trip_id = {
        let db = db.read().unwrap();
        let tid = add_test_trip(&db, "No Wind".to_string(), now,
            now.add(Duration::from_secs(3600)), 5.0, 0.0, 3600000, 0, 0).unwrap();
        // No wind data (None for TWA and TWS)
        add_test_vessel_status(&db, now.add(Duration::from_secs(60)),
            51.5, -0.1, 6.0, 7.0, None, None,
            false, EngineStatus::Off, 1.0, 600000, None, None).unwrap();
        tid
    };
    let (status, json) = call_api(app,
        axum::http::Request::builder()
            .uri(format!("/track?trip_id={}", trip_id))
            .body(Body::empty()).unwrap(),
    ).await;
    assert_eq!(status, StatusCode::OK);
    assert!(json["data"][0]["polar_speed_kn"].is_null());
    assert!(json["data"][0]["polar_ratio"].is_null());
}
```

- [ ] **Step 2: Add the `create_test_app_with_polars` helper**

Add this function near `create_clean_test_app` (around line 2870 in `src/web/api.rs`):

```rust
fn create_test_app_with_polars(
    polars: Option<std::sync::Arc<crate::polars::PolarTable>>,
) -> (Router, Arc<RwLock<crate::db::types::VesselDatabase>>) {
    use crate::db::test_helpers::setup_db;
    let db = Arc::new(RwLock::new(setup_db()));
    let config = Arc::new(crate::config::Config::load_for_context(None).unwrap());
    let signalk_broadcast = Arc::new(SignalKBroadcastChannels::new());
    let state = AppState {
        db: db.clone(),
        config,
        signalk_broadcast,
        backup_in_progress: Arc::new(AtomicBool::new(false)),
        jwt_secret: Arc::new(JwtSecret::generate()),
        ais_cache: crate::ais_target_cache::new_ais_cache(),
        poller_status: Arc::new(std::sync::Mutex::new(
            crate::forecast_poller::ForecastPollerStatus::default(),
        )),
        polars,
    };
    (create_api_router(state), db)
}
```

- [ ] **Step 3: Run the new tests to verify they FAIL (before implementation)**

```bash
cargo test -- --test-threads=1 --include-ignored test_get_track_polar 2>&1 | tail -20
```

Expected: tests fail because `polar_speed_kn` and `polar_ratio` are always `null`.

- [ ] **Step 4: Implement polar computation in the `get_track` handler**

Replace the `Ok(track) =>` branch in `get_track` (`src/web/api.rs` around line 388):

```rust
Ok(mut track) => {
    if let Some(polars) = state.polars() {
        for point in &mut track {
            if let (Some(tws), Some(twa_360), Some(actual)) = (
                point.average_wind_speed_kn,
                point.average_wind_angle_deg,
                point.avg_speed_kn,
            ) {
                let twa = twa_360.min(360.0 - twa_360);
                if let Some(polar_spd) = polars.boat_speed(twa, tws) {
                    point.polar_speed_kn = Some(polar_spd);
                    point.polar_ratio = Some(actual / polar_spd * 100.0);
                }
            }
        }
    }
    Ok(Json(ApiResponse::ok(track)))
}
```

- [ ] **Step 5: Build to confirm no compile errors**

```bash
cargo build 2>&1 | head -40
```

Expected: zero errors.

- [ ] **Step 6: Run all three new tests to confirm they pass**

```bash
cargo test -- --test-threads=1 --include-ignored test_get_track_polar 2>&1 | tail -20
```

Expected: all 3 tests PASS.

- [ ] **Step 7: Run the full test suite to confirm no regressions**

```bash
cargo test -- --test-threads=1 2>&1 | tail -20
```

Expected: all non-ignored tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/web/api.rs
git commit -m "feat: compute polar_speed_kn and polar_ratio in get_track handler"
```

---

### Task 3: Extend `createSpeedChart` with polar overlay

**Files:**
- Modify: `static/trip.html:1316-1372` (the `createSpeedChart` function)

No automated test is possible for frontend chart logic. Verify manually by loading a trip that has wind data with a polar table configured.

- [ ] **Step 1: Replace `createSpeedChart` with the extended version**

Replace lines 1316–1372 in `static/trip.html` with:

```javascript
function createSpeedChart(trackData) {
    const ctx = document.getElementById('speedChart').getContext('2d');
    const colors = getChartColors();

    const validData = trackData.filter(point => !point.moored && point.avg_speed_kn > 0.1);

    const chartData = validData.map(point => ({
        x: new Date(point.timestamp),
        y: point.avg_speed_kn
    }));

    const polarPoints = validData.filter(p => p.polar_ratio != null);
    const hasPolar = polarPoints.length > 0;

    const datasets = [{
        label: 'Average Speed (knots)',
        data: chartData,
        borderColor: '#3498db',
        backgroundColor: 'rgba(52, 152, 219, 0.1)',
        fill: true,
        tension: 0.1,
        pointRadius: 0,
        borderWidth: 1,
        pointHoverRadius: 5,
        yAxisID: 'y'
    }];

    if (hasPolar) {
        datasets.push({
            label: '_polar_ref',
            data: validData.map(p => ({ x: new Date(p.timestamp), y: 100 })),
            borderColor: 'rgba(76, 175, 80, 0.25)',
            borderDash: [4, 4],
            borderWidth: 1,
            pointRadius: 0,
            fill: false,
            tension: 0,
            yAxisID: 'y1'
        });
        datasets.push({
            label: 'Polar %',
            data: validData.map(p => ({ x: new Date(p.timestamp), y: p.polar_ratio })),
            borderColor: '#4caf50',
            borderDash: [5, 3],
            borderWidth: 1.5,
            pointRadius: 0,
            fill: false,
            tension: 0.1,
            yAxisID: 'y1'
        });
    }

    const scales = {
        y: {
            beginAtZero: true,
            title: { display: true, text: 'Speed (knots)', color: colors.text },
            ticks: { color: colors.text },
            grid: { color: colors.grid }
        },
        x: getTimeXScale(colors)
    };

    if (hasPolar) {
        scales.y1 = {
            type: 'linear',
            position: 'right',
            min: 0,
            max: 150,
            title: { display: true, text: 'Polar %', color: colors.text },
            ticks: { color: colors.text, callback: v => v + '%' },
            grid: { display: false }
        };
    }

    const plugins = {
        legend: {
            display: hasPolar,
            labels: {
                color: colors.text,
                filter: item => item.text !== '_polar_ref'
            }
        },
        title: {
            display: true,
            text: 'Boat SOG (Knots)',
            color: colors.text
        }
    };

    if (hasPolar) {
        plugins.tooltip = {
            callbacks: {
                afterBody: function(items) {
                    if (!items.length) return [];
                    const p = validData[items[0].dataIndex];
                    if (p && p.polar_ratio != null) {
                        return ['Polar: ' + p.polar_ratio.toFixed(1) + '%'];
                    }
                    return [];
                }
            }
        };
    }

    return new Chart(ctx, {
        type: 'line',
        data: { datasets },
        options: {
            maintainAspectRatio: false,
            scales,
            plugins
        }
    });
}
```

- [ ] **Step 2: Manual smoke-test — trip without polar table configured**

Load any trip in the browser. The speed chart should render exactly as before (single blue line, no second axis, no legend).

- [ ] **Step 3: Manual smoke-test — trip with polar table configured**

If a polar CSV is configured in `config.json` (`polar_csv_path`), load a sailing trip with wind data. The speed chart should show:
- Blue filled line (boat speed, left Y-axis in knots)
- Dashed green line (polar %, right Y-axis 0–150%)
- Semi-transparent dashed line at 100%
- Legend showing "Average Speed (knots)" and "Polar %" (reference line hidden)
- Tooltip shows "Polar: 82.3%" when hovering

- [ ] **Step 4: Commit**

```bash
git add static/trip.html
git commit -m "feat: add polar performance ratio overlay to speed chart"
```

---

## Done

After Task 3, the feature is complete. The speed chart silently gains the polar overlay for any trip where the server has a polar table configured and wind data is present; it degrades cleanly to the existing chart otherwise.
