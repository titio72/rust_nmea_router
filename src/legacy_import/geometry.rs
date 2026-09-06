use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Europe::Rome;

/// Converts a naive Europe/Rome local timestamp (as stored by the legacy
/// `nmearouter` database) to UTC, DST-aware.
pub fn rome_local_to_utc(naive: NaiveDateTime) -> DateTime<Utc> {
    match Rome.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc),
        // Fall-back DST transition (clocks repeat an hour): pick the earlier
        // (still-summer) offset, consistent with how the ambiguous hour is
        // ordered in the source data.
        chrono::LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc),
        // Spring-forward DST transition (this local time never existed):
        // shift forward by an hour and resolve again.
        chrono::LocalResult::None => {
            let adjusted = naive + chrono::Duration::hours(1);
            match Rome.from_local_datetime(&adjusted) {
                chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc),
                _ => Utc.from_utc_datetime(&naive),
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Reading {
    pub ts: NaiveDateTime,
    pub value: f64,
}

/// Nearest same-metric reading to `target`, only if within `bound_secs`.
/// `readings` must be sorted ascending by `ts`.
pub fn nearest_reading(readings: &[Reading], target: NaiveDateTime, bound_secs: i64) -> Option<f64> {
    if readings.is_empty() {
        return None;
    }
    let idx = readings.partition_point(|r| r.ts <= target);
    let mut best: Option<(i64, f64)> = None;
    if idx > 0 {
        let r = &readings[idx - 1];
        best = Some(((target - r.ts).num_seconds().abs(), r.value));
    }
    if idx < readings.len() {
        let r = &readings[idx];
        let diff = (r.ts - target).num_seconds().abs();
        if best.map_or(true, |(bd, _)| diff < bd) {
            best = Some((diff, r.value));
        }
    }
    best.filter(|(diff, _)| *diff <= bound_secs).map(|(_, v)| v)
}

/// Underway: COG = bearing from the previous position to the current one
/// (`crate::utilities::haversine_heading`), heading = COG. Moored, or no
/// previous position: both `None`.
pub fn derive_cog_heading(prev: Option<(f64, f64)>, cur: (f64, f64), is_moored: bool) -> (Option<f64>, Option<f64>) {
    if is_moored {
        return (None, None);
    }
    match prev {
        Some((plat, plon)) => {
            let cog = crate::utilities::haversine_heading(plat, plon, cur.0, cur.1);
            (Some(cog), Some(cog))
        }
        None => (None, None),
    }
}

/// True Wind Angle (boat-relative) from True Wind Direction (compass-
/// referenced) and course over ground: `(twd - cog) mod 360`.
pub fn derive_wind_angle(twd_deg: Option<f64>, cog_deg: Option<f64>) -> Option<f64> {
    match (twd_deg, cog_deg) {
        (Some(twd), Some(cog)) => Some(((twd - cog) % 360.0 + 360.0) % 360.0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn test_rome_local_to_utc_winter_offset_is_one_hour() {
        // Empirically confirmed: nmearouter's 2020-01-03 07:43:03 local is the
        // same instant as live nmea_router trip id=2's start, 2020-01-03T06:43:03Z.
        let naive = NaiveDate::from_ymd_opt(2020, 1, 3).unwrap().and_hms_opt(7, 43, 3).unwrap();
        let utc = rome_local_to_utc(naive);
        assert_eq!(utc.to_rfc3339(), "2020-01-03T06:43:03+00:00");
    }

    #[test]
    fn test_rome_local_to_utc_summer_offset_is_two_hours() {
        let naive = NaiveDate::from_ymd_opt(2020, 7, 15).unwrap().and_hms_opt(12, 0, 0).unwrap();
        let utc = rome_local_to_utc(naive);
        assert_eq!(utc.to_rfc3339(), "2020-07-15T10:00:00+00:00");
    }

    #[test]
    fn test_nearest_reading_picks_closest_within_bound() {
        let readings = vec![
            Reading { ts: mk_ts(10, 0, 0), value: 5.0 },
            Reading { ts: mk_ts(10, 5, 0), value: 8.0 },
            Reading { ts: mk_ts(10, 10, 0), value: 12.0 },
        ];
        // Target is 2 minutes after the middle reading, 7 minutes before the last.
        let target = mk_ts(10, 7, 0);
        assert_eq!(nearest_reading(&readings, target, 300), Some(8.0));
    }

    #[test]
    fn test_nearest_reading_outside_bound_returns_none() {
        let readings = vec![Reading { ts: mk_ts(10, 0, 0), value: 5.0 }];
        let target = mk_ts(10, 30, 0); // 30 minutes away
        assert_eq!(nearest_reading(&readings, target, 60), None);
    }

    #[test]
    fn test_nearest_reading_empty_list_returns_none() {
        let target = mk_ts(10, 0, 0);
        assert_eq!(nearest_reading(&[], target, 300), None);
    }

    #[test]
    fn test_derive_cog_heading_underway_uses_bearing() {
        // Due north: bearing should be ~0 degrees.
        let (cog, hdg) = derive_cog_heading(Some((43.0, 10.0)), (43.01, 10.0), false);
        assert!(cog.unwrap().abs() < 0.5);
        assert_eq!(cog, hdg);
    }

    #[test]
    fn test_derive_cog_heading_moored_is_none() {
        let (cog, hdg) = derive_cog_heading(Some((43.0, 10.0)), (43.0, 10.0), true);
        assert_eq!(cog, None);
        assert_eq!(hdg, None);
    }

    #[test]
    fn test_derive_cog_heading_no_previous_position_is_none() {
        let (cog, hdg) = derive_cog_heading(None, (43.0, 10.0), false);
        assert_eq!(cog, None);
        assert_eq!(hdg, None);
    }

    #[test]
    fn test_derive_wind_angle_matches_empirical_formula() {
        // From live overlap-window data: TWD=213.34385, COG=3.304 => TWA=210.040
        let twa = derive_wind_angle(Some(213.34384954985424), Some(3.304)).unwrap();
        assert!((twa - 210.040).abs() < 0.01);
    }

    #[test]
    fn test_derive_wind_angle_wraps_below_zero() {
        // TWD < COG must wrap around 360, not go negative.
        let twa = derive_wind_angle(Some(10.0), Some(350.0)).unwrap();
        assert!((twa - 20.0).abs() < 1e-9);
    }

    #[test]
    fn test_derive_wind_angle_none_without_cog() {
        assert_eq!(derive_wind_angle(Some(200.0), None), None);
    }

    fn mk_ts(h: u32, m: u32, s: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2022, 6, 5).unwrap().and_hms_opt(h, m, s).unwrap()
    }
}
