# Legacy Trip Import Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Import the 106 pre-2020 trips from the legacy `nmearouter` database (old `trip`/`track`/`meteo` tables) into the live `nmea_router` production database, synthesizing the fields `track` never recorded (COG, heading, wind angle) from position and the separate `meteo` table.

**Architecture:** A new standalone binary `import_legacy_trips`, mirroring the existing `gap_filler.rs` pattern. Pure ETL logic (timezone conversion, COG/heading synthesis, wind-angle derivation, JSON building) lives in a new `src/legacy_import/` module tree and is unit-tested without any DB connection. The binary reads legacy rows via a plain `mysql::Pool` against `nmearouter`, transforms each trip into the JSON shape `VesselDatabase::import_trip` already accepts, and calls that existing, tested function — which handles the transactional insert, aggregate recompute, and cache invalidation.

**Tech Stack:** Rust, `mysql` crate (raw pool for the legacy DB), `chrono` + `chrono-tz` (new dependency, DST-aware Europe/Rome → UTC conversion), `serde_json`, `uuid`.

**Spec:** [docs/superpowers/specs/2026-09-04-legacy-trip-import-design.md](../specs/2026-09-04-legacy-trip-import-design.md)

## Global Constraints

- Legacy timestamps (`trip.fromTS/toTS`, `track.TS`, `meteo.TS`) are Europe/Rome **local** time — convert with `chrono-tz`, not a flat offset (DST matters across 3.5 years of data).
- Cutover boundary, used exactly as validated: legacy trips are those with `trip.toTS <= '2020-01-03 07:43:03'` (local, compared directly against the legacy column — 106 rows, zero straddling the boundary). Never touch anything at or after this boundary — it is already live in `nmea_router`.
- Every trip JSON sent to `import_trip` **must** include a freshly generated `uuid`. Without it, `import_trip` falls into its overlap-check branch (`end_timestamp >= new_start` against existing trips), which would false-positive on every legacy trip since all live trips already start after 2020.
- `trip.dist`/`distSail`/`distMotor` are never read or trusted — always send `0` placeholders for `dist_sail`/`dist_motor`/`t_sail`/`t_motor`/`t_moor` in the trip JSON; `import_trip` recomputes the real aggregates from the inserted `vs[]` rows.
- COG/heading: underway (`anchor=0`) rows derive `cog_deg` from `haversine_heading(prev_point, this_point)` and set `average_heading_deg = cog_deg`; moored (`anchor=1`) rows get `NULL` for both — never carry forward a previous heading (this was an explicit decision against `gap_filler/synthesizer.rs`'s carry-forward convention, in favor of matching what `nmearouter`'s own historical data actually contains).
- `average_wind_angle_deg` (TWA) is derived as `(TWD − cog_deg) mod 360` using the just-derived `cog_deg` — only computable underway; `NULL` while moored. `average_wind_speed_kn` is a direct nearest-timestamp join from `meteo` TWS (metric_id 5), with no heading dependency, moored or not.
- Wind values are only joined from `meteo` when the nearest same-metric reading falls within the track row's own `dTime` window (minimum 60s bound when `dTime` is 0/NULL); otherwise `NULL`.
- MySQL `DECIMAL` columns (`track.lat`, `track.lon`, `track.dist`) come back as `mysql::Value::Bytes` — convert via `String::from_utf8(b)?.parse::<f64>()`, per this project's CLAUDE.md.
- Units are non-negotiable per CLAUDE.md: all distances stay nautical miles, speeds knots, angles decimal degrees 0–360 — the legacy columns are already in these units, so this is a passthrough, not a conversion.

---

### Task 1: Legacy types, timezone conversion, and position/wind synthesis

**Files:**
- Modify: `Cargo.toml` (add `chrono-tz` dependency and the new `[[bin]]` target)
- Create: `src/legacy_import/mod.rs`
- Create: `src/legacy_import/geometry.rs`
- Test: inline `#[cfg(test)] mod tests` in `src/legacy_import/geometry.rs`

**Interfaces:**
- Produces (used by Task 2 and Task 3):
  - `pub struct LegacyTrip { pub id: i64, pub description: Option<String>, pub from_ts: chrono::NaiveDateTime, pub to_ts: chrono::NaiveDateTime }`
  - `pub struct LegacyTrackRow { pub ts: chrono::NaiveDateTime, pub lat: f64, pub lon: f64, pub anchor: i32, pub d_time: Option<i64>, pub dist: Option<f64>, pub speed: f64, pub max_speed: f64, pub engine: u8 }`
  - `pub struct LegacyMeteoRow { pub ts: chrono::NaiveDateTime, pub metric_id: u8, pub v: f64, pub v_min: Option<f64>, pub v_max: Option<f64>, pub unit: Option<String> }`
  - `pub fn rome_local_to_utc(naive: chrono::NaiveDateTime) -> chrono::DateTime<chrono::Utc>`
  - `pub struct Reading { pub ts: chrono::NaiveDateTime, pub value: f64 }`
  - `pub fn nearest_reading(readings: &[Reading], target: chrono::NaiveDateTime, bound_secs: i64) -> Option<f64>` (readings must be sorted ascending by `ts`)
  - `pub fn derive_cog_heading(prev: Option<(f64, f64)>, cur: (f64, f64), is_moored: bool) -> (Option<f64>, Option<f64>)`
  - `pub fn derive_wind_angle(twd_deg: Option<f64>, cog_deg: Option<f64>) -> Option<f64>`

- [ ] **Step 1: Add the `chrono-tz` dependency and new bin target**

In `Cargo.toml`, add to `[dependencies]` (near the existing `chrono = "0.4"` line):

```toml
chrono-tz = "0.8"
```

And add to the `[[bin]]` list (after the `backfill_legs` entry):

```toml
[[bin]]
name = "import_legacy_trips"
path = "src/bin/import_legacy_trips.rs"
```

- [ ] **Step 2: Run `cargo check` to confirm the new dependency resolves**

Run: `cargo check`
Expected: succeeds (the new bin target will fail to compile until Task 4 creates `src/bin/import_legacy_trips.rs` — for now, temporarily comment out the `[[bin]]` block added in Step 1, run `cargo check`, confirm it passes, then leave the block in place; it will start compiling once Task 4 lands).

- [ ] **Step 3: Create `src/legacy_import/mod.rs` with the shared types**

```rust
// Shared data types for importing pre-2020 trips from the legacy `nmearouter`
// database (old `trip`/`track`/`meteo` tables) into the current schema.
// See docs/superpowers/specs/2026-09-04-legacy-trip-import-design.md.

pub mod geometry;
pub mod transform;
pub mod source;

#[derive(Debug, Clone)]
pub struct LegacyTrip {
    pub id: i64,
    pub description: Option<String>,
    pub from_ts: chrono::NaiveDateTime,
    pub to_ts: chrono::NaiveDateTime,
}

#[derive(Debug, Clone)]
pub struct LegacyTrackRow {
    pub ts: chrono::NaiveDateTime,
    pub lat: f64,
    pub lon: f64,
    /// 0 = underway, non-zero = anchored/moored.
    pub anchor: i32,
    /// Seconds since the previous track row; NULL on the first row of a trip.
    pub d_time: Option<i64>,
    /// Nautical miles since the previous track row.
    pub dist: Option<f64>,
    pub speed: f64,
    pub max_speed: f64,
    /// 0 = off, 1 = on, 2 = unknown (same encoding as engine_on).
    pub engine: u8,
}

#[derive(Debug, Clone)]
pub struct LegacyMeteoRow {
    pub ts: chrono::NaiveDateTime,
    pub metric_id: u8,
    pub v: f64,
    pub v_min: Option<f64>,
    pub v_max: Option<f64>,
    pub unit: Option<String>,
}
```

(`source.rs` and `transform.rs` don't exist yet — this won't compile until Steps 4-6 create `geometry.rs`, and Task 2/3 create `transform.rs`/`source.rs`. That's expected; this crate is only reachable via `src/bin/import_legacy_trips.rs`'s `#[path]` include added in Task 4, so `cargo check` on the main package won't touch it until then.)

- [ ] **Step 4: Write the failing tests for `geometry.rs`**

Create `src/legacy_import/geometry.rs`:

```rust
use chrono::{DateTime, NaiveDateTime, Utc};

/// Converts a naive Europe/Rome local timestamp (as stored by the legacy
/// `nmearouter` database) to UTC, DST-aware.
pub fn rome_local_to_utc(naive: NaiveDateTime) -> DateTime<Utc> {
    unimplemented!()
}

#[derive(Debug, Clone, Copy)]
pub struct Reading {
    pub ts: NaiveDateTime,
    pub value: f64,
}

/// Nearest same-metric reading to `target`, only if within `bound_secs`.
/// `readings` must be sorted ascending by `ts`.
pub fn nearest_reading(readings: &[Reading], target: NaiveDateTime, bound_secs: i64) -> Option<f64> {
    unimplemented!()
}

/// Underway: COG = bearing from the previous position to the current one
/// (`crate::utilities::haversine_heading`), heading = COG. Moored, or no
/// previous position: both `None`.
pub fn derive_cog_heading(prev: Option<(f64, f64)>, cur: (f64, f64), is_moored: bool) -> (Option<f64>, Option<f64>) {
    unimplemented!()
}

/// True Wind Angle (boat-relative) from True Wind Direction (compass-
/// referenced) and course over ground: `(twd - cog) mod 360`.
pub fn derive_wind_angle(twd_deg: Option<f64>, cog_deg: Option<f64>) -> Option<f64> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn test_rome_local_to_utc_winter_offset_is_one_hour() {
        // Empirically confirmed: nmearouter's 2020-01-03 07:43:03 local is the
        // same instant as live nmea_router trip id=2's start, 2020-01-03T06:43:03Z.
        let naive = NaiveDate::from_ymd_opt(2020, 1, 3).unwrap().and_hms_opt(7, 43, 3).unwrap();
        let utc = rome_local_to_utc(naive);
        assert_eq!(utc.to_rfc3339(), "2020-01-03T06:43:03+00:00");
    }

    #[test]
    fn test_rome_local_to_utc_summer_offset_is_two_hours() {
        let naive = NaiveDate::from_ymd_opt(2020, 7, 15).unwrap().and_hms_opt(12, 0, 0).unwrap();
        let utc = rome_local_to_utc(naive);
        assert_eq!(utc.to_rfc3339(), "2020-07-15T10:00:00+00:00");
    }

    #[test]
    fn test_nearest_reading_picks_closest_within_bound() {
        let readings = vec![
            Reading { ts: mk_ts(10, 0, 0), value: 5.0 },
            Reading { ts: mk_ts(10, 5, 0), value: 8.0 },
            Reading { ts: mk_ts(10, 10, 0), value: 12.0 },
        ];
        // Target is 2 minutes after the middle reading, 7 minutes before the last.
        let target = mk_ts(10, 7, 0);
        assert_eq!(nearest_reading(&readings, target, 300), Some(8.0));
    }

    #[test]
    fn test_nearest_reading_outside_bound_returns_none() {
        let readings = vec![Reading { ts: mk_ts(10, 0, 0), value: 5.0 }];
        let target = mk_ts(10, 30, 0); // 30 minutes away
        assert_eq!(nearest_reading(&readings, target, 60), None);
    }

    #[test]
    fn test_nearest_reading_empty_list_returns_none() {
        let target = mk_ts(10, 0, 0);
        assert_eq!(nearest_reading(&[], target, 300), None);
    }

    #[test]
    fn test_derive_cog_heading_underway_uses_bearing() {
        // Due north: bearing should be ~0 degrees.
        let (cog, hdg) = derive_cog_heading(Some((43.0, 10.0)), (43.01, 10.0), false);
        assert!(cog.unwrap().abs() < 0.5);
        assert_eq!(cog, hdg);
    }

    #[test]
    fn test_derive_cog_heading_moored_is_none() {
        let (cog, hdg) = derive_cog_heading(Some((43.0, 10.0)), (43.0, 10.0), true);
        assert_eq!(cog, None);
        assert_eq!(hdg, None);
    }

    #[test]
    fn test_derive_cog_heading_no_previous_position_is_none() {
        let (cog, hdg) = derive_cog_heading(None, (43.0, 10.0), false);
        assert_eq!(cog, None);
        assert_eq!(hdg, None);
    }

    #[test]
    fn test_derive_wind_angle_matches_empirical_formula() {
        // From live overlap-window data: TWD=213.34385, COG=3.304 => TWA=210.040
        let twa = derive_wind_angle(Some(213.34384954985424), Some(3.304)).unwrap();
        assert!((twa - 210.040).abs() < 0.01);
    }

    #[test]
    fn test_derive_wind_angle_wraps_below_zero() {
        // TWD < COG must wrap around 360, not go negative.
        let twa = derive_wind_angle(Some(10.0), Some(350.0)).unwrap();
        assert!((twa - 20.0).abs() < 1e-9);
    }

    #[test]
    fn test_derive_wind_angle_none_without_cog() {
        assert_eq!(derive_wind_angle(Some(200.0), None), None);
    }

    fn mk_ts(h: u32, m: u32, s: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2022, 6, 5).unwrap().and_hms_opt(h, m, s).unwrap()
    }
}
```

- [ ] **Step 5: Run the tests to confirm they fail**

Run: `cargo test --lib legacy_import::geometry 2>&1 | head -50`
Expected: this actually won't run yet since `legacy_import` isn't wired into any compiled target (it's only reachable via the Task 4 binary's `#[path]` include). Instead, temporarily add `#[path = "legacy_import/mod.rs"] mod legacy_import;` to the top of `src/main.rs` (or use `cargo test --bin nmea_router` after adding it) to compile-check the module now; expect `unimplemented!()` panics when the tests run. Remove this temporary include again in Step 7 once Task 4 provides the real, permanent wiring via the new binary.

- [ ] **Step 6: Implement `geometry.rs`**

Replace each `unimplemented!()` body:

```rust
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Europe::Rome;

pub fn rome_local_to_utc(naive: NaiveDateTime) -> DateTime<Utc> {
    match Rome.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc),
        // Fall-back DST transition (clocks repeat an hour): pick the earlier
        // (still-summer) offset, consistent with how the ambiguous hour is
        // ordered in the source data.
        chrono::LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc),
        // Spring-forward DST transition (this local time never existed):
        // shift forward by an hour and resolve again.
        chrono::LocalResult::None => {
            let adjusted = naive + chrono::Duration::hours(1);
            match Rome.from_local_datetime(&adjusted) {
                chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc),
                _ => Utc.from_utc_datetime(&naive),
            }
        }
    }
}
```

```rust
pub fn nearest_reading(readings: &[Reading], target: NaiveDateTime, bound_secs: i64) -> Option<f64> {
    if readings.is_empty() {
        return None;
    }
    let idx = readings.partition_point(|r| r.ts <= target);
    let mut best: Option<(i64, f64)> = None;
    if idx > 0 {
        let r = &readings[idx - 1];
        best = Some(((target - r.ts).num_seconds().abs(), r.value));
    }
    if idx < readings.len() {
        let r = &readings[idx];
        let diff = (r.ts - target).num_seconds().abs();
        if best.map_or(true, |(bd, _)| diff < bd) {
            best = Some((diff, r.value));
        }
    }
    best.filter(|(diff, _)| *diff <= bound_secs).map(|(_, v)| v)
}
```

```rust
pub fn derive_cog_heading(prev: Option<(f64, f64)>, cur: (f64, f64), is_moored: bool) -> (Option<f64>, Option<f64>) {
    if is_moored {
        return (None, None);
    }
    match prev {
        Some((plat, plon)) => {
            let cog = crate::utilities::haversine_heading(plat, plon, cur.0, cur.1);
            (Some(cog), Some(cog))
        }
        None => (None, None),
    }
}
```

```rust
pub fn derive_wind_angle(twd_deg: Option<f64>, cog_deg: Option<f64>) -> Option<f64> {
    match (twd_deg, cog_deg) {
        (Some(twd), Some(cog)) => Some(((twd - cog) % 360.0 + 360.0) % 360.0),
        _ => None,
    }
}
```

- [ ] **Step 7: Run the tests to confirm they pass, then remove the temporary `main.rs` include**

Run: `cargo test --bin nmea_router legacy_import::geometry -- --nocapture`
Expected: all 11 tests PASS. Then remove the temporary `#[path = "legacy_import/mod.rs"] mod legacy_import;` line added to `src/main.rs` in Step 5 — Task 4 provides the real, permanent wiring via `src/bin/import_legacy_trips.rs`.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml src/legacy_import/mod.rs src/legacy_import/geometry.rs
git commit -m "Add legacy trip import: timezone and position/wind synthesis"
```

---

### Task 2: Trip JSON transform

**Files:**
- Create: `src/legacy_import/transform.rs`
- Modify: `src/legacy_import/mod.rs` (already declares `pub mod transform;` from Task 1 — no change needed)

**Interfaces:**
- Consumes: `LegacyTrip`, `LegacyTrackRow`, `LegacyMeteoRow` (Task 1's `mod.rs`); `rome_local_to_utc`, `Reading`, `nearest_reading`, `derive_cog_heading`, `derive_wind_angle` (Task 1's `geometry.rs`)
- Produces (used by Task 4):
  - `pub struct TransformStats { pub track_rows: usize, pub meteo_rows: usize, pub wind_angle_hits: usize }`
  - `pub fn build_trip_json(trip: &LegacyTrip, track_rows: &[LegacyTrackRow], meteo_rows: &[LegacyMeteoRow]) -> (String, TransformStats)`

- [ ] **Step 1: Write the failing test**

Create `src/legacy_import/transform.rs`:

```rust
use super::geometry::{derive_cog_heading, derive_wind_angle, nearest_reading, rome_local_to_utc, Reading};
use super::{LegacyMeteoRow, LegacyTrackRow, LegacyTrip};
use chrono::SecondsFormat;

#[derive(Debug, Clone, Copy, Default)]
pub struct TransformStats {
    pub track_rows: usize,
    pub meteo_rows: usize,
    pub wind_angle_hits: usize,
}

pub fn build_trip_json(
    trip: &LegacyTrip,
    track_rows: &[LegacyTrackRow],
    meteo_rows: &[LegacyMeteoRow],
) -> (String, TransformStats) {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn ts(h: u32, m: u32, s: u32) -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(2018, 6, 1).unwrap().and_hms_opt(h, m, s).unwrap()
    }

    fn sample_trip() -> LegacyTrip {
        LegacyTrip {
            id: 42,
            description: Some("Test Trip".to_string()),
            from_ts: ts(9, 0, 0),
            to_ts: ts(9, 2, 0),
        }
    }

    #[test]
    fn test_build_trip_json_underway_row_gets_derived_cog_and_wind() {
        let trip = sample_trip();
        let track = vec![
            LegacyTrackRow { ts: ts(9, 0, 0), lat: 43.0, lon: 10.0, anchor: 0, d_time: None, dist: Some(0.0), speed: 0.0, max_speed: 0.0, engine: 0 },
            LegacyTrackRow { ts: ts(9, 1, 0), lat: 43.01, lon: 10.0, anchor: 0, d_time: Some(60), dist: Some(0.6), speed: 6.0, max_speed: 6.5, engine: 0 },
        ];
        let meteo = vec![
            LegacyMeteoRow { ts: ts(9, 1, 0), metric_id: 5, v: 12.0, v_min: None, v_max: None, unit: Some("Kn".to_string()) },
            LegacyMeteoRow { ts: ts(9, 1, 0), metric_id: 6, v: 213.34384954985424, v_min: None, v_max: None, unit: Some("Deg".to_string()) },
        ];

        let (json_str, stats) = build_trip_json(&trip, &track, &meteo);
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(stats.track_rows, 2);
        assert_eq!(stats.meteo_rows, 2);
        assert_eq!(stats.wind_angle_hits, 1);

        assert!(json["trip"]["uuid"].as_str().is_some());
        assert!(uuid::Uuid::parse_str(json["trip"]["uuid"].as_str().unwrap()).is_ok());
        assert_eq!(json["trip"]["desc"], "Test Trip");
        assert_eq!(json["trip"]["dist_sail"], 0.0);
        assert_eq!(json["trip"]["t_sail"], 0);

        let second_row = &json["vs"][1];
        assert!(second_row["cog"].as_f64().is_some());
        assert_eq!(second_row["cog"], second_row["hdg"]);
        assert!(second_row["tws"].as_f64().unwrap() > 11.9);
        assert!(second_row["twa"].as_f64().is_some());

        let first_row = &json["vs"][0];
        assert!(first_row["cog"].is_null()); // no previous position yet
    }

    #[test]
    fn test_build_trip_json_moored_row_has_null_heading_and_wind_angle() {
        let trip = sample_trip();
        let track = vec![
            LegacyTrackRow { ts: ts(9, 0, 0), lat: 43.0, lon: 10.0, anchor: 0, d_time: None, dist: Some(0.0), speed: 0.0, max_speed: 0.0, engine: 0 },
            LegacyTrackRow { ts: ts(9, 1, 0), lat: 43.0, lon: 10.0, anchor: 1, d_time: Some(60), dist: Some(0.0), speed: 0.0, max_speed: 0.0, engine: 0 },
        ];
        let meteo = vec![
            LegacyMeteoRow { ts: ts(9, 1, 0), metric_id: 5, v: 8.0, v_min: None, v_max: None, unit: Some("Kn".to_string()) },
            LegacyMeteoRow { ts: ts(9, 1, 0), metric_id: 6, v: 200.0, v_min: None, v_max: None, unit: Some("Deg".to_string()) },
        ];

        let (json_str, stats) = build_trip_json(&trip, &track, &meteo);
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(stats.wind_angle_hits, 0);
        let moored_row = &json["vs"][1];
        assert_eq!(moored_row["moor"], true);
        assert!(moored_row["cog"].is_null());
        assert!(moored_row["hdg"].is_null());
        assert!(moored_row["twa"].is_null());
        // Wind speed has no heading dependency, so it still joins while moored.
        assert!(moored_row["tws"].as_f64().is_some());
    }

    #[test]
    fn test_build_trip_json_missing_description_falls_back() {
        let mut trip = sample_trip();
        trip.description = None;
        let (json_str, _) = build_trip_json(&trip, &[], &[]);
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(json["trip"]["desc"], "Trip 42");
    }

    #[test]
    fn test_build_trip_json_all_meteo_rows_carried_to_environmental_data() {
        let trip = sample_trip();
        let meteo = vec![
            LegacyMeteoRow { ts: ts(9, 0, 0), metric_id: 1, v: 101325.0, v_min: Some(101300.0), v_max: Some(101350.0), unit: Some("Pa".to_string()) },
            LegacyMeteoRow { ts: ts(9, 0, 30), metric_id: 7, v: 3.2, v_min: None, v_max: None, unit: Some("Deg".to_string()) },
        ];
        let (json_str, stats) = build_trip_json(&trip, &[], &meteo);
        let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(stats.meteo_rows, 2);
        assert_eq!(json["em"].as_array().unwrap().len(), 2);
        assert_eq!(json["em"][0]["mid"], 1);
        assert_eq!(json["em"][1]["mid"], 7);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: temporarily add `#[path = "legacy_import/mod.rs"] mod legacy_import;` to `src/main.rs` again (same technique as Task 1 Step 5), then `cargo test --bin nmea_router legacy_import::transform -- --nocapture`
Expected: FAIL with "not implemented" panics from `build_trip_json`.

- [ ] **Step 3: Implement `build_trip_json`**

```rust
pub fn build_trip_json(
    trip: &LegacyTrip,
    track_rows: &[LegacyTrackRow],
    meteo_rows: &[LegacyMeteoRow],
) -> (String, TransformStats) {
    let desc = trip
        .description
        .clone()
        .unwrap_or_else(|| format!("Trip {}", trip.id));
    let uuid = uuid::Uuid::new_v4().to_string();

    let mut tws_readings: Vec<Reading> = meteo_rows
        .iter()
        .filter(|m| m.metric_id == 5)
        .map(|m| Reading { ts: m.ts, value: m.v })
        .collect();
    tws_readings.sort_by_key(|r| r.ts);

    let mut twd_readings: Vec<Reading> = meteo_rows
        .iter()
        .filter(|m| m.metric_id == 6)
        .map(|m| Reading { ts: m.ts, value: m.v })
        .collect();
    twd_readings.sort_by_key(|r| r.ts);

    let mut vs = Vec::with_capacity(track_rows.len());
    let mut prev_pos: Option<(f64, f64)> = None;
    let mut wind_angle_hits = 0usize;

    for row in track_rows {
        let is_moored = row.anchor != 0;
        let bound_secs = row.d_time.unwrap_or(60).max(60);
        let (cog, hdg) = derive_cog_heading(prev_pos, (row.lat, row.lon), is_moored);
        let tws = nearest_reading(&tws_readings, row.ts, bound_secs);
        let twd = nearest_reading(&twd_readings, row.ts, bound_secs);
        let twa = derive_wind_angle(twd, cog);
        if twa.is_some() {
            wind_angle_hits += 1;
        }

        vs.push(serde_json::json!({
            "ts": rome_local_to_utc(row.ts).to_rfc3339_opts(SecondsFormat::Millis, true),
            "lat": row.lat,
            "lon": row.lon,
            "sog": row.speed,
            "sog_max": row.max_speed,
            "moor": is_moored,
            "eng": row.engine,
            "dist": row.dist.unwrap_or(0.0),
            "dur": (row.d_time.unwrap_or(0).max(0) as u64) * 1000,
            "tws": tws,
            "twa": twa,
            "cog": cog,
            "hdg": hdg,
        }));

        prev_pos = Some((row.lat, row.lon));
    }

    let em: Vec<serde_json::Value> = meteo_rows
        .iter()
        .map(|m| {
            serde_json::json!({
                "ts": rome_local_to_utc(m.ts).to_rfc3339_opts(SecondsFormat::Millis, true),
                "mid": m.metric_id,
                "avg": m.v,
                "max": m.v_max,
                "min": m.v_min,
                "unit": m.unit,
            })
        })
        .collect();

    let payload = serde_json::json!({
        "trip": {
            "desc": desc,
            "start": rome_local_to_utc(trip.from_ts).to_rfc3339_opts(SecondsFormat::Millis, true),
            "end": rome_local_to_utc(trip.to_ts).to_rfc3339_opts(SecondsFormat::Millis, true),
            "dist_sail": 0.0,
            "dist_motor": 0.0,
            "t_sail": 0,
            "t_motor": 0,
            "t_moor": 0,
            "uuid": uuid,
        },
        "vs": vs,
        "em": em,
    });

    let stats = TransformStats {
        track_rows: track_rows.len(),
        meteo_rows: meteo_rows.len(),
        wind_angle_hits,
    };

    (payload.to_string(), stats)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --bin nmea_router legacy_import::transform -- --nocapture`
Expected: all 4 tests PASS. Then remove the temporary `main.rs` include again (same as Task 1 Step 7).

- [ ] **Step 5: Commit**

```bash
git add src/legacy_import/transform.rs
git commit -m "Add legacy trip import: JSON transform"
```

---

### Task 3: Legacy database source queries

**Files:**
- Create: `src/legacy_import/source.rs`
- Modify: `src/legacy_import/mod.rs` (already declares `pub mod source;` from Task 1 — no change needed)

**Interfaces:**
- Consumes: `LegacyTrip`, `LegacyTrackRow`, `LegacyMeteoRow` (Task 1's `mod.rs`)
- Produces (used by Task 4):
  - `pub const LEGACY_CUTOVER_LOCAL: &str = "2020-01-03 07:43:03"`
  - `pub fn legacy_pool(host: &str, port: u16, user: &str, password: &str, database: &str) -> Result<mysql::Pool, crate::error::AppError>`
  - `pub fn fetch_legacy_trips(pool: &mysql::Pool) -> Result<Vec<LegacyTrip>, crate::error::AppError>`
  - `pub fn fetch_track_rows(pool: &mysql::Pool, trip: &LegacyTrip) -> Result<Vec<LegacyTrackRow>, crate::error::AppError>`
  - `pub fn fetch_meteo_rows(pool: &mysql::Pool, trip: &LegacyTrip) -> Result<Vec<LegacyMeteoRow>, crate::error::AppError>`

This task needs the live `nmearouter` database (credentials `nmearouter`/`nmearouter` on `localhost:3306`) — its tests are `#[ignore]`, matching this project's convention for DB-backed tests (see `CLAUDE.md`/`DB_ANALYST.md`).

- [ ] **Step 1: Write the failing (ignored) tests**

Create `src/legacy_import/source.rs`:

**Important:** this project's `mysql` crate build has no `chrono` feature
enabled — `mysql_common`'s locked dependencies pull in the `time` crate, not
`chrono` (confirmed: `sync.rs`/`import_export.rs` always pass pre-formatted
date **strings** into `params!`, and read timestamps back by destructuring
`mysql::Value::Date(...)` manually — e.g. `mysql_datetime_to_iso` in
`import_export.rs` — never a typed `chrono::NaiveDateTime`/`SystemTime`).
So every DATETIME column below is read as a raw `mysql::Value` and converted
with a local helper, and every DATETIME bound into a query is first
formatted to a `"%Y-%m-%d %H:%M:%S"` string — matching that existing
convention exactly.

```rust
use super::{LegacyMeteoRow, LegacyTrackRow, LegacyTrip};
use crate::error::AppError;
use chrono::{NaiveDate, NaiveDateTime};
use mysql::prelude::Queryable;
use mysql::{params, Pool};

pub const LEGACY_CUTOVER_LOCAL: &str = "2020-01-03 07:43:03";

pub fn legacy_pool(host: &str, port: u16, user: &str, password: &str, database: &str) -> Result<Pool, AppError> {
    unimplemented!()
}

fn mysql_value_to_naive_datetime(v: mysql::Value) -> Result<NaiveDateTime, AppError> {
    unimplemented!()
}

fn decimal_to_f64(v: mysql::Value) -> Result<f64, AppError> {
    unimplemented!()
}

pub fn fetch_legacy_trips(pool: &Pool) -> Result<Vec<LegacyTrip>, AppError> {
    unimplemented!()
}

pub fn fetch_track_rows(pool: &Pool, trip: &LegacyTrip) -> Result<Vec<LegacyTrackRow>, AppError> {
    unimplemented!()
}

pub fn fetch_meteo_rows(pool: &Pool, trip: &LegacyTrip) -> Result<Vec<LegacyMeteoRow>, AppError> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn test_pool() -> Pool {
        legacy_pool("localhost", 3306, "nmearouter", "nmearouter", "nmearouter").unwrap()
    }

    #[test]
    #[ignore] // Requires the live `nmearouter` MariaDB instance.
    fn test_fetch_legacy_trips_returns_106_trips_before_cutover() {
        let pool = test_pool();
        let trips = fetch_legacy_trips(&pool).unwrap();
        assert_eq!(trips.len(), 106);
        let cutover = NaiveDate::from_ymd_opt(2020, 1, 3).unwrap().and_hms_opt(7, 43, 3).unwrap();
        assert!(trips.iter().all(|t| t.to_ts <= cutover));
    }

    #[test]
    #[ignore]
    fn test_fetch_track_and_meteo_rows_for_first_legacy_trip() {
        let pool = test_pool();
        let trips = fetch_legacy_trips(&pool).unwrap();
        let first = trips.iter().find(|t| t.id == 14).expect("trip 14 should exist");
        assert_eq!(first.description.as_deref(), Some("Agosto 2016 - Isole"));

        let track = fetch_track_rows(&pool, first).unwrap();
        let meteo = fetch_meteo_rows(&pool, first).unwrap();
        assert!(!track.is_empty());
        assert!(!meteo.is_empty());
        // Trip 14 starts at the very first-ever legacy track row.
        assert_eq!(track.first().unwrap().ts, first.from_ts);
        assert!(meteo.iter().all(|m| (1..=7).contains(&m.metric_id)));
    }

    #[test]
    #[ignore]
    fn test_decimal_lat_lon_parse_as_plausible_coordinates() {
        let pool = test_pool();
        let trips = fetch_legacy_trips(&pool).unwrap();
        let first = trips.iter().find(|t| t.id == 14).unwrap();
        let track = fetch_track_rows(&pool, first).unwrap();
        // Boat lives in the Tyrrhenian Sea; sanity-bound the coordinates.
        assert!(track.iter().all(|r| (35.0..47.0).contains(&r.lat) && (5.0..20.0).contains(&r.lon)));
    }
}
```

- [ ] **Step 2: Confirm the tests are structurally ready (they'll stay `#[ignore]`d by default)**

Run: `cargo test --bin nmea_router legacy_import::source -- --list` (with the same temporary `main.rs` include as Task 1/2)
Expected: the 3 tests are listed, none run (all `#[ignore]`d) — this confirms the module compiles once implemented in Step 3.

- [ ] **Step 3: Implement `source.rs`**

```rust
pub fn legacy_pool(host: &str, port: u16, user: &str, password: &str, database: &str) -> Result<Pool, AppError> {
    let url = format!("mysql://{}:{}@{}:{}/{}", user, password, host, port, database);
    Pool::new(url.as_str()).map_err(|e| AppError::Database(e.to_string()))
}

fn mysql_value_to_naive_datetime(v: mysql::Value) -> Result<NaiveDateTime, AppError> {
    match v {
        mysql::Value::Date(y, mo, d, h, mi, s, _us) => NaiveDate::from_ymd_opt(y as i32, mo as u32, d as u32)
            .and_then(|nd| nd.and_hms_opt(h as u32, mi as u32, s as u32))
            .ok_or_else(|| AppError::Parse(format!(
                "Invalid legacy datetime: {:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, mo, d, h, mi, s
            ))),
        other => Err(AppError::Database(format!("Expected DATETIME value, got: {:?}", other))),
    }
}

fn decimal_to_f64(v: mysql::Value) -> Result<f64, AppError> {
    match v {
        mysql::Value::Bytes(b) => String::from_utf8(b)
            .map_err(|e| AppError::Parse(e.to_string()))?
            .parse::<f64>()
            .map_err(|e| AppError::Parse(e.to_string())),
        mysql::Value::Double(d) => Ok(d),
        mysql::Value::Float(f) => Ok(f as f64),
        mysql::Value::Int(i) => Ok(i as f64),
        mysql::Value::UInt(u) => Ok(u as f64),
        other => Err(AppError::Database(format!("Unexpected numeric value: {:?}", other))),
    }
}

pub fn fetch_legacy_trips(pool: &Pool) -> Result<Vec<LegacyTrip>, AppError> {
    let mut conn = pool.get_conn()?;
    let rows: Vec<(i64, Option<String>, mysql::Value, mysql::Value)> = conn.exec(
        "SELECT id, description, fromTS, toTS FROM trip WHERE toTS <= :cutover ORDER BY fromTS ASC",
        params! { "cutover" => LEGACY_CUTOVER_LOCAL },
    )?;
    rows.into_iter()
        .map(|(id, description, from_ts, to_ts)| {
            Ok(LegacyTrip {
                id,
                description,
                from_ts: mysql_value_to_naive_datetime(from_ts)?,
                to_ts: mysql_value_to_naive_datetime(to_ts)?,
            })
        })
        .collect()
}

pub fn fetch_track_rows(pool: &Pool, trip: &LegacyTrip) -> Result<Vec<LegacyTrackRow>, AppError> {
    let mut conn = pool.get_conn()?;
    let from_str = trip.from_ts.format("%Y-%m-%d %H:%M:%S").to_string();
    let to_str = trip.to_ts.format("%Y-%m-%d %H:%M:%S").to_string();
    let rows: Vec<(mysql::Value, mysql::Value, mysql::Value, i32, Option<i64>, mysql::Value, f64, f64, u8)> = conn.exec(
        "SELECT TS, lat, lon, anchor, dTime, dist, speed, maxSpeed, engine
         FROM track WHERE TS BETWEEN :from_ts AND :to_ts ORDER BY TS ASC",
        params! { "from_ts" => &from_str, "to_ts" => &to_str },
    )?;
    rows.into_iter()
        .map(|(ts, lat, lon, anchor, d_time, dist, speed, max_speed, engine)| {
            let dist = match dist {
                mysql::Value::NULL => None,
                v => Some(decimal_to_f64(v)?),
            };
            Ok(LegacyTrackRow {
                ts: mysql_value_to_naive_datetime(ts)?,
                lat: decimal_to_f64(lat)?,
                lon: decimal_to_f64(lon)?,
                anchor,
                d_time,
                dist,
                speed,
                max_speed,
                engine,
            })
        })
        .collect()
}

pub fn fetch_meteo_rows(pool: &Pool, trip: &LegacyTrip) -> Result<Vec<LegacyMeteoRow>, AppError> {
    let mut conn = pool.get_conn()?;
    let from_str = trip.from_ts.format("%Y-%m-%d %H:%M:%S").to_string();
    let to_str = trip.to_ts.format("%Y-%m-%d %H:%M:%S").to_string();
    let rows: Vec<(mysql::Value, Option<u8>, f64, Option<f64>, Option<f64>, Option<String>)> = conn.exec(
        "SELECT TS, metric_id, v, vMin, vMax, unit FROM meteo
         WHERE TS BETWEEN :from_ts AND :to_ts AND metric_id IS NOT NULL ORDER BY TS ASC",
        params! { "from_ts" => &from_str, "to_ts" => &to_str },
    )?;
    rows.into_iter()
        .map(|(ts, metric_id, v, v_min, v_max, unit)| {
            let ts = mysql_value_to_naive_datetime(ts)?;
            Ok(metric_id.map(|mid| LegacyMeteoRow { ts, metric_id: mid, v, v_min, v_max, unit }))
        })
        .collect::<Result<Vec<Option<LegacyMeteoRow>>, AppError>>()
        .map(|v| v.into_iter().flatten().collect())
}
```

- [ ] **Step 4: Run the ignored tests against the live legacy database to confirm they pass**

Run: `cargo test --bin nmea_router legacy_import::source -- --ignored --nocapture` (with the temporary `main.rs` include still in place)
Expected: all 3 tests PASS against the real `nmearouter` database. Then remove the temporary `main.rs` include (same as previous tasks) — Task 4 provides the permanent wiring.

- [ ] **Step 5: Commit**

```bash
git add src/legacy_import/source.rs
git commit -m "Add legacy trip import: source queries against nmearouter"
```

---

### Task 4: CLI binary

**Files:**
- Create: `src/bin/import_legacy_trips.rs`
- Test: inline `#[cfg(test)] mod tests` in `src/bin/import_legacy_trips.rs` (for `parse_args` only — the rest is DB-orchestration, verified manually via `--dry-run` in Task 5)

**Interfaces:**
- Consumes: everything from Task 1-3 (`legacy_import::{source, transform}`, `db::VesselDatabase::import_trip`, `config::Config`)

- [ ] **Step 1: Write the failing test for argument parsing**

Create `src/bin/import_legacy_trips.rs`:

```rust
// Imports the 106 pre-2020 legacy trips from `nmearouter` (old trip/track/meteo
// tables) into this project's live database, via VesselDatabase::import_trip.
//
// Usage:
//   import_legacy_trips [--dry-run] [--config <path>]
//                        [--legacy-host H] [--legacy-port P] [--legacy-user U]
//                        [--legacy-password P] [--legacy-database D]
//
// See docs/superpowers/specs/2026-09-04-legacy-trip-import-design.md.
#[path = "../utilities.rs"]
pub mod utilities;

#[path = "../position_utils.rs"]
pub mod position_utils;

#[path = "../config.rs"]
pub mod config;

#[path = "../trip.rs"]
pub mod trip;

#[path = "../environmental_monitor.rs"]
pub mod environmental_monitor;

#[path = "../db/mod.rs"]
pub mod db;

#[path = "../legacy_import/mod.rs"]
pub mod legacy_import;

#[path = "../error.rs"]
pub mod error;

use config::Config;
use db::VesselDatabase;
use legacy_import::{source, transform};
use tracing::info;

#[derive(Debug, PartialEq)]
struct CliArgs {
    dry_run: bool,
    config_path: Option<String>,
    legacy_host: String,
    legacy_port: u16,
    legacy_user: String,
    legacy_password: String,
    legacy_database: String,
}

fn parse_args(args: &[String]) -> Result<CliArgs, error::AppError> {
    unimplemented!()
}

fn main() {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[test]
    fn test_parse_args_defaults() {
        let args = parse_args(&[s("--dry-run")]).unwrap();
        assert_eq!(
            args,
            CliArgs {
                dry_run: true,
                config_path: None,
                legacy_host: s("localhost"),
                legacy_port: 3306,
                legacy_user: s("nmearouter"),
                legacy_password: s("nmearouter"),
                legacy_database: s("nmearouter"),
            }
        );
    }

    #[test]
    fn test_parse_args_overrides() {
        let args = parse_args(&[
            s("--config"), s("/tmp/config.json"),
            s("--legacy-host"), s("192.168.1.5"),
            s("--legacy-port"), s("3307"),
            s("--legacy-user"), s("bob"),
            s("--legacy-password"), s("secret"),
            s("--legacy-database"), s("old_boat"),
        ]).unwrap();
        assert_eq!(args.dry_run, false);
        assert_eq!(args.config_path, Some(s("/tmp/config.json")));
        assert_eq!(args.legacy_host, "192.168.1.5");
        assert_eq!(args.legacy_port, 3307);
        assert_eq!(args.legacy_user, "bob");
        assert_eq!(args.legacy_password, "secret");
        assert_eq!(args.legacy_database, "old_boat");
    }

    #[test]
    fn test_parse_args_unknown_flag_errors() {
        assert!(parse_args(&[s("--nonsense")]).is_err());
    }

    #[test]
    fn test_parse_args_missing_value_errors() {
        assert!(parse_args(&[s("--legacy-host")]).is_err());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --bin import_legacy_trips -- --nocapture`
Expected: FAIL — `parse_args` panics with "not implemented", and `main` won't be exercised by tests.

- [ ] **Step 3: Implement `parse_args` and `main`**

```rust
fn parse_args(args: &[String]) -> Result<CliArgs, error::AppError> {
    let mut dry_run = false;
    let mut config_path: Option<String> = None;
    let mut legacy_host = "localhost".to_string();
    let mut legacy_port: u16 = 3306;
    let mut legacy_user = "nmearouter".to_string();
    let mut legacy_password = "nmearouter".to_string();
    let mut legacy_database = "nmearouter".to_string();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dry-run" => dry_run = true,
            "--config" => {
                i += 1;
                config_path = Some(args.get(i).ok_or_else(|| error::AppError::Configuration("--config requires a value".to_string()))?.clone());
            }
            "--legacy-host" => {
                i += 1;
                legacy_host = args.get(i).ok_or_else(|| error::AppError::Configuration("--legacy-host requires a value".to_string()))?.clone();
            }
            "--legacy-port" => {
                i += 1;
                legacy_port = args
                    .get(i)
                    .ok_or_else(|| error::AppError::Configuration("--legacy-port requires a value".to_string()))?
                    .parse()
                    .map_err(|_| error::AppError::Configuration("Invalid --legacy-port".to_string()))?;
            }
            "--legacy-user" => {
                i += 1;
                legacy_user = args.get(i).ok_or_else(|| error::AppError::Configuration("--legacy-user requires a value".to_string()))?.clone();
            }
            "--legacy-password" => {
                i += 1;
                legacy_password = args.get(i).ok_or_else(|| error::AppError::Configuration("--legacy-password requires a value".to_string()))?.clone();
            }
            "--legacy-database" => {
                i += 1;
                legacy_database = args.get(i).ok_or_else(|| error::AppError::Configuration("--legacy-database requires a value".to_string()))?.clone();
            }
            other => return Err(error::AppError::Configuration(format!("Unknown argument: {}", other))),
        }
        i += 1;
    }

    Ok(CliArgs {
        dry_run,
        config_path,
        legacy_host,
        legacy_port,
        legacy_user,
        legacy_password,
        legacy_database,
    })
}

fn main() {
    tracing_subscriber::fmt().with_max_level(tracing::Level::INFO).init();

    let cli_args: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&cli_args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {}", e);
            eprintln!();
            eprintln!("Usage:");
            eprintln!("  import_legacy_trips [--dry-run] [--config <path>] [--legacy-host H] [--legacy-port P] [--legacy-user U] [--legacy-password P] [--legacy-database D]");
            std::process::exit(1);
        }
    };

    let config = match &args.config_path {
        Some(path) => {
            let content = std::fs::read_to_string(path).unwrap_or_else(|e| {
                eprintln!("Cannot read config file '{}': {}", path, e);
                std::process::exit(1);
            });
            serde_json::from_str::<Config>(&content).unwrap_or_else(|e| {
                eprintln!("Cannot parse config file '{}': {}", path, e);
                std::process::exit(1);
            })
        }
        None => Config::load_for_context(None).unwrap_or_else(|e| {
            eprintln!("Cannot load configuration: {}", e);
            std::process::exit(1);
        }),
    };

    let conn_url = format!(
        "mysql://{}:{}@{}:{}/{}",
        config.database.connection.username,
        config.database.connection.password,
        config.database.connection.host,
        config.database.connection.port,
        config.database.connection.database_name,
    );
    let target_db = VesselDatabase::new(&conn_url, config.database.connection.pool_min, config.database.connection.pool_max)
        .unwrap_or_else(|e| {
            eprintln!("Cannot connect to target database: {}", e);
            std::process::exit(1);
        });

    let legacy_pool = source::legacy_pool(&args.legacy_host, args.legacy_port, &args.legacy_user, &args.legacy_password, &args.legacy_database)
        .unwrap_or_else(|e| {
            eprintln!("Cannot connect to legacy database: {}", e);
            std::process::exit(1);
        });

    let trips = source::fetch_legacy_trips(&legacy_pool).unwrap_or_else(|e| {
        eprintln!("Cannot fetch legacy trips: {}", e);
        std::process::exit(1);
    });

    info!("Found {} legacy trips to import (cutover: {})", trips.len(), source::LEGACY_CUTOVER_LOCAL);
    if args.dry_run {
        info!("DRY-RUN mode: no changes will be made to the target database");
    }

    let mut imported = 0usize;
    let mut failed = 0usize;
    let mut skipped_empty = 0usize;

    for trip in &trips {
        let track_rows = match source::fetch_track_rows(&legacy_pool, trip) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Trip {}: failed to fetch track rows: {}", trip.id, e);
                failed += 1;
                continue;
            }
        };
        let meteo_rows = match source::fetch_meteo_rows(&legacy_pool, trip) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Trip {}: failed to fetch meteo rows: {}", trip.id, e);
                failed += 1;
                continue;
            }
        };

        if track_rows.is_empty() {
            eprintln!("Trip {} ({:?}): 0 track rows in window — skipping", trip.id, trip.description);
            skipped_empty += 1;
            continue;
        }

        let (json, stats) = transform::build_trip_json(trip, &track_rows, &meteo_rows);
        let wind_hit_pct = if stats.track_rows > 0 {
            100.0 * stats.wind_angle_hits as f64 / stats.track_rows as f64
        } else {
            0.0
        };
        info!(
            "Trip {} ({:?}): {} track rows, {} meteo rows, wind-angle join {:.0}%",
            trip.id, trip.description, stats.track_rows, stats.meteo_rows, wind_hit_pct
        );

        if args.dry_run {
            continue;
        }

        match target_db.import_trip(&json) {
            Ok(new_id) => {
                info!("Trip {} imported as new trip id {}", trip.id, new_id);
                imported += 1;
            }
            Err(e) => {
                eprintln!("Trip {} failed to import: {}", trip.id, e);
                failed += 1;
            }
        }
    }

    println!();
    println!("=== Legacy Import Report ===");
    println!("Mode            : {}", if args.dry_run { "DRY RUN" } else { "live" });
    println!("Legacy trips    : {}", trips.len());
    println!("Imported        : {}", imported);
    println!("Failed          : {}", failed);
    println!("Skipped (empty) : {}", skipped_empty);
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --bin import_legacy_trips -- --nocapture`
Expected: all 4 `parse_args` tests PASS.

- [ ] **Step 5: Build the whole workspace to confirm everything compiles together**

Run: `cargo build --release`
Expected: succeeds, producing `target/release/import_legacy_trips` alongside the other binaries.

- [ ] **Step 6: Commit**

```bash
git add src/bin/import_legacy_trips.rs
git commit -m "Add import_legacy_trips CLI binary"
```

---

### Task 5: Dry-run, backup, and live import (operational — requires explicit go-ahead)

This task has no code changes. It is the actual data migration, run against the real `nmea_router` production database — **do not run the live (non-`--dry-run`) step without the user explicitly confirming first**, per this project's standing rule about hard-to-reverse actions on shared/production state.

**Files:** none.

- [ ] **Step 1: Dry-run against the real databases**

Run: `./target/release/import_legacy_trips --dry-run`
Expected: a report line per trip (106 total) showing track/meteo row counts and wind-angle join percentage, ending in a summary with `Mode: DRY RUN`, `Imported: 0`. Review the per-trip counts for anything surprising (e.g. an unexpectedly low wind-join percentage, or a trip flagged "0 track rows").

- [ ] **Step 2: Back up the target database**

Per `DB_ANALYST.md`'s >1000-row rule (this run inserts ~983k rows):

```bash
mysqldump -u nmea -pnmea nmea_router > /tmp/nmea_router_pre_legacy_import_backup.sql
```

- [ ] **Step 3: Confirm with the user, then run live**

Only after the user has reviewed the dry-run report and explicitly says to proceed:

Run: `./target/release/import_legacy_trips`
Expected: summary report with `Mode: live`, `Imported: 106`, `Failed: 0`, `Skipped (empty): 0`.

- [ ] **Step 4: Verify**

```sql
-- Row count sanity check: should now be 106 more trips than before the import.
SELECT COUNT(*) FROM trips WHERE start_timestamp < '2020-01-03 06:43:03';

-- Spot-check one trip's total distance against the old system's own total
-- (informational bound, not a source of truth — see design doc).
SELECT description, total_distance_sailed + total_distance_motoring AS total_nm
FROM trips WHERE description = 'Agosto 2016 - Isole';
-- Compare against nmearouter.trip.dist for id=14 (~309.9 nm).
```

---

## Self-Review

**Spec coverage:**
- Cutover boundary (2020-01-03 07:43:03 local, 106 trips) → `Global Constraints`, Task 3 `LEGACY_CUTOVER_LOCAL`/`fetch_legacy_trips`. ✓
- Timezone conversion (DST-aware) → Task 1 `rome_local_to_utc`. ✓
- `track` → `vessel_status` field mapping → Task 2 `build_trip_json`. ✓
- COG/heading synthesis (underway vs. moored-NULL decision) → Task 1 `derive_cog_heading`, tested both branches. ✓
- Wind speed join (no heading dependency) and wind angle derivation (TWD − COG, heading-dependent, NULL moored) → Task 1 `nearest_reading`/`derive_wind_angle`, Task 2 tests both underway and moored cases. ✓
- `meteo` → `environmental_data` full passthrough (all 7 metric types) → Task 2 test `test_build_trip_json_all_meteo_rows_carried_to_environmental_data`. ✓
- Reuse of `import_trip` for insert + aggregate recompute + cache invalidation → Task 4 `main`. ✓
- Mandatory `uuid` to avoid the false-positive overlap check → `Global Constraints`, Task 2 test asserts `uuid` is present and valid. ✓
- Never trust `trip.dist*` → `Global Constraints`, Task 2 sends `0` placeholders, Task 3 never even queries those columns. ✓
- Backup + dry-run-first + verify protocol from `DB_ANALYST.md` → Task 5. ✓

**Placeholder scan:** no "TBD"/"TODO"/"handle appropriately" language; every code step has real code (the `unimplemented!()` bodies are intentional TDD red-phase stubs, replaced within the same task).

**Type consistency:** `LegacyTrip`/`LegacyTrackRow`/`LegacyMeteoRow` field names and types are identical everywhere they're used (Task 1 defines, Task 2/3 consume). `TransformStats` fields (`track_rows`, `meteo_rows`, `wind_angle_hits`) match between Task 2's definition and Task 4's usage. `CliArgs` fields match between Task 4's `parse_args` and its tests.
