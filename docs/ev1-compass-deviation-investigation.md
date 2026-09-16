# EV-1 Compass Deviation Investigation

**Status:** likely root cause found (2026-09-15) — a loose spare foam panel
with embedded metal sheets in the aft cabin, near the EV-1 mount. Awaiting
confirmation from trips after the panel was removed/relocated.

## Background

The Raymarine EV-1 solid-state compass lost its linearization (in-motion
hard/soft-iron calibration) in April 2026. It was relinearized, but
`average_heading_deg` vs `cog_deg` in `vessel_status` kept showing a
materially different bias afterward than before, and the bias has continued
to shift across subsequent trips.

## Method

For underway samples (`is_moored = 0`, filtered to a minimum SOG so slow/
noisy readings don't dominate), compute the circular signed difference

```
diff = MOD(average_heading_deg - cog_deg + 540, 360) - 180
```

(the `+540`/`MOD 360` avoids MariaDB's negative-operand `MOD` behavior giving
values outside `[-180, 180)`). Aggregate `mean(diff)`, `mean(|diff|)`, and
`stddev(diff)` per trip, and break down by COG bucket (45° bins) or by
sequence-within-trip to look for heading-dependence vs. time-dependence.

**Known confound:** magnetic declination is not an issue —
`average_heading_deg` is already true heading. But real current/set produces
a COG-vs-heading divergence that varies with heading and position in the
*same sinusoidal shape* as genuine compass deviation. A heading-bucket
"deviation curve" alone cannot distinguish compass error from current —
cross-check against lat/lon and against repeatability at the same location
on different days before concluding either way.

## Trip-set timeline

| Set | Trips | Window | mean \|diff\| | mean signed diff | stddev | Note |
|---|---|---|---|---|---|---|
| A (pre-fault baseline) | 139, 142, 144, 148, 151 | Feb 27–Apr 9 2026 | 4.09° | +1.99° | 4.80° | SOG>3kn |
| — (unstable transition) | 152, 153 | May 3–7 2026 | 19.3° / 10.4° | — | — | excluded, right after relinearization |
| B (post, settled) | 156, 158, 161, 164, 165 | May 23–Jul 12 2026 | 6.04° | +4.45° | 6.15° | real shift from Set A |
| trip 174 | 174 | Jul 24–Aug 8 2026 | 4.02° | −1.14° | — | new cruising ground (Capraia/Corsica); north-heading bias (Set B's dominant anomaly) absent; new anomaly at 270°/300° |
| cross-location check | 165, 177, 178, 183 | Jul 9–Sep 9 2026 | 4.53° | −0.43° | 5.61° | n=26,865, SOG>4kn; close to Set A baseline — compass looked re-settled |
| last-3-trips check | 397, 398, 413 | Aug 15–Sep 13 2026 | 5.64° / 3.22° / **7.81°** | +0.67° / +1.16° / **+6.44°** | 6.12° / 3.97° / 6.56° | SOG≥5kn; trip 413 (Sep 12–13) anomalous — see below |

Excluded from all analysis: trip 132 (Jan 29) — `cog_deg == average_heading_deg`
on every row, a backfill artifact from when the heading sensor wasn't
reporting, not real data.

Cross-location corroboration (2026-09-09, over trips 165/177/178/183): the
+000° heading bucket showed a consistent +3.8° to +5.2° bias across three
unrelated cruising grounds (Capraia, Isole 2026, home waters) — the first
evidence pointing at a real compass deviation near north rather than
current. A separate −270° bucket anomaly was almost entirely from one trip
(Isole 2026) and remained unconfirmed (could be local current on that leg).

## Root cause found: trip 413, 2026-09-15

Trip 413 (Sep 12–13, "Capraia" — same cruising ground as trip 397 four weeks
earlier) showed mean signed diff +6.44°, well outside the established
baseline, driven by specific heading sectors (0°/360°: +16-18°; 180°/225°:
+8-10°) while the dominant 45° heading bucket sat at +1.14°, in line with
baseline.

The user recalled removing a spare foam panel with embedded metal sheets
from the aft cabin, near the EV-1, partway through this trip. Verified
numerically on the trip's return leg by holding COG to a narrow 30-70° band
(controlling for heading-dependence) and looking at diff over the sequence
of samples:

| segment | duration | diff |
|---|---|---|
| before | ~3.5 hours | flat, noisy, **+6° to +9°** |
| transition | ~3 hours (coincides with the trip's one engine-on episode) | **smooth, monotonic decline** through zero to about −1.4° |
| after | ~5 hours | flat, noisy, **−0.4° to −5.6°** |

The diff does **not** revert to the pre-transition level after the engine
goes back off — ruling out an ongoing electromagnetic effect from the engine
itself. The engine-on timing is very likely coincidental: removing a cabin
panel is a belowdecks task, naturally done during a motoring stretch rather
than while actively sailing. This is the signature of a one-time relocation
of a nearby ferrous object: stable deviation, a transition while the object
moves/settles, a new stable deviation afterward. No exact timestamp for the
removal was available, so the ~3-hour transition window is the best
available bound, not a precise moment.

## Current state / open questions

- Likely cause: the aft-cabin foam/metal panel, not a compass fault or
  current. Not yet confirmed by a clean before/after trip pair with the
  panel definitively out (or relocated as a controlled test).
- The north-heading bias seen independently across trips 156/165/177/178/183
  (before this panel was found) is unexplained by the panel theory and may
  be a separate, smaller, genuine compass deviation near north — don't
  conflate the two.
- Offshore steady-heading test / dockside interference sweep (see below)
  still not run as of 2026-09-15 — lower priority now given the more direct
  physical finding, but still useful if removing the panel doesn't fully
  resolve things.

### Suggested next steps

1. Confirm the panel is out (or track where it ends up) and re-run this same
   trip-diff analysis on subsequent trips to see if deviation settles back
   toward the pre-fault baseline (Set A: ~4.09° mean abs, +1.99° signed).
2. If the user relocates the panel again as a deliberate test, re-run the
   before/after step-detection method (narrow COG band, sequence-ordered
   diff) for a cleaner single-event signature without the engine-timing
   overlap.
3. If deviation doesn't fully resolve: offshore test in a spot with no
   forecast current, away from land, holding steady headings ~2 min each on
   8-12 compass points (not continuous circling — `vessel_status` only logs
   30s averages while underway, too coarse to resolve heading during a
   continuous turn).
4. Dockside interference sweep (handheld/phone compass near the EV-1 mount
   while cycling engine, autopilot, VHF TX, bilge pump, nav lights) as a
   cheap way to rule other sources in/out.
5. If the EV-1 needs relocating: don't default to the bow — Raymarine's
   manual discourages extremities due to pitch/heave motion noise, and a bow
   mount risks new interference from an anchor windlass. Pick the new spot
   from the manual's clearance/mounting guidance (magnetically clean *and*
   close to centerline/center of pitch).

### Query reference

```sql
SELECT
  t.id AS trip_id, t.description,
  COUNT(*) AS n,
  ROUND(AVG(ABS(MOD(vs.average_heading_deg - vs.cog_deg + 540, 360) - 180)), 2) AS mean_abs_diff,
  ROUND(AVG(MOD(vs.average_heading_deg - vs.cog_deg + 540, 360) - 180), 2) AS mean_signed_diff,
  ROUND(STDDEV(MOD(vs.average_heading_deg - vs.cog_deg + 540, 360) - 180), 2) AS stddev_diff
FROM trips t
JOIN vessel_status vs ON vs.timestamp BETWEEN t.start_timestamp AND t.end_timestamp
WHERE t.id IN (<trip_ids>)
  AND vs.average_speed_kn >= 5
  AND vs.average_heading_deg IS NOT NULL
  AND vs.cog_deg IS NOT NULL
  AND vs.is_moored = 0
GROUP BY t.id, t.description
ORDER BY t.id;
```
