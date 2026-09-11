# Wind Calibration Investigation

**Status:** open — root cause not fully identified. This document summarizes an
investigation into an apparent wind-instrument discrepancy observed across
several tacks, and records what's been ruled out so the next person (or
session) doesn't re-walk the same ground.

## Trigger

Looking at the tack around 08:50 UTC on 2026-08-08, the wind appeared fairly
constant before and after the maneuver, but the *computed* true wind
direction (TWD) shifted by ~15-20°. The initial hypothesis was that the
apparent wind speed (AWS) sensor was miscalibrated.

## Method

For a tack or jibe, true wind (speed and direction) shouldn't change over the
few minutes the maneuver takes. So: take a steady-state window of track data
immediately before the maneuver and another immediately after, average each,
and compute absolute true wind direction as

```
TWD = heading + average_wind_angle_deg   (mod 360)
```

`average_wind_angle_deg` (from `vessel_status` / `get_track`) is true wind
angle **relative to the bow**, not an absolute bearing — see
`vessel_monitor.rs`, which stores `normalize0_360(true_wind_angle_deg)` from
`calculate_true_wind()` without adding heading. Any difference between the
pre- and post-maneuver TWD is unexplained by real wind and points at
something in the sensor chain or the calculation.

To go further, the apparent wind vector fed into `calculate_true_wind()` can
be recovered exactly, since the function is invertible:

```
aw_x = TWS·cos(TWA) + SOG
aw_y = TWS·sin(TWA)
AWS  = sqrt(aw_x² + aw_y²)
AWA  = atan2(aw_y, aw_x)
```

This lets various correction hypotheses (scaling AWS, rotating AWA, fixing
the boat-velocity vector) be tested against the pre/post data by asking "what
value of the correction parameter makes TWD_pre = TWD_post?"

**Caveat on method:** the per-event pre/post averages in this investigation
were computed as simple arithmetic means of `average_wind_angle_deg` /
`average_heading_deg` across each window, not the project's mandated
`atan2(avg_sin, avg_cos)` circular mean (see CLAUDE.md, "Angle averaging").
This is a minor methodological shortcut — over each window the angles stayed
within a ~30-70° span with no wraparound, so the arithmetic mean and the
proper circular mean would agree to a small fraction of a degree — but any
follow-up work that touches instrumentation code should use the circular
mean as usual.

## Events analyzed

| Event | Type | Pre AWA / HDG / SOG / TWS | Post AWA / HDG / SOG / TWS | TWD gap | Leeway (HDG−COG) pre/post | Roll pre/post |
|---|---|---|---|---|---|---|
| 2026-08-08 ~08:50 UTC | Tack | 49.6° / 290.1° / 5.84kn / 11.28kn | 311.2° / 11.6° / 6.22kn / 11.99kn | **16.8°** | +10.9° / −1.0° | −17.4° / +21.0° |
| 2026-07-30 ~16:08 UTC | Jibe | 227.5° / 70.1° / 5.19kn / 10.49kn | 158.6° / 143.4° / 4.19kn / 10.44kn | **4.4°** | +0.2° / −4.0° | +4.0° / −1.5° |
| 2026-07-31 ~08:46 UTC | Tack | 312.2° / 48.3° / 3.91kn / 5.61kn | 64.8° / 320.3° / 4.64kn / 6.30kn | **24.6°** | −3.6° / +6.4° | +5.6° / −5.5° |
| 2026-08-05 ~07:31 UTC | Tack | 48.7° / 67.2° / 4.51kn / 7.37kn | 311.9° / 147.9° / 5.72kn / 9.10kn | **16.2°** | +4.8° / +2.3° | −7.5° / +14.0° |

Pattern: **all three tacks show a 16-25° TWD gap; the one jibe shows only
~4°.** This point-of-sail split is the most solid empirical fact to come out
of the investigation and constrains every hypothesis below.

## Hypotheses tested and ruled out

### 1. AWS (apparent wind speed) scale error

Solved for a constant multiplier `k` (`AWS_actual = k × AWS_displayed`) that
reconciles each event's pre/post TWD:

- Aug 8 tack: k ≈ 1.75
- Jul 31 tack: k ≈ 1.75 (matches, despite a completely different heading
  sector and much lighter wind — rules out a heading-dependent compass
  deviation curve as the driver)
- Jul 30 jibe: k ≈ 1.17 (doesn't match the tacks)

**Ruled out:** a real fixed calibration factor should give the same `k`
regardless of point of sail; it doesn't (jibe disagrees). Independently, a
sensor reading 75-80% low would be obvious in daily use — but live AWA/AWS
readouts look correct while sailing, which is decisive against this.

### 2. AWA (wind angle) offset

Solved for a constant rotation `β` applied to the apparent wind vector
before the true-wind calculation:

- Aug 8 tack: β ≈ 42.6°
- Jul 31 tack: β ≈ 37.7°
- Jul 30 jibe: β ≈ 38.5°

These three cluster tightly (~38-43°) regardless of point of sail, which
initially looked like a much better fit than the AWS-scale theory.

**Ruled out:** a masthead/wind-vane misalignment of ~40° would be blatantly
obvious sailing — AWA readouts would look physically wrong against
telltales and sail trim at all times, not just show up as a subtle
5-minute-averaged discrepancy. The live instrument reads correctly while
sailing, which rules this out regardless of how well the numbers cluster.

### 3. Heel-proportional sensor error (masthead tilt / mainsail upwash)

Roll (heel) data exists in `environmental_data` (`metric_id = 7`, ~30s
sampling) — `get_metrics(roll)` returned empty for these windows during the
investigation (tool issue, not a data gap); querying the table directly
works. If a heeled masthead unit read apparent wind low, or upwash from a
heeled mainsail affected the reading, the TWD gap should scale with heel
angle.

**Ruled out:** the Jul 31 tack had much less heel (~5.6° avg) than the Aug 8
tack (~19°) yet produced the *larger* TWD gap (24.6° vs 16.8°) — the
opposite of what a heel-proportional theory predicts.

### 4. Port/starboard rig or sensor asymmetry

Leeway (`heading − COG`) showed a pattern correlated with which side the
wind was on (roughly +6 to +11° when wind was to starboard, −1 to −4° when
to port) across the first two tacks, independent of absolute heading —
suggestive of a physical asymmetry (mast unit not centered/level, or a real
port/starboard difference in how the boat makes leeway).

**Weakened, not fully ruled out:** the fourth tack (Aug 5) broke the
pattern — leeway was small on *both* sides that day (+4.8°/+2.3°, both
positive) yet the gap was still ~16°, as large as events with much bigger
leeway asymmetry. Kept as a loose thread rather than a leading theory.

## Root cause candidate: a `calculate_true_wind()` modeling gap

`calculate_true_wind()` in `src/utilities.rs` takes three scalars —
apparent wind speed, apparent wind angle (relative to bow), and
`boat_speed_kn` — and does all its work in the boat's own body frame. It has
**no heading or COG parameter**. The relevant line:

```rust
// Subtract boat speed from the x component
let tw_x = aw_x - bs;
let tw_y = aw_y;
```

This subtracts `boat_speed_kn` **only from the x-component**, i.e. it
implicitly assumes the boat's own velocity lies entirely along the bow axis
with zero athwartships (sideways) component. That assumption is exactly
correct if `boat_speed_kn` is speed-through-water (STW, by definition along
the keel line). It is **not** correct if `boat_speed_kn` is SOG
(speed-over-ground), because SOG's true direction is COG, not heading — and
the code does feed it SOG (`sog_kn` from the COG/SOG PGN, confirmed in both
`vessel_monitor.rs` and `environmental_monitor.rs`).

Whenever there's leeway (COG ≠ heading — reliably present when close-hauled/
tacking, reliably near zero when running/jibing, matching the observed
tack-vs-jibe split above), the boat's true ground-velocity has an
athwartships component that this line silently drops to zero. This corrupts
the *derived* true wind angle without touching the raw AWA/AWS readings at
all — consistent with the instrument looking correct while sailing, since a
sailor trims to apparent wind, not to this internal calculation.

**Tested:** resolving the boat's velocity vector using its actual direction
(COG − heading, i.e. the leeway angle) instead of forcing it onto the x-axis,
and re-subtracting:

| Event | Gap before | Gap after correction | Reduction |
|---|---|---|---|
| Aug 8 tack | 16.8° | ~12.1° | ~28% |
| Jul 30 jibe | 4.4° | ~2.9° | ~35% |
| Jul 31 tack | 24.6° | ~20.4° | ~17% |
| Aug 5 tack | 16.2° | ~15.0° | ~7% |

**Conclusion:** this is a real, verified defect — the correction helped on
all four events, never hurt — but it is clearly **not sufficient** to
explain most of the gap. The Aug 5 event is the strongest counter-evidence:
it has the smallest leeway of the four (+4.8°/+2.3°) yet a gap as large as
Aug 8's (which had leeway more than double that). If leeway/COG omission
were the dominant driver, gap size should track leeway magnitude — it
doesn't, cleanly.

## Current state / open questions

- **Not** an AWS calibration problem, **not** an AWA alignment problem — both
  ruled out primarily because live readings look correct while sailing (a
  75-80% AWS error or ~40° AWA offset would not).
- **Not** heel-proportional.
- The `calculate_true_wind()` SOG/leeway omission is real and worth fixing,
  but only accounts for 7-35% of the observed gap, inconsistently across
  events.
- The dominant remaining driver is **unidentified**. The strongest structural
  clue — tacks show 16-25° gaps, the one jibe shows ~4° — still isn't fully
  explained by anything tested so far.

### Suggested next steps

1. Fix the `calculate_true_wind()` leeway omission regardless — it's a real,
   independently-confirmed defect. Either pass it a leeway/COG angle so it
   can resolve `boat_speed_kn` into both body-frame components instead of
   assuming zero athwartships velocity, or feed it STW instead of SOG if a
   through-water speed source is available (this would make the existing
   x-only logic correct as written).
2. After that fix, re-run this same pre/post-maneuver TWD comparison on all
   four events (plus new ones) to see how much of the gap remains.
3. Collect a tack with deliberately near-zero leeway (if the conditions
   allow finding one) — if the gap shrinks toward the jibe's ~4°, that
   confirms leeway as the dominant driver despite Aug 5's inconsistent
   scaling; if the gap stays large regardless, the driver is something else
   entirely (short averaging-window artifacts, true wind not being as
   steady as assumed during these particular maneuvers, GPS/compass
   smoothing lag around the turn, etc.).
4. Don't re-propose the AWS-scale-factor, AWA-offset, or heel-proportional
   theories without new evidence — all three were tested against multiple
   events and ruled out above.
