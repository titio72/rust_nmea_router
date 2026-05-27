# Trip Planning Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `plan.html` page that shows the 7-day forecast as coloured wind arrows on a Leaflet map, with a route-planning mode that colours a straight-line passage by forecast wind speed at estimated ETA.

**Architecture:** Two new backend endpoints (`/api/forecast/grid-points`, `/api/forecast/route`) serve the frontend. The route endpoint reuses `compute_route_overlay` (new function in `src/forecast.rs`) which uses the same IDW interpolation as the existing trip overlay. The planning page is active-trip-only and is linked from `trip.html`.

**Tech Stack:** Rust/Axum backend, vanilla JS + Leaflet 1.9.4 frontend, MariaDB, existing `shared-theme.js` / `shared.css`.

---

## Files

| File | Action |
|------|--------|
| `src/db/operations/forecast.rs` | Add `GridPointForecast` struct, `get_grid_points_at`, `fetch_forecast_fetches` |
| `src/forecast.rs` | Add `RouteOverlayPoint` struct, `generate_route_track`, `compute_route_overlay` |
| `src/web/api.rs` | Add `ForecastGridPointsQuery`, `ForecastRouteQuery`, two handlers |
| `src/web/server.rs` | Wire two new GET routes |
| `static/plan.html` | New planning page |
| `static/trip.html` | Add "Planning →" button for active trips |

---

### Task 1: DB — grid-points query and forecast fetches loader

**Files:**
- Modify: `src/db/operations/forecast.rs`

#### Context

`forecast_fetch` stores one row per grid point per poll cycle (every 3 h). For the planning page we want the *latest* fetch per unique `(lat, lon)` pair so we always show the freshest data. `forecast_hourly` stores one row per hour per `fetch_id`.

The existing `load_hourly` private method is used by the new methods below — it already handles `DECIMAL` columns via `parse_decimal_opt`.

`parse_iso_to_db` (already public in this file) converts ISO-8601 to `"YYYY-MM-DD HH:MM:SS"` for MariaDB DATETIME comparisons.

---

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block at the bottom of `src/db/operations/forecast.rs`:

```rust
#[test]
#[ignore]
fn test_get_grid_points_at_returns_latest_fetch() {
    let db = setup_db();
    let trip_id = make_trip(&db);
    let area_id = db.create_forecast_area(&NewTripForecastArea {
        trip_id, lat_min: 43.0, lat_max: 44.0, lon_min: 8.0, lon_max: 9.0,
    }).unwrap();

    let ts = "2026-05-14T09:00:00Z";
    let hourly = vec![ForecastHourlyPoint {
        timestamp: ts.to_string(),
        wind_speed_kn: Some(10.0),
        wind_direction_deg: Some(90.0),
        wind_gust_kn: Some(14.0),
        wave_height_m: Some(1.0),
        wave_period_s: Some(6.0),
        wave_direction_deg: Some(95.0),
        cape_j_kg: Some(50.0),
    }];
    // First (older) fetch — wind 10 kn
    db.insert_forecast(trip_id, area_id, 43.5, 8.5, DateTime::parse_from_rfc3339("2026-05-14T06:00:00Z").unwrap().with_timezone(&Utc), &hourly).unwrap();

    let hourly2 = vec![ForecastHourlyPoint {
        timestamp: ts.to_string(),
        wind_speed_kn: Some(20.0),
        wind_direction_deg: Some(180.0),
        wind_gust_kn: Some(25.0),
        wave_height_m: Some(1.5),
        wave_period_s: Some(7.0),
        wave_direction_deg: Some(185.0),
        cape_j_kg: Some(100.0),
    }];
    // Second (newer) fetch — wind 20 kn — this should win
    db.insert_forecast(trip_id, area_id, 43.5, 8.5, DateTime::parse_from_rfc3339("2026-05-14T09:00:00Z").unwrap().with_timezone(&Utc), &hourly2).unwrap();

    let pts = db.get_grid_points_at(trip_id, ts).unwrap();
    assert_eq!(pts.len(), 1);
    assert!((pts[0].wind_speed_kn.unwrap() - 20.0).abs() < 0.1, "Expected latest fetch (20 kn), got {:?}", pts[0].wind_speed_kn);
}

#[test]
#[ignore]
fn test_fetch_forecast_fetches_returns_all_grid_points() {
    let db = setup_db();
    let trip_id = make_trip(&db);
    let area_id = db.create_forecast_area(&NewTripForecastArea {
        trip_id, lat_min: 43.0, lat_max: 44.0, lon_min: 8.0, lon_max: 9.0,
    }).unwrap();

    let hourly = vec![ForecastHourlyPoint {
        timestamp: "2026-05-14T09:00:00Z".to_string(),
        wind_speed_kn: Some(12.0), wind_direction_deg: Some(90.0),
        wind_gust_kn: Some(15.0), wave_height_m: Some(1.0),
        wave_period_s: Some(6.0), wave_direction_deg: Some(95.0),
        cape_j_kg: Some(0.0),
    }];
    db.insert_forecast(trip_id, area_id, 43.2, 8.3, Utc::now(), &hourly).unwrap();
    db.insert_forecast(trip_id, area_id, 43.6, 8.7, Utc::now(), &hourly).unwrap();

    let fetches = db.fetch_forecast_fetches(trip_id).unwrap();
    assert_eq!(fetches.len(), 2);
    assert_eq!(fetches[0].hourly.len(), 1);
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test -- --test-threads=1 --include-ignored test_get_grid_points_at 2>&1 | tail -20
cargo test -- --test-threads=1 --include-ignored test_fetch_forecast_fetches 2>&1 | tail -20
```

Expected: compile errors (`get_grid_points_at` / `fetch_forecast_fetches` not found) or test failure.

- [ ] **Step 3: Add `GridPointForecast` struct**

At the top of `src/db/operations/forecast.rs`, after the existing `TripForecastArea` struct, add:

```rust
#[derive(Debug, Serialize, Clone)]
pub struct GridPointForecast {
    pub lat: f64,
    pub lon: f64,
    pub wind_speed_kn: Option<f64>,
    pub wind_direction_deg: Option<f64>,
    pub wind_gust_kn: Option<f64>,
    pub wave_height_m: Option<f64>,
    pub wave_period_s: Option<f64>,
    pub wave_direction_deg: Option<f64>,
    pub cape_j_kg: Option<f64>,
}
```

- [ ] **Step 4: Add `get_grid_points_at` method**

Inside `impl VesselDatabase` in `src/db/operations/forecast.rs`, after the `fetch_trip_forecast_inputs` method:

```rust
/// Returns the most recent forecast values for every grid point of `trip_id`
/// at the given UTC hour.  `timestamp_iso` is RFC-3339, e.g. "2026-05-14T09:00:00Z".
pub fn get_grid_points_at(
    &self,
    trip_id: u32,
    timestamp_iso: &str,
) -> Result<Vec<GridPointForecast>, AppError> {
    let ts_db = parse_iso_to_db(timestamp_iso)?;
    let mut conn = self.pool.get_conn()?;
    let rows: Vec<mysql::Row> = conn.exec(
        "SELECT ff.lat, ff.lon,
                fh.wind_speed_kn, fh.wind_direction_deg, fh.wind_gust_kn,
                fh.wave_height_m, fh.wave_period_s, fh.wave_direction_deg, fh.cape_j_kg
         FROM forecast_fetch ff
         JOIN forecast_hourly fh ON fh.fetch_id = ff.id
         WHERE ff.trip_id = :trip_id
           AND fh.timestamp = :ts
           AND ff.fetched_at = (
               SELECT MAX(inner_ff.fetched_at)
               FROM forecast_fetch inner_ff
               WHERE inner_ff.trip_id = ff.trip_id
                 AND inner_ff.lat = ff.lat
                 AND inner_ff.lon = ff.lon
           )",
        params! { "trip_id" => trip_id, "ts" => &ts_db },
    )?;
    rows.iter()
        .map(|r| {
            Ok(GridPointForecast {
                lat: parse_decimal(r, "lat")?,
                lon: parse_decimal(r, "lon")?,
                wind_speed_kn: parse_decimal_opt(r, "wind_speed_kn"),
                wind_direction_deg: parse_decimal_opt(r, "wind_direction_deg"),
                wind_gust_kn: parse_decimal_opt(r, "wind_gust_kn"),
                wave_height_m: parse_decimal_opt(r, "wave_height_m"),
                wave_period_s: parse_decimal_opt(r, "wave_period_s"),
                wave_direction_deg: parse_decimal_opt(r, "wave_direction_deg"),
                cape_j_kg: parse_decimal_opt(r, "cape_j_kg"),
            })
        })
        .collect()
}
```

- [ ] **Step 5: Add `fetch_forecast_fetches` method**

Inside `impl VesselDatabase`, after `get_grid_points_at`:

```rust
/// Loads all FetchWithHourly records for a trip without loading the vessel track.
/// Used by the route forecast endpoint.
pub fn fetch_forecast_fetches(
    &self,
    trip_id: u32,
) -> Result<Vec<FetchWithHourly>, AppError> {
    let mut conn = self.pool.get_conn()?;
    let fetch_rows: Vec<mysql::Row> = conn.exec(
        "SELECT id, lat, lon FROM forecast_fetch WHERE trip_id = :trip_id",
        params! { "trip_id" => trip_id },
    )?;
    let mut fetches = Vec::new();
    for frow in &fetch_rows {
        let fid: u32 = match frow.get("id") { Some(v) => v, None => continue };
        let flat = parse_decimal(frow, "lat")?;
        let flon = parse_decimal(frow, "lon")?;
        let hourly = self.load_hourly(&mut conn, fid)?;
        fetches.push(FetchWithHourly { lat: flat, lon: flon, hourly });
    }
    Ok(fetches)
}
```

- [ ] **Step 6: Run tests to confirm they pass**

```bash
cargo test -- --test-threads=1 --include-ignored test_get_grid_points_at 2>&1 | tail -20
cargo test -- --test-threads=1 --include-ignored test_fetch_forecast_fetches 2>&1 | tail -20
```

Expected: both PASS.

- [ ] **Step 7: Run full test suite**

```bash
cargo test 2>&1 | tail -5
```

Expected: all non-ignored tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/db/operations/forecast.rs
git commit -m "feat: add get_grid_points_at and fetch_forecast_fetches DB methods"
```

---

### Task 2: `generate_route_track` and `compute_route_overlay` in `src/forecast.rs`

**Files:**
- Modify: `src/forecast.rs`

#### Context

`compute_trip_overlay` (already in this file) takes a `TripForecastInputs` whose `track` field is `Vec<(f64, f64, DateTime<Utc>)>`. For route planning we generate a *synthetic* track from start→end at constant speed, then run the same IDW logic. Rather than wiring through `TripForecastInputs`, we expose `compute_route_overlay` which calls the private `nearest_hourly` and `interpolate_idw` helpers already in this file.

`haversine_distance_nm` is already imported from `crate::utilities`.

The `FetchWithHourly` type is already imported from `crate::db::operations::forecast`.

---

- [ ] **Step 1: Write failing tests**

Add to `#[cfg(test)] mod tests` in `src/forecast.rs`:

```rust
#[test]
fn test_generate_route_track_point_count() {
    use chrono::TimeZone;
    let dep = Utc.with_ymd_and_hms(2026, 5, 14, 6, 0, 0).unwrap();
    // Livorno → Capraia ≈ 35 nm at 5 kn → 7 h passage → 8 points (h=0..7)
    let track = generate_route_track(43.55, 10.29, 43.05, 9.84, dep, 5.0);
    assert!(track.len() >= 7 && track.len() <= 9, "Expected 7–9 points, got {}", track.len());
    // First point at departure position
    assert!((track[0].0 - 43.55).abs() < 0.01);
    assert!((track[0].2 - dep).num_seconds() == 0);
    // Last point near destination
    let last = track.last().unwrap();
    assert!((last.0 - 43.05).abs() < 0.1, "Expected near 43.05, got {}", last.0);
}

#[test]
fn test_generate_route_track_timestamps_advance_hourly() {
    use chrono::TimeZone;
    let dep = Utc.with_ymd_and_hms(2026, 5, 14, 6, 0, 0).unwrap();
    let track = generate_route_track(43.55, 10.29, 43.05, 9.84, dep, 5.0);
    for i in 1..track.len() {
        let diff = (track[i].2 - track[i-1].2).num_hours();
        assert_eq!(diff, 1, "Expected 1-hour steps");
    }
}

#[test]
fn test_compute_route_overlay_returns_points_with_coords() {
    use chrono::TimeZone;
    use crate::db::operations::forecast::{FetchWithHourly, ForecastHourlyPoint};

    let dep = Utc.with_ymd_and_hms(2026, 5, 14, 9, 0, 0).unwrap();
    let track = generate_route_track(43.5, 9.0, 43.5, 9.5, dep, 10.0);
    // Single grid point near the route
    let fetches = vec![FetchWithHourly {
        lat: 43.5, lon: 9.25,
        hourly: vec![pt(12.0, 180.0, 15.0, 1.0, 6.0, 185.0, 0.0)],
    }];
    let overlay = compute_route_overlay(&track, &fetches);
    // Every point should have lat/lon
    for p in &overlay {
        assert!(p.lat >= 43.4 && p.lat <= 43.6, "lat out of range: {}", p.lat);
    }
    assert!(!overlay.is_empty());
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test generate_route_track 2>&1 | tail -10
cargo test compute_route_overlay 2>&1 | tail -10
```

Expected: compile errors — `generate_route_track` and `compute_route_overlay` not defined yet.

- [ ] **Step 3: Add `RouteOverlayPoint` struct**

At the top of `src/forecast.rs`, after `TripOverlayPoint`:

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
}
```

- [ ] **Step 4: Add `generate_route_track`**

After `compute_trip_overlay` in `src/forecast.rs`:

```rust
/// Generates one synthetic track point per hour along the straight-line route
/// from (from_lat, from_lon) to (to_lat, to_lon) at constant `speed_kn`.
/// Returns `Vec<(lat, lon, utc_timestamp)>`.
pub fn generate_route_track(
    from_lat: f64,
    from_lon: f64,
    to_lat: f64,
    to_lon: f64,
    departure: DateTime<Utc>,
    speed_kn: f64,
) -> Vec<(f64, f64, DateTime<Utc>)> {
    let distance_nm = haversine_distance_nm(from_lat, from_lon, to_lat, to_lon);
    if distance_nm < 0.1 || speed_kn <= 0.0 {
        return vec![(from_lat, from_lon, departure)];
    }
    let total_hours = distance_nm / speed_kn;
    let num_steps = total_hours.ceil() as i64 + 1;
    (0..num_steps)
        .map(|h| {
            let frac = (h as f64 / total_hours).min(1.0);
            let lat = from_lat + frac * (to_lat - from_lat);
            let lon = from_lon + frac * (to_lon - from_lon);
            let ts = departure + Duration::hours(h);
            (lat, lon, ts)
        })
        .collect()
}
```

- [ ] **Step 5: Add `compute_route_overlay`**

After `generate_route_track`:

```rust
/// IDW-interpolates forecast values at each synthetic track point.
/// Uses the same `nearest_hourly` and `interpolate_idw` helpers as
/// `compute_trip_overlay`.
pub fn compute_route_overlay(
    track: &[(f64, f64, DateTime<Utc>)],
    fetches: &[crate::db::operations::forecast::FetchWithHourly],
) -> Vec<RouteOverlayPoint> {
    track
        .iter()
        .filter_map(|(lat, lon, ts)| {
            let samples: Vec<(f64, f64, ForecastHourlyPoint)> = fetches
                .iter()
                .filter_map(|f| {
                    nearest_hourly(&f.hourly, *ts).map(|pt| (f.lat, f.lon, pt))
                })
                .collect();
            let interp = interpolate_idw(*lat, *lon, &samples)?;
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
        })
        .collect()
}
```

Also add `use crate::db::operations::forecast::FetchWithHourly;` to the import at the top of the file (merge into the existing import line):

```rust
use crate::db::operations::forecast::{FetchWithHourly, ForecastHourlyPoint, TripForecastInputs};
```

- [ ] **Step 6: Run tests to confirm they pass**

```bash
cargo test generate_route_track 2>&1 | tail -10
cargo test compute_route_overlay 2>&1 | tail -10
```

Expected: all PASS.

- [ ] **Step 7: Run full test suite**

```bash
cargo test 2>&1 | tail -5
```

Expected: all non-ignored tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/forecast.rs
git commit -m "feat: add generate_route_track and compute_route_overlay"
```

---

### Task 3: API handlers and route wiring

**Files:**
- Modify: `src/web/api.rs`
- Modify: `src/web/server.rs`

#### Context

Existing pattern for GET endpoints: query structs with `#[derive(Deserialize)]`, handler function takes `State(state): State<AppState>` and `Query(params): Query<MyQuery>`, returns `Result<Json<ApiResponse<T>>, StatusCode>`.

The public GET routes (no auth) are registered in `create_api_router` in `src/web/api.rs`, around line 1573–1575. The routes requiring auth (POST/DELETE) use a nested router around line 1610–1613.

The two new endpoints are read-only and can sit in the same public block as `/forecast/trip-overlay`, `/forecast/areas` (GET), and `/forecast/status`.

---

- [ ] **Step 1: Add query structs**

In `src/web/api.rs`, after the existing `ForecastAreaTripQuery` struct (around line 242):

```rust
#[derive(Debug, Deserialize)]
pub struct ForecastGridPointsQuery {
    pub trip_id: u32,
    pub timestamp: String,
}

#[derive(Debug, Deserialize)]
pub struct ForecastRouteQuery {
    pub trip_id: u32,
    pub from_lat: f64,
    pub from_lon: f64,
    pub to_lat: f64,
    pub to_lon: f64,
    pub departure: String,
    pub speed_kn: f64,
}
```

- [ ] **Step 2: Add `get_forecast_grid_points` handler**

In `src/web/api.rs`, after `get_forecast_status`:

```rust
pub async fn get_forecast_grid_points(
    State(state): State<AppState>,
    Query(params): Query<ForecastGridPointsQuery>,
) -> Result<Json<ApiResponse<Vec<crate::db::operations::forecast::GridPointForecast>>>, StatusCode> {
    match state.db().get_grid_points_at(params.trip_id, &params.timestamp) {
        Ok(pts) => Ok(Json(ApiResponse::ok(pts))),
        Err(e) => {
            error!(error = %e, trip_id = params.trip_id, "Failed to get grid points");
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}
```

- [ ] **Step 3: Add `get_forecast_route` handler**

In `src/web/api.rs`, after `get_forecast_grid_points`:

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
    let fetches = match state.db().fetch_forecast_fetches(params.trip_id) {
        Ok(f) => f,
        Err(e) => {
            error!(error = %e, trip_id = params.trip_id, "Failed to load forecast fetches for route");
            return Ok(Json(ApiResponse::error(e.to_string())));
        }
    };
    let track = crate::forecast::generate_route_track(
        params.from_lat, params.from_lon,
        params.to_lat, params.to_lon,
        departure,
        params.speed_kn,
    );
    let overlay = crate::forecast::compute_route_overlay(&track, &fetches);
    Ok(Json(ApiResponse::ok(overlay)))
}
```

- [ ] **Step 4: Wire routes in `create_api_router`**

In `src/web/api.rs`, find the block with `.route("/forecast/trip-overlay", ...)` (around line 1573). Add two more lines immediately after:

```rust
        .route("/forecast/trip-overlay", get(get_forecast_trip_overlay))
        .route("/forecast/areas", get(get_forecast_areas))
        .route("/forecast/status", get(get_forecast_status))
        .route("/forecast/grid-points", get(get_forecast_grid_points))
        .route("/forecast/route", get(get_forecast_route))
```

- [ ] **Step 5: Compile check**

```bash
cargo build 2>&1 | grep -E "^error" | head -20
```

Expected: no errors.

- [ ] **Step 6: Smoke-test the endpoint**

With the server running:

```bash
curl -s "http://localhost:8080/api/forecast/grid-points?trip_id=154&timestamp=2026-05-14T09%3A00%3A00Z" | python3 -m json.tool | head -20
```

Expected: `{"status":"ok","data":[...]}` — may be empty array if no forecast data for trip 154 at that hour, but must not be 404 or 500.

- [ ] **Step 7: Run full test suite**

```bash
cargo test 2>&1 | tail -5
```

Expected: all non-ignored tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/web/api.rs src/web/server.rs
git commit -m "feat: add /api/forecast/grid-points and /api/forecast/route endpoints"
```

---

### Task 4: `static/plan.html` — planning page

**Files:**
- Create: `static/plan.html`

#### Context

Follow existing UI conventions exactly:
- 1500 px wide, centered, `max-width:1500px; margin:0 auto; padding:20px;`
- `<link rel="stylesheet" href="/shared.css">` + `<script src="/js/shared-theme.js?v=3">` in `<head>`
- `document.getElementById('headerContainer').innerHTML = createHeaderBar('trips')` + `initializeTheme()` at top of script
- Leaflet 1.9.4 CDN (same links as trip.html)
- Containers use `class="level-1-container"` for background/border styling
- No Chart.js needed (no charts on this page)

The page is loaded via `plan.html?id=<trip_id>`. Trip ID is read with `Number(new URLSearchParams(window.location.search).get('id'))`.

Arrow rendering: each grid point gets an `L.marker` with an `L.divIcon` containing an inline SVG polygon rotated to wind direction and coloured by speed. Markers are stored in `arrowMarkers[]`, cleared on each refresh.

Route mode: the map receives `click` events only when `routeMode` is true. Two clicks place FROM (green circle) and TO (red circle). "Compute" fetches `/api/forecast/route` and draws coloured `L.polyline` segments. All route layers are in `routeSegments[]`.

Departure time: `<input type="datetime-local">` labelled "UTC". The value (e.g. `"2026-05-14T09:00"`) is sent to the API as `"2026-05-14T09:00:00Z"` by appending `":00Z"`.

Speed default: 5.5 kn, persisted in `localStorage` key `"plan_speed_kn"`.

---

- [ ] **Step 1: Create `static/plan.html`**

```html
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Trip Planning</title>
    <link rel="stylesheet" href="/shared.css">
    <link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/leaflet/1.9.4/leaflet.min.css" />
    <script src="/js/shared-theme.js?v=3"></script>
    <script src="https://cdnjs.cloudflare.com/ajax/libs/leaflet/1.9.4/leaflet.min.js"></script>
    <style>
        #planMap { height: calc(100vh - 280px); min-height: 380px; border-radius: 6px; }
        .day-tab { padding: 5px 12px; border-radius: 4px; cursor: pointer; font-size: 13px;
                   background: var(--bg-secondary); color: var(--text-secondary);
                   border: 1px solid var(--border-color); }
        .day-tab.active { background: var(--link-color); color: #fff; border-color: var(--link-color); }
        .day-tab:hover:not(.active) { background: var(--bg-hover); }
    </style>
</head>
<body>
    <div id="headerContainer"></div>

    <div style="max-width:1500px; margin:0 auto; padding:20px;">

        <!-- Time scrubber -->
        <div class="level-1-container" style="padding:14px 20px; margin-bottom:10px;">
            <div style="display:flex; justify-content:space-between; align-items:center; margin-bottom:10px;">
                <div id="dayTabsContainer" style="display:flex; gap:5px; flex-wrap:wrap;"></div>
                <div style="display:flex; gap:8px; align-items:center;">
                    <button id="planRouteBtn" onclick="toggleRouteMode()"
                        style="padding:5px 14px; background:var(--link-color); color:#fff; border:none;
                               border-radius:4px; cursor:pointer; font-size:13px;">
                        Plan Route
                    </button>
                </div>
            </div>
            <div style="display:flex; align-items:center; gap:12px;">
                <input type="range" id="hourSlider" min="0" max="23" value="0"
                    style="flex:1; accent-color:var(--link-color);">
                <span id="selectedTime"
                    style="font-size:13px; color:var(--text-bold); min-width:185px; text-align:right;"></span>
                <button onclick="jumpToNow()"
                    style="padding:4px 10px; font-size:12px; border:1px solid var(--border-color);
                           border-radius:4px; background:transparent; color:var(--text-secondary); cursor:pointer;">
                    Now
                </button>
            </div>
        </div>

        <!-- Route bar (hidden until route mode) -->
        <div id="routeBar" class="level-1-container"
             style="display:none; padding:10px 20px; margin-bottom:10px;">
            <div style="display:flex; align-items:center; gap:14px; flex-wrap:wrap; font-size:13px;">
                <span style="color:#48bb78; font-weight:bold;">● FROM</span>
                <span id="fromLabel" style="color:var(--text-secondary);">click map to set</span>
                <span style="color:var(--text-secondary);">→</span>
                <span style="color:#fc8181; font-weight:bold;">● TO</span>
                <span id="toLabel" style="color:var(--text-secondary);">click map to set</span>
                <span style="color:var(--border-color); margin:0 4px;">|</span>
                <label style="color:var(--text-secondary);">Dep (UTC):
                    <input type="datetime-local" id="depInput"
                        style="margin-left:6px; background:var(--bg-secondary);
                               border:1px solid var(--border-color); border-radius:4px;
                               padding:3px 8px; color:var(--text-primary); font-size:13px;">
                </label>
                <label style="color:var(--text-secondary);">Speed:
                    <input type="number" id="speedInput" value="5.5" min="1" max="20" step="0.5"
                        style="width:64px; margin-left:6px; background:var(--bg-secondary);
                               border:1px solid var(--border-color); border-radius:4px;
                               padding:3px 8px; color:var(--text-primary); font-size:13px;"> kn
                </label>
                <button id="computeBtn" onclick="computeRoute()" disabled
                    style="padding:5px 14px; background:var(--link-color); color:#fff; border:none;
                           border-radius:4px; cursor:pointer; font-size:13px; opacity:0.4;">
                    Compute
                </button>
            </div>
        </div>

        <!-- Map -->
        <div class="level-1-container" style="padding:0; overflow:hidden; margin-bottom:10px;">
            <div id="planMap"></div>
        </div>

        <!-- Stats bar -->
        <div class="level-1-container" style="padding:12px 20px;">
            <div style="display:flex; gap:28px; flex-wrap:wrap; font-size:13px; color:var(--text-secondary);">
                Wind: <strong id="statWind" style="color:var(--text-primary);">—</strong>
                &nbsp;Gust: <strong id="statGust" style="color:var(--text-primary);">—</strong>
                &nbsp;Wave: <strong id="statWave" style="color:var(--text-primary);">—</strong>
                &nbsp;Period: <strong id="statPeriod" style="color:var(--text-primary);">—</strong>
                &nbsp;CAPE: <strong id="statCape" style="color:var(--text-primary);">—</strong>
            </div>
        </div>

    </div>

    <script>
        document.getElementById('headerContainer').innerHTML = createHeaderBar('trips');
        initializeTheme();

        const urlParams = new URLSearchParams(window.location.search);
        const tripId = Number(urlParams.get('id'));

        // ── State ─────────────────────────────────────────────────────────────────
        let planMap = null;
        let arrowMarkers = [];
        let availableDays = [];
        let selectedDay = 0;
        let selectedHour = 0;
        let hourDebounceTimer = null;

        let routeMode = false;
        let routeFrom = null, routeTo = null;
        let fromMarker = null, toMarker = null;
        let routeSegments = [];

        // ── Init ─────────────────────────────────────────────────────────────────
        async function init() {
            planMap = L.map('planMap').setView([43.0, 9.0], 6);
            L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
                attribution: '© OpenStreetMap', maxZoom: 16
            }).addTo(planMap);
            planMap.on('click', onMapClick);

            const saved = localStorage.getItem('plan_speed_kn');
            if (saved) document.getElementById('speedInput').value = saved;

            await loadAvailableDays();
        }

        async function loadAvailableDays() {
            try {
                const resp = await fetch('/api/forecast/status?trip_id=' + tripId);
                const json = await resp.json();
                if (!json.data || !json.data.last_fetch) {
                    document.getElementById('dayTabsContainer').innerHTML =
                        '<span style="font-size:13px; color:var(--text-secondary);">' +
                        'No forecast data. Add areas on the trip page first.</span>';
                    return;
                }
                const now = new Date();
                availableDays = Array.from({ length: 7 }, (_, d) => {
                    const day = new Date(now);
                    day.setUTCDate(now.getUTCDate() + d);
                    day.setUTCHours(0, 0, 0, 0);
                    return {
                        date: day,
                        label: day.toLocaleDateString('en-GB', {
                            weekday: 'short', day: 'numeric', month: 'short', timeZone: 'UTC'
                        })
                    };
                });
                renderDayTabs();
                selectDay(0, false);
            } catch (err) {
                console.error('Failed to load forecast status', err);
            }
        }

        // ── Time scrubber ─────────────────────────────────────────────────────────
        function renderDayTabs() {
            document.getElementById('dayTabsContainer').innerHTML = availableDays
                .map((d, i) =>
                    `<button class="day-tab${i === selectedDay ? ' active' : ''}"
                             onclick="selectDay(${i})">${d.label}</button>`)
                .join('');
        }

        function selectDay(i, doLoad = true) {
            selectedDay = i;
            selectedHour = 0;
            document.getElementById('hourSlider').value = 0;
            renderDayTabs();
            updateSelectedTime();
            if (doLoad) loadGridPoints();
        }

        function updateSelectedTime() {
            if (!availableDays[selectedDay]) return;
            const d = new Date(availableDays[selectedDay].date);
            d.setUTCHours(selectedHour);
            document.getElementById('selectedTime').textContent =
                d.toLocaleDateString('en-GB', {
                    weekday: 'short', day: 'numeric', month: 'short', timeZone: 'UTC'
                }) + ' · ' + String(selectedHour).padStart(2, '0') + ':00 UTC';
        }

        function getSelectedISO() {
            if (!availableDays[selectedDay]) return null;
            const d = new Date(availableDays[selectedDay].date);
            d.setUTCHours(selectedHour, 0, 0, 0);
            return d.toISOString();
        }

        document.getElementById('hourSlider').addEventListener('input', function () {
            selectedHour = parseInt(this.value);
            updateSelectedTime();
            clearTimeout(hourDebounceTimer);
            hourDebounceTimer = setTimeout(loadGridPoints, 150);
        });

        function jumpToNow() {
            const now = new Date();
            const todayUTC = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate()));
            const idx = availableDays.findIndex(d => d.date.getTime() === todayUTC.getTime());
            if (idx >= 0) {
                selectedHour = now.getUTCHours();
                document.getElementById('hourSlider').value = selectedHour;
                selectedDay = idx;
                renderDayTabs();
                updateSelectedTime();
                loadGridPoints();
            }
        }

        // ── Grid-point arrows ──────────────────────────────────────────────────────
        async function loadGridPoints() {
            const ts = getSelectedISO();
            if (!ts) return;
            try {
                const resp = await fetch(
                    `/api/forecast/grid-points?trip_id=${tripId}&timestamp=${encodeURIComponent(ts)}`
                );
                const json = await resp.json();
                const pts = json.data || [];
                renderArrows(pts);
                renderStats(pts);
            } catch (err) {
                console.error('Failed to load grid points', err);
            }
        }

        function windColor(speed) {
            const t = Math.min((speed || 0) / 30, 1);
            if (t < 0.5) return `rgb(${Math.round(2 * t * 255)},255,0)`;
            return `rgb(255,${Math.round(2 * (1 - t) * 255)},0)`;
        }

        function renderArrows(pts) {
            arrowMarkers.forEach(m => planMap.removeLayer(m));
            arrowMarkers = [];
            pts.forEach(pt => {
                const speed = pt.wind_speed_kn || 0;
                const dir = pt.wind_direction_deg || 0;
                const color = windColor(speed);
                const svg =
                    `<svg width="22" height="22" viewBox="0 0 22 22" xmlns="http://www.w3.org/2000/svg">` +
                    `<g transform="rotate(${dir},11,11)">` +
                    `<polygon points="11,2 15,16 11,12 7,16" fill="${color}" ` +
                    `stroke="rgba(0,0,0,0.35)" stroke-width="0.8"/>` +
                    `</g></svg>`;
                const icon = L.divIcon({ html: svg, className: '', iconSize: [22, 22], iconAnchor: [11, 11] });
                const m = L.marker([pt.lat, pt.lon], { icon }).addTo(planMap);
                m.bindPopup(
                    `<b>Wind:</b> ${speed.toFixed(1)} kn ${(dir).toFixed(0)}°<br>` +
                    `<b>Gust:</b> ${(pt.wind_gust_kn || 0).toFixed(1)} kn<br>` +
                    `<b>Wave:</b> ${(pt.wave_height_m || 0).toFixed(1)} m ` +
                    `${(pt.wave_period_s || 0).toFixed(0)} s<br>` +
                    `<b>CAPE:</b> ${(pt.cape_j_kg || 0).toFixed(0)} J/kg`
                );
                arrowMarkers.push(m);
            });
        }

        function renderStats(pts) {
            if (!pts.length) return;
            const avg = arr => arr.length ? arr.reduce((a, b) => a + b, 0) / arr.length : null;
            const winds = pts.map(p => p.wind_speed_kn).filter(v => v != null);
            const dirs  = pts.map(p => p.wind_direction_deg).filter(v => v != null);
            const gusts = pts.map(p => p.wind_gust_kn).filter(v => v != null);
            const waves = pts.map(p => p.wave_height_m).filter(v => v != null);
            const periods = pts.map(p => p.wave_period_s).filter(v => v != null);
            const capes = pts.map(p => p.cape_j_kg).filter(v => v != null);

            let dirStr = '';
            if (dirs.length) {
                const sinS = dirs.reduce((s, d) => s + Math.sin(d * Math.PI / 180), 0);
                const cosS = dirs.reduce((s, d) => s + Math.cos(d * Math.PI / 180), 0);
                const avgDir = ((Math.atan2(sinS, cosS) * 180 / Math.PI) + 360) % 360;
                const names = ['N','NNE','NE','ENE','E','ESE','SE','SSE',
                                'S','SSW','SW','WSW','W','WNW','NW','NNW'];
                dirStr = ' ' + names[Math.round(avgDir / 22.5) % 16];
            }
            const fmt = (arr, dec, unit) => {
                const v = avg(arr);
                return v != null ? `${v.toFixed(dec)} ${unit}` : '—';
            };
            document.getElementById('statWind').textContent =
                winds.length ? `${avg(winds).toFixed(1)} kn${dirStr}` : '—';
            document.getElementById('statGust').textContent   = fmt(gusts, 1, 'kn');
            document.getElementById('statWave').textContent   = fmt(waves, 1, 'm');
            document.getElementById('statPeriod').textContent = fmt(periods, 0, 's');
            document.getElementById('statCape').textContent   = fmt(capes, 0, 'J/kg');
        }

        // ── Route mode ────────────────────────────────────────────────────────────
        function toggleRouteMode() {
            if (routeMode) {
                clearRoute();
            } else {
                routeMode = true;
                document.getElementById('planRouteBtn').textContent = 'Clear Route';
                document.getElementById('planRouteBtn').style.background = '#e74c3c';
                document.getElementById('routeBar').style.display = '';
                planMap.getContainer().style.cursor = 'crosshair';
                // Pre-fill departure from current scrubber
                const iso = getSelectedISO();
                if (iso) document.getElementById('depInput').value = iso.slice(0, 16);
            }
        }

        function clearRoute() {
            routeMode = false;
            routeFrom = routeTo = null;
            if (fromMarker) { planMap.removeLayer(fromMarker); fromMarker = null; }
            if (toMarker)   { planMap.removeLayer(toMarker);   toMarker   = null; }
            routeSegments.forEach(l => planMap.removeLayer(l));
            routeSegments = [];
            document.getElementById('routeBar').style.display = 'none';
            document.getElementById('fromLabel').textContent = 'click map to set';
            document.getElementById('toLabel').textContent   = 'click map to set';
            document.getElementById('computeBtn').disabled = true;
            document.getElementById('computeBtn').style.opacity = '0.4';
            document.getElementById('planRouteBtn').textContent = 'Plan Route';
            document.getElementById('planRouteBtn').style.background = '';
            planMap.getContainer().style.cursor = '';
        }

        function onMapClick(e) {
            if (!routeMode) return;
            if (!routeFrom) {
                routeFrom = e.latlng;
                fromMarker = L.circleMarker(e.latlng, {
                    color: '#48bb78', fillColor: '#48bb78', fillOpacity: 1, radius: 7, weight: 2
                }).addTo(planMap);
                document.getElementById('fromLabel').textContent =
                    `${e.latlng.lat.toFixed(4)}°N  ${e.latlng.lng.toFixed(4)}°E`;
            } else if (!routeTo) {
                routeTo = e.latlng;
                toMarker = L.circleMarker(e.latlng, {
                    color: '#fc8181', fillColor: '#fc8181', fillOpacity: 1, radius: 7, weight: 2
                }).addTo(planMap);
                document.getElementById('toLabel').textContent =
                    `${e.latlng.lat.toFixed(4)}°N  ${e.latlng.lng.toFixed(4)}°E`;
            } else {
                // Third click: replace FROM marker, reset TO and route
                planMap.removeLayer(fromMarker);
                routeFrom = e.latlng;
                fromMarker = L.circleMarker(e.latlng, {
                    color: '#48bb78', fillColor: '#48bb78', fillOpacity: 1, radius: 7, weight: 2
                }).addTo(planMap);
                document.getElementById('fromLabel').textContent =
                    `${e.latlng.lat.toFixed(4)}°N  ${e.latlng.lng.toFixed(4)}°E`;
                if (toMarker) { planMap.removeLayer(toMarker); toMarker = null; }
                routeTo = null;
                document.getElementById('toLabel').textContent = 'click map to set';
                routeSegments.forEach(l => planMap.removeLayer(l));
                routeSegments = [];
            }
            checkComputeReady();
        }

        function checkComputeReady() {
            const dep   = document.getElementById('depInput').value;
            const speed = parseFloat(document.getElementById('speedInput').value);
            const ready = !!(routeFrom && routeTo && dep && speed > 0);
            document.getElementById('computeBtn').disabled = !ready;
            document.getElementById('computeBtn').style.opacity = ready ? '1' : '0.4';
        }

        document.getElementById('depInput').addEventListener('change', checkComputeReady);
        document.getElementById('speedInput').addEventListener('input', function () {
            localStorage.setItem('plan_speed_kn', this.value);
            checkComputeReady();
        });

        async function computeRoute() {
            const dep   = document.getElementById('depInput').value;   // "2026-05-14T09:00"
            const speed = parseFloat(document.getElementById('speedInput').value);
            if (!routeFrom || !routeTo || !dep || !(speed > 0)) return;

            const departure = dep.slice(0, 16) + ':00Z';               // treat as UTC
            const url = `/api/forecast/route?trip_id=${tripId}` +
                `&from_lat=${routeFrom.lat}&from_lon=${routeFrom.lng}` +
                `&to_lat=${routeTo.lat}&to_lon=${routeTo.lng}` +
                `&departure=${encodeURIComponent(departure)}&speed_kn=${speed}`;

            const btn = document.getElementById('computeBtn');
            btn.textContent = 'Computing…';
            btn.disabled = true;
            try {
                const resp = await fetch(url);
                const json = await resp.json();
                drawRouteLine(json.data || []);
            } catch (err) {
                console.error('Route forecast failed', err);
            } finally {
                btn.textContent = 'Compute';
                checkComputeReady();
            }
        }

        function drawRouteLine(pts) {
            routeSegments.forEach(l => planMap.removeLayer(l));
            routeSegments = [];
            if (pts.length < 2) return;

            for (let i = 0; i < pts.length - 1; i++) {
                const p = pts[i];
                const seg = L.polyline(
                    [[p.lat, p.lon], [pts[i + 1].lat, pts[i + 1].lon]],
                    { color: windColor(p.wind_speed_kn || 0), weight: 5, opacity: 0.9 }
                ).addTo(planMap);
                const ts = new Date(p.timestamp);
                seg.bindPopup(
                    `<b>${ts.toUTCString()}</b><br>` +
                    `Wind: ${(p.wind_speed_kn || 0).toFixed(1)} kn ` +
                    `${(p.wind_direction_deg || 0).toFixed(0)}°<br>` +
                    `Gust: ${(p.wind_gust_kn || 0).toFixed(1)} kn<br>` +
                    `Wave: ${(p.wave_height_m || 0).toFixed(1)} m`
                );
                routeSegments.push(seg);
            }

            // ETA tooltip at destination
            const last = pts[pts.length - 1];
            const eta  = new Date(last.timestamp);
            const etaStr =
                eta.toLocaleDateString('en-GB', { weekday:'short', day:'numeric', month:'short', timeZone:'UTC' }) +
                ' ' + eta.toLocaleTimeString('en-GB', { hour:'2-digit', minute:'2-digit', timeZone:'UTC' }) + ' UTC';
            const windStr = last.wind_speed_kn != null ? `${last.wind_speed_kn.toFixed(1)} kn` : '—';
            const tt = L.tooltip({ permanent: true, direction: 'right' })
                .setContent(`ETA ${etaStr} · ${windStr}`)
                .setLatLng([last.lat, last.lon]);
            planMap.addLayer(tt);
            routeSegments.push(tt);
        }

        init();
    </script>
</body>
</html>
```

- [ ] **Step 2: Verify the page loads without JS errors**

Open `http://localhost:8080/plan.html?id=154` in a browser. Open DevTools console. Expected: no JS errors on load. The day tabs area shows "No forecast data" or renders tabs if trip 154 has forecast data.

- [ ] **Step 3: Verify time scrubber**

If trip 154 has forecast data: click the day tabs, drag the hour slider. Expected: `selectedTime` updates, grid points load (check Network tab).

- [ ] **Step 4: Verify route mode**

Click "Plan Route". Expected: cursor changes to crosshair, route bar appears, departure pre-filled. Click two points on the map. Expected: green and red circles placed. Fill departure and speed, click "Compute". Expected: coloured line drawn on map, ETA tooltip at destination.

- [ ] **Step 5: Commit**

```bash
git add static/plan.html
git commit -m "feat: add trip planning page with forecast wind arrows and route forecast"
```

---

### Task 5: "Planning →" button on trip.html

**Files:**
- Modify: `static/trip.html`

#### Context

The "Forecast Areas" section header already has this structure (around line 316–320):
```html
<div class="level-1-container" id="forecastAreasContainer" style="display:none; margin-top:20px;">
    <div style="display:flex; justify-content:space-between; align-items:center; margin-bottom:15px;">
        <h2 ...>Forecast Areas</h2>
        <span id="forecastPollerStatus" ...></span>
    </div>
```

The `isActive` variable is set at line 891 inside `loadTripDetails`. The `Planning →` link should be added to the Forecast Areas header row, rendered only when `isActive` is true and the section is being shown.

`initForecastAreasSection(tripId, isActive)` is the right place to conditionally render this button — it already knows both `tripId` and `isActive`.

---

- [ ] **Step 1: Add Planning button to `initForecastAreasSection`**

Find the `initForecastAreasSection` function in `static/trip.html` (around line 2994). After `document.getElementById('forecastAreaControls').style.display = isActive ? '' : 'none';`, add:

```javascript
const planBtn = document.getElementById('forecastPlanningBtn');
if (planBtn) {
    planBtn.style.display = isActive ? '' : 'none';
    planBtn.href = 'plan.html?id=' + tripId;
}
```

- [ ] **Step 2: Add the button element to the HTML**

Find the Forecast Areas section header in the HTML (around line 317–320):

```html
<div style="display:flex; justify-content:space-between; align-items:center; margin-bottom:15px;">
    <h2 style="font-size:16px; font-weight:bold; color:var(--text-bold); margin:0;">Forecast Areas</h2>
    <span id="forecastPollerStatus" style="font-size:12px; color:var(--text-secondary);"></span>
</div>
```

Replace with:

```html
<div style="display:flex; justify-content:space-between; align-items:center; margin-bottom:15px;">
    <h2 style="font-size:16px; font-weight:bold; color:var(--text-bold); margin:0;">Forecast Areas</h2>
    <div style="display:flex; align-items:center; gap:14px;">
        <span id="forecastPollerStatus" style="font-size:12px; color:var(--text-secondary);"></span>
        <a id="forecastPlanningBtn" href="#" style="display:none; padding:5px 14px;
           background:var(--link-color); color:#fff; border-radius:4px;
           text-decoration:none; font-size:13px;">Planning →</a>
    </div>
</div>
```

- [ ] **Step 3: Verify in browser**

Open trip page for an active trip (trip 154). Scroll to Forecast Areas. Expected: a blue "Planning →" link appears next to the status. Clicking it opens `plan.html?id=154`.

Open trip page for a completed trip. Expected: "Planning →" link is not shown.

- [ ] **Step 4: Run full test suite**

```bash
cargo test 2>&1 | tail -5
```

Expected: all non-ignored tests pass.

- [ ] **Step 5: Commit**

```bash
git add static/trip.html
git commit -m "feat: add Planning button to trip page for active trips"
```
