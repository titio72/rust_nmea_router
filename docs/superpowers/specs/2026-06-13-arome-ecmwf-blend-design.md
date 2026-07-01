# AROME short-term + ECMWF long-term forecast blending

**Date:** 2026-06-13
**Status:** Approved (design)

## Goal

Enhance the forecast pipeline to use Météo-France **AROME** (high resolution,
~1.5 km via Open-Meteo) for the short term and keep **ECMWF IFS** (~9 km) for the
longer term. AROME provides finer-grained, more accurate wind near the coast for
the first ~48 hours; ECMWF continues to cover the full 7-day horizon.

## Key decisions

| Question | Decision |
|----------|----------|
| Time blend | **Hard cutoff at 48h** — AROME for hours 0–48, ECMWF beyond. |
| Grid density | **Native AROME grid** (~1.5 km) stored as its own points. |
| Coverage gap | **Fall back to ECMWF only** when AROME has no data for a point. |
| Provenance | **Add a `model` column** so each forecast value's source is known. |

### Why models are stored separately (not merged at storage time)

A single merged per-point hourly series is not possible because:

1. **Different grids** — native AROME points (~1.5 km) and ECMWF points (~9 km)
   sit at different lat/lon, so they cannot be stitched into one series keyed by
   a shared grid point.
2. **Waves are ECMWF-only** — AROME has no wave output; waves come from the
   ECMWF WAM marine model.

Therefore both models are stored side-by-side as separate, `model`-tagged
fetches, and the consumer selects between them **at read time**.

### Why the 48h cutoff needs no explicit time arithmetic

AROME is fetched with `forecast_days=2`, so only 0–48h hourly rows ever exist for
AROME grid points. The read-time selection rule is:

- **Wind / gust / CAPE:** if an AROME sample exists near the requested time *and*
  within interpolation range → use AROME; otherwise ECMWF.
- **Waves:** always ECMWF.

Beyond ~48h no AROME rows exist, so the system automatically falls back to ECMWF.
Outside AROME's geographic coverage, no AROME points fall within range → ECMWF.
Both the hard-cutoff and coverage-fallback behaviours emerge from this one rule.

## Components & changes

### 1. Schema (`schema.sql`)

Add a per-fetch model tag to `forecast_fetch`:

```sql
ALTER TABLE forecast_fetch
  ADD COLUMN model VARCHAR(16) NOT NULL DEFAULT 'ecmwf' AFTER area_id;
```

- `model` is per-fetch (a whole fetch is one model), not per-hour.
- Existing rows default to `'ecmwf'`.
- Update the `CREATE TABLE forecast_fetch` definition in `schema.sql` to include
  the column for fresh installs.

### 2. `src/forecast.rs`

- **New** `build_arome_bbox_url(lat_min, lat_max, lon_min, lon_max)`:
  - host `api.open-meteo.com/v1/forecast`
  - `models=meteofrance_arome_france_hd`
  - `hourly=wind_speed_10m,wind_direction_10m,wind_gusts_10m,cape`
  - `wind_speed_unit=kn`, `forecast_days=2`, `timezone=UTC`
  - **Caveat:** Open-Meteo exposes AROME France HD at ~1.5 km, the closest
    available to Météo-France's native 1.3 km grid.
- `FetchedForecast` gains a `model: String` field.
- `fetch_area_forecast` makes **three** calls:
  - AROME wind → `FetchedForecast { model: "arome", wave fields None, .. }`
  - ECMWF wind (`ecmwf_ifs`) + ECMWF marine (`ecmwf_wam`) merged as today →
    `FetchedForecast { model: "ecmwf", .. }`
  - AROME failure or empty response is **non-fatal** (logged via `warn!`); ECMWF
    is still fetched and stored. ECMWF failure remains fatal for the area (retry),
    matching current behaviour.
  - Returns the concatenation of ECMWF and AROME `FetchedForecast`s.
- **Model-aware interpolation** (`interpolate_idw`, `nearest_forecast_wind`,
  `compute_route_overlay`):
  - Wind family (`wind_speed_kn`, `wind_direction_deg`, `wind_gust_kn`,
    `cape_j_kg`): interpolate from AROME samples if any AROME sample is within
    range for the requested time; else from ECMWF samples.
  - Wave family (`wave_height_m`, `wave_period_s`, `wave_direction_deg`): always
    interpolate from ECMWF samples.
  - The interpolated result records which model supplied the wind.

### 3. `src/db/operations/forecast.rs`

- `insert_forecast` takes a new `model: &str` parameter; writes it to
  `forecast_fetch.model`.
- `FetchWithHourly` and `GridPointForecast` gain a `model: String` field.
- `fetch_forecast_fetches` selects the `model` column and populates it.
- `get_grid_points_at`: for the requested timestamp, return AROME grid points if
  any exist for that hour; otherwise return ECMWF points. This avoids rendering a
  dense AROME grid and a sparse ECMWF grid on top of each other. Each returned
  point carries its `model`.

### 4. Callers

- `src/forecast_poller.rs`: pass each fetch's own `model` into `insert_forecast`.
- `src/web/api.rs::refresh_forecast`: same.
- Poller cadence unchanged (3h); both models fetched together in one pass.

### 5. API / UI

- `model` is carried through `GridPointForecast` and `RouteOverlayPoint` JSON.
- Planning page (`static/`) gets a **small** model indicator/legend showing which
  model is driving the displayed wind. No full per-leg relabelling.

## Defaults / non-goals

- **No new config block.** Model names and horizons (AROME `forecast_days=2`,
  ECMWF `forecast_days=7`) are module constants, matching the existing style
  (`FETCH_INTERVAL_SECS`, model names already hardcoded in URL builders). Avoids
  `config.json` / `config.example.json` churn.
- **Poller schedule unchanged** — no separate AROME schedule.
- AROME is wind-only here; waves/CAPE behaviour for ECMWF is unchanged.

## Testing

- Unit: `build_arome_bbox_url` contains expected params (`meteofrance_arome_france_hd`,
  `forecast_days=2`, `wind_speed_unit=kn`).
- Unit: model-aware IDW —
  - AROME preferred when an AROME sample exists within window/range;
  - ECMWF fallback when the requested time is beyond AROME's range;
  - ECMWF fallback when no AROME point is within range (outside coverage);
  - waves always sourced from ECMWF even when AROME supplies the wind.
- DB integration (`#[ignore]`): `insert_forecast` with `model` round-trips through
  `fetch_forecast_fetches` / `get_grid_points_at`.

## Notes for implementation

- Project git rules: **do not commit**. The implementer writes code and stops.
- Distance/bearing via Haversine; angle averaging via `atan2(avg_sin, avg_cos)`
  — already used in `interpolate_idw`'s `angular_idw`.
- Speed in knots (`wind_speed_unit=kn`), as today.
