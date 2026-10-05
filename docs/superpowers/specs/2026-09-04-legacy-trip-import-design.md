# Legacy Trip Import — Design

## Background

`nmearouter` (MariaDB, localhost, credentials `nmearouter`/`nmearouter`) is the
predecessor system to this project. Investigation confirmed:

- `nmearouter.vessel_status` and the live `nmea_router.vessel_status` are the
  **same data** from 2020-01-03 onward (identical per-year row counts through
  2025, and live `trips.id=2` starts at `2020-01-03T06:43:03Z`, matching
  `nmearouter`'s `2020-01-03 07:43:03` local time exactly). `nmearouter` stops
  being written on `2026-02-07`; live `nmea_router` picks up seamlessly from
  there. So `nmearouter` was cut over into what is now the production
  database at some point, and everything from 2020-01-03 onward is already
  present live — **it must not be re-imported.**
- The only genuinely missing data is the **pre-2020 legacy window**:
  `2016-08-14 09:52:52` (local) → `2020-01-03 07:43:03` (local), stored in
  `nmearouter`'s old-style tables (`trip`, `track`, `meteo`), which predate
  the `vessel_status`-shaped table.
- The boundary is clean: zero legacy `trip` rows straddle the cutover
  (`toTS <= cutover` for exactly 106 of the 231 legacy trips; the rest are
  already covered live).

## Source schema (`nmearouter`, legacy tables only)

```
trip  (106 rows in scope)
  id, description, fromTS, toTS (local time), dist, distSail, distMotor
  — distSail/distMotor NULL on 106 of 231 rows; not trustworthy as a source
    of truth for aggregates.

track (151,260 rows in scope)
  lat, lon, TS (local time), id, anchor (0/1), dTime (seconds since
  previous row — same semantics as vessel_status.total_time_ms),
  dist (nm, since previous row), speed (avg kn), maxSpeed (kn),
  engine (0/1/2, same encoding as engine_on)
  — no wind, COG, or heading columns.

meteo (831,715 rows in scope)
  type (3-char code), TS (local time), vMin, v (avg), vMax, metric_id (1-7,
  identical numbering to environmental_data.metric_id), unit
  — metric_id 5 = TWS (true wind speed, kn), 6 = TWD (true wind DIRECTION,
    compass-referenced — see below, this is NOT the same quantity as
    vessel_status.average_wind_angle_deg).
```

## Target: reuse `VesselDatabase::import_trip`

`src/db/operations/import_export.rs:286` already does everything hard:
UUID-based dedup, transactional insert of one trip + its `vessel_status` +
`environmental_data` rows, full aggregate recompute (including the
upwind/reaching/running point-of-sail split) via `recalculate_and_update_trip`,
and heatmap-cache invalidation. It is already unit-tested
(`test_import_trip_computes_point_of_sail`).

The import tool's only job is **ETL**: for each of the 106 legacy trips,
build the same JSON shape `export_trip_to_string` produces
(`{trip: {...}, vs: [...], em: [...]}`) from the legacy tables, then call
`import_trip()`.

## Field-by-field translation

### Timestamps
All legacy timestamps (`trip.fromTS/toTS`, `track.TS`, `meteo.TS`) are local
Europe/Rome time, not UTC. Convert with `chrono-tz` (DST-aware — a flat
offset breaks across the March/October transitions covered by 3.5 years of
data).

### `vessel_status` rows, one per `track` row
| Field | Source |
|---|---|
| `ts` | `track.TS`, converted to UTC |
| `lat` / `lon` | `track.lat` / `track.lon` |
| `sog` / `sog_max` | `track.speed` / `track.maxSpeed` (already kn) |
| `moor` | `track.anchor != 0` |
| `eng` | `track.engine` (already 0/1/2) |
| `dist` | `track.dist` (already nm) |
| `dur` | `track.dTime * 1000` (already "since previous row", matches `total_time_ms` semantics) |

### Course / heading — synthesized from consecutive positions
`track` carries no COG or heading. Confirmed via the 2020-2026 overlap
window that this boat has **never** had a real compass heading sensor:
`nmearouter.vessel_status.cog_deg == average_heading_deg` on every sampled
row. The project's own `gap_filler/synthesizer.rs` already implements this
exact synthesis for a different missing-data scenario — reuse the same
building block:

- **Underway rows** (`anchor = 0`): `cog_deg = haversine_heading(prev_point,
  this_point)` (existing `utilities::haversine_heading`, already used by
  `gap_filler`); `average_heading_deg = cog_deg`.
- **Moored rows** (`anchor = 1`): both `cog_deg` and `average_heading_deg` =
  `NULL`. (Decision: matches `nmearouter`'s own historical data exactly —
  every moored sample checked has `cog_deg = NULL`. This diverges from
  `gap_filler/synthesizer.rs`'s carry-forward-while-moored convention, which
  was rejected in favor of fidelity to what the historical data actually
  contains, per user decision 2026-09-04.)

### Wind — `average_wind_speed_kn` joined, `average_wind_angle_deg` derived
`meteo.TWD` (metric_id 6) is an **absolute**, compass-referenced True Wind
Direction — not the boat-relative TWA that `vessel_status.average_wind_angle_deg`
actually stores (confirmed against `calculate_true_wind` in
`utilities.rs:27`, which operates on boat-relative angles throughout).
Empirically verified across the overlap window, holding to 2-3 decimal
places on every sampled row:

```
TWD = (average_wind_angle_deg + cog_deg) mod 360
  ⇒ average_wind_angle_deg = (TWD − cog_deg) mod 360
```

- `average_wind_speed_kn`: nearest `meteo` TWS (metric_id 5) reading to the
  track row's timestamp, only if within the track row's own `dTime` window;
  otherwise `NULL`. No heading dependency — works moored or underway.
- `average_wind_angle_deg`: only computable where `cog_deg` is known, i.e.
  **underway rows only**; nearest TWD (metric_id 6) reading within the same
  bound, combined with the just-derived `cog_deg`. `NULL` while moored — an
  anchored boat swings freely, so a boat-relative wind angle isn't a
  meaningful quantity there anyway, consistent with `cog_deg` also being
  unknown.

### `environmental_data` rows, one per `meteo` row
All 7 metric types (not just wind) carry straight across — `metric_id`
numbering is already identical between `meteo` and `environmental_data`.
`ts` converted to UTC; `value_avg/max/min` = `v/vMax/vMin`; `unit` copied
as-is.

### Trip JSON
`desc` = `trip.description` (fallback `"Trip {id}"` if NULL); `start`/`end`
= converted UTC bounds; `uuid` freshly generated;
`dist_sail`/`dist_motor`/`t_sail`/`t_motor`/`t_moor` sent as `0` placeholders
— `import_trip` recomputes the real values from the inserted `vs[]` rows
whenever any exist, so the legacy `trip.dist*` columns (incomplete on 106 of
231 rows) are never trusted as a source of truth.

## Implementation shape

New standalone binary `src/bin/import_legacy_trips.rs`, following the
`gap_filler.rs` pattern (`#[path]` module includes, `Config::load_for_context`,
`--dry-run`, `--config`):

1. Two DB connections: a raw `mysql::Pool` (read-only) against `nmearouter`,
   and `VesselDatabase::new()` against the target `nmea_router`.
2. For each of the 106 legacy `trip` rows (`toTS <= 2020-01-03 07:43:03`
   local): build the ETL'd JSON per the mapping above, call
   `db.import_trip(&json)`.
3. `--dry-run`: report per-trip counts (track rows, meteo rows, wind-join
   hit rate) without writing.
4. Flag (don't silently insert) any trip whose window yields zero `track`
   rows.

## Safety / operational plan

- Take a `mysqldump` backup of `nmea_router` first (per `DB_ANALYST.md`'s
  >1000-row rule — this is ~983k rows across the run).
- Dry-run first, review the report.
- After a live run: verify `COUNT(*)` / `MIN`/`MAX(timestamp)` per newly
  imported trip against the legacy `track`/`meteo` window it was sourced
  from, and spot-check total distance against the old `trip.dist` value as a
  sanity bound (not a source of truth).

## Explicitly out of scope

- Re-importing anything from 2020-01-03 onward (already live).
- Trusting `trip.dist`/`distSail`/`distMotor` for aggregates.
- Any attempt to recover a real compass heading — none ever existed for this
  boat.
