# Sail-Preference Bias in Isochrone Routing — Design Spec

## Goal

Add a user-controlled parameter that biases the isochrone routing algorithm toward maximising sailing legs over motoring legs, at the cost of a potentially longer total journey time.

## Background

The isochrone algorithm currently optimises purely for time: at each step, the candidate that reaches farthest from the origin per sector wins. This means a motoring path and a sailing path covering the same distance are treated identically. A user who prefers to sail (for fuel economy, noise, or enjoyment) has no way to express that preference beyond the hard `min_sail_speed_kn` cutoff.

## Approach: Cumulative Sailed Hours as Pruning Score Bonus

Each `IsochronePoint` tracks `sailed_hours: f64` — the cumulative hours of sailing in its ancestry chain. Sector pruning scores candidates using:

```
score = dist_from_origin + sail_weight_kn × sailed_hours
```

`sail_weight_kn` is in nautical miles per hour. A value of 5 means "1 hour of sailing is worth 5 extra nm of pruning advantage." With `sail_weight_kn = 0` the behaviour is identical to the current implementation.

---

## Data Structures

### `IsochronePoint` (`src/routing.rs`)

Add one field:

```rust
struct IsochronePoint {
    lat: f64,
    lon: f64,
    time: DateTime<Utc>,
    sailed_hours: f64,   // cumulative hours under sail in this path's ancestry
    parent_idx: Option<usize>,
}
```

Seed point: `sailed_hours: 0.0`.

During candidate generation, for each new candidate:
- If the leg was sailed (polar speed passed efficiency and `min_sail_speed_kn` threshold): `sailed_hours = parent.sailed_hours + 1.0`
- If the leg was motored: `sailed_hours = parent.sailed_hours`

### `run_isochrone` signature

```rust
pub fn run_isochrone(
    from: (f64, f64),
    to: (f64, f64),
    departure: DateTime<Utc>,
    motoring_speed_kn: f64,
    polar_efficiency: f64,
    min_sail_speed_kn: f64,
    sail_weight_kn: f64,          // NEW — 0.0 = pure time optimisation
    polars: &crate::polars::PolarTable,
    fetches: &[FetchWithHourly],
) -> IsochroneResult
```

### `prune_isochrone` signature

```rust
fn prune_isochrone(
    candidates: Vec<IsochronePoint>,
    origin: (f64, f64),
    sail_weight_kn: f64,          // NEW
) -> Vec<IsochronePoint>
```

Score inside:
```rust
let score = dist + sail_weight_kn * pt.sailed_hours;
if score > sector_score[sector] {
    sector_score[sector] = score;
    sectors[sector] = Some(pt);
}
```

---

## API

### `OptimalRouteQuery` (`src/web/api.rs`)

```rust
#[serde(default)]
pub sail_weight_kn: f64,
```

Default `0.0` — backward compatible; existing callers without the parameter behave identically to before.

Pass through to `run_isochrone`.

---

## UI (`static/plan.html`)

Add a "Sail preference" input to the Optimize Route panel:

```html
<label>Sail preference</label>
<input type="number" id="sailWeightInput" value="0" min="0" max="10" step="0.5"> kn
```

- Range: 0–10 kn, step 0.5, default 0
- Persisted to `localStorage` key `plan_sail_weight`
- Sent in `optimizeRoute()` URL as `&sail_weight_kn=${sailWeight}`

Label tooltip / help text: "Higher values favour routes with more sailing, at the cost of journey time. 0 = fastest route."

---

## Testing (`src/routing.rs`)

### Existing tests

Update the three existing `run_isochrone` call sites to pass `sail_weight_kn: 0.0` — behaviour must be unchanged.

### New tests

**`test_sail_weight_zero_matches_unbiased`**
Run the same scenario twice — once with `sail_weight_kn = 0.0`, once with the old pruning logic (distance only). Assert the winning sector candidates are identical. Verifies the new scoring path is a strict superset of the old one when weight is zero.

**`test_sail_weight_prefers_sailing_candidate`**
Construct a sector scenario where a motoring candidate reaches 10 nm from origin and a sailing candidate reaches 9 nm from origin. With `sail_weight_kn = 0`, the motoring candidate wins (it's farther). With `sail_weight_kn = 2.0` and the sailing candidate carrying `sailed_hours = 1.0`, its score is `9 + 2 × 1 = 11 > 10`, so the sailing candidate wins.

---

## Constraints

- `sail_weight_kn` is clamped to `[0.0, ∞)` — negative values are rejected with HTTP 400.
- Very high values (> 20) may cause the optimizer to chase sailing routes that add many hours to the journey; no hard cap in the backend, but the UI limits the slider to 10 kn.
- `sailed_hours` is not surfaced in the API response (not needed for display).
