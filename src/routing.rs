use chrono::{DateTime, Utc};
use crate::db::operations::forecast::FetchWithHourly;
use crate::forecast::compute_twa;
use crate::utilities::{advance_position, haversine_distance_nm, haversine_heading};

// ── Constants ─────────────────────────────────────────────────────────────────

const HEADING_STEP_DEG: f64 = 5.0;
const SECTOR_COUNT: usize = 72;
const MAX_STEPS: usize = 168;
const ARRIVAL_THRESHOLD_NM: f64 = 5.0;

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
) -> IsochroneResult {
    if motoring_speed_kn <= 0.0 {
        return IsochroneResult { track: vec![], reached_destination: false };
    }

    let seed = IsochronePoint { lat: from.0, lon: from.1, time: departure, sailed_hours: 0.0, parent_idx: None };
    let mut isochrones: Vec<Vec<IsochronePoint>> = vec![vec![seed]];

    for _step in 1..=MAX_STEPS {
        let prev = isochrones.last().unwrap();
        let mut candidates: Vec<IsochronePoint> = Vec::new();

        for (parent_idx, parent) in prev.iter().enumerate() {
            let wind = crate::forecast::nearest_forecast_wind(fetches, parent.lat, parent.lon, parent.time);

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

                let new_pos = advance_position(parent.lat, parent.lon, heading, speed_kn);
                candidates.push(IsochronePoint {
                    lat: new_pos.0,
                    lon: new_pos.1,
                    time: parent.time + chrono::Duration::hours(1),
                    sailed_hours: parent.sailed_hours + if was_sailing { 1.0 } else { 0.0 },
                    parent_idx: Some(parent_idx),
                });
            }
        }

        isochrones.push(prune_isochrone(candidates, from, sail_weight_kn));

        for (idx, pt) in isochrones.last().unwrap().iter().enumerate() {
            if haversine_distance_nm(pt.lat, pt.lon, to.0, to.1) <= ARRIVAL_THRESHOLD_NM {
                let track = backtrack(&isochrones, idx, to);
                return IsochroneResult { track, reached_destination: true };
            }
        }
    }

    // Destination not reached — best-effort track to closest point
    let last_iso = isochrones.last().unwrap();
    if last_iso.is_empty() {
        return IsochroneResult { track: vec![], reached_destination: false };
    }
    let best_idx = last_iso
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            haversine_distance_nm(a.lat, a.lon, to.0, to.1)
                .partial_cmp(&haversine_distance_nm(b.lat, b.lon, to.0, to.1))
                .unwrap()
        })
        .map(|(i, _)| i)
        .unwrap_or(0);

    let track = backtrack(&isochrones, best_idx, to);
    IsochroneResult { track, reached_destination: false }
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
) -> Vec<(f64, f64, DateTime<Utc>)> {
    let mut path: Vec<(f64, f64, DateTime<Utc>)> = Vec::new();

    let arrival = &isochrones.last().unwrap()[arrival_idx];
    path.push((destination.0, destination.1, arrival.time));

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

        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, &polars, &[]);
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

        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, &polars, &[]);
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
}
