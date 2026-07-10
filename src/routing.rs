use crate::db::operations::forecast::FetchWithHourly;
use crate::forecast::compute_twa;
use crate::utilities::{advance_position, haversine_distance_nm, haversine_heading};
use chrono::{DateTime, Utc};

// ── Constants ─────────────────────────────────────────────────────────────────

const HEADING_STEP_DEG: f64 = 2.0;
// Full circle sampled at HEADING_STEP_DEG resolution; also doubles as the frontier's
// angular pruning bin count, so it must stay in lockstep with HEADING_STEP_DEG.
const SECTOR_COUNT: usize = (360.0 / HEADING_STEP_DEG) as usize;
const MAX_STEPS: usize = 336; // 336 × 30 min = 168 h (7 days)
const STEP_HOURS: f64 = 0.5;
// Stop expanding once the best direct-to-destination ETA hasn't improved for this many steps.
const STAGNANT_STEPS: usize = 6; // 6 × 30 min = 3 h without improvement

// ── Data structures ───────────────────────────────────────────────────────────

#[derive(Clone)]
struct IsochronePoint {
    lat: f64,
    lon: f64,
    time: DateTime<Utc>,
    sailed_hours: f64,
    parent_idx: Option<usize>,
    // Leg data for the hop from parent -> this point; meaningless on the seed point.
    speed_kn: f64,
    motoring: bool,
    wind_speed_kn: Option<f64>,
    wind_dir_deg: Option<f64>,
    relative_wind_deg: Option<f64>,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct FrontierPoint {
    pub lat: f64,
    pub lon: f64,
    pub parent_idx: usize,
    pub speed_kn: f64,
    pub motoring: bool,
    pub wind_speed_kn: Option<f64>,
    pub wind_dir_deg: Option<f64>,
    pub relative_wind_deg: Option<f64>,
}

pub struct IsochroneResult {
    pub track: Vec<(f64, f64, DateTime<Utc>)>,
    pub reached_destination: bool,
    pub frontiers: Vec<Vec<FrontierPoint>>,
}

// ── Public function ───────────────────────────────────────────────────────────

pub fn run_isochrone(
    from: (f64, f64),
    to: (f64, f64),
    departure: DateTime<Utc>,
    motoring_speed_kn: f64,
    polar_efficiency: f64,
    min_sail_speed_kn: f64,
    min_twa_deg: f64,
    sail_weight_kn: f64,
    polars: &crate::polars::PolarTable,
    fetches: &[FetchWithHourly],
    land_mask: Option<&crate::land_mask::LandMask>,
) -> IsochroneResult {
    if motoring_speed_kn <= 0.0 {
        return IsochroneResult {
            track: vec![],
            reached_destination: false,
            frontiers: vec![],
        };
    }
    if land_mask.map_or(false, |m| m.is_land(to.0, to.1)) {
        return IsochroneResult {
            track: vec![],
            reached_destination: false,
            frontiers: vec![],
        };
    }

    // Parsed once and reused for every wind lookup below — the alternative (re-parsing every
    // hourly timestamp string on every lookup) dominates runtime once the search does the full
    // number of real 30-min steps a route needs.
    let parsed_fetches = crate::forecast::parse_fetches(fetches);

    let seed = IsochronePoint {
        lat: from.0,
        lon: from.1,
        time: departure,
        sailed_hours: 0.0,
        parent_idx: None,
        speed_kn: 0.0,
        motoring: false,
        wind_speed_kn: None,
        wind_dir_deg: None,
        relative_wind_deg: None,
    };
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
            let wind = crate::forecast::nearest_forecast_wind(
                &parsed_fetches,
                parent.lat,
                parent.lon,
                parent.time,
            );
            let dist_to_dest = haversine_distance_nm(parent.lat, parent.lon, to.0, to.1);
            let bearing_to_dest = haversine_heading(parent.lat, parent.lon, to.0, to.1);

            for h in 0..SECTOR_COUNT {
                let heading = h as f64 * HEADING_STEP_DEG;
                let relative_wind_deg = wind.map(|(_, wind_dir)| compute_twa(heading, wind_dir));

                let (speed_kn, was_sailing) = match wind {
                    Some((wind_spd, wind_dir)) if wind_spd > 0.0 => {
                        let twa = compute_twa(heading, wind_dir);
                        if twa < min_twa_deg {
                            continue;
                        }
                        match polars.boat_speed(twa, wind_spd).filter(|&s| s > 0.0) {
                            Some(raw) => {
                                let eff = raw * polar_efficiency;
                                if eff >= min_sail_speed_kn {
                                    (eff, true)
                                } else {
                                    (motoring_speed_kn, false)
                                }
                            }
                            None => (motoring_speed_kn, false),
                        }
                    }
                    _ => (motoring_speed_kn, false),
                };

                let new_pos =
                    advance_position(parent.lat, parent.lon, heading, speed_kn * STEP_HOURS);
                if land_mask.map_or(false, |m| m.is_land(new_pos.0, new_pos.1)) {
                    continue;
                }

                // Genuine arrival only: this heading is one of the real, physically-computed
                // candidates above (wind/polar-derived, tacking angles included, land-checked)
                // — not an assumed direct-bearing shortcut. It counts as reaching the
                // destination only if it's both aimed at the destination (snapped to the
                // nearest of the discretely-sampled headings) and fast enough to cover the
                // remaining distance within this step. If the true bearing happens to be
                // unsailable, arrival simply isn't declared yet — the search must keep tacking
                // toward it over further real steps, exactly like a real sailor would.
                let heading_diff = ((heading - bearing_to_dest + 540.0) % 360.0 - 180.0).abs();
                if heading_diff <= HEADING_STEP_DEG / 2.0 && dist_to_dest <= speed_kn * STEP_HOURS {
                    let eta = parent.time
                        + chrono::Duration::seconds(
                            (dist_to_dest / speed_kn * 3600.0).round() as i64
                        );
                    if best.as_ref().map_or(true, |(b, _, _)| eta < *b) {
                        best = Some((eta, prev_step_idx, parent_idx));
                    }
                }

                candidates.push(IsochronePoint {
                    lat: new_pos.0,
                    lon: new_pos.1,
                    time: parent.time + chrono::Duration::minutes((STEP_HOURS * 60.0) as i64),
                    sailed_hours: parent.sailed_hours + if was_sailing { STEP_HOURS } else { 0.0 },
                    parent_idx: Some(parent_idx),
                    speed_kn,
                    motoring: !was_sailing,
                    wind_speed_kn: wind.map(|(spd, _)| spd),
                    wind_dir_deg: wind.map(|(_, dir)| dir),
                    relative_wind_deg,
                });
            }
        }

        isochrones.push(prune_isochrone(candidates, from, sail_weight_kn));

        // Stagnation check: once the best ETA stops improving for STAGNANT_STEPS consecutive steps
        // the frontier has moved past the destination — terminate early. Only counts once a best
        // candidate exists at all (i.e. some parent has come within genuine arrival range) —
        // before that, `best` is None every step by design and must not be mistaken for stagnation.
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

    // Every step's frontier after the seed (step 0, which is just the origin). parent_idx
    // indexes into the previous step's frontier (or, for step 0 of this slice, always 0,
    // meaning "the search origin" — the seed itself is never exposed as its own frontier
    // entry). Every point here comes from `isochrones[1..]`, so `parent_idx` is always
    // `Some(_)` by construction (only the seed at `isochrones[0]` has `None`).
    let frontiers: Vec<Vec<FrontierPoint>> = isochrones[1..]
        .iter()
        .map(|frontier| {
            frontier
                .iter()
                .map(|p| FrontierPoint {
                    lat: p.lat,
                    lon: p.lon,
                    parent_idx: p.parent_idx.unwrap(),
                    speed_kn: p.speed_kn,
                    motoring: p.motoring,
                    wind_speed_kn: p.wind_speed_kn,
                    wind_dir_deg: p.wind_dir_deg,
                    relative_wind_deg: p.relative_wind_deg,
                })
                .collect()
        })
        .collect();

    if let Some((eta, step_idx, pt_idx)) = best {
        let track = backtrack(&isochrones[..=step_idx], pt_idx, to, eta);
        return IsochroneResult {
            track,
            reached_destination: true,
            frontiers,
        };
    }

    IsochroneResult {
        track: vec![],
        reached_destination: false,
        frontiers,
    }
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
        let candidates: Vec<IsochronePoint> = (0..720)
            .map(|i| {
                let bearing = (i as f64) * 0.5;
                let dist = 5.0 + (i % 10) as f64;
                let (lat, lon) = advance_position(origin.0, origin.1, bearing, dist);
                IsochronePoint {
                    lat,
                    lon,
                    time: chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
                    sailed_hours: 0.0,
                    parent_idx: Some(0),
                    speed_kn: 0.0,
                    motoring: false,
                    wind_speed_kn: None,
                    wind_dir_deg: None,
                    relative_wind_deg: None,
                }
            })
            .collect();
        let pruned = prune_isochrone(candidates, origin, 0.0);
        assert!(
            pruned.len() <= SECTOR_COUNT,
            "expected ≤72, got {}",
            pruned.len()
        );
    }

    #[test]
    fn test_isochrone_reaches_nearby_destination() {
        let from = (43.0, 8.0);
        let to = (43.29, 8.0); // ~20 nm north
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let polars = dummy_polars();

        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, 0.0, &polars, &[], None);
        assert!(
            result.reached_destination,
            "should reach destination in ~4h at 6 kn"
        );
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

        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, 0.0, &polars, &[], None);
        let times: Vec<_> = result.track.iter().map(|p| p.2).collect();
        for w in times.windows(2) {
            assert!(
                w[1] >= w[0],
                "timestamps not monotonic: {:?} then {:?}",
                w[0],
                w[1]
            );
        }
    }

    #[test]
    fn test_sail_weight_prefers_sailing_candidate() {
        let origin = (43.0, 8.0);

        // Two candidates in the same sector (both heading north, 0°).
        // Motoring candidate: 10 nm from origin, no sail history.
        // Sailing candidate:   9 nm from origin, 1 hour of sail history.
        let motoring_pos = advance_position(origin.0, origin.1, 0.0, 10.0);
        let sailing_pos = advance_position(origin.0, origin.1, 0.0, 9.0);
        let t = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();

        let motoring = IsochronePoint {
            lat: motoring_pos.0,
            lon: motoring_pos.1,
            time: t,
            sailed_hours: 0.0,
            parent_idx: None,
            speed_kn: 0.0,
            motoring: true,
            wind_speed_kn: None,
            wind_dir_deg: None,
            relative_wind_deg: None,
        };
        let sailing = IsochronePoint {
            lat: sailing_pos.0,
            lon: sailing_pos.1,
            time: t,
            sailed_hours: 1.0,
            parent_idx: None,
            speed_kn: 0.0,
            motoring: false,
            wind_speed_kn: None,
            wind_dir_deg: None,
            relative_wind_deg: None,
        };

        // sail_weight_kn = 0 → raw distance wins → motoring (10 nm) beats sailing (9 nm)
        let result_zero = prune_isochrone(vec![motoring.clone(), sailing.clone()], origin, 0.0);
        assert_eq!(result_zero.len(), 1);
        let d = haversine_distance_nm(result_zero[0].lat, result_zero[0].lon, origin.0, origin.1);
        assert!(
            (d - 10.0).abs() < 0.5,
            "expected motoring winner at ~10 nm, got {:.2}",
            d
        );

        // sail_weight_kn = 2.0 → sailing score = 9 + 2×1 = 11 > motoring score = 10 + 2×0 = 10
        let result_biased = prune_isochrone(vec![motoring, sailing], origin, 2.0);
        assert_eq!(result_biased.len(), 1);
        let d = haversine_distance_nm(
            result_biased[0].lat,
            result_biased[0].lon,
            origin.0,
            origin.1,
        );
        assert!(
            (d - 9.0).abs() < 0.5,
            "expected sailing winner at ~9 nm, got {:.2}",
            d
        );
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
        let to = (43.29, 8.0);
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let polars = dummy_polars();

        let result = run_isochrone(
            from,
            to,
            departure,
            6.0,
            1.0,
            0.0,
            0.0,
            0.0,
            &polars,
            &[],
            Some(&mask),
        );
        assert!(
            !result.reached_destination,
            "should not reach destination blocked by land"
        );
    }

    /// A polar that can't sail closer than 42° to the true wind (matches `constant_for_test`'s
    /// breakpoints) but sails at a flat `speed_kn` on any sailable heading.
    fn upwind_polars(speed_kn: f64) -> crate::polars::PolarTable {
        crate::polars::PolarTable::constant_for_test(speed_kn)
    }

    #[test]
    fn test_arrival_prefers_tacking_over_forced_motor() {
        // Destination is due north — dead upwind, since the wind blows from the north too.
        // Sailing at the true bearing is impossible (TWA 0° < the polar's 42° minimum), but
        // tacking at 45° off the wind sails at 6 kn, well above the 3 kn motoring speed. A
        // router that only checks the single direct bearing for its final approach would be
        // forced to motor the last stretch despite the wind; a genuinely tacking-aware router
        // keeps making fast progress via real headings all the way to arrival.
        let from = (43.0, 8.0);
        let to = (43.2, 8.0); // ~12 nm due north
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let motoring_speed_kn = 3.0;
        let polars = upwind_polars(6.0);

        let fetches = vec![crate::db::operations::forecast::FetchWithHourly {
            lat: 43.0,
            lon: 8.0,
            model: "ecmwf".to_string(),
            hourly: (0..48)
                .map(|h| crate::db::operations::forecast::ForecastHourlyPoint {
                    timestamp: (departure + chrono::Duration::hours(h))
                        .format("%Y-%m-%dT%H:%M:%SZ")
                        .to_string(),
                    wind_speed_kn: Some(10.0),
                    wind_direction_deg: Some(0.0), // wind from the north, blowing toward the south
                    wind_gust_kn: None,
                    wave_height_m: None,
                    wave_period_s: None,
                    wave_direction_deg: None,
                    cape_j_kg: None,
                })
                .collect(),
        }];

        let result = run_isochrone(
            from,
            to,
            departure,
            motoring_speed_kn,
            1.0,
            0.0,
            0.0,
            0.0,
            &polars,
            &fetches,
            None,
        );
        assert!(
            result.reached_destination,
            "should reach destination via tacking"
        );

        let total_hours =
            (result.track.last().unwrap().2 - departure).num_seconds() as f64 / 3600.0;
        let motoring_only_hours =
            haversine_distance_nm(from.0, from.1, to.0, to.1) / motoring_speed_kn;
        assert!(
            total_hours < motoring_only_hours,
            "expected tacking ({:.2}h) to beat pure motoring ({:.2}h)",
            total_hours,
            motoring_only_hours
        );
    }

    #[test]
    fn test_frontiers_exclude_seed_and_respect_sector_count() {
        let from = (43.0, 8.0);
        let to = (43.29, 8.0); // ~20 nm north
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let polars = dummy_polars();

        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, 0.0, &polars, &[], None);

        assert!(
            !result.frontiers.is_empty(),
            "expected at least one frontier to be recorded"
        );
        for frontier in &result.frontiers {
            assert!(
                frontier.len() <= SECTOR_COUNT,
                "frontier has {} points, expected <= {}",
                frontier.len(),
                SECTOR_COUNT
            );
            assert!(!frontier.is_empty(), "frontier must not be empty");
            assert!(
                frontier.iter().all(|p| p.relative_wind_deg.is_none()),
                "no wind data supplied, relative_wind_deg should stay None"
            );
        }
        // The seed step is a single point at the origin — it must not appear as a "frontier".
        assert!(
            !result
                .frontiers
                .iter()
                .any(|f| f.len() == 1 && (f[0].lat, f[0].lon) == from),
            "seed (origin-only) frontier should be excluded"
        );
    }

    #[test]
    fn test_frontier_parent_idx_chains_to_origin() {
        let from = (43.0, 8.0);
        let to = (43.29, 8.0); // ~20 nm north
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let polars = dummy_polars();

        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, 0.0, &polars, &[], None);
        assert!(!result.frontiers.is_empty());

        // Each point's parent_idx must be a valid index into the previous step (or 0 for
        // step 0, meaning "the origin").
        for (s, frontier) in result.frontiers.iter().enumerate() {
            let prev_len = if s == 0 {
                1 // seed step, never exposed, exactly one point
            } else {
                result.frontiers[s - 1].len()
            };
            for pt in frontier {
                assert!(
                    pt.parent_idx < prev_len,
                    "step {} point has parent_idx {} but previous step has {} points",
                    s,
                    pt.parent_idx,
                    prev_len
                );
            }
        }

        // Walking parent_idx from any point in the last frontier must terminate at step 0
        // index 0 within frontiers.len() hops.
        let last_step = result.frontiers.len() - 1;
        let mut s = last_step;
        let mut i = 0usize; // first point in last frontier
        let mut hops = 0usize;
        loop {
            let pt = result.frontiers[s][i];
            if s == 0 {
                assert_eq!(pt.parent_idx, 0, "step 0 parent_idx is always 0 (origin)");
                break;
            }
            i = pt.parent_idx;
            s -= 1;
            hops += 1;
            assert!(
                hops <= result.frontiers.len(),
                "parent_idx chain did not terminate within frontiers.len() hops"
            );
        }
    }

    #[test]
    fn test_frontier_relative_wind_deg_matches_heading_and_wind() {
        use crate::db::operations::forecast::{FetchWithHourly, ForecastHourlyPoint};

        let from = (43.0, 8.0);
        let to = (43.29, 8.0); // ~20 nm north
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let polars = dummy_polars();

        let hourly = vec![ForecastHourlyPoint {
            timestamp: departure.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            wind_speed_kn: Some(12.0),
            wind_direction_deg: Some(90.0), // wind from due east
            wind_gust_kn: None, wave_height_m: None, wave_period_s: None,
            wave_direction_deg: None, cape_j_kg: None,
        }];
        let fetches = vec![FetchWithHourly { lat: 43.0, lon: 8.0, model: "ecmwf".to_string(), hourly }];

        let result = run_isochrone(from, to, departure, 6.0, 1.0, 0.0, 0.0, 0.0, &polars, &fetches, None);
        assert!(!result.frontiers.is_empty());

        let first_step = &result.frontiers[0];
        let mut checked_any = false;
        for pt in first_step {
            if let (Some(rel), Some(wind_dir)) = (pt.relative_wind_deg, pt.wind_dir_deg) {
                let bearing = haversine_heading(from.0, from.1, pt.lat, pt.lon);
                let expected = crate::forecast::compute_twa(bearing, wind_dir);
                assert!(
                    (rel - expected).abs() < 1.0,
                    "relative_wind_deg {} doesn't match compute_twa(heading, wind_dir) {}",
                    rel, expected
                );
                checked_any = true;
            }
        }
        assert!(checked_any, "expected at least one candidate with wind data and relative_wind_deg set");
    }

    #[test]
    fn test_min_twa_deg_excludes_heading_never_motors_through_headwind() {
        // Motoring is fast (5 kn); the only sailable headings once min_twa_deg = 80.0 excludes
        // anything closer to the wind have a poor VMG toward this dead-upwind destination
        // (6 kn boat speed * cos(80°) ≈ 1.04 kn) — much slower than motoring. If the router
        // ever substituted "motor" for an excluded heading (the bug this test guards against),
        // it would simply motor the direct heading and reach in ~motoring_only_hours. The
        // correct behavior is to never offer that shortcut: the excluded heading must simply
        // not exist as a candidate, forcing a much slower sail-only route around the headwind.
        let from = (43.0, 8.0);
        let to = (43.2, 8.0); // ~12 nm due north
        let departure = chrono::Utc.with_ymd_and_hms(2026, 6, 1, 6, 0, 0).unwrap();
        let motoring_speed_kn = 5.0;
        let polars = upwind_polars(6.0);

        let fetches = vec![crate::db::operations::forecast::FetchWithHourly {
            lat: 43.0,
            lon: 8.0,
            model: "ecmwf".to_string(),
            hourly: (0..48)
                .map(|h| crate::db::operations::forecast::ForecastHourlyPoint {
                    timestamp: (departure + chrono::Duration::hours(h))
                        .format("%Y-%m-%dT%H:%M:%SZ")
                        .to_string(),
                    wind_speed_kn: Some(10.0),
                    wind_direction_deg: Some(0.0), // wind from the north, blowing toward the south
                    wind_gust_kn: None,
                    wave_height_m: None,
                    wave_period_s: None,
                    wave_direction_deg: None,
                    cape_j_kg: None,
                })
                .collect(),
        }];

        let result = run_isochrone(
            from,
            to,
            departure,
            motoring_speed_kn,
            1.0,
            0.0,
            80.0,
            0.0,
            &polars,
            &fetches,
            None,
        );
        assert!(
            result.reached_destination,
            "should still reach destination via slow tacking"
        );

        let total_hours =
            (result.track.last().unwrap().2 - departure).num_seconds() as f64 / 3600.0;
        let motoring_only_hours =
            haversine_distance_nm(from.0, from.1, to.0, to.1) / motoring_speed_kn;
        assert!(
            total_hours > motoring_only_hours * 1.5,
            "expected much slower than pure motoring ({:.2}h) since min_twa_deg=80 must exclude \
             the headwind heading rather than motor through it, got {:.2}h",
            motoring_only_hours,
            total_hours
        );
    }
}
