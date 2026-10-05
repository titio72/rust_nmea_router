# Min TWA: Exclude Heading Instead Of Motoring — Correction Design

## Purpose

Corrects a misunderstanding in the just-shipped minimum true wind angle (TWA) constraint
([2026-07-06-min-twa-constraint-design.md](2026-07-06-min-twa-constraint-design.md)). The
intended feature is: **choose a route that avoids sailing too close to the wind, even if that
route is longer — not switch the engine on because of a headwind.** The current implementation in
`run_isochrone` does the opposite of the second half of that: whenever a candidate heading's TWA
is tighter than `min_twa_deg`, it substitutes `(motoring_speed_kn, false)` as a valid candidate —
meaning the search can and does turn the engine on to push straight through a headwind, which is
exactly what this parameter was meant to prevent.

## Scope

Only `run_isochrone` (`src/routing.rs`) changes. Confirmed with the user: `generate_route_track`
(`src/forecast.rs`, the "Compute" button's fixed-waypoint simulator) keeps its current behavior —
its waypoints are fixed by the user with no alternate path to detour through, so motoring a
headwind leg remains the only sane simulation of that fixed course. No changes to the API query
structs/validation (`src/web/api.rs`) or the frontend (`static/plan.html`) — parameter name,
default (60°), and validated range `[0, 180]` are unchanged.

## Behavior change

In `run_isochrone`'s per-heading candidate loop (`src/routing.rs`, inside `for h in
0..SECTOR_COUNT`), change:

```rust
let twa = compute_twa(heading, wind_dir);
if twa < min_twa_deg {
    (motoring_speed_kn, false)
} else {
    match polars.boat_speed(twa, wind_spd).filter(|&s| s > 0.0) { ... }
}
```

to skip generating any candidate for that heading at all when wind is present and the heading is
too close to it:

```rust
let twa = compute_twa(heading, wind_dir);
if twa < min_twa_deg {
    continue;
}
match polars.boat_speed(twa, wind_spd).filter(|&s| s > 0.0) { ... }
```

`continue` skips to the next sampled heading in the inner loop — no `IsochronePoint` is pushed for
the excluded heading, so it contributes nothing to that step's frontier (no sailing candidate, no
motoring candidate). The search can only make progress in directions that are actually sailable at
or beyond `min_twa_deg`, forcing it to route around a headwind via a longer tacking path instead of
motoring through it.

The existing `_ => (motoring_speed_kn, false)` fallback (no forecast wind data at this point/time,
or wind speed ≤ 0) is unchanged — that's a genuinely different situation (nothing to sail on at
all), not a headwind exclusion, and motoring is still the only sane behavior there.

This also means: a "genuine arrival" candidate (the direct-bearing heading reaching the
destination within this step) is never available via an excluded heading, since the candidate is
never generated in the first place — consistent with never using the engine to punch through a
headwind, even for the final approach.

No changes to `MAX_STEPS`, `prune_isochrone`'s pruning/scoring, or stagnation detection.

## Test rework

The Task 1 test `test_min_twa_deg_forces_motor_below_threshold` was written against the old (now
known-wrong) semantics and asserted "no faster than pure motoring" — under the old logic that
happened to hold because the excluded heading became a motoring candidate whose speed tied the
baseline. That assertion doesn't distinguish old from new behavior clearly enough (both could
produce similar totals in that scenario), so it needs replacing with a test that makes the two
behaviors diverge sharply:

- Motoring is deliberately fast (5 kn).
- `min_twa_deg` is set high enough (80°) that the *only* sailable headings remaining have a poor
  VMG toward the destination (`6 kn × cos(80°) ≈ 1.04 kn`) — much slower than motoring.
- Under the old (wrong) behavior, the router would simply motor the direct heading at 5 kn,
  reaching in roughly `motoring_only_hours`.
- Under the new (correct) behavior, the excluded direct heading is never a candidate at all — the
  router is forced onto the slow, shallow-angle sailing headings, taking substantially longer than
  pure motoring would have.
- Assertion: `reached_destination` is still `true` (it does eventually get there), and
  `total_hours` is substantially greater than `motoring_only_hours` — proving the engine was never
  used to shortcut the headwind, even though motoring was objectively faster.

This replaces the old test; it does not add a second one alongside it.

## Testing

Same test module (`src/routing.rs`), same `run_isochrone` call signature (no parameter changes —
this is purely an internal gate-logic change, not a signature change). No other test call sites are
affected, since none of the 13 existing calls pass a non-zero `min_twa_deg`.
