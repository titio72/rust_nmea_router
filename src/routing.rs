use chrono::{DateTime, Utc};
use crate::db::operations::forecast::FetchWithHourly;
use crate::forecast::compute_twa;
use crate::utilities::{advance_position, haversine_distance_nm, haversine_heading};

// ── Constants ─────────────────────────────────────────────────────────────────

const HEADING_STEP_DEG: f64 = 5.0;
// Full circle sampled at HEADING_STEP_DEG resolution; also doubles as the frontier's
// angular pruning bin count, so it must stay in lockstep with HEADING_STEP_DEG.
const SECTOR_COUNT: usize = (360.0 / HEADING_STEP_DEG) as usize;
const MAX_STEPS: usize = 336;        // 336 × 30 min = 168 h (7 days)
const STEP_HOURS: f64 = 0.5;
// Stop expanding once the best direct-to-destination ETA hasn't improved for this many steps.
const STAGNANT_STEPS: usize = 6;    // 6 × 30 min = 3 h without improvement
// The direct-to-destination ETA shortcut may only claim a frontier point as "best" when it's
// already this close to the destination — matches the land-avoidance design's 5 nm max
// resolution target. Beyond this range the frontier must keep expanding in real 30-min steps
// rather than assuming a single constant-speed leg can cover the remaining distance.
const MAX_FINAL_LEG_NM: f64 = 5.0;

// ── Internal helpers ──────────────────────────────────────────────────────────

// Takes an already-looked-up `wind` rather than a position/time to look up itself: callers in
// the hot loop below already need the same wind sample for their own heading-candidate logic,
// so looking it up twice for identical (lat, lon, time) inputs would double the cost for nothing.
fn speed_for_final_leg(
    from_lat: f64,
    from_lon: f64,
    to: (f64, f64),
    wind: Option<(f64, f64)>,
    motoring_speed_kn: f64,
    polar_efficiency: f64,
    min_sail_speed_kn: f64,
    polars: &crate::polars::PolarTable,
) -> f64 {
    let bearing = crate::utilities::haversine_heading(from_lat, from_lon, to.0, to.1);
    match wind {
        Some((wind_spd, wind_dir)) if wind_spd > 0.0 => {
            let twa = compute_twa(bearing, wind_dir);
            match polars.boat_speed(twa, wind_spd).filter(|&s| s > 0.0) {
                Some(raw) => {
                    let eff = raw * polar_efficiency;
                    if eff >= min_sail_speed_kn { eff } else { motoring_speed_kn }
                }
                None => motoring_speed_kn,
            }
        }
        _ => motoring_speed_kn,
    }
}

/// True if the straight line from `from` to `to` passes through land at any of a series of
/// evenly-spaced samples. Guards the direct-to-destination ETA shortcuts below, which would
/// otherwise let the router claim a final leg that sails straight through an island.
fn path_crosses_land(
    from: (f64, f64),
    to: (f64, f64),
    land_mask: Option<&crate::land_mask::LandMask>,
) -> bool {
    let mask = match land_mask {
        Some(m) => m,
        None => return false,
    };
    let dist_nm = haversine_distance_nm(from.0, from.1, to.0, to.1);
    if dist_nm < 0.01 {
        return false;
    }
    let bearing = haversine_heading(from.0, from.1, to.0, to.1);
    // One sample per nautical mile is finer than the land mask's own grid resolution.
    let samples = (dist_nm.ceil() as usize).max(4);
    (1..samples).any(|i| {
        let d = dist_nm * i as f64 / samples as f64;
        let (lat, lon) = advance_position(from.0, from.1, bearing, d);
        mask.is_land(lat, lon)
    })
}

// ── Data structures ───────────────────────────────────────────────────────────

#[derive(Clone)]
struct IsochronePoint {
    lat: f64,
    lon: f64,
    time: DateTime<Utc>,
    sailed_hours: f64,
    parent_idx: Option<usize>,
}

pub struct IsochroneResult {
    pub track: Vec<(f64, f64, DateTime<Utc>)>,
    pub reached_destination: bool,
}

// ── Public function ───────────────────────────────────────────────────────────

pub fn run_isochrone(
    from: (f64, f64),
    to: (f64, f64),
    departure: DateTime<Utc>,
    motoring_speed_kn: f64,
    polar_efficiency: f64,
    min_sail_speed_kn: f64,
    sail_weight_kn: f64,
    polars: &crate::polars::PolarTable,
    fetches: &[FetchWithHourly],
    land_mask: Option<&crate::land_mask::LandMask>,
) -> IsochroneResult {
    if motoring_speed_kn <= 0.0 {
        return IsochroneResult { track: vec![], reached_destination: false };
    }
    if land_mask.map_or(false, |m| m.is_land(to.0, to.1)) {
        return IsochroneResult { track: vec![], reached_destination: false };
    }

    // Parsed once and reused for every wind lookup below — the alternative (re-parsing every
    // hourly timestamp string on every lookup) dominates runtime once the search does the full
    // number of real 30-min steps a route needs.
    let parsed_fetches = crate::forecast::parse_fetches(fetches);

    let seed = IsochronePoint { lat: from.0, lon: from.1, time: departure, sailed_hours: 0.0, parent_idx: None };
    let mut isochrones: Vec<Vec<IsochronePoint>> = vec![vec![seed]];

    // Global best: (destination_eta, step_idx_into_isochrones, pt_idx_within_step)
    let mut best: Option<(DateTime<Utc>, usize, usize)> = None;
    let mut last_improved_eta: Option<DateTime<Utc>> = None;
    let mut stagnant = 0usize;

    for _step in 1..=MAX_STEPS {
        let prev_step_idx = isochrones.len() - 1;
        let prev = isochrones.last().unwrap();
        let mut candidates: Vec<IsochronePoint> = Vec::new();

        for (parent_idx, parent) in prev.iter().enumerate() {
            // Looked up once per parent and reused both for the arrival check right below and
            // for every heading candidate's speed in the loop further down — they're all the
            // same (lat, lon, time), so a second lookup would just repeat the same work.
            let wind = crate::forecast::nearest_forecast_wind(&parsed_fetches, parent.lat, parent.lon, parent.time);

            // Check if this parent can reach the destination within the current step.
            // This catches mid-step arrivals that fall between two 30-minute boundaries.
            let dist_to_dest = haversine_distance_nm(parent.lat, parent.lon, to.0, to.1);
            let speed_to_dest = speed_for_final_leg(
                parent.lat, parent.lon, to, wind,
                motoring_speed_kn, polar_efficiency, min_sail_speed_kn, polars,
            );
            if dist_to_dest <= speed_to_dest * STEP_HOURS
                && !path_crosses_land((parent.lat, parent.lon), to, land_mask)
            {
                let eta = parent.time + chrono::Duration::seconds(
                    (dist_to_dest / speed_to_dest * 3600.0).round() as i64,
                );
                if best.as_ref().map_or(true, |(b, _, _)| eta < *b) {
                    best = Some((eta, prev_step_idx, parent_idx));
                }
            }

            for h in 0..SECTOR_COUNT {
                let heading = h as f64 * HEADING_STEP_DEG;

                let (speed_kn, was_sailing) = match wind {
                    Some((wind_spd, wind_dir)) if wind_spd > 0.0 => {
                        let twa = compute_twa(heading, wind_dir);
                        match polars.boat_speed(twa, wind_spd).filter(|&s| s > 0.0) {
                            Some(raw) => {
                                let eff = raw * polar_efficiency;
                                if eff >= min_sail_speed_kn { (eff, true) } else { (motoring_speed_kn, false) }
                            }
                            None => (motoring_speed_kn, false),
                        }
                    }
                    _ => (motoring_speed_kn, false),
                };

                let new_pos = advance_position(parent.lat, parent.lon, heading, speed_kn * STEP_HOURS);
                if land_mask.map_or(false, |m| m.is_land(new_pos.0, new_pos.1)) {
                    continue;
                }
                candidates.push(IsochronePoint {
                    lat: new_pos.0,
                    lon: new_pos.1,
                    time: parent.time + chrono::Duration::minutes((STEP_HOURS * 60.0) as i64),
                    sailed_hours: parent.sailed_hours + if was_sailing { STEP_HOURS } else { 0.0 },
                    parent_idx: Some(parent_idx),
                });
            }
        }

        isochrones.push(prune_isochrone(candidates, from, sail_weight_kn));

        // Check every new frontier point: compute its direct-to-destination ETA and keep the global minimum.
        // This handles heading-quantization drift: the optimal frontier may be slightly outside any
        // fixed arrival threshold yet still represent a better ETA than the threshold-based winner.
        let new_step_idx = isochrones.len() - 1;
        for (idx, pt) in isochrones[new_step_idx].iter().enumerate() {
            let remaining = haversine_distance_nm(pt.lat, pt.lon, to.0, to.1);
            if remaining > MAX_FINAL_LEG_NM {
                continue;
            }
            let wind = crate::forecast::nearest_forecast_wind(&parsed_fetches, pt.lat, pt.lon, pt.time);
            let speed = speed_for_final_leg(
                pt.lat, pt.lon, to, wind,
                motoring_speed_kn, polar_efficiency, min_sail_speed_kn, polars,
            );
            let eta = pt.time + chrono::Duration::seconds(
                (remaining / speed * 3600.0).round() as i64,
            );
            if best.as_ref().map_or(true, |(b, _, _)| eta < *b)
                && !path_crosses_land((pt.lat, pt.lon), to, land_mask)
            {
                best = Some((eta, new_step_idx, idx));
            }
        }

        // Stagnation check: once the best ETA stops improving for STAGNANT_STEPS consecutive steps
        // the frontier has moved past the destination — terminate early. Only counts once a best
        // candidate exists at all (i.e. the frontier has come within MAX_FINAL_LEG_NM) — before
        // that, `best` is None every step by design and must not be mistaken for stagnation.
        let current_eta = best.as_ref().map(|(e, _, _)| *e);
        if current_eta.is_some() {
            if current_eta == last_improved_eta {
                stagnant += 1;
                if stagnant >= STAGNANT_STEPS {
                    break;
                }
            } else {
                stagnant = 0;
                last_improved_eta = current_eta;
            }
        }
    }

    if let Some((eta, step_idx, pt_idx)) = best {
        let track = backtrack(&isochrones[..=step_idx], pt_idx, to, eta);
        return IsochroneResult { track, reached_destination: true };
    }

    IsochroneResult { track: vec![], reached_destination: false }
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn prune_isochrone(
    candidates: Vec<IsochronePoint>,
    origin: (f64, f64),
    sail_weight_kn: f64,
) -> Vec<IsochronePoint> {
    debug_assert!(sail_weight_kn >= 0.0, "sail_weight_kn must be non-negative");
    let mut sectors: Vec<Option<IsochronePoint>> = vec![None; SECTOR_COUNT];
    let mut sector_score: Vec<f64> = vec![f64::NEG_INFINITY; SECTOR_COUNT];

    for pt in candidates {
        let bearing = haversine_heading(origin.0, origin.1, pt.lat, pt.lon);
        let sector = ((bearing / (360.0 / SECTOR_COUNT as f64)) as usize) % SECTOR_COUNT;
        let dist = haversine_distance_nm(origin.0, origin.1, pt.lat, pt.lon);
        let score = dist + sail_weight_kn * pt.sailed_hours;
        if score > sector_score[sector] {
            sector_score[sector] = score;
            sectors[sector] = Some(pt);
        }
    }

    sectors.into_iter().flatten().collect()
}

fn backtrack(
    isochrones: &[Vec<IsochronePoint>],
    arrival_idx: usize,
    destination: (f64, f64),
    destination_time: DateTime<Utc>,
) -> Vec<(f64, f64, DateTime<Utc>)> {
    let mut path: Vec<(f64, f64, DateTime<Utc>)> = Vec::new();

    path.push((destination.0, destination.1, destination_time));

    let mut cur_idx = arrival_idx;
    for step in (0..isochrones.len()).rev() {
        let pt = &isochrones[step][cur_idx];
        path.push((pt.lat, pt.lon, pt.time));
        match pt.parent_idx {
            Some(idx) => cur_idx = idx,
            None => break,
        }
    }

    path.reverse();
    path
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dummy_polars() -> crate::polars::PolarTable {
        crate::polars::PolarTable::constant_for_test(6.0)
    }

    #[test]
    fn test_prune_retains_at_most_72_points() {
        let origin = (43.0, 8.0);
        let candidates: Vec<IsochronePoint> = (0..720).map(|i| {
            let bearing = (i as f64) * 0.5;
            let dist = 5.0 + (i % 10) as f64;
            let (lat, lon) = advance_position(origin.0, origin.1, bearing, dist);
            IsochronePoint { lat, lon, time: chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(), sailed_hours: 0.0, parent_idx: Some(0) }
        }).collect();
        let pruned = prune_isochrone(candidates, origin, 0.0);
        assert!(pruned.len() <= SECTOR_COUNT, "expected ≤72, got {}", pruned.len());
    }

    #[test]
    fn test_isochrone_reaches_nearby_destination() {
        let from = (43.0, 8.0);
        let to = (43.29, 8.0);   // ~20 nm north
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let polars = dummy_polars();

        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, &polars, &[], None);
        assert!(result.reached_destination, "should reach destination in ~4h at 6 kn");
        assert!(result.track.len() >= 2);
        let last = result.track.last().unwrap();
        let dist = haversine_distance_nm(last.0, last.1, to.0, to.1);
        assert!(dist < 10.0, "last point is {}nm from destination", dist);
    }

    #[test]
    fn test_backtrack_produces_monotonic_timestamps() {
        let from = (43.0, 8.0);
        let to = (43.29, 8.0);
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let polars = dummy_polars();

        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, &polars, &[], None);
        let times: Vec<_> = result.track.iter().map(|p| p.2).collect();
        for w in times.windows(2) {
            assert!(w[1] >= w[0], "timestamps not monotonic: {:?} then {:?}", w[0], w[1]);
        }
    }

    #[test]
    fn test_sail_weight_prefers_sailing_candidate() {
        let origin = (43.0, 8.0);

        // Two candidates in the same sector (both heading north, 0°).
        // Motoring candidate: 10 nm from origin, no sail history.
        // Sailing candidate:   9 nm from origin, 1 hour of sail history.
        let motoring_pos = advance_position(origin.0, origin.1, 0.0, 10.0);
        let sailing_pos  = advance_position(origin.0, origin.1, 0.0, 9.0);
        let t = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();

        let motoring = IsochronePoint { lat: motoring_pos.0, lon: motoring_pos.1, time: t, sailed_hours: 0.0, parent_idx: None };
        let sailing  = IsochronePoint { lat: sailing_pos.0,  lon: sailing_pos.1,  time: t, sailed_hours: 1.0, parent_idx: None };

        // sail_weight_kn = 0 → raw distance wins → motoring (10 nm) beats sailing (9 nm)
        let result_zero = prune_isochrone(vec![motoring.clone(), sailing.clone()], origin, 0.0);
        assert_eq!(result_zero.len(), 1);
        let d = haversine_distance_nm(result_zero[0].lat, result_zero[0].lon, origin.0, origin.1);
        assert!((d - 10.0).abs() < 0.5, "expected motoring winner at ~10 nm, got {:.2}", d);

        // sail_weight_kn = 2.0 → sailing score = 9 + 2×1 = 11 > motoring score = 10 + 2×0 = 10
        let result_biased = prune_isochrone(vec![motoring, sailing], origin, 2.0);
        assert_eq!(result_biased.len(), 1);
        let d = haversine_distance_nm(result_biased[0].lat, result_biased[0].lon, origin.0, origin.1);
        assert!((d - 9.0).abs() < 0.5, "expected sailing winner at ~9 nm, got {:.2}", d);
    }

    #[test]
    fn test_land_mask_blocks_candidate() {
        let json = serde_json::json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [[[7.0, 43.0], [17.0, 43.0], [17.0, 53.0], [7.0, 53.0], [7.0, 43.0]]]
                },
                "properties": {}
            }]
        });
        let mask = crate::land_mask::LandMask::from_geojson_value_for_test(&json, 0.1).unwrap();

        let from = (42.0, 8.0);
        let to   = (43.29, 8.0);
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let polars = dummy_polars();

        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, &polars, &[], Some(&mask));
        assert!(!result.reached_destination, "should not reach destination blocked by land");
    }

    #[test]
    fn test_final_leg_does_not_cross_land() {
        // A thin land strip directly between `from` and `to`, far too long to sail around
        // within the isochrone's stagnation window. A direct line from any point near the
        // strip's western shore to `to` would cross it — the router must not claim that leg.
        let json = serde_json::json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [[[8.2, 30.0], [8.3, 30.0], [8.3, 55.0], [8.2, 55.0], [8.2, 30.0]]]
                },
                "properties": {}
            }]
        });
        let mask = crate::land_mask::LandMask::from_geojson_value_for_test(&json, 0.05).unwrap();

        let from = (43.0, 8.0);
        let to = (43.0, 8.5);
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let polars = dummy_polars();

        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, &polars, &[], Some(&mask));

        for w in result.track.windows(2) {
            assert!(
                !path_crosses_land((w[0].0, w[0].1), (w[1].0, w[1].1), Some(&mask)),
                "segment from {:?} to {:?} crosses land", w[0], w[1]
            );
        }
    }

    #[test]
    fn test_final_leg_shortcut_respects_max_distance() {
        // A short land barrier forces a brief detour. Once clear of it, ~27 nm still remain to
        // the destination — that must be closed via real 30-min steps, not a single "beeline"
        // that assumes the whole remaining distance is coverable at a projected speed just
        // because nothing currently in front of it happens to be land.
        let json = serde_json::json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [[[8.2, 42.95], [8.3, 42.95], [8.3, 43.05], [8.2, 43.05], [8.2, 42.95]]]
                },
                "properties": {}
            }]
        });
        let mask = crate::land_mask::LandMask::from_geojson_value_for_test(&json, 0.05).unwrap();

        let from = (43.0, 8.0);
        let to = (43.0, 8.8);
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let polars = dummy_polars();

        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, &polars, &[], Some(&mask));
        assert!(result.reached_destination, "should route around the short barrier and arrive");

        for w in result.track.windows(2) {
            let d = haversine_distance_nm(w[0].0, w[0].1, w[1].0, w[1].1);
            assert!(
                d <= MAX_FINAL_LEG_NM + 1.0,
                "segment from {:?} to {:?} covers {:.2} nm, exceeding the final-leg cap", w[0], w[1], d
            );
        }
    }
}
