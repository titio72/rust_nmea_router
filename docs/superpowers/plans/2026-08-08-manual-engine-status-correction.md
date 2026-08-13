# Manual Engine Status Correction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a user click a start point and an end point on a trip's track and set `engine_on` for every `vessel_status` row in that range, correcting misfires from the automatic RPM-based engine detection.

**Architecture:** A new DB operation (`correct_engine_status`) overwrites `vessel_status.engine_on` for a clamped timestamp range, reuses the existing `recalculate_and_update_trip` to refresh the trip's sailing/motoring aggregates, and invalidates `trip_legs_cache`/`heatmap_cache` — the same cascade `trim_trip` already follows. A thin Axum handler exposes it at `POST /api/correct_engine_status`. A new page, `static/fix-engine-status.html`, renders the trip's full-resolution track on a Leaflet map colored by current engine state, lets the user click two points, and applies the correction. `trip.html` gets a button linking to it.

**Tech Stack:** Rust (axum, mysql crate, chrono), vanilla JS + Leaflet on the frontend, MariaDB.

## Global Constraints

- Backend: Rust only. Frontend: HTML + vanilla JavaScript (CLAUDE.md).
- `snake_case` functions/modules, `PascalCase` structs (CLAUDE.md).
- Never call `now()` inside business logic; only in event handlers (CLAUDE.md). None of this feature's logic needs the current time, so this is naturally satisfied.
- Durations in milliseconds; all timestamps UTC (CLAUDE.md).
- SQL: parameterized queries (`params!` macro) always; transactions for multi-statement ops (CLAUDE.md).
- No unused imports, no partial implementations committed to main (CLAUDE.md).
- Pages are 1500px wide, centered; structure is `<div class="header-bar">` then one or more `<div class="level-1-container">`; all pages load `shared-theme.js` and `shared.css`; theme toggle `id="themeBtn"`, brand logo `id="brandLogo"` (AGENTS.md / CLAUDE.md).
- Database tests are `#[ignore]`d and require a live MariaDB via `test_config.json`, run with `--test-threads=1` (CLAUDE.md).
- Per CLAUDE.md project rules for this repo: do not run `git commit` or `git push`, do not stage files — stop after writing code for the user to review.

---

### Task 1: DB operation `correct_engine_status`

**Files:**
- Modify: `src/db/operations/trip.rs` (add method to `impl VesselDatabase` block, add imports, add test to the existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `VesselDatabase::fetch_trip(&self, trip_id: u32) -> Result<Option<TripSummary>, AppError>` (`src/db/operations/query.rs:188`); `TripSummary::start_timestamp(&self) -> Result<SystemTime, Box<dyn Error>>` / `TripSummary::end_timestamp(&self) -> Result<SystemTime, Box<dyn Error>>` (`src/db/types.rs:71,79`); `VesselDatabase::recalculate_and_update_trip(&self, trip_id: i64, trip_start: SystemTime, trip_end: SystemTime) -> Result<(), AppError>` (`src/db/operations/gap_fill.rs:306`); `VesselDatabase::invalidate_trip_legs_cache(&self, trip_id: u32) -> Result<(), AppError>` (`src/db/operations/query.rs:1323`); `VesselDatabase::invalidate_heatmap_cache(&self, start_date: NaiveDate, end_date: NaiveDate) -> Result<(), AppError>` (`src/db/operations/query.rs:1934`); `EngineStatus` enum (`src/utilities.rs:188`, variants `Off=0`, `On=1`, `Unknown=2`, methods `as_u8()`, `is_unknown()`).
- Produces: `VesselDatabase::correct_engine_status(&self, trip_id: u32, start: DateTime<Utc>, end: DateTime<Utc>, engine_on: EngineStatus) -> Result<(), AppError>` — used by Task 2's API handler.

- [ ] **Step 1: Add imports**

At the top of `src/db/operations/trip.rs`, change:

```rust
use crate::db::types::VesselDatabase;
use crate::error::AppError;
use mysql::params;
use mysql::prelude::Queryable;
use tracing::warn;
```

to:

```rust
use crate::db::types::VesselDatabase;
use crate::error::AppError;
use crate::utilities::EngineStatus;
use chrono::{DateTime, Utc};
use mysql::params;
use mysql::prelude::Queryable;
use tracing::warn;
```

- [ ] **Step 2: Implement `correct_engine_status`**

Add this method inside the existing `impl VesselDatabase { ... }` block in `src/db/operations/trip.rs`, right before the block's closing `}` (i.e. after `set_nav_override`, which currently ends at line 194):

```rust
    /// Overwrite `engine_on` for every `vessel_status` row between `start` and `end`
    /// (clamped to the trip's own window), then recompute the trip's sailing/motoring
    /// aggregates and invalidate the caches derived from vessel_status. Used to correct
    /// misfires from the automatic RPM-based engine detection in vessel_monitor.rs.
    pub fn correct_engine_status(
        &self,
        trip_id: u32,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        engine_on: EngineStatus,
    ) -> Result<(), AppError> {
        if engine_on.is_unknown() {
            return Err(AppError::Parse(
                "engine_on must be On or Off, not Unknown".to_string(),
            ));
        }
        if start >= end {
            return Err(AppError::Parse(
                "start_timestamp must be before end_timestamp".to_string(),
            ));
        }

        let trip = self
            .fetch_trip(trip_id)?
            .ok_or_else(|| AppError::Database(format!("Trip {} not found", trip_id)))?;
        let trip_start = trip
            .start_timestamp()
            .map_err(|e| AppError::Parse(e.to_string()))?;
        let trip_end = trip
            .end_timestamp()
            .map_err(|e| AppError::Parse(e.to_string()))?;
        let trip_start_dt = DateTime::<Utc>::from(trip_start);
        let trip_end_dt = DateTime::<Utc>::from(trip_end);

        let clamped_start = start.max(trip_start_dt);
        let clamped_end = end.min(trip_end_dt);
        if clamped_start >= clamped_end {
            return Err(AppError::Parse(
                "Requested range does not overlap the trip".to_string(),
            ));
        }

        let start_str = clamped_start.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        let end_str = clamped_end.format("%Y-%m-%d %H:%M:%S%.3f").to_string();

        let mut conn = self.pool.get_conn()?;
        let mut tx = conn.start_transaction(mysql::TxOpts::default())?;
        tx.exec_drop(
            "UPDATE vessel_status SET engine_on = :val WHERE timestamp BETWEEN :start AND :end",
            params! {
                "val" => engine_on.as_u8(),
                "start" => &start_str,
                "end" => &end_str,
            },
        )?;
        let affected = tx.affected_rows();
        tx.commit()?;

        if affected == 0 {
            return Err(AppError::Database(
                "No track points found in the requested range".to_string(),
            ));
        }

        self.recalculate_and_update_trip(trip_id as i64, trip_start, trip_end)?;
        self.invalidate_trip_legs_cache(trip_id)?;
        self.invalidate_heatmap_cache(clamped_start.date_naive(), clamped_end.date_naive())?;

        Ok(())
    }
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: builds with no errors or warnings about this new code.

- [ ] **Step 4: Add the DB-layer test**

In `src/db/operations/trip.rs`, inside `mod tests { ... }`, add `EngineStatus` and `fetch_vessel_status_by_id` to the existing `use crate::db::test_helpers::{...}` import so it reads:

```rust
    use crate::db::test_helpers::{
        add_test_trip, add_test_vessel_status, fetch_vessel_status_by_id, setup_db,
    };
    use crate::utilities::EngineStatus;
    use std::{
        ops::{Add, Sub},
        time::{Duration, SystemTime},
    };
```

(`fetch_vessel_status_by_id` is already imported; only `EngineStatus` is new.)

Then add this test function inside `mod tests`:

```rust
    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_correct_engine_status() {
        let db = setup_db();
        let t = SystemTime::now();

        let trip_id: u32 = add_test_trip(
            &db,
            "Engine Fix Test".to_string(),
            t,
            t.add(Duration::from_secs(1800)),
            0.0,
            0.0,
            0,
            0,
            0,
        )
        .expect("Failed to insert test trip");

        // First half of the trip: sailing (engine off). Second half: also inserted as
        // engine off, but this is the half we'll correct to "on".
        let first_id = add_test_vessel_status(
            &db,
            t,
            43.0,
            11.0,
            5.0,
            6.0,
            None,
            None,
            false,
            EngineStatus::Off,
            1.0,
            900_000,
            None,
            None,
        )
        .expect("Failed to insert first vessel status");

        let mid_ts = t.add(Duration::from_secs(900));
        let second_id = add_test_vessel_status(
            &db,
            mid_ts,
            43.1,
            11.0,
            5.0,
            6.0,
            None,
            None,
            false,
            EngineStatus::Off,
            1.0,
            900_000,
            None,
            None,
        )
        .expect("Failed to insert second vessel status");

        let mid_dt = chrono::DateTime::<chrono::Utc>::from(mid_ts);
        let end_dt = chrono::DateTime::<chrono::Utc>::from(t.add(Duration::from_secs(1800)));

        db.correct_engine_status(trip_id, mid_dt, end_dt, EngineStatus::On)
            .expect("correct_engine_status should succeed");

        // The first point (before the corrected range) must be untouched.
        let first = fetch_vessel_status_by_id(&db, first_id)
            .expect("fetch failed")
            .expect("first record missing");
        assert_eq!(
            first.engine_on,
            EngineStatus::Off,
            "point before the corrected range must stay Off"
        );

        // The second point (inside the corrected range) must now be On.
        let second = fetch_vessel_status_by_id(&db, second_id)
            .expect("fetch failed")
            .expect("second record missing");
        assert_eq!(
            second.engine_on,
            EngineStatus::On,
            "point inside the corrected range must become On"
        );

        // Trip aggregates must reflect the new split: 1.0 nm sailed, 1.0 nm motored.
        let trip = db
            .fetch_trip(trip_id)
            .expect("fetch_trip failed")
            .expect("trip missing");
        assert!(
            (trip.sailing_distance_nm - 1.0).abs() < 0.01,
            "sailing distance should be 1.0 nm, got {}",
            trip.sailing_distance_nm
        );
        assert!(
            (trip.motoring_distance_nm - 1.0).abs() < 0.01,
            "motoring distance should be 1.0 nm, got {}",
            trip.motoring_distance_nm
        );
    }

    #[test]
    #[ignore] // Requires a live MariaDB test database (see CLAUDE.md / DB_ANALYST.md).
    fn test_correct_engine_status_rejects_unknown() {
        let db = setup_db();
        let t = SystemTime::now();
        let trip_id: u32 = add_test_trip(
            &db,
            "Reject Unknown".to_string(),
            t,
            t.add(Duration::from_secs(600)),
            0.0,
            0.0,
            0,
            0,
            0,
        )
        .expect("Failed to insert test trip");

        let start_dt = chrono::DateTime::<chrono::Utc>::from(t);
        let end_dt = chrono::DateTime::<chrono::Utc>::from(t.add(Duration::from_secs(600)));

        let result = db.correct_engine_status(trip_id, start_dt, end_dt, EngineStatus::Unknown);
        assert!(
            result.is_err(),
            "correct_engine_status must reject EngineStatus::Unknown"
        );
    }
```

- [ ] **Step 5: Run the tests (requires `test_config.json` and a live MariaDB)**

Run: `cargo test --package nmea_router correct_engine_status -- --ignored --test-threads=1`
Expected: both `test_correct_engine_status` and `test_correct_engine_status_rejects_unknown` PASS. If no test database is configured in this environment, skip running and note that in the task handoff — the code must still compile (Step 3 already verified that).

- [ ] **Step 6: Commit**

```bash
git add src/db/operations/trip.rs
git commit -m "Add correct_engine_status DB operation for manual engine detection fixes"
```

(Per this repo's CLAUDE.md, only run this commit step if the user has explicitly asked you to commit — otherwise stop after Step 5 and leave the change for review.)

---

### Task 2: API endpoint `POST /api/correct_engine_status`

**Files:**
- Modify: `src/web/api.rs` (add request struct, handler, route registration, test)

**Interfaces:**
- Consumes: `VesselDatabase::correct_engine_status(&self, trip_id: u32, start: DateTime<Utc>, end: DateTime<Utc>, engine_on: EngineStatus) -> Result<(), AppError>` (Task 1); `parse_required_datetime(s: &str) -> Result<DateTime<Utc>, StatusCode>` (`src/web/api.rs:330`, already defined); `ApiResponse::ok`/`ApiResponse::error` (already defined, same file).
- Produces: `pub async fn correct_engine_status(...) -> Result<Json<ApiResponse<()>>, StatusCode>` registered at `POST /correct_engine_status`, mounted under `/api` — used by Task 3's frontend page as `POST /api/correct_engine_status`.

- [ ] **Step 1: Add the request struct**

In `src/web/api.rs`, near the other query/body structs (right after `TripDescriptionQuery`, currently at line ~120-123), add:

```rust
#[derive(Debug, Deserialize)]
pub struct CorrectEngineStatusQuery {
    pub trip_id: u32,
    pub start_timestamp: String,
    pub end_timestamp: String,
    pub engine_on: bool,
}
```

- [ ] **Step 2: Add the handler**

In `src/web/api.rs`, right after `pub async fn trim_trip(...)` (ends at line 646), add:

```rust
pub async fn correct_engine_status(
    State(state): State<AppState>,
    Json(params): Json<CorrectEngineStatusQuery>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    info!(?params, "POST /api/correct_engine_status called");

    let start = parse_required_datetime(&params.start_timestamp)?;
    let end = parse_required_datetime(&params.end_timestamp)?;
    let engine_on = if params.engine_on {
        crate::utilities::EngineStatus::On
    } else {
        crate::utilities::EngineStatus::Off
    };

    match state
        .db()
        .correct_engine_status(params.trip_id, start, end, engine_on)
    {
        Ok(()) => {
            info!(
                trip_id = params.trip_id,
                "Engine status corrected successfully"
            );
            Ok(Json(ApiResponse::ok(())))
        }
        Err(e) => {
            error!(error = %e, trip_id = params.trip_id, "Failed to correct engine status");
            {
                let bt = Backtrace::force_capture();
                error!(?bt, "Backtrace for error");
                Ok(Json(ApiResponse::error(e.to_string())))
            }
        }
    }
}
```

- [ ] **Step 3: Register the route**

In `src/web/api.rs`, inside the `if !read_only { router = router ... }` block (starting at line 1999), add the new route right after `.route("/trim_trip", post(trim_trip))` (line 2003):

```rust
            .route("/trim_trip", post(trim_trip))
            .route("/correct_engine_status", post(correct_engine_status))
            .route("/invalidate_trip_legs", post(invalidate_trip_legs))
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: builds with no errors.

- [ ] **Step 5: Add the seeded API test**

In `src/web/api.rs`, inside the `#[cfg(test)] mod tests { ... }` block, add this test near `test_update_trip_description_seeded`:

```rust
    #[tokio::test]
    #[ignore]
    async fn test_correct_engine_status_seeded() {
        use crate::db::test_helpers::{add_test_trip, add_test_vessel_status};
        use crate::utilities::EngineStatus;
        use std::ops::Add;
        use std::time::{Duration, SystemTime};

        let (app, db) = create_clean_test_app();
        let now = SystemTime::now();
        let (trip_id, mid_ts, trip_end) = {
            let db = db.read().unwrap();
            let trip_end = now.add(Duration::from_secs(1800));
            let trip_id = add_test_trip(
                &db,
                "API Engine Fix Test".to_string(),
                now,
                trip_end,
                0.0,
                0.0,
                0,
                0,
                0,
            )
            .unwrap();
            add_test_vessel_status(
                &db, now, 43.0, 11.0, 5.0, 6.0, None, None, false, EngineStatus::Off, 1.0,
                900_000, None, None,
            )
            .unwrap();
            let mid = now.add(Duration::from_secs(900));
            add_test_vessel_status(
                &db, mid, 43.1, 11.0, 5.0, 6.0, None, None, false, EngineStatus::Off, 1.0,
                900_000, None, None,
            )
            .unwrap();
            (trip_id, mid, trip_end)
        };

        let mid_dt = chrono::DateTime::<chrono::Utc>::from(mid_ts);
        let end_dt = chrono::DateTime::<chrono::Utc>::from(trip_end);
        let body = json!({
            "trip_id": trip_id,
            "start_timestamp": mid_dt.to_rfc3339(),
            "end_timestamp": end_dt.to_rfc3339(),
            "engine_on": true
        })
        .to_string();

        let (status, json) = call_api(
            app.clone(),
            axum::http::Request::builder()
                .method("POST")
                .uri("/correct_engine_status")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["status"], "ok");

        let (_, trip_json) = call_api(
            app,
            axum::http::Request::builder()
                .uri(format!("/trip?id={}", trip_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert!(
            (trip_json["data"]["motoring_distance_nm"].as_f64().unwrap() - 1.0).abs() < 0.01
        );
        assert!(
            (trip_json["data"]["sailing_distance_nm"].as_f64().unwrap() - 1.0).abs() < 0.01
        );
    }
```

- [ ] **Step 6: Run the test (requires `test_config.json` and a live MariaDB)**

Run: `cargo test --package nmea_router test_correct_engine_status_seeded -- --ignored --test-threads=1`
Expected: PASS. If no test database is configured in this environment, skip running and note that in the task handoff — Step 4 already verified compilation.

- [ ] **Step 7: Commit**

```bash
git add src/web/api.rs
git commit -m "Add POST /api/correct_engine_status endpoint"
```

(Only if the user has explicitly asked you to commit.)

---

### Task 3: Frontend page `static/fix-engine-status.html`

**Files:**
- Create: `static/fix-engine-status.html`

**Interfaces:**
- Consumes: `GET /api/trip?id=<id>` (existing, returns `ApiResponse<TripSummary>` with `description`, `start_date`, `end_date` fields); `GET /api/track?trip_id=<id>` (existing, no `max_points` → full-resolution `ApiResponse<Vec<TrackPoint>>`, each point has `timestamp` (ISO-8601 string, lexicographically sortable), `latitude`, `longitude`, `engine_on` (0/1/2)); `POST /api/correct_engine_status` (Task 2, body `{trip_id, start_timestamp, end_timestamp, engine_on}`); global helpers from `/js/shared-theme.js`: `createHeaderBar(currentPage)`, `initializeTheme()`, `formatDate(dateStr)`, `formatDateTime(dateStr)`; `addLatLonPanel(map, position)` from `/js/map-lat-lon-panel.js`.
- Produces: a page reachable at `/fix-engine-status.html?id=<tripId>`, linked from Task 4.

- [ ] **Step 1: Create the page**

Create `static/fix-engine-status.html` with this content:

```html
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Fix Engine Status - NMEA Router</title>
    <link rel="icon" type="image/png" href="/images/nmeasail.png">
    <link rel="stylesheet" href="/shared.css">
    <link rel="stylesheet" href="/libs/leaflet.min.css" />
    <script src="/my-icons.js"></script>
    <script src="/js/shared-theme.js?v=3"></script>
    <script src="/libs/leaflet.min.js"></script>
    <script src="/js/map-lat-lon-panel.js"></script>

    <style>
        .fix-engine-header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            gap: 12px;
        }
        .fix-engine-help {
            color: var(--text-secondary);
            font-size: 13px;
            margin: 8px 0;
        }
        #map {
            height: 520px;
            border-radius: 8px;
        }
        .selection-panel {
            display: none;
            margin-top: 16px;
            padding: 14px 16px;
            border-radius: 8px;
            border: 1px solid var(--border-color);
            background: var(--bg-secondary);
        }
        .selection-panel.visible {
            display: block;
        }
        .selection-range {
            font-size: 14px;
            color: var(--text-primary);
            margin-bottom: 10px;
        }
        .engine-choice {
            display: flex;
            gap: 8px;
            margin-bottom: 12px;
        }
        .engine-choice-btn {
            flex: 1;
            padding: 8px 12px;
            border-radius: 6px;
            border: 1px solid var(--border-color);
            background: var(--bg-tertiary);
            color: var(--text-primary);
            cursor: pointer;
        }
        .engine-choice-btn.selected {
            background: var(--link-color);
            color: #fff;
            border-color: var(--link-color);
        }
        .selection-actions {
            display: flex;
            gap: 8px;
        }
        .status-bar {
            margin-top: 16px;
            padding: 10px 14px;
            border-radius: 6px;
            font-size: 14px;
            display: none;
        }
        .status-bar.success {
            background: #d4edda;
            color: #155724;
            border: 1px solid #c3e6cb;
        }
        body.dark-theme .status-bar.success {
            background: #1a3d2b;
            color: #7bcea0;
            border-color: #2d6a4f;
        }
        .status-bar.error {
            background: #f8d7da;
            color: #721c24;
            border: 1px solid #f5c6cb;
        }
        body.dark-theme .status-bar.error {
            background: #3d1a1e;
            color: #f1a8ad;
            border-color: #6a2d32;
        }
    </style>
</head>
<body>
    <div id="headerContainer"></div>

    <div class="level-1-container">
        <div class="app-card">
            <div class="fix-engine-header">
                <div>
                    <h2 id="tripTitle" style="margin: 0;">Fix Engine Status</h2>
                    <div id="tripSubtitle" style="color: var(--text-secondary); font-size: 13px;"></div>
                </div>
                <a class="app-btn" id="backLink" href="/trip.html">&larr; Back to Trip</a>
            </div>
            <div class="fix-engine-help">
                Click a start point and an end point on the track, choose Engine ON or OFF, then Apply.
                Grey segments are currently marked engine-on; gold segments are unknown.
            </div>
            <div id="map"></div>
            <div class="selection-panel" id="selectionPanel">
                <div class="selection-range" id="selectionRange"></div>
                <div class="engine-choice">
                    <button class="engine-choice-btn" id="choiceOn" onclick="chooseEngineState(true)">Engine ON</button>
                    <button class="engine-choice-btn" id="choiceOff" onclick="chooseEngineState(false)">Engine OFF</button>
                </div>
                <div class="selection-actions">
                    <button class="app-btn" id="applyBtn" onclick="applyCorrection()" disabled>Apply</button>
                    <button class="app-btn" onclick="clearSelection()">Cancel</button>
                </div>
            </div>
            <div id="statusBar" class="status-bar"></div>
        </div>
    </div>

    <script>
        document.getElementById('headerContainer').innerHTML = createHeaderBar('trips');
        initializeTheme();

        const urlParams = new URLSearchParams(window.location.search);
        const tripId = urlParams.get('id');
        document.getElementById('backLink').href = tripId ? ('/trip.html?id=' + tripId) : '/trip.html';

        let map = null;
        let trackData = [];
        let trackLayers = [];
        let highlightLayer = null;
        let markerA = null;
        let markerB = null;
        let selectedA = null;
        let selectedB = null;
        let pendingEngineOn = null; // true/false once chosen, null before

        function showStatus(message, type) {
            const bar = document.getElementById('statusBar');
            bar.textContent = message;
            bar.className = 'status-bar ' + type;
            bar.style.display = 'block';
            if (type === 'success') {
                setTimeout(() => { bar.style.display = 'none'; }, 4000);
            }
        }

        function segmentColor(point) {
            if (point.engine_on === 1) return '#888888';
            if (point.engine_on === 2) return '#FFD700';
            return '#2ecc71';
        }

        function renderTrack() {
            trackLayers.forEach(l => l.remove());
            trackLayers = [];
            if (trackData.length < 2) return;

            const flush = (color, latlngs) => {
                if (latlngs.length < 2) return;
                const line = L.polyline(latlngs, { color, weight: 4, opacity: 1.0 }).addTo(map);
                trackLayers.push(line);
            };

            let groupColor = segmentColor(trackData[1]);
            let groupLatLngs = [
                [trackData[0].latitude, trackData[0].longitude],
                [trackData[1].latitude, trackData[1].longitude]
            ];
            for (let i = 1; i < trackData.length - 1; i++) {
                const c = segmentColor(trackData[i + 1]);
                if (c !== groupColor) {
                    flush(groupColor, groupLatLngs);
                    groupColor = c;
                    groupLatLngs = [[trackData[i].latitude, trackData[i].longitude]];
                }
                groupLatLngs.push([trackData[i + 1].latitude, trackData[i + 1].longitude]);
            }
            flush(groupColor, groupLatLngs);
        }

        function findNearestPoint(latlng) {
            let nearest = null, minD = Infinity;
            for (const p of trackData) {
                if (p.latitude == null || p.longitude == null) continue;
                const d = (p.latitude - latlng.lat) ** 2 + (p.longitude - latlng.lng) ** 2;
                if (d < minD) { minD = d; nearest = p; }
            }
            return nearest;
        }

        function pointMarker(point, color) {
            return L.circleMarker([point.latitude, point.longitude], {
                radius: 8, color: '#fff', weight: 2, fillColor: color, fillOpacity: 1
            }).addTo(map);
        }

        function clearSelection() {
            if (markerA) { markerA.remove(); markerA = null; }
            if (markerB) { markerB.remove(); markerB = null; }
            if (highlightLayer) { highlightLayer.remove(); highlightLayer = null; }
            selectedA = null;
            selectedB = null;
            pendingEngineOn = null;
            document.getElementById('selectionPanel').classList.remove('visible');
            document.getElementById('choiceOn').classList.remove('selected');
            document.getElementById('choiceOff').classList.remove('selected');
            document.getElementById('applyBtn').disabled = true;
        }

        function showSelectionPanel() {
            document.getElementById('selectionPanel').classList.add('visible');
            const count = trackData.filter(
                p => p.timestamp >= selectedA.timestamp && p.timestamp <= selectedB.timestamp
            ).length;
            document.getElementById('selectionRange').textContent =
                formatDateTime(selectedA.timestamp) + '  →  ' + formatDateTime(selectedB.timestamp) +
                '  (' + count + ' points)';
        }

        function onMapClick(e) {
            const point = findNearestPoint(e.latlng);
            if (!point) return;

            if (selectedA && selectedB) {
                clearSelection();
            }

            if (!selectedA) {
                selectedA = point;
                markerA = pointMarker(point, '#3388ff');
                return;
            }

            if (point.timestamp === selectedA.timestamp) return;

            if (point.timestamp < selectedA.timestamp) {
                selectedB = selectedA;
                markerB = markerA;
                selectedA = point;
                markerA = pointMarker(point, '#3388ff');
            } else {
                selectedB = point;
                markerB = pointMarker(point, '#e74c3c');
            }

            const rangeLatLngs = trackData
                .filter(p => p.timestamp >= selectedA.timestamp && p.timestamp <= selectedB.timestamp)
                .map(p => [p.latitude, p.longitude]);
            highlightLayer = L.polyline(rangeLatLngs, { color: '#3388ff', weight: 6, opacity: 0.5 }).addTo(map);

            showSelectionPanel();
        }

        function chooseEngineState(isOn) {
            pendingEngineOn = isOn;
            document.getElementById('choiceOn').classList.toggle('selected', isOn);
            document.getElementById('choiceOff').classList.toggle('selected', !isOn);
            document.getElementById('applyBtn').disabled = false;
        }

        async function applyCorrection() {
            if (!selectedA || !selectedB || pendingEngineOn === null) return;
            const applyBtn = document.getElementById('applyBtn');
            applyBtn.disabled = true;
            try {
                const resp = await fetch('/api/correct_engine_status', {
                    method: 'POST',
                    credentials: 'same-origin',
                    headers: { 'content-type': 'application/json' },
                    body: JSON.stringify({
                        trip_id: parseInt(tripId, 10),
                        start_timestamp: selectedA.timestamp,
                        end_timestamp: selectedB.timestamp,
                        engine_on: pendingEngineOn
                    })
                });
                const result = await resp.json();
                if (result.status !== 'ok') throw new Error(result.error || 'Failed');
                clearSelection();
                await loadTrack();
                showStatus('Engine status updated.', 'success');
            } catch (err) {
                showStatus('Failed to update engine status: ' + err.message, 'error');
                applyBtn.disabled = false;
            }
        }

        async function loadTrack() {
            const resp = await fetch('/api/track?trip_id=' + tripId);
            const result = await resp.json();
            if (result.status !== 'ok' || !result.data) {
                throw new Error(result.error || 'Failed to load track');
            }
            trackData = result.data.filter(p => p.latitude != null && p.longitude != null);
            renderTrack();
        }

        async function init() {
            if (!tripId) {
                showStatus('No trip ID specified.', 'error');
                return;
            }
            try {
                const tripResp = await fetch('/api/trip?id=' + tripId);
                const tripResult = await tripResp.json();
                if (tripResult.status === 'ok' && tripResult.data) {
                    document.getElementById('tripTitle').textContent =
                        'Fix Engine Status: ' + tripResult.data.description;
                    document.getElementById('tripSubtitle').textContent =
                        formatDate(tripResult.data.start_date) + ' – ' + formatDate(tripResult.data.end_date);
                }

                await loadTrack();
                if (trackData.length === 0) {
                    showStatus('No track points found for this trip.', 'error');
                    return;
                }

                let minLat = Infinity, maxLat = -Infinity, minLng = Infinity, maxLng = -Infinity;
                trackData.forEach(p => {
                    minLat = Math.min(minLat, p.latitude);
                    maxLat = Math.max(maxLat, p.latitude);
                    minLng = Math.min(minLng, p.longitude);
                    maxLng = Math.max(maxLng, p.longitude);
                });
                const latPadding = (maxLat - minLat) * 0.1 || 0.01;
                const lngPadding = (maxLng - minLng) * 0.1 || 0.01;

                map = L.map('map').setView([(minLat + maxLat) / 2, (minLng + maxLng) / 2], 12);
                L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
                    attribution: '&copy; OpenStreetMap contributors',
                    maxZoom: 19
                }).addTo(map);
                map.fitBounds([
                    [minLat - latPadding, minLng - lngPadding],
                    [maxLat + latPadding, maxLng + lngPadding]
                ]);
                map.on('click', onMapClick);
                addLatLonPanel(map, 'bottomleft');

                renderTrack();
            } catch (err) {
                showStatus('Failed to load trip track: ' + err.message, 'error');
            }
        }

        init();
    </script>
</body>
</html>
```

- [ ] **Step 2: Verify the app still builds and serves static files**

Run: `cargo build`
Expected: builds with no errors (this is a static asset; no Rust code references it directly, but this confirms the workspace still compiles after the file addition).

- [ ] **Step 3: Manual browser verification**

Run the app (`./target/release/nmea_router` or `cargo run`) against a database with at least one trip that has a stretch of misdetected engine status. Navigate to `/fix-engine-status.html?id=<a real trip id>` and verify:
- The track renders colored by current engine state (grey = on, gold = unknown, green = off).
- Clicking two points places markers, highlights the range between them, and shows the selection panel with correct start/end times and point count.
- Clicking "Engine ON" or "Engine OFF" then "Apply" updates the track colors after reload and shows a success message.
- Clicking the map again before Apply starts a fresh selection.
- Clicking "Cancel" clears the current selection.

This matches CLAUDE.md's requirement to test UI changes in a browser before reporting the task complete.

- [ ] **Step 4: Commit**

```bash
git add static/fix-engine-status.html
git commit -m "Add manual engine status correction page"
```

(Only if the user has explicitly asked you to commit.)

---

### Task 4: Link from `trip.html` and update README

**Files:**
- Modify: `static/trip.html`
- Modify: `README.md`

**Interfaces:**
- Consumes: `static/fix-engine-status.html?id=<tripId>` (Task 3).
- Produces: nothing consumed by later tasks — this is the last task.

- [ ] **Step 1: Add the button**

In `static/trip.html`, find the `navButtons` div (around line 219-224):

```html
                            <div class="navButtons">
                                <button class="app-btn" onclick="previousLeg()" title="Previous">&lt;&lt;</button>
                                <div id="legDisplay" class="legDisplay"></div>
                                <button class="app-btn" onclick="nextLeg()" title="Next">&gt;&gt;</button>
                                <button class="app-btn" data-ro-hidden onclick="recalculateLegs(event)" title="Recalculate Legs">&#x21BA;</button>
                            </div>
```

Change it to add a new button after the recalculate button:

```html
                            <div class="navButtons">
                                <button class="app-btn" onclick="previousLeg()" title="Previous">&lt;&lt;</button>
                                <div id="legDisplay" class="legDisplay"></div>
                                <button class="app-btn" onclick="nextLeg()" title="Next">&gt;&gt;</button>
                                <button class="app-btn" data-ro-hidden onclick="recalculateLegs(event)" title="Recalculate Legs">&#x21BA;</button>
                                <button class="app-btn" data-ro-hidden onclick="goToFixEngineStatus()" title="Fix Engine Status">&#x1F527;</button>
                            </div>
```

- [ ] **Step 2: Add the navigation function**

In `static/trip.html`, right after the `recalculateLegs` function (ends at line 606, just before `function updateLegSelector`), add:

```javascript
        function goToFixEngineStatus() {
            window.location.href = '/fix-engine-status.html?id=' + tripId;
        }
```

- [ ] **Step 3: Verify it compiles / loads**

Run: `cargo build`
Expected: builds with no errors (trip.html is a static asset; this confirms nothing else broke).

Then open `trip.html?id=<a real trip id>` in a browser, confirm the new 🔧 button appears next to the leg recalculate button, and clicking it navigates to `/fix-engine-status.html?id=<the same trip id>`.

- [ ] **Step 4: Update README**

In `README.md`, in the `## Features` list, right after the "Trips Viewer Sync" bullet, add:

```markdown
- **Manual Engine Status Correction**: Fix mistaken automatic engine on/off detection by clicking a start and end point on a trip's track and choosing the correct state; recomputes the trip's sailing/motoring totals and invalidates dependent caches (`fix-engine-status.html`, linked from the trip detail page)
```

- [ ] **Step 5: Commit**

```bash
git add static/trip.html README.md
git commit -m "Link trip page to the new engine status correction tool"
```

(Only if the user has explicitly asked you to commit.)
