use crate::db::operations::forecast::{FetchWithHourly, ForecastHourlyPoint, TripForecastInputs};
use crate::error::AppError;
use crate::utilities::haversine_distance_nm;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

// ── Public API types ──────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct FetchedForecast {
    pub lat: f64,
    pub lon: f64,
    pub fetched_at: DateTime<Utc>,
    pub hourly: Vec<ForecastHourlyPoint>,
}

#[derive(Debug, serde::Serialize, Clone)]
pub struct TripOverlayPoint {
    pub timestamp: String,
    pub wind_speed_kn: Option<f64>,
    pub wind_direction_deg: Option<f64>,
    pub wind_gust_kn: Option<f64>,
    pub wave_height_m: Option<f64>,
    pub wave_period_s: Option<f64>,
    pub wave_direction_deg: Option<f64>,
    pub cape_j_kg: Option<f64>,
}

#[derive(Debug, serde::Serialize, Clone)]
pub struct RouteOverlayPoint {
    pub lat: f64,
    pub lon: f64,
    pub timestamp: String,
    pub wind_speed_kn: Option<f64>,
    pub wind_direction_deg: Option<f64>,
    pub wind_gust_kn: Option<f64>,
    pub wave_height_m: Option<f64>,
    pub wave_period_s: Option<f64>,
    pub wave_direction_deg: Option<f64>,
    pub cape_j_kg: Option<f64>,
}

// ── Open-Meteo deserialisation types (private) ────────────────────────────────

#[derive(Debug, Deserialize)]
struct MeteoHourly {
    time: Vec<String>,
    wind_speed_10m: Option<Vec<Option<f64>>>,
    wind_direction_10m: Option<Vec<Option<f64>>>,
    wind_gusts_10m: Option<Vec<Option<f64>>>,
    cape: Option<Vec<Option<f64>>>,
}

#[derive(Debug, Deserialize)]
struct MeteoResponse {
    latitude: f64,
    longitude: f64,
    hourly: MeteoHourly,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct MarineHourly {
    time: Vec<String>,
    wave_height: Option<Vec<Option<f64>>>,
    wave_period: Option<Vec<Option<f64>>>,
    wave_direction: Option<Vec<Option<f64>>>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct MarineResponse {
    latitude: f64,
    longitude: f64,
    hourly: MarineHourly,
}

/// Handles both single-object and array responses from Open-Meteo.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

impl<T> OneOrMany<T> {
    fn into_vec(self) -> Vec<T> {
        match self {
            Self::One(v) => vec![v],
            Self::Many(v) => v,
        }
    }
}

// ── Constants ─────────────────────────────────────────────────────────────────

const MAX_DISTANCE_NM: f64 = 25.0;

// ── Public functions ──────────────────────────────────────────────────────────

pub(crate) fn build_meteo_bbox_url(lat_min: f64, lat_max: f64, lon_min: f64, lon_max: f64) -> String {
    format!(
        "https://api.open-meteo.com/v1/forecast\
         ?latitude_min={lat_min}&latitude_max={lat_max}\
         &longitude_min={lon_min}&longitude_max={lon_max}\
         &models=ecmwf_ifs\
         &hourly=wind_speed_10m,wind_direction_10m,wind_gusts_10m,cape\
         &wind_speed_unit=kn&forecast_days=7&timezone=UTC",
        lat_min = lat_min, lat_max = lat_max,
        lon_min = lon_min, lon_max = lon_max,
    )
}

pub(crate) fn build_marine_bbox_url(lat_min: f64, lat_max: f64, lon_min: f64, lon_max: f64) -> String {
    format!(
        "https://marine-api.open-meteo.com/v1/marine\
         ?latitude_min={lat_min}&latitude_max={lat_max}\
         &longitude_min={lon_min}&longitude_max={lon_max}\
         &models=ecmwf_wam\
         &hourly=wave_height,wave_period,wave_direction\
         &forecast_days=7&timezone=UTC",
        lat_min = lat_min, lat_max = lat_max,
        lon_min = lon_min, lon_max = lon_max,
    )
}

pub async fn fetch_area_forecast(
    lat_min: f64,
    lat_max: f64,
    lon_min: f64,
    lon_max: f64,
) -> Result<Vec<FetchedForecast>, AppError> {
    let client = reqwest::Client::new();
    let fetched_at = Utc::now();

    let meteo_url = build_meteo_bbox_url(lat_min, lat_max, lon_min, lon_max);
    let meteo_resp = client
        .get(&meteo_url)
        .send()
        .await
        .map_err(|e| AppError::Io(format!("Open-Meteo forecast request failed: {}", e)))?;
    if !meteo_resp.status().is_success() {
        return Err(AppError::Io(format!("Open-Meteo forecast returned HTTP {}", meteo_resp.status())));
    }
    let meteo_raw: serde_json::Value = meteo_resp
        .json()
        .await
        .map_err(|e| AppError::Parse(format!("Open-Meteo forecast parse failed: {}", e)))?;
    let meteo_responses: Vec<MeteoResponse> =
        serde_json::from_value::<OneOrMany<MeteoResponse>>(meteo_raw)
            .map_err(|e| AppError::Parse(e.to_string()))?
            .into_vec();

    let marine_url = build_marine_bbox_url(lat_min, lat_max, lon_min, lon_max);
    let marine_resp = client
        .get(&marine_url)
        .send()
        .await
        .map_err(|e| AppError::Io(format!("Open-Meteo marine request failed: {}", e)))?;
    if !marine_resp.status().is_success() {
        return Err(AppError::Io(format!("Open-Meteo marine returned HTTP {}", marine_resp.status())));
    }
    let marine_raw: serde_json::Value = marine_resp
        .json()
        .await
        .map_err(|e| AppError::Parse(format!("Open-Meteo marine parse failed: {}", e)))?;
    let marine_responses: Vec<MarineResponse> =
        serde_json::from_value::<OneOrMany<MarineResponse>>(marine_raw)
            .map_err(|e| AppError::Parse(e.to_string()))?
            .into_vec();

    let mut results = Vec::new();
    for (i, meteo) in meteo_responses.iter().enumerate() {
        let marine = marine_responses.get(i);
        let n = meteo.hourly.time.len();
        let mut hourly = Vec::with_capacity(n);

        for j in 0..n {
            let raw = &meteo.hourly.time[j];
            let timestamp = if raw.len() == 16 {
                // "YYYY-MM-DDTHH:MM" → append :00Z
                format!("{}:00Z", raw)
            } else {
                raw.clone()
            };
            hourly.push(ForecastHourlyPoint {
                timestamp,
                wind_speed_kn: meteo.hourly.wind_speed_10m.as_ref().and_then(|v| v.get(j).copied().flatten()),
                wind_direction_deg: meteo.hourly.wind_direction_10m.as_ref().and_then(|v| v.get(j).copied().flatten()),
                wind_gust_kn: meteo.hourly.wind_gusts_10m.as_ref().and_then(|v| v.get(j).copied().flatten()),
                wave_height_m: marine.and_then(|m| m.hourly.wave_height.as_ref()?.get(j).copied().flatten()),
                wave_period_s: marine.and_then(|m| m.hourly.wave_period.as_ref()?.get(j).copied().flatten()),
                wave_direction_deg: marine.and_then(|m| m.hourly.wave_direction.as_ref()?.get(j).copied().flatten()),
                cape_j_kg: meteo.hourly.cape.as_ref().and_then(|v| v.get(j).copied().flatten()),
            });
        }

        results.push(FetchedForecast {
            lat: meteo.latitude,
            lon: meteo.longitude,
            fetched_at,
            hourly,
        });
    }

    Ok(results)
}

pub fn compute_trip_overlay(inputs: &TripForecastInputs) -> Vec<TripOverlayPoint> {
    let mut result = Vec::new();
    let mut hour = inputs.trip_start;

    while hour <= inputs.trip_end {
        let boat_pos = nearest_track_pos(&inputs.track, hour);

        if let Some((boat_lat, boat_lon)) = boat_pos {
            let samples: Vec<(f64, f64, ForecastHourlyPoint)> = inputs
                .fetches
                .iter()
                .filter_map(|fetch| {
                    nearest_hourly(&fetch.hourly, hour)
                        .map(|pt| (fetch.lat, fetch.lon, pt))
                })
                .collect();

            let interp = interpolate_idw(boat_lat, boat_lon, &samples);
            result.push(TripOverlayPoint {
                timestamp: hour.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                wind_speed_kn: interp.as_ref().and_then(|p| p.wind_speed_kn),
                wind_direction_deg: interp.as_ref().and_then(|p| p.wind_direction_deg),
                wind_gust_kn: interp.as_ref().and_then(|p| p.wind_gust_kn),
                wave_height_m: interp.as_ref().and_then(|p| p.wave_height_m),
                wave_period_s: interp.as_ref().and_then(|p| p.wave_period_s),
                wave_direction_deg: interp.as_ref().and_then(|p| p.wave_direction_deg),
                cape_j_kg: interp.as_ref().and_then(|p| p.cape_j_kg),
            });
        }

        hour += Duration::hours(1);
    }

    result
}

pub fn generate_route_track(
    from_lat: f64,
    from_lon: f64,
    to_lat: f64,
    to_lon: f64,
    departure: DateTime<Utc>,
    speed_kn: f64,
) -> Vec<(f64, f64, DateTime<Utc>)> {
    let distance_nm = haversine_distance_nm(from_lat, from_lon, to_lat, to_lon);
    if distance_nm < 0.1 || speed_kn <= 0.0 {
        return vec![(from_lat, from_lon, departure)];
    }
    let total_hours = distance_nm / speed_kn;
    let num_steps = total_hours.ceil() as i64 + 1;
    (0..num_steps)
        .map(|h| {
            let frac = (h as f64 / total_hours).min(1.0);
            let lat = from_lat + frac * (to_lat - from_lat);
            let lon = from_lon + frac * (to_lon - from_lon);
            let ts = departure + Duration::hours(h);
            (lat, lon, ts)
        })
        .collect()
}

pub fn compute_route_overlay(
    track: &[(f64, f64, DateTime<Utc>)],
    fetches: &[FetchWithHourly],
) -> Vec<RouteOverlayPoint> {
    track
        .iter()
        .filter_map(|(lat, lon, ts)| {
            let samples: Vec<(f64, f64, ForecastHourlyPoint)> = fetches
                .iter()
                .filter_map(|f| {
                    nearest_hourly(&f.hourly, *ts).map(|pt| (f.lat, f.lon, pt))
                })
                .collect();
            let interp = interpolate_idw(*lat, *lon, &samples)?;
            Some(RouteOverlayPoint {
                lat: *lat,
                lon: *lon,
                timestamp: ts.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                wind_speed_kn: interp.wind_speed_kn,
                wind_direction_deg: interp.wind_direction_deg,
                wind_gust_kn: interp.wind_gust_kn,
                wave_height_m: interp.wave_height_m,
                wave_period_s: interp.wave_period_s,
                wave_direction_deg: interp.wave_direction_deg,
                cape_j_kg: interp.cape_j_kg,
            })
        })
        .collect()
}

fn nearest_track_pos(
    track: &[(f64, f64, DateTime<Utc>)],
    ts: DateTime<Utc>,
) -> Option<(f64, f64)> {
    track
        .iter()
        .min_by_key(|(_, _, t)| (*t - ts).num_seconds().unsigned_abs())
        .filter(|(_, _, t)| (*t - ts).num_seconds().abs() < 7200)
        .map(|(lat, lon, _)| (*lat, *lon))
}

fn nearest_hourly(hourly: &[ForecastHourlyPoint], ts: DateTime<Utc>) -> Option<ForecastHourlyPoint> {
    hourly
        .iter()
        .min_by_key(|p| {
            DateTime::parse_from_rfc3339(&p.timestamp)
                .map(|t| (t.with_timezone(&Utc) - ts).num_seconds().unsigned_abs())
                .unwrap_or(u64::MAX)
        })
        .filter(|p| {
            DateTime::parse_from_rfc3339(&p.timestamp)
                .map(|t| (t.with_timezone(&Utc) - ts).num_seconds().abs() < 7200)
                .unwrap_or(false)
        })
        .cloned()
}

fn interpolate_idw(
    target_lat: f64,
    target_lon: f64,
    samples: &[(f64, f64, ForecastHourlyPoint)],
) -> Option<ForecastHourlyPoint> {
    let within: Vec<(f64, &ForecastHourlyPoint)> = samples
        .iter()
        .filter_map(|(lat, lon, pt)| {
            let d = haversine_distance_nm(target_lat, target_lon, *lat, *lon);
            if d <= MAX_DISTANCE_NM { Some((d, pt)) } else { None }
        })
        .collect();

    if within.is_empty() {
        return None;
    }

    // If the boat is essentially at a forecast point, return it directly.
    if let Some((_, pt)) = within.iter().find(|(d, _)| *d < 0.01) {
        return Some((*pt).clone());
    }

    let weights: Vec<f64> = within.iter().map(|(d, _)| 1.0 / (d * d)).collect();

    let scalar_idw = |get: &dyn Fn(&ForecastHourlyPoint) -> Option<f64>| -> Option<f64> {
        let pairs: Vec<(f64, f64)> = within
            .iter()
            .zip(&weights)
            .filter_map(|((_, pt), w)| get(pt).map(|v| (v, *w)))
            .collect();
        if pairs.is_empty() { return None; }
        let sum_w: f64 = pairs.iter().map(|(_, w)| w).sum();
        Some(pairs.iter().map(|(v, w)| v * w).sum::<f64>() / sum_w)
    };

    let angular_idw = |get: &dyn Fn(&ForecastHourlyPoint) -> Option<f64>| -> Option<f64> {
        let pairs: Vec<(f64, f64)> = within
            .iter()
            .zip(&weights)
            .filter_map(|((_, pt), w)| get(pt).map(|deg| (deg.to_radians(), *w)))
            .collect();
        if pairs.is_empty() { return None; }
        let sin_sum: f64 = pairs.iter().map(|(r, w)| r.sin() * w).sum();
        let cos_sum: f64 = pairs.iter().map(|(r, w)| r.cos() * w).sum();
        let deg = sin_sum.atan2(cos_sum).to_degrees();
        Some(if deg < 0.0 { deg + 360.0 } else { deg })
    };

    Some(ForecastHourlyPoint {
        timestamp: within[0].1.timestamp.clone(),
        wind_speed_kn: scalar_idw(&|p| p.wind_speed_kn),
        wind_direction_deg: angular_idw(&|p| p.wind_direction_deg),
        wind_gust_kn: scalar_idw(&|p| p.wind_gust_kn),
        wave_height_m: scalar_idw(&|p| p.wave_height_m),
        wave_period_s: scalar_idw(&|p| p.wave_period_s),
        wave_direction_deg: angular_idw(&|p| p.wave_direction_deg),
        cape_j_kg: scalar_idw(&|p| p.cape_j_kg),
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(wind_speed: f64, wind_dir: f64, gust: f64, wave_h: f64, wave_p: f64, wave_dir: f64, cape: f64) -> ForecastHourlyPoint {
        ForecastHourlyPoint {
            timestamp: "2026-05-06T06:00:00Z".to_string(),
            wind_speed_kn: Some(wind_speed),
            wind_direction_deg: Some(wind_dir),
            wind_gust_kn: Some(gust),
            wave_height_m: Some(wave_h),
            wave_period_s: Some(wave_p),
            wave_direction_deg: Some(wave_dir),
            cape_j_kg: Some(cape),
        }
    }

    #[test]
    fn test_bbox_url_contains_expected_params() {
        let url = build_meteo_bbox_url(43.0, 44.0, 8.0, 9.0);
        assert!(url.contains("latitude_min=43"), "url: {}", url);
        assert!(url.contains("latitude_max=44"), "url: {}", url);
        assert!(url.contains("longitude_min=8"), "url: {}", url);
        assert!(url.contains("longitude_max=9"), "url: {}", url);
        // URL must not use old comma-separated coordinate lists (latitude=X,Y style)
        assert!(!url.contains("latitude="), "URL should not have comma-separated coords: {}", url);
        assert!(!url.contains("longitude="), "URL should not have comma-separated coords: {}", url);
    }

    #[test]
    fn test_idw_no_samples_returns_none() {
        let result = interpolate_idw(43.0, 9.0, &[]);
        assert!(result.is_none());
    }

    #[test]
    fn test_idw_beyond_25nm_returns_none() {
        // ~50NM away from target
        let sample = (43.0 + 0.9, 9.0, pt(10.0, 180.0, 14.0, 1.0, 7.0, 185.0, 0.0));
        let result = interpolate_idw(43.0, 9.0, &[sample]);
        assert!(result.is_none());
    }

    #[test]
    fn test_idw_single_source_returns_its_values() {
        // ~5NM away
        let sample = (43.0 + 0.08, 9.0, pt(12.0, 180.0, 16.0, 1.5, 7.0, 190.0, 100.0));
        let result = interpolate_idw(43.0, 9.0, &[sample]).unwrap();
        assert!((result.wind_speed_kn.unwrap() - 12.0).abs() < 0.01);
        assert!((result.wind_direction_deg.unwrap() - 180.0).abs() < 0.01);
    }

    #[test]
    fn test_idw_two_equidistant_sources_averages_scalars() {
        // Both ~8NM away on opposite sides of target latitude
        let s1 = (43.0 + 0.13, 9.0, pt(10.0, 0.0, 14.0, 1.0, 6.0, 0.0, 0.0));
        let s2 = (43.0 - 0.13, 9.0, pt(20.0, 0.0, 26.0, 3.0, 8.0, 0.0, 0.0));
        let result = interpolate_idw(43.0, 9.0, &[s1, s2]).unwrap();
        let ws = result.wind_speed_kn.unwrap();
        assert!((ws - 15.0).abs() < 0.5, "Expected ~15kn, got {}", ws);
    }

    #[test]
    fn test_idw_angular_wraparound_at_north() {
        // Two sources: 350° and 10° equidistant → result should be near 0° not 180°
        let s1 = (43.0 + 0.13, 9.0, pt(10.0, 350.0, 14.0, 1.0, 6.0, 350.0, 0.0));
        let s2 = (43.0 - 0.13, 9.0, pt(10.0,  10.0, 14.0, 1.0, 6.0,  10.0, 0.0));
        let result = interpolate_idw(43.0, 9.0, &[s1, s2]).unwrap();
        let wd = result.wind_direction_deg.unwrap();
        // Result should be ~0° (or 360°), definitely not ~180°
        assert!(wd < 20.0 || wd > 340.0, "Expected ~0°, got {}°", wd);
    }

    #[test]
    fn test_generate_route_track_point_count() {
        use chrono::TimeZone;
        let dep = Utc.with_ymd_and_hms(2026, 5, 14, 6, 0, 0).unwrap();
        // Livorno → Capraia ≈ 35 nm at 5 kn → 7 h passage → 8 points (h=0..7)
        let track = generate_route_track(43.55, 10.29, 43.05, 9.84, dep, 5.0);
        assert!(track.len() >= 7 && track.len() <= 9, "Expected 7–9 points, got {}", track.len());
        // First point at departure position
        assert!((track[0].0 - 43.55).abs() < 0.01);
        assert!((track[0].2 - dep).num_seconds() == 0);
        // Last point near destination
        let last = track.last().unwrap();
        assert!((last.0 - 43.05).abs() < 0.1, "Expected near 43.05, got {}", last.0);
    }

    #[test]
    fn test_generate_route_track_timestamps_advance_hourly() {
        use chrono::TimeZone;
        let dep = Utc.with_ymd_and_hms(2026, 5, 14, 6, 0, 0).unwrap();
        let track = generate_route_track(43.55, 10.29, 43.05, 9.84, dep, 5.0);
        for i in 1..track.len() {
            let diff = (track[i].2 - track[i-1].2).num_hours();
            assert_eq!(diff, 1, "Expected 1-hour steps");
        }
    }

    #[test]
    fn test_compute_route_overlay_returns_points_with_coords() {
        use chrono::TimeZone;
        use crate::db::operations::forecast::{FetchWithHourly, ForecastHourlyPoint};

        let dep = Utc.with_ymd_and_hms(2026, 5, 14, 9, 0, 0).unwrap();
        let track = generate_route_track(43.5, 9.0, 43.5, 9.5, dep, 10.0);
        // Build hourly points that span the route timestamps
        let hourly: Vec<ForecastHourlyPoint> = track.iter().map(|(_, _, ts)| ForecastHourlyPoint {
            timestamp: ts.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            wind_speed_kn: Some(12.0),
            wind_direction_deg: Some(180.0),
            wind_gust_kn: Some(15.0),
            wave_height_m: Some(1.0),
            wave_period_s: Some(6.0),
            wave_direction_deg: Some(185.0),
            cape_j_kg: Some(0.0),
        }).collect();
        // Single grid point near the route
        let fetches = vec![FetchWithHourly {
            lat: 43.5, lon: 9.25,
            hourly,
        }];
        let overlay = compute_route_overlay(&track, &fetches);
        // Every point should have lat/lon
        for p in &overlay {
            assert!(p.lat >= 43.4 && p.lat <= 43.6, "lat out of range: {}", p.lat);
        }
        assert!(!overlay.is_empty());
    }
}
