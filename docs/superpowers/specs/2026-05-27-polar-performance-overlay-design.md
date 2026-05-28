# Polar Performance Overlay on Speed Chart

**Date:** 2026-05-27  
**Status:** Approved

## Summary

Add a polar performance ratio overlay to the existing speed chart on `trip.html`. For each track point that has true wind angle (TWA) and true wind speed (TWS), compute the expected polar speed from the boat's polar table and express actual speed as a percentage of it. This ratio is plotted as a dashed green line on a second Y-axis (right side, 0–150%) alongside the existing boat speed line (left side, knots).

## Data Source

Each `vessel_status` row (surfaced as a `TrackPoint` via `/api/track`) already carries:
- `average_speed_kn` — actual boat speed
- `average_wind_angle_deg` — true wind angle, 0–360°
- `average_wind_speed_kn` — true wind speed (knots)

The polar table (`PolarTable` in `src/polars.rs`) is already loaded at startup and stored in `AppState`. Its `boat_speed(twa_deg, tws_kn)` method takes TWA in 0–180° and returns expected speed in knots.

## Backend Changes

### `src/db/types.rs` — extend `TrackPoint`

Add two optional fields:

```rust
pub polar_speed_kn: Option<f64>,  // expected speed from polar table (knots)
pub polar_ratio: Option<f64>,     // actual / polar * 100 (percent)
```

Both fields are `null` when:
- No polar table is configured in `AppState`
- The point has no TWA, no TWS, or no actual speed
- The polar table returns `None` for the given TWA/TWS (e.g. TWA below minimum polar angle, zero wind)

### `src/web/api.rs` — `get_track` handler

After `state.db().fetch_track(...)` returns the `Vec<TrackPoint>`, post-process in the handler (not in the DB layer) so the polar table stays out of the database layer:

```rust
if let Some(polars) = state.polars() {
    for point in &mut track {
        if let (Some(tws), Some(twa_360), Some(actual)) = (
            point.average_wind_speed_kn,
            point.average_wind_angle_deg,
            point.avg_speed_kn,
        ) {
            let twa = twa_360.min(360.0 - twa_360); // map 0–360° to 0–180°
            if let Some(polar_spd) = polars.boat_speed(twa, tws) {
                point.polar_speed_kn = Some(polar_spd);
                point.polar_ratio = Some(actual / polar_spd * 100.0);
            }
        }
    }
}
```

`fetch_track` returns `Vec<TrackPoint>` (not a reference), so mutating after the call is straightforward.

## Frontend Changes

### `trip.html` — `createSpeedChart(trackData)`

The function currently builds a single Chart.js dataset for `avg_speed_kn`. Extend it as follows:

**1. Detect polar data availability**

```js
const polarPoints = validData.filter(p => p.polar_ratio != null && !p.moored && p.avg_speed_kn > 0.1);
const hasPolar = polarPoints.length > 0;
```

**2. If `hasPolar`, add a second Y-axis**

```js
scales: {
    y:  { position: 'left',  title: { text: 'Speed (kn)' } },
    y1: { position: 'right', min: 0, max: 150, title: { text: 'Polar %' }, grid: { drawOnChartArea: false } }
}
```

**3. Add two datasets when polar data is present**

- **100% reference line** — constant dataset on `y1`, all values 100, `borderColor: 'rgba(76,175,80,0.25)'`, `borderDash: [4,4]`, `pointRadius: 0`, `tension: 0`. Not shown in the legend.
- **Performance ratio** — dashed green dataset on `y1`: `borderColor: '#4caf50'`, `borderDash: [5,3]`, `pointRadius: 0`, `borderWidth: 1.5`.

When `hasPolar` is false, no second axis and no additional datasets are added — the chart renders exactly as it does today.

**4. Tooltip extension**

When `hasPolar`, add the ratio to the tooltip for each point:

```
Speed: 6.5 kn  ↑ NNE
Polar: 83%
```

## Edge Cases

| Case | Behaviour |
|------|-----------|
| No polar table configured | All `polar_ratio` fields are `null`; chart unchanged |
| Point has TWA or TWS null | `polar_ratio` is `null`; point omitted from ratio dataset |
| TWA < polar minimum (42°) | `polars.boat_speed()` returns `None`; `polar_ratio` is `null` |
| `polar_ratio` > 150% | Clipped visually by `y1` max; raw value stored and shown in tooltip |
| Moored points | Excluded from ratio dataset by the same `!p.moored && p.avg_speed_kn > 0.1` guard used for the speed dataset |

## Out of Scope

- Scatter plot of ratio vs TWA (could be a follow-up)
- Polar performance on any page other than `trip.html`
- Storing polar ratio in the database
- Configuring the polar table path via the UI
