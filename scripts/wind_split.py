#!/usr/bin/env python3
"""Measure the port/starboard true-wind-direction (TWD) bias across tacks and jibes.

For every maneuver, compare the TWD on the steady leg with the wind on the starboard
side against the steady leg with the wind on the port side:

    gap = TWD(starboard leg) - TWD(port leg),   TWD = heading + true wind angle

True wind does not change across a tack, so a systematic positive gap means the
instrument chain reads the wind angle too far from the bow.  See
docs/wind-calibration-investigation.md for the background and the baseline numbers.

Data comes from the router's REST API (/api/track), so no database access is needed.
Standard library only.

Examples:
    scripts/wind_split.py --url http://localhost:1111
    scripts/wind_split.py --url http://localhost:1111 --from 2026-06-01 --compare-from 2026-12-01
    scripts/wind_split.py --url http://localhost:1111 --events --cache /tmp/track.json
    scripts/wind_split.py --cache /tmp/track.json --null-test --fit-correction
"""
import argparse
import bisect
import datetime as dt
import json
import math
import statistics
import sys
import urllib.request

# --- maneuver detection and leg selection (changing these invalidates comparison
# --- with the baseline in the docs; keep them in sync with the documented method)
TURN_DEG = 40  # heading change that marks a maneuver ...
TURN_WINDOW_S = 60  # ... within this many seconds
CLUSTER_GAP_S = 120  # marks closer than this belong to the same maneuver
MAX_MANEUVER_S = 240  # longer clusters are not a clean tack/jibe
LEG_TRIM_S = 30  # skip this long next to the maneuver
MIN_SOG_KN = 2.0
MIN_TWS_KN = 4.0
MAX_ENGINE_FRACTION = 0.10
MIN_LEG_SAMPLES = 12
MAX_HEADING_DEV_DEG = 20  # leg samples must stay this close to the leg's mean heading
MIN_SIDE_FRACTION = 0.85  # a leg must stay on one side of the bow
MAX_TWS_CHANGE = 0.35  # relative TWS difference allowed between the two legs


def wrap180(x):
    return ((x + 180.0) % 360.0) - 180.0


def circular_mean(angles):
    s = sum(math.sin(math.radians(a)) for a in angles) / len(angles)
    c = sum(math.cos(math.radians(a)) for a in angles) / len(angles)
    return math.degrees(math.atan2(s, c)) % 360.0


def parse_time(s):
    return dt.datetime.fromisoformat(s.replace("Z", "+00:00"))


def fetch_track(base_url, start, end):
    """Fetch /api/track one day at a time (a day is always under the 10,000 point cap)."""
    rows, day = [], start
    while day <= end:
        nxt = day + dt.timedelta(days=1)
        url = (f"{base_url}/api/track?start={day}T00:00:00Z&end={nxt}T00:00:00Z"
               "&max_points=10000")
        with urllib.request.urlopen(url, timeout=60) as resp:
            rows.extend(json.load(resp).get("data") or [])
        day = nxt
    seen, unique = set(), []
    for r in rows:
        if r["timestamp"] not in seen:
            seen.add(r["timestamp"])
            unique.append(r)
    unique.sort(key=lambda r: r["timestamp"])
    return unique


def load_points(rows):
    """(t, heading, true wind angle, true wind speed, sog, engine_on) for usable rows."""
    pts = []
    for r in rows:
        h, twa = r["average_heading_deg"], r["average_wind_angle_deg"]
        tws, sog = r["average_wind_speed_kn"], r["avg_speed_kn"]
        if r["moored"] or None in (h, twa, tws, sog):
            continue
        pts.append((parse_time(r["timestamp"]).timestamp(), h, twa, tws, sog, r["engine_on"]))
    return pts


def find_maneuvers(pts, times):
    marks = []
    for i, p in enumerate(pts):
        j = bisect.bisect_right(times, p[0] + TURN_WINDOW_S) - 1
        if j > i and times[j] - p[0] <= TURN_WINDOW_S + 15 \
                and abs(wrap180(pts[j][1] - p[1])) > TURN_DEG:
            marks.append((p[0], times[j]))
    clusters = []
    for a, b in marks:
        if clusters and a - clusters[-1][1] < CLUSTER_GAP_S:
            clusters[-1][1] = max(clusters[-1][1], b)
        else:
            clusters.append([a, b])
    return [c for c in clusters if c[1] - c[0] <= MAX_MANEUVER_S]


def select_leg(pts, times, t0, t1):
    """Steady-leg samples in [t0, t1] on a single side of the bow, or None."""
    i, j = bisect.bisect_left(times, t0), bisect.bisect_right(times, t1)
    leg = [p for p in pts[i:j] if p[4] > MIN_SOG_KN and p[3] > MIN_TWS_KN]
    if len(leg) < MIN_LEG_SAMPLES:
        return None
    if sum(1 for p in leg if p[5] != 0) / len(leg) > MAX_ENGINE_FRACTION:
        return None  # engine on (1) or unknown (2): motor-sailing distorts the wind
    mean_heading = circular_mean([p[1] for p in leg])
    leg = [p for p in leg if abs(wrap180(p[1] - mean_heading)) < MAX_HEADING_DEV_DEG]
    if len(leg) < MIN_LEG_SAMPLES:
        return None
    starboard = sum(1 for p in leg if 0 < p[2] < 180) / len(leg)
    if MIN_SIDE_FRACTION > max(starboard, 1 - starboard):
        return None
    side = 1 if starboard >= 0.5 else -1
    leg = [p for p in leg if (0 < p[2] < 180) == (side == 1)]
    return (side, leg) if len(leg) >= MIN_LEG_SAMPLES else None


def twd(leg):
    return circular_mean([(p[1] + p[2]) % 360.0 for p in leg])


def measure_events(rows, leg_minutes):
    pts = load_points(rows)
    times = [p[0] for p in pts]
    span = leg_minutes * 60
    events = []
    for start, end in find_maneuvers(pts, times):
        pre = select_leg(pts, times, start - span, start - LEG_TRIM_S)
        post = select_leg(pts, times, end + LEG_TRIM_S, end + span)
        if not pre or not post or pre[0] == post[0]:
            continue
        stbd, port = (pre, post) if pre[0] == 1 else (post, pre)
        tws_pre = sum(p[3] for p in pre[1]) / len(pre[1])
        tws_post = sum(p[3] for p in post[1]) / len(post[1])
        if abs(tws_pre - tws_post) / max(tws_pre, tws_post) > MAX_TWS_CHANGE:
            continue
        both = pre[1] + post[1]
        events.append({
            "time": dt.datetime.fromtimestamp(start, dt.timezone.utc),
            "gap": wrap180(twd(stbd[1]) - twd(port[1])),
            "abs_twa": sum(min(p[2], 360 - p[2]) for p in both) / len(both),
            "tws": (tws_pre + tws_post) / 2,
            "heading_stbd": circular_mean([p[1] for p in stbd[1]]),
            "heading_port": circular_mean([p[1] for p in port[1]]),
            "samples": (len(pre[1]), len(post[1])),
            # (heading, TWA, TWS, SOG) per sample, kept for --fit-correction
            "stbd": [p[1:5] for p in stbd[1]],
            "port": [p[1:5] for p in port[1]],
        })
    return events


# --- AWA correction prototype (--fit-correction) ---------------------------------
#
# The stored true wind is inverted back to apparent wind (calculate_true_wind() is
# invertible), the apparent angle is narrowed by eps degrees (eps > 0 means the vane
# over-reads |AWA|), true wind is recomputed and the maneuver gap measured again.
# Each model gives eps as a function of the sample's apparent angle and speed.

AWA_BINS = ((0, 45), (45, 90), (90, 135), (135, 181))
CORRECTION_MODELS = {
    # name: (parameter names, eps(params, |awa|, aws))
    "constant": (["eps"], lambda p, awa, aws: p[0]),
    "AWA table": ([f"eps|AWA|{lo}-{hi}" for lo, hi in AWA_BINS],
                  lambda p, awa, aws: p[next(i for i, (lo, hi) in enumerate(AWA_BINS)
                                             if lo <= awa < hi)]),
    # gravity-vs-aerodynamic vane deflection; b is the extra eps at 10 kn AWS
    "a + b(10/AWS)^2": (["a", "b@10kn"],
                        lambda p, awa, aws: p[0] + p[1] * (10.0 / max(aws, 3.0)) ** 2),
    # angle and speed together: smooth (linear in |AWA|) and binned
    "a+c|AWA|/90+b(..)^2": (["a", "c@90deg", "b@10kn"],
                            lambda p, awa, aws: p[0] + p[1] * awa / 90.0
                            + p[2] * (10.0 / max(aws, 3.0)) ** 2),
    "AWA table+b(..)^2": ([f"eps|AWA|{lo}-{hi}" for lo, hi in AWA_BINS] + ["b@10kn"],
                          lambda p, awa, aws: p[next(i for i, (lo, hi) in enumerate(AWA_BINS)
                                                     if lo <= awa < hi)]
                          + p[4] * (10.0 / max(aws, 3.0)) ** 2),
}


def apparent_from_true(twa, tws, sog):
    t = math.radians(twa)
    x, y = tws * math.cos(t) + sog, tws * math.sin(t)
    return math.degrees(math.atan2(y, x)), math.hypot(x, y)


def corrected_twd(leg, eps_fn, params):
    out = []
    for h, twa, tws, sog in leg:
        awa, aws = apparent_from_true(twa, tws, sog)
        eps = eps_fn(params, abs(awa), aws)
        awa_c = math.copysign(max(abs(awa) - eps, 0.0), awa)
        a = math.radians(awa_c)
        out.append((h + math.degrees(math.atan2(aws * math.sin(a),
                                                aws * math.cos(a) - sog))) % 360.0)
    return circular_mean(out)


def corrected_gaps(events, eps_fn, params):
    return [wrap180(corrected_twd(e["stbd"], eps_fn, params)
                    - corrected_twd(e["port"], eps_fn, params)) for e in events]


def solve(a, b):
    """Gaussian elimination with partial pivoting for a small dense system."""
    n = len(b)
    m = [row[:] + [b[i]] for i, row in enumerate(a)]
    for c in range(n):
        piv = max(range(c, n), key=lambda r: abs(m[r][c]))
        m[c], m[piv] = m[piv], m[c]
        for r in range(c + 1, n):
            f = m[r][c] / m[c][c]
            for k in range(c, n + 1):
                m[r][k] -= f * m[c][k]
    x = [0.0] * n
    for r in reversed(range(n)):
        x[r] = (m[r][n] - sum(m[r][k] * x[k] for k in range(r + 1, n))) / m[r][r]
    return x


def fit_correction(events, n_params, eps_fn):
    """Least squares on the gaps (damped Gauss-Newton, numeric Jacobian)."""
    p, lam, step = [5.0] * n_params, 1e-3, 0.05
    r = corrected_gaps(events, eps_fn, p)
    cost = sum(x * x for x in r)
    for _ in range(40):
        jac = []
        for k in range(n_params):
            q = p[:]
            q[k] += step
            rk = corrected_gaps(events, eps_fn, q)
            jac.append([(rk[i] - r[i]) / step for i in range(len(r))])
        jtj = [[sum(jac[a][i] * jac[b][i] for i in range(len(r))) + (lam if a == b else 0)
                for b in range(n_params)] for a in range(n_params)]
        jtr = [-sum(jac[a][i] * r[i] for i in range(len(r))) for a in range(n_params)]
        delta = solve(jtj, jtr)
        q = [p[k] + delta[k] for k in range(n_params)]
        rq = corrected_gaps(events, eps_fn, q)
        cq = sum(x * x for x in rq)
        if cq < cost:
            p, r, converged = q, rq, cost - cq < 1e-6 * cost
            cost = cq
            lam = max(lam / 3, 1e-6)
            if converged:
                break
        else:
            lam *= 10
    return p


def gap_stats(gaps):
    rms = math.sqrt(sum(g * g for g in gaps) / len(gaps))
    sd = statistics.stdev(gaps) if len(gaps) > 1 else 0.0
    return f"mean={statistics.mean(gaps):+6.2f} sd={sd:5.2f} rms={rms:5.2f}"


def report_correction(events, train_before):
    cut = dt.datetime.combine(train_before, dt.time(), dt.timezone.utc)
    train = [e for e in events if e["time"] < cut]
    test = [e for e in events if e["time"] >= cut]
    print(f"\nAWA CORRECTION FIT  (eps narrows the apparent angle; gap target 0)")
    print(f"  out-of-sample check: fit on {len(train)} events before {train_before}, "
          f"score on {len(test)} events after")
    print(f"  {'model':18s} {'test (fitted on train)':42s} {'all data (fitted on all)':42s} params (all data)")
    none = [e["gap"] for e in events]
    print(f"  {'none':18s} {gap_stats([e['gap'] for e in test]):42s} {gap_stats(none):42s} -")
    fitted = {}
    for name, (pnames, fn) in CORRECTION_MODELS.items():
        p_train = fit_correction(train, len(pnames), fn) if train else None
        test_line = gap_stats(corrected_gaps(test, fn, p_train)) if (test and p_train) else "-"
        p_all = fit_correction(events, len(pnames), fn)
        fitted[name] = (fn, p_all)
        params = " ".join(f"{n}={v:.1f}" for n, v in zip(pnames, p_all))
        print(f"  {name:18s} {test_line:42s} "
              f"{gap_stats(corrected_gaps(events, fn, p_all)):42s} {params}")
    print("\n  residual gap by point of sail (all-data fits):")
    bins = ((0, 60), (60, 90), (90, 140), (140, 181))
    header = "".join(f"{f'|TWA| {lo}-{hi}':>20s}" for lo, hi in bins)
    print(f"  {'model':18s}{header}")
    rows = [("none", None, None)] + [(n, fn, p) for n, (fn, p) in fitted.items()]
    for name, fn, p in rows:
        cells = []
        for lo, hi in bins:
            sub = [e for e in events if lo <= e["abs_twa"] < hi]
            g = [e["gap"] for e in sub] if fn is None else corrected_gaps(sub, fn, p)
            cells.append(f"{statistics.mean(g):+6.1f} (n={len(g):2d})" if g else "-")
        print(f"  {name:18s}" + "".join(f"{c:>20s}" for c in cells))


def null_pairs(rows, leg_minutes):
    """Control: TWD difference between two consecutive legs on the SAME side with no
    maneuver between them, spaced like a maneuver pair.  The mean should be ~0 (the
    method has no bias of its own) and the sd is the noise floor of a single gap."""
    pts = load_points(rows)
    times = [p[0] for p in pts]
    span = leg_minutes * 60
    gap_s = 2 * LEG_TRIM_S + 60  # roughly the dead time around a maneuver
    diffs, i = [], 0
    while i < len(pts):
        t = times[i]
        a = select_leg(pts, times, t, t + span - LEG_TRIM_S)
        b = select_leg(pts, times, t + span - LEG_TRIM_S + gap_s, t + 2 * span + gap_s)
        if a and b and a[0] == b[0] and abs(wrap180(
                circular_mean([p[1] for p in a[1]])
                - circular_mean([p[1] for p in b[1]]))) < MAX_HEADING_DEV_DEG:
            diffs.append(wrap180(twd(b[1]) - twd(a[1])))
            i = bisect.bisect_left(times, t + 2 * span + gap_s)
        else:
            i = bisect.bisect_left(times, t + 60)
    return diffs


def summarize(label, events):
    if not events:
        print(f"  {label:26s} none")
        return
    gaps = [e["gap"] for e in events]
    sd = statistics.stdev(gaps) if len(gaps) > 1 else 0.0
    se = sd / math.sqrt(len(gaps)) if len(gaps) > 1 else 0.0
    pos = sum(1 for g in gaps if g > 0)
    print(f"  {label:26s} n={len(gaps):3d}  mean={statistics.mean(gaps):+6.2f}  "
          f"median={statistics.median(gaps):+6.2f}  sd={sd:5.1f}  se={se:4.1f}  "
          f"positive={pos}/{len(gaps)}")


def report(title, events):
    print(f"\n{title}")
    summarize("all maneuvers", events)
    print("  by mean |true wind angle| of the two legs:")
    for lo, hi in ((0, 60), (60, 90), (90, 140), (140, 181)):
        summarize(f"|TWA| {lo}-{hi}", [e for e in events if lo <= e["abs_twa"] < hi])
    print("  by true wind speed:")
    for lo, hi in ((0, 8), (8, 12), (12, 99)):
        summarize(f"TWS {lo}-{hi} kn", [e for e in events if lo <= e["tws"] < hi])
    if len(events) > 2:
        xs, ys = [e["tws"] for e in events], [e["gap"] for e in events]
        print(f"  correlation(gap, TWS) = {statistics.correlation(xs, ys):+.2f}")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0],
                                 formatter_class=argparse.RawDescriptionHelpFormatter,
                                 epilog="Examples:" + __doc__.split("Examples:")[1])
    ap.add_argument("--url", default="http://localhost:8080", help="router base URL")
    ap.add_argument("--from", dest="start", type=dt.date.fromisoformat,
                    default=dt.date(2025, 6, 1), help="first day (UTC), default 2025-06-01")
    ap.add_argument("--to", dest="end", type=dt.date.fromisoformat,
                    default=dt.datetime.now(dt.timezone.utc).date(), help="last day (UTC)")
    ap.add_argument("--leg-minutes", type=float, default=8, help="leg length either side")
    ap.add_argument("--compare-from", type=dt.date.fromisoformat,
                    help="also report events from this date on separately (e.g. the day "
                         "new wind hardware was installed)")
    ap.add_argument("--events", action="store_true", help="list every event")
    ap.add_argument("--null-test", action="store_true",
                    help="control: same-side leg pairs with no maneuver (bias ~0, noise floor)")
    ap.add_argument("--fit-correction", action="store_true",
                    help="fit candidate AWA corrections that would cancel the gap")
    ap.add_argument("--train-before", type=dt.date.fromisoformat,
                    default=dt.date(2026, 1, 1),
                    help="--fit-correction: fit on events before this date and score on "
                         "the rest (default 2026-01-01)")
    ap.add_argument("--cache", help="JSON file: reuse it if present, else write the fetch to it")
    args = ap.parse_args()

    rows = None
    if args.cache:
        try:
            with open(args.cache) as f:
                rows = json.load(f)
        except (OSError, ValueError):
            rows = None
    if rows is None:
        try:
            rows = fetch_track(args.url.rstrip("/"), args.start, args.end)
        except OSError as e:
            sys.exit(f"cannot read {args.url}: {e} (is the server up? is web.auth_password set?)")
        if args.cache:
            with open(args.cache, "w") as f:
                json.dump(rows, f)
    print(f"{len(rows)} track points, {rows[0]['timestamp'] if rows else '-'} .. "
          f"{rows[-1]['timestamp'] if rows else '-'}")

    events = measure_events(rows, args.leg_minutes)
    if args.compare_from:
        cut = dt.datetime.combine(args.compare_from, dt.time(), dt.timezone.utc)
        report(f"BEFORE {args.compare_from}", [e for e in events if e["time"] < cut])
        report(f"FROM {args.compare_from}", [e for e in events if e["time"] >= cut])
    else:
        report("ALL EVENTS", events)
        print("\n  by month:")
        months = sorted({e["time"].strftime("%Y-%m") for e in events})
        for m in months:
            summarize(m, [e for e in events if e["time"].strftime("%Y-%m") == m])
    if args.fit_correction:
        report_correction(events, args.train_before)
    if args.null_test:
        d = null_pairs(rows, args.leg_minutes)
        if len(d) > 1:
            print(f"\nNULL TEST (same-side consecutive legs, no maneuver): n={len(d)} "
                  f"mean={statistics.mean(d):+.2f} sd={statistics.stdev(d):.2f}")
    if args.events:
        print("\nevents (gap = TWD starboard leg - TWD port leg):")
        for e in events:
            print(f"  {e['time']:%Y-%m-%d %H:%M} gap={e['gap']:+6.1f} |TWA|={e['abs_twa']:3.0f} "
                  f"TWS={e['tws']:4.1f} legs={e['samples'][0]}/{e['samples'][1]} samples")


if __name__ == "__main__":
    main()
