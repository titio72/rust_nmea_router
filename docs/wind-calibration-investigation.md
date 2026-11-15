# Wind Calibration Investigation

**Status (2026-11-11):** the bias is measured and reproducible; its mechanism is
not identified. All data so far comes from the original wind-sensor hardware
(ESP32 internal ADC), which is therefore the baseline for any hardware change.

## The problem

True wind does not change across a tack, but the computed true wind direction
(TWD = heading + true wind angle) does: with the wind on the starboard side it
reads systematically higher than with the wind on the port side. A tack shows
about twice the per-side error, because the error flips sign with the side.
A positive gap (starboard TWD above port TWD) means the instrument chain reads
the wind angle too far from the bow.

It was first noticed on 2026-08-08 (~08:50 UTC): the wind looked constant across
a tack but TWD moved by ~17°. Live apparent wind readings look right while
sailing, which is why the problem is easy to miss; it only shows up in derived
true wind.

## How to measure it

`scripts/wind_split.py` reads `/api/track` from a running router (standard
library only, no database access):

```bash
scripts/wind_split.py --url http://localhost:8080                      # everything
scripts/wind_split.py --url http://localhost:8080 --events             # list each maneuver
scripts/wind_split.py --url http://localhost:8080 --compare-from 2026-12-01   # before/after a change
scripts/wind_split.py --url http://localhost:8080 --cache /tmp/track.json     # reuse the download
scripts/wind_split.py --cache /tmp/track.json --null-test --fit-correction    # control + correction fit
```

If `web.auth_password` is set the API needs a session, and the script will fail
with a connection error.

**Method.** A maneuver is a heading change of more than 40° within 60 s
(clustered, at most 240 s long). For each one, take the 8 minutes before and
after it, skipping 30 s next to the turn. A leg is kept only if the engine is
off (≤10% of samples), SOG > 2 kn, TWS > 4 kn, the heading stays within 20° of
the leg mean, and the wind stays on one side of the bow (≥85%). The two legs
must be on opposite sides and have a TWS within 35% of each other. Then:

```
gap = circular_mean(heading + TWA, starboard leg) - circular_mean(heading + TWA, port leg)
```

Circular means throughout. The thresholds are constants at the top of the
script. Changing them breaks comparability with the baseline below.

Timestamps in this document and in the script are UTC.

## Baseline: original hardware

178,877 track points, 2025-06-02 to 2026-11-08; 87 valid maneuvers.

| Group | n | Mean gap | sd | Positive |
|---|---|---|---|---|
| All maneuvers | 87 | **+14.7°** | 13.0 | 77 / 87 |
| \|TWA\| < 60° | 30 | +17.4° | 9.8 | 30 / 30 |
| \|TWA\| 60–90° | 9 | +22.5° | 10.3 | 9 / 9 |
| \|TWA\| 90–140° | 9 | +18.7° | 8.5 | 9 / 9 |
| \|TWA\| > 140° (running) | 39 | +9.9° | 14.9 | 29 / 39 |
| TWS < 8 kn | 33 | +20.9° | 13.7 | 31 / 33 |
| TWS 8–12 kn | 29 | +11.7° | 12.0 | 25 / 29 |
| TWS > 12 kn | 25 | +10.1° | 9.8 | 21 / 25 |

Upwind and reaching (\|TWA\| < 140°) all 48 events are positive, mean about
+18°. Correlation between gap and TWS is −0.38. Wind speed and point of sail are
correlated in this data, so the TWS rows do not separate the two.

**Control (`--null-test`).** The same measurement on 1,230 pairs of consecutive
legs on the *same* side with no maneuver between them gives mean −0.1°, sd 8.1°.
So the method has no bias of its own (the +14.7° is real), and about 8° of
per-event scatter is noise: wind variability over ~10 minutes plus instrument
noise. No correction can remove that part. Per-event spread beyond noise is
√(13.0² − 8.1²) ≈ 10°.

The most recent event, 2026-10-31 14:29–14:32 UTC (wind from port at 242° true
wind angle, heading 189°, to wind from starboard at 90° true wind angle, heading
5°, TWS 13 kn both sides): TWD 71.2° → 95.5°, gap +24°, which is typical for a
tack.

## AWA correction prototype

`--fit-correction` inverts the stored true wind back to apparent wind (exact:
`calculate_true_wind()` is invertible; at zero correction the recomputed gaps match
the stored ones to 1e-13°), narrows the apparent angle by ε, recomputes true
wind and the gap, and fits ε by least squares on the gaps. To check
generalisation, each model is also fitted on the 36 events of 2025 and scored on
the 51 events of 2026.

| Model | Params (all data) | 2026 out of sample: mean / sd / rms | All data: mean / sd |
|---|---|---|---|
| none | – | +15.7 / 14.9 / 21.5 | +14.7 / 13.0 |
| constant ε | ε = 7.3° | +3.8 / 14.6 / 14.9 | +0.8 / 12.5 |
| table by \|AWA\| (4 bins) | 6.0 / 11.8 / 8.3 / 8.2° | +3.7 / 14.5 / 14.8 | +0.3 / 12.2 |
| ε = a + b·(10/AWS)² | a = 5.0°, b = 1.9° | **+1.9 / 13.6 / 13.6** | −0.4 / 11.4 |
| ε = a + c·\|AWA\|/90 + b·(10/AWS)² | a = 5.3°, c = −0.9°, b = 2.0° | +1.9 / 13.5 / 13.5 | −0.2 / 11.3 |
| table by \|AWA\| + b·(10/AWS)² | 4.4 / 8.2 / 0.8 / 7.2°, b = 2.1° | +1.6 / 13.5 / 13.5 | −0.2 / 11.1 |

Residual mean gap by point of sail after the all-data fit:

| Model | \|TWA\| < 60 | 60–90 | 90–140 | > 140 |
|---|---|---|---|---|
| none | +17.4 | +22.5 | +18.7 | +9.9 |
| constant | −3.3 | +4.4 | +7.6 | +1.7 |
| \|AWA\| table | −1.3 | +2.0 | +2.7 | +0.5 |
| a + b(10/AWS)² | −1.2 | +4.1 | +4.7 | −2.1 |
| a + c\|AWA\| + b(10/AWS)² | −1.6 | +4.0 | +4.9 | −1.4 |
| \|AWA\| table + b(10/AWS)² | −0.8 | +1.9 | +1.9 | −0.6 |

Reading:

- **The size of the bias is about 7°** on the apparent angle, about twice the old 2.5–4° estimate. A constant ε cancels the mean bias and generalises from 2025 to 2026, leaving +3.8° out of sample.
- **The angle table is not worth its parameters.** Out of sample it is no better than a constant (rms 14.8 vs 14.9).
- **The wind-speed form is the only one that also reduces the spread**, in and out of sample. Spread beyond the 8.1° noise floor drops from ~10° to ~8° (ε ≈ 12.6° at 5 kn AWS, 7° at 10 kn, 5.8° at 15 kn). This supports a speed-dependent vane error (gravity-vs-aerodynamic deflection, upwash, or both), but modestly.
- **Adding angle dependence on top of wind speed gains nothing.** The smooth form puts the angle slope at c = −0.9° per 90°, effectively zero. The binned form improves the in-sample residual table, but its bins jump around (90–135° drops to 0.8° while its neighbours sit at 7–8°), and out of sample it is no better than the speed-only form (13.5 vs 13.6). Upwind sailing also has higher apparent wind speed, so the two variables overlap in this data, and once speed is in the model nothing remains for angle.
- **About 8° of per-event variation stays unexplained** beyond noise, so the error is not a fixed function of apparent angle and speed. Something that varies from trip to trip (sail trim or reefing, heel, sea state) is involved.
- A correction is safe to apply in the sense that it improves every point of sail. It is not a calibration: individual true-wind readings would still be off by around ±8° at times.

## What has been tested

| Candidate | Verdict | Evidence |
|---|---|---|
| AWS scale error | ruled out | A fitted factor of ~1.75 would be obvious while sailing, and it differed between tacks and the one jibe tested. |
| Fixed AWA offset (~40°) | ruled out | Would be blatantly wrong against telltales and sail trim. |
| Instrument averaging/filter lag | ruled out | The vane's own averaging is ~5 s; the artifact persists for minutes. |
| Heel-proportional error | ruled out | Heel flips sign on a tack just as the wind side does, so heel and side are indistinguishable in tack data; a heel-proportional fit is rejected, and heel acts at most as a proxy for sail load. |
| Compass deviation | not supported | Adding a heading-dependent first harmonic to the fit across the 87 events changes the residual RMS from 12.88° to 12.80°. A constant heading offset cancels out of the gap by construction. |
| Large sin/cos calibration error | bounded | The sensor's `ellipse` quality metric stays within ~5% of 1.00 (user measurement), which limits offset/gain/quadrature errors to about ±3°. |
| Leeway/COG omission in `calculate_true_wind()` | real but minor | See below. Does not reduce the gap on the 2026-10-31 event (24.2° → 25.3°). |
| Masthead flow distortion / mainsail upwash | plausible, unconfirmed | Magnitude is in the documented range and the sign matches, but sails-down evidence is a single case. |
| Heeled-vane gravity deflection | partly consistent | The gap falls as wind speed rises, as a gravity-versus-aerodynamic-torque model predicts. The `a + b/AWS²` correction is the best model out of sample, but it removes only ~20% of the spread beyond noise. |
| ADC non-linearity (ESP32 internal ADC) | untested | The firmware uses the ESP32 internal ADC. The Raymarine output (667–2000 mV after the divider) sits partly above the ADC's stated linear range. The planned MCP3202 board is the test. |

### The leeway defect in `calculate_true_wind()`

`calculate_true_wind()` in `src/utilities.rs` has no heading or COG parameter and
subtracts `boat_speed_kn` only from the bow-axis component. That is correct for
speed through water, but the callers pass SOG, whose direction is COG, not the
bow direction. Whenever there is leeway, the sideways part of the boat velocity is
dropped. It is a real defect and worth fixing, but it accounts for only a small
and inconsistent share of the gap.

### Traps

- **Single-event pre/post comparisons are degenerate.** On a tack the heel sign,
  the wind-side sign and (through wind pressure) heel magnitude all flip or move
  together, so one-parameter fits to hand-picked pairs all fit equally well.
  Several earlier "breakthroughs" were this degeneracy. Use the population
  measurement.
- **Hand-picked events are biased** toward large gaps. Do not compare a single
  event with the population mean without allowing for that.
- **Do not use days with a real wind rotation.** On 2026-09-06 the wind rotated
  ~200° through the day (land to sea breeze); the constant-wind premise fails.
- **`engine_on` is not a "sails down" proxy.** Motor-sailing to windward is
  routine, and the motoring-only bins show large gaps for that reason.
- **An earlier +8.9° figure (93 windows, 30-minute windows) could not be
  reproduced.** Re-running a 30-minute-window split on the same data gave 44
  windows and +13.8°. The sign is robust (39/44 positive), the magnitude depends
  on the method, and the windowing details of the earlier run are lost. The
  maneuver-pair numbers above supersede it.

## Hardware context

The wind reader is custom firmware (the `n2k_wind` PlatformIO project) driving a
Raymarine ST50/ST60 masthead vane whose two hall sensors give sin and cos; the
angle is `atan2(sin, cos)` after each channel is normalised by a self-calibrated
min/max. Only PGN 130306 (apparent angle and speed) reaches the bus, so
calibration quality (`ellipse`, raw channels) is invisible to this router.

A board using an external MCP3202 ADC has been designed but is **not installed**
(as of 2026-11-11). The boat still runs the original equipment.

## Next steps

1. **Keep the baseline clean.** Everything above is the original hardware. When
   the new board is installed, note the date.
2. **Re-measure after the change.** Run
   `scripts/wind_split.py --compare-from <install date>` once there are enough new
   tacks. With a tack-like baseline of about +18° (sd ~10°), ten new events detect
   a shift of roughly 7° at two standard errors. Compare like with like: the same
   \|TWA\| bins, since the TWS and point-of-sail mix differs from trip to trip.
3. **Fix `calculate_true_wind()`** to resolve boat speed along its real direction
   (leeway/COG angle) or to use speed through water if a log is fitted.
4. **Consider an AWA correction**, applied to the apparent angle, either in the
   firmware or at the router's `calculate_true_wind` call sites. On the baseline
   the best candidate is `ε = 5.0° + 1.9°·(10/AWS)²` (see the prototype above). Refit
   it on post-swap data if the new ADC changes the picture. After applying it, the
   gap should go to zero in this same measurement. Any change to how true wind is
   computed also changes the stored data, so record the deploy date and keep it
   apart from the hardware-swap date.
5. **Persist calibration quality.** `ellipse` and the raw sin/cos channels never
   reach the DB, so ADC or calibration problems cannot be seen from the router.
