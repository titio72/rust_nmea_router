# AROME + ECMWF Forecast Blending Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Use Météo-France AROME (~1.5 km) for the first 48 h of wind/gust/CAPE and keep ECMWF (~9 km) for the longer horizon and all wave data, selecting the model per query at read time.

**Architecture:** AROME and ECMWF are fetched as separate `model`-tagged fetches and stored side-by-side (different native grids; waves are ECMWF-only). Read-time interpolation prefers AROME wind samples when one exists near the requested time/place, otherwise falls back to ECMWF; waves always come from ECMWF. AROME is fetched with `forecast_days=2`, so beyond ~48 h no AROME rows exist and the fallback to ECMWF is automatic.

**Tech Stack:** Rust (Axum, mysql crate, reqwest, chrono, serde), MariaDB, vanilla JS + Leaflet frontend.

> ⚠️ **PROJECT GIT RULE:** Do **NOT** run `git commit`, `git add`, or `git push`. The skill's standard "Commit" step is replaced in every task by a **build/test verification** step. After each task, stop and let the user review. The user commits manually.

---

## Reference: design spec

`docs/superpowers/specs/2026-06-13-arome-ecmwf-blend-design.md`

## File structure

- `schema.sql` — add `model` column to `forecast_fetch`.
- `src/db/operations/forecast.rs` — `model` on insert/read types and queries.
- `src/forecast.rs` — AROME URL builder, three-call fetch, model-aware interpolation, `model` on `RouteOverlayPoint`.
- `src/routing.rs` — no signature change (consumes `nearest_forecast_wind`); verify it still compiles.
- `src/forecast_poller.rs`, `src/web/api.rs` — pass `model` into `insert_forecast`.
- `static/plan.html` — small model indicator.

## Test commands

- Unit tests: `cargo test forecast` (and `cargo test` for the full non-DB suite).
- DB integration tests (marked `#[ignore]`): `cargo test -- --test-threads=1 --include-ignored` (requires live MariaDB per `test_config.json`).

---

### Task 1: Add `model` column to `forecast_fetch`

**Files:**
- Modify: `schema.sql:244-256` (the `CREATE TABLE forecast_fetch` block)

- [ ] **Step 1: Add the column to the schema definition**

In `schema.sql`, change the `forecast_fetch` table so the `model` column sits right after `area_id`:

```sql
CREATE TABLE IF NOT EXISTS forecast_fetch (
    id              INT AUTO_INCREMENT PRIMARY KEY,
    area_id         INT NOT NULL,
    model           VARCHAR(16) NOT NULL DEFAULT 'ecmwf',
    lat             DECIMAL(9,6) NOT NULL,
    lon             DECIMAL(9,6) NOT NULL,
    fetched_at      DATETIME NOT NULL,
    forecast_from   DATETIME NOT NULL,
    forecast_to     DATETIME NOT NULL,
    INDEX idx_area_id (area_id),
    INDEX idx_fetched_at (fetched_at),
    CONSTRAINT fk_forecast_fetch_area FOREIGN KEY (area_id) REFERENCES forecast_area(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='One record per grid point per fetch operation, tagged with area and model';
```

- [ ] **Step 2: Apply the migration to the live/test DB**

Run against any already-initialised database (the `CREATE TABLE IF NOT EXISTS` will not alter an existing table):

```sql
ALTER TABLE forecast_fetch
  ADD COLUMN model VARCHAR(16) NOT NULL DEFAULT 'ecmwf' AFTER area_id;
```

Expected: column added; existing rows default to `'ecmwf'`.

- [ ] **Step 3: Verify**

Run: `mysql ... -e "SHOW COLUMNS FROM forecast_fetch LIKE 'model';"`
Expected: one row showing `model varchar(16) NO  ecmwf`.

---

### Task 2: Thread `model` through the DB layer

**Files:**
- Modify: `src/db/operations/forecast.rs`

This task changes `insert_forecast`'s signature, so its existing callers will stop compiling until Task 5. That is expected — Task 2 ends at "the DB module's own tests pass when run in isolation"; full-crate compilation is restored in Task 5. To keep the crate buildable between tasks, also do the trivial caller updates noted in Step 6 here.

- [ ] **Step 1: Add `model` to the read types**

In `src/db/operations/forecast.rs`, add a `model` field to `GridPointForecast` (after `cape_j_kg`, line ~33) and to `FetchWithHourly` (after `lon`, line ~59):

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
    pub model: String,
}
```

```rust
#[derive(Debug, Clone)]
pub struct FetchWithHourly {
    pub lat: f64,
    pub lon: f64,
    pub model: String,
    pub hourly: Vec<ForecastHourlyPoint>,
}
```

- [ ] **Step 2: Add `model` parameter to `insert_forecast`**

Change the signature (line ~112) and the `INSERT INTO forecast_fetch` statement (lines ~133-142):

```rust
    pub fn insert_forecast(
        &self,
        area_id: u32,
        model: &str,
        lat: f64,
        lon: f64,
        fetched_at: DateTime<Utc>,
        hourly: &[ForecastHourlyPoint],
    ) -> Result<(), AppError> {
```

```rust
        tx.exec_drop(
            "INSERT INTO forecast_fetch (area_id, model, lat, lon, fetched_at, forecast_from, forecast_to)
             VALUES (:area_id, :model, :lat, :lon, :fetched_at, :from, :to)",
            params! {
                "area_id" => area_id,
                "model" => model,
                "lat" => lat, "lon" => lon,
                "fetched_at" => &fetched_at_str,
                "from" => &from_str, "to" => &to_str,
            },
        )?;
```

- [ ] **Step 3: Select `model` in `fetch_forecast_fetches`**

Update the query and row mapping (lines ~243-265):

```rust
    pub fn fetch_forecast_fetches(&self) -> Result<Vec<FetchWithHourly>, AppError> {
        let mut conn = self.pool.get_conn()?;
        let fetch_rows: Vec<mysql::Row> = conn.exec(
            "SELECT ff.id, ff.model, ff.lat, ff.lon FROM forecast_fetch ff
             WHERE ff.fetched_at = (
                 SELECT MAX(inner_ff.fetched_at)
                 FROM forecast_fetch inner_ff
                 WHERE inner_ff.lat = ff.lat
                   AND inner_ff.lon = ff.lon
                   AND inner_ff.model = ff.model
             )
             ORDER BY ff.id",
            (),
        )?;
        let mut fetches = Vec::new();
        for frow in &fetch_rows {
            let fid: u32 = match frow.get("id") { Some(v) => v, None => continue };
            let model: String = frow.get("model").unwrap_or_else(|| "ecmwf".to_string());
            let flat = parse_decimal(frow, "lat")?;
            let flon = parse_decimal(frow, "lon")?;
            let hourly = self.load_hourly(&mut conn, fid)?;
            fetches.push(FetchWithHourly { lat: flat, lon: flon, model, hourly });
        }
        Ok(fetches)
    }
```

Note the added `AND inner_ff.model = ff.model` so the "latest fetch per point" is computed per model.

- [ ] **Step 4: Make `get_grid_points_at` prefer AROME, else ECMWF**

Replace the body (lines ~203-239) so it picks AROME points for the timestamp when any exist, otherwise ECMWF. Implement by selecting all latest points at the timestamp (per model+point), then partitioning in Rust:

```rust
    pub fn get_grid_points_at(
        &self,
        timestamp_iso: &str,
    ) -> Result<Vec<GridPointForecast>, AppError> {
        let ts_db = parse_iso_to_db(timestamp_iso)?;
        let mut conn = self.pool.get_conn()?;
        let rows: Vec<mysql::Row> = conn.exec(
            "SELECT ff.model, ff.lat, ff.lon,
                    fh.wind_speed_kn, fh.wind_direction_deg, fh.wind_gust_kn,
                    fh.wave_height_m, fh.wave_period_s, fh.wave_direction_deg, fh.cape_j_kg
             FROM forecast_fetch ff
             JOIN forecast_hourly fh ON fh.fetch_id = ff.id
             WHERE fh.timestamp = :ts
               AND ff.fetched_at = (
                   SELECT MAX(inner_ff.fetched_at)
                   FROM forecast_fetch inner_ff
                   WHERE inner_ff.lat = ff.lat
                     AND inner_ff.lon = ff.lon
                     AND inner_ff.model = ff.model
               )",
            params! { "ts" => &ts_db },
        )?;
        let mut all: Vec<GridPointForecast> = rows.iter()
            .map(|r| -> Result<GridPointForecast, AppError> {
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
                    model: r.get("model").unwrap_or_else(|| "ecmwf".to_string()),
                })
            })
            .collect::<Result<_, _>>()?;

        // Prefer AROME coverage for this hour; fall back to ECMWF when AROME has none.
        let has_arome = all.iter().any(|p| p.model == "arome");
        if has_arome {
            all.retain(|p| p.model == "arome");
        }
        Ok(all)
    }
```

- [ ] **Step 5: Update the DB-module test call sites and add a `model` round-trip test**

In the `tests` module of the same file, every `insert_forecast(...)` call now needs a model argument. Update the existing calls to pass `"ecmwf"`:

- `test_insert_forecast_and_fetch_fetches`: `db.insert_forecast(area_id, "ecmwf", 43.5, 8.5, Utc::now(), &hourly).unwrap();`
- `test_delete_area_cascades_to_fetches`: `db.insert_forecast(area_id, "ecmwf", 43.5, 8.5, Utc::now(), &hourly).unwrap();`
- `test_get_grid_points_at_returns_latest_fetch`: both inserts use `"ecmwf"`, e.g. `db.insert_forecast(area_id, "ecmwf", 43.5, 8.5, ...)`.
- `test_fetch_forecast_fetches_returns_all_grid_points`: both inserts use `"ecmwf"`.

Then add this new test at the end of the `tests` module:

```rust
    #[test]
    #[ignore]
    fn test_grid_points_prefers_arome_over_ecmwf() {
        let db = setup_db();
        let area_id = make_area(&db);
        let ts = "2026-06-13T09:00:00Z";
        let when = Utc::now();
        // ECMWF point at one location
        db.insert_forecast(area_id, "ecmwf", 43.5, 8.5, when, &vec![make_hourly(ts, 10.0)]).unwrap();
        // AROME point at a (different) location, same hour
        db.insert_forecast(area_id, "arome", 43.51, 8.51, when, &vec![make_hourly(ts, 18.0)]).unwrap();

        let pts = db.get_grid_points_at(ts).unwrap();
        assert!(!pts.is_empty());
        assert!(pts.iter().all(|p| p.model == "arome"),
            "Expected only AROME points when AROME covers the hour, got {:?}",
            pts.iter().map(|p| &p.model).collect::<Vec<_>>());
    }

    #[test]
    #[ignore]
    fn test_grid_points_falls_back_to_ecmwf_when_no_arome() {
        let db = setup_db();
        let area_id = make_area(&db);
        let ts = "2026-06-13T09:00:00Z";
        db.insert_forecast(area_id, "ecmwf", 43.5, 8.5, Utc::now(), &vec![make_hourly(ts, 10.0)]).unwrap();

        let pts = db.get_grid_points_at(ts).unwrap();
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].model, "ecmwf");
    }
```

- [ ] **Step 6: Restore crate compilation (provisional caller updates)**

So the crate still builds at the end of this task, update the two production callers to pass `"ecmwf"` for now (Task 5 finalises them):

- `src/forecast_poller.rs:91-93` → `db.insert_forecast(area.id, "ecmwf", f.lat, f.lon, fetched_at, &f.hourly)`
- `src/web/api.rs:1597-1599` → `db.insert_forecast(area.id, "ecmwf", f.lat, f.lon, fetched_at, &f.hourly)`

- [ ] **Step 7: Verify the build and DB tests**

Run: `cargo build`
Expected: compiles.

Run: `cargo test -- --test-threads=1 --include-ignored db::operations::forecast`
Expected: all forecast DB tests PASS, including the two new ones.

---

### Task 3: AROME fetch in `src/forecast.rs`

**Files:**
- Modify: `src/forecast.rs`
- Test: `src/forecast.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test for the AROME URL builder**

Add to the `tests` module in `src/forecast.rs`:

```rust
    #[test]
    fn test_arome_bbox_url_contains_expected_params() {
        let url = build_arome_bbox_url(43.0, 43.5, 8.0, 8.5);
        assert!(url.contains("bounding_box=43,8,43.5,8.5"), "url: {}", url);
        assert!(url.contains("models=meteofrance_arome_france_hd"), "url: {}", url);
        assert!(url.contains("forecast_days=2"), "url: {}", url);
        assert!(url.contains("wind_speed_unit=kn"), "url: {}", url);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test forecast::tests::test_arome_bbox_url`
Expected: FAIL — `build_arome_bbox_url` not found.

- [ ] **Step 3: Add the AROME URL builder**

Add next to `build_meteo_bbox_url` (after line ~117) in `src/forecast.rs`:

```rust
pub(crate) fn build_arome_bbox_url(lat_min: f64, lat_max: f64, lon_min: f64, lon_max: f64) -> String {
    // AROME France HD (~1.5 km via Open-Meteo) — short-term, wind only, no waves.
    format!(
        "https://api.open-meteo.com/v1/forecast\
         ?bounding_box={lat_min},{lon_min},{lat_max},{lon_max}\
         &models=meteofrance_arome_france_hd\
         &hourly=wind_speed_10m,wind_direction_10m,wind_gusts_10m,cape\
         &wind_speed_unit=kn&forecast_days=2&timezone=UTC",
    )
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test forecast::tests::test_arome_bbox_url`
Expected: PASS.

- [ ] **Step 5: Add `model` to `FetchedForecast` and refactor the fetch**

Add the field to `FetchedForecast` (line ~11):

```rust
#[derive(Debug)]
pub struct FetchedForecast {
    pub lat: f64,
    pub lon: f64,
    pub model: String,
    pub fetched_at: DateTime<Utc>,
    pub hourly: Vec<ForecastHourlyPoint>,
}
```

Add two private helpers above `fetch_area_forecast` (after the constants block, ~line 98). The `fetch_wind_responses` helper DRYs the ECMWF and AROME wind requests; `build_hourly` DRYs the per-point hourly assembly:

```rust
async fn fetch_wind_responses(
    client: &reqwest::Client,
    url: &str,
) -> Result<Vec<MeteoResponse>, AppError> {
    let resp = client.get(url).send().await
        .map_err(|e| AppError::Io(format!("Open-Meteo wind request failed: {}", e)))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(AppError::Io(format!("Open-Meteo wind returned HTTP {}", status)));
    }
    let body = resp.text().await
        .map_err(|e| AppError::Io(format!("Open-Meteo wind body read failed: {}", e)))?;
    let raw: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
        let preview = body.chars().take(300).collect::<String>();
        AppError::Parse(format!("Open-Meteo wind parse failed: {} — body: {}", e, preview))
    })?;
    Ok(serde_json::from_value::<OneOrMany<MeteoResponse>>(raw)
        .map_err(|e| AppError::Parse(e.to_string()))?
        .into_vec())
}

fn build_hourly(meteo: &MeteoResponse, marine: Option<&MarineResponse>) -> Vec<ForecastHourlyPoint> {
    let n = meteo.hourly.time.len();
    let mut hourly = Vec::with_capacity(n);
    for j in 0..n {
        let raw = &meteo.hourly.time[j];
        let timestamp = if raw.len() == 16 { format!("{}:00Z", raw) } else { raw.clone() };
        hourly.push(ForecastHourlyPoint {
            timestamp,
            wind_speed_kn: meteo.hourly.wind_speed_10m.as_ref().and_then(|v| v.get(j).copied().flatten()),
            wind_direction_deg: meteo.hourly.wind_direction_10m.as_ref().and_then(|v| v.get(j).copied().flatten()),
            wind_gust_kn: meteo.hourly.wind_gusts_10m.as_ref().and_then(|v| v.get(j).copied().flatten()),
            wave_height_m: marine.and_then(|m| m.hourly.wave_height.as_ref()?.get(j).copied().flatten()),
            wave_period_s: marine.and_then(|m| m.hourly.wave_period.as_ref()?.get(j).copied().flatten()),
            wave_direction_deg: marine.and_then(|m| m.hourly.wave_direction.as_ref()?.get(j).copied().flatten()),
            cape_j_kg: meteo.hourly.cape.as_ref().and_then(|v| v.get(j).copied().flatten()),
        });
    }
    hourly
}
```

- [ ] **Step 6: Rewrite `fetch_area_forecast` to fetch both models**

Replace the body of `fetch_area_forecast` (lines ~129-241) with the version below. ECMWF wind is fatal on failure (unchanged); marine and AROME are non-fatal:

```rust
pub async fn fetch_area_forecast(
    lat_min: f64,
    lat_max: f64,
    lon_min: f64,
    lon_max: f64,
) -> Result<Vec<FetchedForecast>, AppError> {
    let period_start = Utc::now().format("%Y-%m-%dT%H:00Z").to_string();
    let period_end = (Utc::now() + chrono::Duration::days(7)).format("%Y-%m-%dT%H:00Z").to_string();
    info!(
        lat_min, lat_max, lon_min, lon_max,
        period_start = %period_start, period_end = %period_end,
        "Forecast fetch: starting area request"
    );

    let client = reqwest::Client::new();
    let fetched_at = Utc::now();

    // ── ECMWF wind (fatal on failure) ──
    let ecmwf_url = build_meteo_bbox_url(lat_min, lat_max, lon_min, lon_max);
    debug!(url = %ecmwf_url, "Open-Meteo ECMWF wind: sending request");
    let ecmwf_responses = fetch_wind_responses(&client, &ecmwf_url).await?;
    info!(grid_points = ecmwf_responses.len(), "Open-Meteo ECMWF wind: response OK");

    // ── ECMWF marine (non-fatal) ──
    let marine_url = build_marine_bbox_url(lat_min, lat_max, lon_min, lon_max);
    debug!(url = %marine_url, "Open-Meteo marine: sending request");
    let marine_responses: Vec<MarineResponse> = {
        let fetch: Result<Vec<MarineResponse>, String> = async {
            let resp = client.get(&marine_url).send().await.map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                return Err(format!("HTTP {}", resp.status()));
            }
            let body = resp.text().await.map_err(|e| e.to_string())?;
            let raw: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
            serde_json::from_value::<OneOrMany<MarineResponse>>(raw)
                .map(|v| v.into_vec())
                .map_err(|e| e.to_string())
        }.await;
        match fetch {
            Ok(v) => { info!(grid_points = v.len(), "Open-Meteo marine: response OK"); v }
            Err(e) => { warn!(error = %e, "Open-Meteo marine: unavailable (wave data omitted)"); vec![] }
        }
    };

    // ── AROME wind (non-fatal) ──
    let arome_url = build_arome_bbox_url(lat_min, lat_max, lon_min, lon_max);
    debug!(url = %arome_url, "Open-Meteo AROME wind: sending request");
    let arome_responses: Vec<MeteoResponse> = match fetch_wind_responses(&client, &arome_url).await {
        Ok(v) => { info!(grid_points = v.len(), "Open-Meteo AROME wind: response OK"); v }
        Err(e) => { warn!(error = %e, "Open-Meteo AROME wind: unavailable (using ECMWF only)"); vec![] }
    };

    let mut results = Vec::new();

    // ECMWF results: merge marine waves by grid-point index, as before.
    for (i, meteo) in ecmwf_responses.iter().enumerate() {
        let marine = marine_responses.get(i);
        results.push(FetchedForecast {
            lat: meteo.latitude,
            lon: meteo.longitude,
            model: "ecmwf".to_string(),
            fetched_at,
            hourly: build_hourly(meteo, marine),
        });
    }

    // AROME results: wind only, no waves.
    for meteo in &arome_responses {
        results.push(FetchedForecast {
            lat: meteo.latitude,
            lon: meteo.longitude,
            model: "arome".to_string(),
            fetched_at,
            hourly: build_hourly(meteo, None),
        });
    }

    info!(
        lat_min, lat_max, lon_min, lon_max,
        ecmwf_points = ecmwf_responses.len(),
        arome_points = arome_responses.len(),
        total_grid_points = results.len(),
        "Forecast fetch: area complete"
    );
    Ok(results)
}
```

- [ ] **Step 7: Verify the build and existing tests**

Run: `cargo test forecast`
Expected: PASS (URL test plus all existing forecast tests still compile/pass).

---

### Task 4: Model-aware interpolation in `src/forecast.rs`

**Files:**
- Modify: `src/forecast.rs`
- Test: `src/forecast.rs` (`#[cfg(test)] mod tests`)

The existing low-level `interpolate_idw(target_lat, target_lon, samples)` is kept unchanged and reused. A new `interpolate_blended` runs IDW separately over AROME and ECMWF samples and composes the result: wind family (`wind_speed_kn`, `wind_direction_deg`, `wind_gust_kn`, `cape_j_kg`) from AROME when AROME yields a wind speed, else ECMWF; wave family always from ECMWF.

- [ ] **Step 1: Add `model` to `RouteOverlayPoint`**

In `src/forecast.rs` (line ~18) add a field (after `twa_deg`):

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
}
```

- [ ] **Step 2: Write failing tests for blended interpolation**

Add to the `tests` module. These use the existing `pt(...)` helper (all fields populated) and a wind-only helper for AROME:

```rust
    fn pt_wind_only(wind_speed: f64, wind_dir: f64) -> ForecastHourlyPoint {
        ForecastHourlyPoint {
            timestamp: "2026-06-13T06:00:00Z".to_string(),
            wind_speed_kn: Some(wind_speed),
            wind_direction_deg: Some(wind_dir),
            wind_gust_kn: None,
            wave_height_m: None,
            wave_period_s: None,
            wave_direction_deg: None,
            cape_j_kg: None,
        }
    }

    #[test]
    fn test_blended_prefers_arome_wind() {
        // ECMWF ~5NM away with wind 10kn; AROME ~5NM away with wind 18kn.
        let arome = vec![(43.0 + 0.08, 9.0, pt_wind_only(18.0, 200.0))];
        let ecmwf = vec![(43.0 + 0.08, 9.0, pt(10.0, 180.0, 14.0, 1.5, 7.0, 190.0, 100.0))];
        let r = interpolate_blended(43.0, 9.0, &arome, &ecmwf).unwrap();
        assert!((r.wind_speed_kn.unwrap() - 18.0).abs() < 0.5, "got {:?}", r.wind_speed_kn);
        assert_eq!(r.wind_model.as_deref(), Some("arome"));
        // Waves always from ECMWF, even though AROME drove the wind.
        assert!((r.wave_height_m.unwrap() - 1.5).abs() < 0.1, "got {:?}", r.wave_height_m);
    }

    #[test]
    fn test_blended_falls_back_to_ecmwf_when_no_arome() {
        let ecmwf = vec![(43.0 + 0.08, 9.0, pt(10.0, 180.0, 14.0, 1.5, 7.0, 190.0, 100.0))];
        let r = interpolate_blended(43.0, 9.0, &[], &ecmwf).unwrap();
        assert!((r.wind_speed_kn.unwrap() - 10.0).abs() < 0.5);
        assert_eq!(r.wind_model.as_deref(), Some("ecmwf"));
    }

    #[test]
    fn test_blended_arome_out_of_range_uses_ecmwf() {
        // AROME sample ~50NM away (beyond MAX_DISTANCE_NM) → ignored; ECMWF used.
        let arome = vec![(43.0 + 0.9, 9.0, pt_wind_only(18.0, 200.0))];
        let ecmwf = vec![(43.0 + 0.08, 9.0, pt(10.0, 180.0, 14.0, 1.5, 7.0, 190.0, 100.0))];
        let r = interpolate_blended(43.0, 9.0, &arome, &ecmwf).unwrap();
        assert!((r.wind_speed_kn.unwrap() - 10.0).abs() < 0.5);
        assert_eq!(r.wind_model.as_deref(), Some("ecmwf"));
    }

    #[test]
    fn test_blended_no_data_returns_none() {
        assert!(interpolate_blended(43.0, 9.0, &[], &[]).is_none());
    }
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test forecast::tests::test_blended`
Expected: FAIL — `interpolate_blended` not found.

- [ ] **Step 4: Implement `BlendedForecast` and `interpolate_blended`**

Add near `interpolate_idw` in `src/forecast.rs`:

```rust
pub(crate) struct BlendedForecast {
    pub timestamp: String,
    pub wind_speed_kn: Option<f64>,
    pub wind_direction_deg: Option<f64>,
    pub wind_gust_kn: Option<f64>,
    pub wave_height_m: Option<f64>,
    pub wave_period_s: Option<f64>,
    pub wave_direction_deg: Option<f64>,
    pub cape_j_kg: Option<f64>,
    pub wind_model: Option<String>,
}

/// Interpolates a point by blending two model grids: wind family is taken from
/// AROME when AROME produces a wind speed in range, otherwise from ECMWF; the
/// wave family always comes from ECMWF (AROME has no wave output).
pub(crate) fn interpolate_blended(
    target_lat: f64,
    target_lon: f64,
    arome_samples: &[(f64, f64, ForecastHourlyPoint)],
    ecmwf_samples: &[(f64, f64, ForecastHourlyPoint)],
) -> Option<BlendedForecast> {
    let arome = interpolate_idw(target_lat, target_lon, arome_samples);
    let ecmwf = interpolate_idw(target_lat, target_lon, ecmwf_samples);

    // Wind source: AROME if it yielded a wind speed, else ECMWF.
    let arome_has_wind = arome.as_ref().is_some_and(|p| p.wind_speed_kn.is_some());
    let (wind_src, wind_model): (Option<&ForecastHourlyPoint>, Option<&str>) = if arome_has_wind {
        (arome.as_ref(), Some("arome"))
    } else if ecmwf.is_some() {
        (ecmwf.as_ref(), Some("ecmwf"))
    } else {
        (None, None)
    };

    // If neither model produced anything at all, there is no data here.
    if wind_src.is_none() && ecmwf.is_none() {
        return None;
    }

    let timestamp = wind_src
        .or(ecmwf.as_ref())
        .map(|p| p.timestamp.clone())
        .unwrap_or_default();

    Some(BlendedForecast {
        timestamp,
        wind_speed_kn: wind_src.and_then(|p| p.wind_speed_kn),
        wind_direction_deg: wind_src.and_then(|p| p.wind_direction_deg),
        wind_gust_kn: wind_src.and_then(|p| p.wind_gust_kn),
        cape_j_kg: wind_src.and_then(|p| p.cape_j_kg),
        wave_height_m: ecmwf.as_ref().and_then(|p| p.wave_height_m),
        wave_period_s: ecmwf.as_ref().and_then(|p| p.wave_period_s),
        wave_direction_deg: ecmwf.as_ref().and_then(|p| p.wave_direction_deg),
        wind_model: wind_model.map(|s| s.to_string()),
    })
}
```

- [ ] **Step 5: Run the blended tests to verify they pass**

Run: `cargo test forecast::tests::test_blended`
Expected: PASS (all four).

- [ ] **Step 6: Make `nearest_forecast_wind` model-aware**

Replace `nearest_forecast_wind` (lines ~266-278). It partitions fetches by model, builds per-model sample lists, and blends:

```rust
pub(crate) fn nearest_forecast_wind(
    fetches: &[FetchWithHourly],
    lat: f64,
    lon: f64,
    time: DateTime<Utc>,
) -> Option<(f64, f64)> {
    let collect = |model: &str| -> Vec<(f64, f64, ForecastHourlyPoint)> {
        fetches
            .iter()
            .filter(|f| f.model == model)
            .filter_map(|f| nearest_hourly(&f.hourly, time).map(|pt| (f.lat, f.lon, pt)))
            .collect()
    };
    let arome = collect("arome");
    let ecmwf = collect("ecmwf");
    let interp = interpolate_blended(lat, lon, &arome, &ecmwf)?;
    Some((interp.wind_speed_kn?, interp.wind_direction_deg?))
}
```

- [ ] **Step 7: Make `compute_route_overlay` model-aware**

Replace `compute_route_overlay` (lines ~365-395):

```rust
pub fn compute_route_overlay(
    track: &[RouteTrackPoint],
    fetches: &[FetchWithHourly],
) -> Vec<RouteOverlayPoint> {
    track
        .iter()
        .filter_map(|pt| {
            let collect = |model: &str| -> Vec<(f64, f64, ForecastHourlyPoint)> {
                fetches
                    .iter()
                    .filter(|f| f.model == model)
                    .filter_map(|f| nearest_hourly(&f.hourly, pt.time).map(|p| (f.lat, f.lon, p)))
                    .collect()
            };
            let arome = collect("arome");
            let ecmwf = collect("ecmwf");
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
        })
        .collect()
}
```

- [ ] **Step 8: Update existing tests that construct `FetchWithHourly` or `RouteOverlayPoint`**

In `src/forecast.rs` tests, `test_compute_route_overlay_returns_points_with_coords` builds a `FetchWithHourly` literal — add `model: "ecmwf".to_string(),` after `lon: 9.25,`. Any test that constructs `RouteOverlayPoint` directly must add `wind_model: None,` (none currently do, but verify the build catches it).

The two `FetchWithHourly` literals in `generate_route_track` tests (`test_generate_route_track_uses_polar_speed`, `test_generate_route_track_falls_back_to_motoring_no_wind`) also need `model: "ecmwf".to_string(),`.

- [ ] **Step 9: Verify forecast tests and full non-DB suite**

Run: `cargo test forecast`
Expected: PASS.

Run: `cargo test`
Expected: PASS (whole non-DB suite compiles and passes).

---

### Task 5: Finalise production callers

**Files:**
- Modify: `src/forecast_poller.rs:90-96`
- Modify: `src/web/api.rs:1596-1604`

Both callers currently iterate `forecasts` (now `FetchedForecast` with a `model` field). Pass each fetch's own model into `insert_forecast`.

- [ ] **Step 1: Update the poller**

In `src/forecast_poller.rs`, change the insert loop:

```rust
                    for f in &forecasts {
                        if let Err(e) = db.insert_forecast(
                            area.id, &f.model, f.lat, f.lon, fetched_at, &f.hourly,
                        ) {
                            warn!(area_id = area.id, error = %e, "Failed to store forecast point");
                        }
                    }
```

- [ ] **Step 2: Update `refresh_forecast` in the API**

In `src/web/api.rs`:

```rust
                for f in &forecasts {
                    if let Err(e) = db.insert_forecast(
                        area.id, &f.model, f.lat, f.lon, fetched_at, &f.hourly,
                    ) {
                        warn!(area_id = area.id, error = %e,
                              "refresh_forecast: failed to store point");
                    }
                }
```

- [ ] **Step 3: Verify the build and full non-DB suite**

Run: `cargo build`
Expected: compiles with no warnings about unused `model`.

Run: `cargo test`
Expected: PASS.

---

### Task 6: Surface the active model in the planning UI

**Files:**
- Modify: `static/plan.html`

Keep it minimal: show which model supplied the displayed grid (a small badge) and add the model to each arrow's popup. Grid points now carry `model` (`"arome"` / `"ecmwf"`); since `get_grid_points_at` returns a single model per hour, the active model is `lastGridPts[0].model`.

- [ ] **Step 1: Track the active model when loading grid points**

In `loadGridPoints` (line ~510), after `lastGridPts = json.data || [];` add:

```javascript
                const activeModel = lastGridPts.length ? (lastGridPts[0].model || 'ecmwf') : null;
                const badge = document.getElementById('modelBadge');
                if (badge) {
                    badge.textContent = activeModel ? activeModel.toUpperCase() : '—';
                    badge.title = activeModel === 'arome'
                        ? 'AROME ~1.5 km (short-term)'
                        : 'ECMWF ~9 km';
                }
```

- [ ] **Step 2: Add the badge element next to the legend title**

In the legend block, change the `legendTitle` div (line ~260) to include a badge span:

```html
                            <div id="legendTitle" style="font-weight:600; margin-bottom:4px; letter-spacing:0.3px;">Wind (kn) <span id="modelBadge" style="font-weight:500; font-size:0.8em; opacity:0.75; margin-left:4px;">—</span></div>
```

- [ ] **Step 3: Show the model in the arrow popup**

In `renderArrows` (line ~568), append a model line to the popup HTML. Since interpolated display points may not carry `model`, read it from the active badge:

```javascript
                m.bindPopup(
                    `<b>Wind:</b> ${wind.toFixed(1)} kn ${dir.toFixed(0)}°<br>` +
                    `<b>Gust:</b> ${gust.toFixed(1)} kn<br>` +
                    `<b>Wave:</b> ${(pt.wave_height_m || 0).toFixed(1)} m ` +
                    `${(pt.wave_period_s || 0).toFixed(0)} s<br>` +
                    `<b>CAPE:</b> ${(pt.cape_j_kg || 0).toFixed(0)} J/kg<br>` +
                    `<b>Model:</b> ${(document.getElementById('modelBadge')?.textContent) || '—'}`
                );
```

- [ ] **Step 4: Verify in the browser**

Run the app (`cargo run` with `config.json`, or the existing dev workflow), open the planning page, load a forecast, and confirm: the badge shows `AROME` for near-term timestamps that have AROME coverage and `ECMWF` for far-term timestamps; the arrow popup shows the matching model.

(If desired, use the `verify` skill to drive the app and observe this.)

---

## Self-review notes

- **Spec coverage:** schema `model` column (Task 1); model on DB types + AROME-preferred `get_grid_points_at` + per-model "latest fetch" (Task 2); AROME URL builder + three-call fetch with non-fatal AROME/marine (Task 3); model-aware blended interpolation with waves-always-ECMWF and `wind_model` provenance (Task 4); caller wiring (Task 5); UI indicator (Task 6). Constants-not-config and unchanged poller cadence are honoured (no config files touched).
- **Type consistency:** `insert_forecast(area_id, model, lat, lon, fetched_at, hourly)` is used identically in Tasks 2, 5, and the DB tests. `FetchWithHourly` and `GridPointForecast` gain `model: String`; `FetchedForecast` gains `model: String`; `RouteOverlayPoint` gains `wind_model: Option<String>`; `BlendedForecast.wind_model` is `Option<String>`. `interpolate_blended` signature is identical in Tasks 4, 6's callers, and `nearest_forecast_wind` / `compute_route_overlay`.
- **No commits:** every task verifies via `cargo build` / `cargo test` and stops for user review, per the project git rule.
