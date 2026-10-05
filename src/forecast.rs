use crate::db::operations::forecast::{FetchWithHourly, ForecastHourlyPoint};
use crate::error::AppError;
use crate::utilities::haversine_distance_nm;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use tracing::{debug, info, warn};

// ── Public API types ──────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct FetchedForecast {
    pub lat: f64,
    pub lon: f64,
    pub model: String,
    pub fetched_at: DateTime<Utc>,
    pub hourly: Vec<ForecastHourlyPoint>,
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
    pub speed_kn: Option<f64>,
    pub twa_deg: Option<f64>,
    pub wind_model: Option<String>,
    pub relative_wind_deg: Option<f64>,
    pub heading_deg: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct RouteTrackPoint {
    pub lat: f64,
    pub lon: f64,
    pub time: DateTime<Utc>,
    pub speed_kn: Option<f64>,
    pub twa_deg: Option<f64>,
    pub relative_wind_deg: Option<f64>,
    pub heading_deg: Option<f64>,
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

// ── Pre-parsed forecast lookup ────────────────────────────────────────────────

/// A `FetchWithHourly` with its hourly timestamps parsed once. Wind lookups run many times
/// (up to tens of thousands per isochrone route) against the same, unchanging fetch data —
/// re-parsing each `ForecastHourlyPoint.timestamp` string on every lookup dominates isochrone
/// runtime. Callers should `parse_fetches()` once per request and reuse the result.
pub(crate) struct ParsedFetch<'a> {
    lat: f64,
    lon: f64,
    model: &'a str,
    hourly: Vec<(DateTime<Utc>, &'a ForecastHourlyPoint)>,
}

pub(crate) fn parse_fetches(fetches: &[FetchWithHourly]) -> Vec<ParsedFetch<'_>> {
    fetches
        .iter()
        .map(|f| ParsedFetch {
            lat: f.lat,
            lon: f.lon,
            model: &f.model,
            hourly: f.hourly
                .iter()
                .filter_map(|p| {
                    DateTime::parse_from_rfc3339(&p.timestamp)
                        .ok()
                        .map(|t| (t.with_timezone(&Utc), p))
                })
                .collect(),
        })
        .collect()
}

// ── Public functions ──────────────────────────────────────────────────────────

/// Normalise true wind angle to 0–180°.
/// `cog_deg`: vessel course over ground (0–360°).
/// `wind_dir_deg`: meteorological wind direction the wind is coming FROM (0–360°).
pub fn compute_twa(cog_deg: f64, wind_dir_deg: f64) -> f64 {
    let diff = (wind_dir_deg - cog_deg).rem_euclid(360.0);
    if diff <= 180.0 { diff } else { 360.0 - diff }
}

pub(crate) fn build_meteo_bbox_url(lat_min: f64, lat_max: f64, lon_min: f64, lon_max: f64) -> String {
    format!(
        "https://api.open-meteo.com/v1/forecast\
         ?bounding_box={lat_min},{lon_min},{lat_max},{lon_max}\
         &models=ecmwf_ifs\
         &hourly=wind_speed_10m,wind_direction_10m,wind_gusts_10m,cape\
         &wind_speed_unit=kn&forecast_days=7&timezone=UTC",
    )
}

pub(crate) fn build_marine_bbox_url(lat_min: f64, lat_max: f64, lon_min: f64, lon_max: f64) -> String {
    format!(
        "https://marine-api.open-meteo.com/v1/marine\
         ?bounding_box={lat_min},{lon_min},{lat_max},{lon_max}\
         &models=ecmwf_wam\
         &hourly=wave_height,wave_period,wave_direction\
         &forecast_days=7&timezone=UTC",
    )
}

pub(crate) fn build_arome_bbox_url(lat_min: f64, lat_max: f64, lon_min: f64, lon_max: f64) -> String {
    // AROME France (~2.5 km via Open-Meteo) — short-term, wind only, no waves.
    // The 1.5 km "_hd" variant hits Open-Meteo's 1000-location cap on our configured
    // forecast areas (a 0.7°×0.5° box already yields ~1900 HD grid points); this 2.5 km
    // variant covers the same area in ~600 points, comfortably under the cap.
    format!(
        "https://api.open-meteo.com/v1/forecast\
         ?bounding_box={lat_min},{lon_min},{lat_max},{lon_max}\
         &models=meteofrance_arome_france\
         &hourly=wind_speed_10m,wind_direction_10m,wind_gusts_10m,cape\
         &wind_speed_unit=kn&forecast_days=2&timezone=UTC",
    )
}

async fn fetch_wind_responses(
    client: &reqwest::Client,
    url: &str,
) -> Result<Vec<MeteoResponse>, AppError> {
    let resp = client.get(url).send().await
        .map_err(|e| AppError::Io(format!("Open-Meteo wind request failed: {}", e)))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(AppError::Io(format!("Open-Meteo wind returned HTTP {}", status)));
    }
    let body = resp.text().await
        .map_err(|e| AppError::Io(format!("Open-Meteo wind body read failed: {}", e)))?;
    let raw: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
        let preview = body.chars().take(300).collect::<String>();
        AppError::Parse(format!("Open-Meteo wind parse failed: {} — body: {}", e, preview))
    })?;
    Ok(serde_json::from_value::<OneOrMany<MeteoResponse>>(raw)
        .map_err(|e| AppError::Parse(e.to_string()))?
        .into_vec())
}

fn build_hourly(meteo: &MeteoResponse, marine: Option<&MarineResponse>) -> Vec<ForecastHourlyPoint> {
    let n = meteo.hourly.time.len();
    let mut hourly = Vec::with_capacity(n);
    for j in 0..n {
        let raw = &meteo.hourly.time[j];
        let timestamp = if raw.len() == 16 { format!("{}:00Z", raw) } else { raw.clone() };
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
    hourly
}

pub async fn fetch_area_forecast(
    lat_min: f64,
    lat_max: f64,
    lon_min: f64,
    lon_max: f64,
) -> Result<Vec<FetchedForecast>, AppError> {
    let period_start = Utc::now().format("%Y-%m-%dT%H:00Z").to_string();
    let period_end = (Utc::now() + chrono::Duration::days(7)).format("%Y-%m-%dT%H:00Z").to_string();
    info!(
        lat_min, lat_max, lon_min, lon_max,
        period_start = %period_start, period_end = %period_end,
        "Forecast fetch: starting area request"
    );

    let client = reqwest::Client::new();
    let fetched_at = Utc::now();

    // ── ECMWF wind (fatal on failure) ──
    let ecmwf_url = build_meteo_bbox_url(lat_min, lat_max, lon_min, lon_max);
    debug!(url = %ecmwf_url, "Open-Meteo ECMWF wind: sending request");
    let ecmwf_responses = fetch_wind_responses(&client, &ecmwf_url).await?;
    info!(grid_points = ecmwf_responses.len(), "Open-Meteo ECMWF wind: response OK");

    // ── ECMWF marine (non-fatal) ──
    let marine_url = build_marine_bbox_url(lat_min, lat_max, lon_min, lon_max);
    debug!(url = %marine_url, "Open-Meteo marine: sending request");
    let marine_responses: Vec<MarineResponse> = {
        let fetch: Result<Vec<MarineResponse>, String> = async {
            let resp = client.get(&marine_url).send().await.map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                return Err(format!("HTTP {}", resp.status()));
            }
            let body = resp.text().await.map_err(|e| e.to_string())?;
            let raw: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
            serde_json::from_value::<OneOrMany<MarineResponse>>(raw)
                .map(|v| v.into_vec())
                .map_err(|e| e.to_string())
        }.await;
        match fetch {
            Ok(v) => { info!(grid_points = v.len(), "Open-Meteo marine: response OK"); v }
            Err(e) => { warn!(error = %e, "Open-Meteo marine: unavailable (wave data omitted)"); vec![] }
        }
    };

    // ── AROME wind (non-fatal) ──
    let arome_url = build_arome_bbox_url(lat_min, lat_max, lon_min, lon_max);
    debug!(url = %arome_url, "Open-Meteo AROME wind: sending request");
    let arome_responses: Vec<MeteoResponse> = match fetch_wind_responses(&client, &arome_url).await {
        Ok(v) => { info!(grid_points = v.len(), "Open-Meteo AROME wind: response OK"); v }
        Err(e) => { warn!(error = %e, "Open-Meteo AROME wind: unavailable (using ECMWF only)"); vec![] }
    };

    let mut results = Vec::new();

    // ECMWF results: merge marine waves by grid-point index, as before.
    for (i, meteo) in ecmwf_responses.iter().enumerate() {
        let marine = marine_responses.get(i);
        results.push(FetchedForecast {
            lat: meteo.latitude,
            lon: meteo.longitude,
            model: "ecmwf".to_string(),
            fetched_at,
            hourly: build_hourly(meteo, marine),
        });
    }

    // AROME results: wind only, no waves.
    for meteo in &arome_responses {
        results.push(FetchedForecast {
            lat: meteo.latitude,
            lon: meteo.longitude,
            model: "arome".to_string(),
            fetched_at,
            hourly: build_hourly(meteo, None),
        });
    }

    info!(
        lat_min, lat_max, lon_min, lon_max,
        ecmwf_points = ecmwf_responses.len(),
        arome_points = arome_responses.len(),
        total_grid_points = results.len(),
        "Forecast fetch: area complete"
    );
    Ok(results)
}

/// Parses "lat1,lon1;lat2,lon2;…" into a Vec of (lat, lon) pairs.
/// Returns Err if fewer than 2 pairs or any pair is malformed.
pub fn parse_waypoints(s: &str) -> Result<Vec<(f64, f64)>, String> {
    let pairs: Vec<(f64, f64)> = s
        .split(';')
        .filter(|p| !p.trim().is_empty())
        .map(|pair| {
            let mut it = pair.splitn(2, ',');
            let lat = it.next().and_then(|v| v.trim().parse::<f64>().ok());
            let lon = it.next().and_then(|v| v.trim().parse::<f64>().ok());
            let (lat, lon) = lat.zip(lon).ok_or_else(|| format!("invalid waypoint pair: '{}'", pair))?;
            if lat.abs() > 90.0 || lon.abs() > 180.0 {
                return Err(format!("waypoint out of range: lat={} lon={}", lat, lon));
            }
            Ok((lat, lon))
        })
        .collect::<Result<_, _>>()?;
    if pairs.len() < 2 {
        return Err(format!("at least 2 waypoints required, got {}", pairs.len()));
    }
    Ok(pairs)
}

pub(crate) fn nearest_forecast_wind(
    fetches: &[ParsedFetch],
    lat: f64,
    lon: f64,
    time: DateTime<Utc>,
) -> Option<(f64, f64, Option<f64>)> {
    let collect = |model: &str| -> Vec<(f64, f64, ForecastHourlyPoint)> {
        fetches
            .iter()
            .filter(|f| f.model == model)
            // Cheap distance check before the per-point timestamp scan, so grid points far
            // outside interpolation range (interpolate_idw's own MAX_DISTANCE_NM cutoff below)
            // never pay for a nearest_hourly lookup they'd be discarded after anyway.
            .filter(|f| haversine_distance_nm(lat, lon, f.lat, f.lon) <= MAX_DISTANCE_NM)
            .filter_map(|f| nearest_hourly(&f.hourly, time).map(|pt| (f.lat, f.lon, pt)))
            .collect()
    };
    let arome = collect("arome");
    let ecmwf = collect("ecmwf");
    let interp = interpolate_blended(lat, lon, &arome, &ecmwf)?;
    Some((interp.wind_speed_kn?, interp.wind_direction_deg?, interp.wind_gust_kn))
}

/// Wind-aware step simulation: advances the vessel along the route, using polar performance
/// when forecast wind data is available, falling back to `motoring_speed_kn` otherwise.
///
/// `polar_efficiency` (0.0–1.0): scales raw polar speed to account for sea state, crew
/// fatigue, sail trim, etc. A value of 0.95 means the boat achieves 95% of polar.
///
/// `min_sail_speed_kn`: if the effective polar speed (after efficiency) would be below this
/// threshold the boat motors instead, even when wind is available.
pub fn generate_route_track(
    waypoints: &[(f64, f64)],
    departure: DateTime<Utc>,
    motoring_speed_kn: f64,
    polar_efficiency: f64,
    min_sail_speed_kn: f64,
    min_twa_deg: f64,
    polars: Option<&crate::polars::PolarTable>,
    fetches: &[FetchWithHourly],
) -> Vec<RouteTrackPoint> {
    if waypoints.len() < 2 || motoring_speed_kn <= 0.0 {
        return vec![];
    }
    let efficiency = polar_efficiency.clamp(0.01, 1.0);
    let parsed = parse_fetches(fetches);

    let mut track: Vec<RouteTrackPoint> = Vec::new();
    let mut leg_start_time = departure;

    for w in waypoints.windows(2) {
        let (from_lat, from_lon) = w[0];
        let (to_lat, to_lon) = w[1];

        let mut pos = (from_lat, from_lon);
        let mut t = leg_start_time;

        if track.is_empty() {
            track.push(RouteTrackPoint { lat: pos.0, lon: pos.1, time: t, speed_kn: None, twa_deg: None, relative_wind_deg: None, heading_deg: None });
        }

        loop {
            let remaining_nm = crate::utilities::haversine_distance_nm(pos.0, pos.1, to_lat, to_lon);
            if remaining_nm < 0.1 {
                break;
            }

            let bearing = crate::utilities::haversine_heading(pos.0, pos.1, to_lat, to_lon);
            // Single wind sample for this step, taken at its start position/time — reused for
            // both the sail/motor decision below AND relative_wind_deg, so the two can never
            // disagree (a prior version recomputed relative_wind_deg later from a separately
            // re-interpolated end-of-step sample, which could silently differ from the sample
            // that actually drove the decision).
            let wind = nearest_forecast_wind(&parsed, pos.0, pos.1, t);
            let relative_wind_deg = wind.map(|(_, wind_dir, _)| compute_twa(bearing, wind_dir));

            let (speed_kn, twa) = match (wind, polars) {
                (Some((wind_spd, wind_dir, _)), Some(p)) if wind_spd > 0.0 => {
                    let twa = compute_twa(bearing, wind_dir);
                    if twa < min_twa_deg {
                        (motoring_speed_kn, None) // TWA below the user's minimum — motor
                    } else {
                        match p.boat_speed(twa, wind_spd).filter(|&s| s > 0.0) {
                            Some(raw) => {
                                let eff = raw * efficiency;
                                if eff >= min_sail_speed_kn {
                                    (eff, Some(twa))
                                } else {
                                    (motoring_speed_kn, None) // polar speed too low — motor
                                }
                            }
                            None => (motoring_speed_kn, None), // TWA below polar minimum — motor
                        }
                    }
                }
                _ => (motoring_speed_kn, None),
            };

            let hours_to_wp = remaining_nm / speed_kn;
            let step_hours = hours_to_wp.min(0.5);
            let dist_nm = speed_kn * step_hours;

            pos = crate::utilities::advance_position(pos.0, pos.1, bearing, dist_nm);
            t += Duration::seconds((step_hours * 3600.0).round() as i64);

            track.push(RouteTrackPoint { lat: pos.0, lon: pos.1, time: t, speed_kn: Some(speed_kn), twa_deg: twa, relative_wind_deg, heading_deg: Some(bearing) });

            if hours_to_wp <= 0.5 {
                break;
            }
        }

        leg_start_time = t;
    }

    track
}

/// IDW-interpolates forecast values at each synthetic track point.
/// Points for which no forecast data is available within range are omitted
/// from the output — callers should not assume output.len() == track.len().
/// Wind family is sourced from AROME when available, wave family always from ECMWF.
pub fn compute_route_overlay(
    track: &[RouteTrackPoint],
    fetches: &[FetchWithHourly],
) -> Vec<RouteOverlayPoint> {
    let parsed = parse_fetches(fetches);
    track
        .iter()
        .filter_map(|pt| {
            let collect = |model: &str| -> Vec<(f64, f64, ForecastHourlyPoint)> {
                parsed
                    .iter()
                    .filter(|f| f.model == model)
                    .filter(|f| haversine_distance_nm(pt.lat, pt.lon, f.lat, f.lon) <= MAX_DISTANCE_NM)
                    .filter_map(|f| nearest_hourly(&f.hourly, pt.time).map(|p| (f.lat, f.lon, p)))
                    .collect()
            };
            let arome = collect("arome");
            let ecmwf = collect("ecmwf");
            let interp = interpolate_blended(pt.lat, pt.lon, &arome, &ecmwf)?;
            Some(RouteOverlayPoint {
                lat: pt.lat,
                lon: pt.lon,
                timestamp: pt.time.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                wind_speed_kn: interp.wind_speed_kn,
                wind_direction_deg: interp.wind_direction_deg,
                wind_gust_kn: interp.wind_gust_kn,
                wave_height_m: interp.wave_height_m,
                wave_period_s: interp.wave_period_s,
                wave_direction_deg: interp.wave_direction_deg,
                cape_j_kg: interp.cape_j_kg,
                speed_kn: pt.speed_kn,
                twa_deg: pt.twa_deg,
                wind_model: interp.wind_model,
                relative_wind_deg: pt.relative_wind_deg,
                heading_deg: pt.heading_deg,
            })
        })
        .collect()
}

fn nearest_hourly(hourly: &[(DateTime<Utc>, &ForecastHourlyPoint)], ts: DateTime<Utc>) -> Option<ForecastHourlyPoint> {
    hourly
        .iter()
        .min_by_key(|(t, _)| (*t - ts).num_seconds().unsigned_abs())
        .filter(|(t, _)| (*t - ts).num_seconds().abs() < 7200)
        .map(|(_, p)| (*p).clone())
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

pub(crate) struct BlendedForecast {
    pub wind_speed_kn: Option<f64>,
    pub wind_direction_deg: Option<f64>,
    pub wind_gust_kn: Option<f64>,
    pub wave_height_m: Option<f64>,
    pub wave_period_s: Option<f64>,
    pub wave_direction_deg: Option<f64>,
    pub cape_j_kg: Option<f64>,
    pub wind_model: Option<String>,
}

/// Interpolates a point by blending two model grids: wind family is taken from
/// AROME when AROME produces a wind speed in range, otherwise from ECMWF; the
/// wave family always comes from ECMWF (AROME has no wave output).
pub(crate) fn interpolate_blended(
    target_lat: f64,
    target_lon: f64,
    arome_samples: &[(f64, f64, ForecastHourlyPoint)],
    ecmwf_samples: &[(f64, f64, ForecastHourlyPoint)],
) -> Option<BlendedForecast> {
    let arome = interpolate_idw(target_lat, target_lon, arome_samples);
    let ecmwf = interpolate_idw(target_lat, target_lon, ecmwf_samples);

    // Wind source: AROME if it yielded a wind speed, else ECMWF.
    let arome_has_wind = arome.as_ref().is_some_and(|p| p.wind_speed_kn.is_some());
    let (wind_src, wind_model): (Option<&ForecastHourlyPoint>, Option<&str>) = if arome_has_wind {
        (arome.as_ref(), Some("arome"))
    } else if ecmwf.is_some() {
        (ecmwf.as_ref(), Some("ecmwf"))
    } else {
        (None, None)
    };

    // If neither model produced anything at all, there is no data here.
    // (wind_src is None only when both AROME has no wind and ECMWF is None.)
    let wind_src = wind_src?;

    Some(BlendedForecast {
        wind_speed_kn: wind_src.wind_speed_kn,
        wind_direction_deg: wind_src.wind_direction_deg,
        wind_gust_kn: wind_src.wind_gust_kn,
        cape_j_kg: wind_src.cape_j_kg,
        wave_height_m: ecmwf.as_ref().and_then(|p| p.wave_height_m),
        wave_period_s: ecmwf.as_ref().and_then(|p| p.wave_period_s),
        wave_direction_deg: ecmwf.as_ref().and_then(|p| p.wave_direction_deg),
        wind_model: wind_model.map(|s| s.to_string()),
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn pt_wind_only(wind_speed: f64, wind_dir: f64) -> ForecastHourlyPoint {
        ForecastHourlyPoint {
            timestamp: "2026-06-13T06:00:00Z".to_string(),
            wind_speed_kn: Some(wind_speed),
            wind_direction_deg: Some(wind_dir),
            wind_gust_kn: None,
            wave_height_m: None,
            wave_period_s: None,
            wave_direction_deg: None,
            cape_j_kg: None,
        }
    }

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
    fn test_blended_prefers_arome_wind() {
        // ECMWF ~5NM away with wind 10kn; AROME ~5NM away with wind 18kn.
        let arome = vec![(43.0 + 0.08, 9.0, pt_wind_only(18.0, 200.0))];
        let ecmwf = vec![(43.0 + 0.08, 9.0, pt(10.0, 180.0, 14.0, 1.5, 7.0, 190.0, 100.0))];
        let r = interpolate_blended(43.0, 9.0, &arome, &ecmwf).unwrap();
        assert!((r.wind_speed_kn.unwrap() - 18.0).abs() < 0.5, "got {:?}", r.wind_speed_kn);
        assert_eq!(r.wind_model.as_deref(), Some("arome"));
        // Waves always from ECMWF, even though AROME drove the wind.
        assert!((r.wave_height_m.unwrap() - 1.5).abs() < 0.1, "got {:?}", r.wave_height_m);
    }

    #[test]
    fn test_blended_falls_back_to_ecmwf_when_no_arome() {
        let ecmwf = vec![(43.0 + 0.08, 9.0, pt(10.0, 180.0, 14.0, 1.5, 7.0, 190.0, 100.0))];
        let r = interpolate_blended(43.0, 9.0, &[], &ecmwf).unwrap();
        assert!((r.wind_speed_kn.unwrap() - 10.0).abs() < 0.5);
        assert_eq!(r.wind_model.as_deref(), Some("ecmwf"));
    }

    #[test]
    fn test_blended_arome_out_of_range_uses_ecmwf() {
        // AROME sample ~50NM away (beyond MAX_DISTANCE_NM) → ignored; ECMWF used.
        let arome = vec![(43.0 + 0.9, 9.0, pt_wind_only(18.0, 200.0))];
        let ecmwf = vec![(43.0 + 0.08, 9.0, pt(10.0, 180.0, 14.0, 1.5, 7.0, 190.0, 100.0))];
        let r = interpolate_blended(43.0, 9.0, &arome, &ecmwf).unwrap();
        assert!((r.wind_speed_kn.unwrap() - 10.0).abs() < 0.5);
        assert_eq!(r.wind_model.as_deref(), Some("ecmwf"));
    }

    #[test]
    fn test_blended_no_data_returns_none() {
        assert!(interpolate_blended(43.0, 9.0, &[], &[]).is_none());
    }

    #[test]
    fn test_bbox_url_contains_expected_params() {
        let url = build_meteo_bbox_url(43.0, 43.5, 8.0, 8.5);
        assert!(url.contains("bounding_box=43,8,43.5,8.5"), "url: {}", url);
        assert!(url.contains("models=ecmwf_ifs"), "url: {}", url);
        assert!(url.contains("wind_speed_unit=kn"), "url: {}", url);
    }

    #[test]
    fn test_arome_bbox_url_contains_expected_params() {
        let url = build_arome_bbox_url(43.0, 43.5, 8.0, 8.5);
        assert!(url.contains("bounding_box=43,8,43.5,8.5"), "url: {}", url);
        assert!(url.contains("models=meteofrance_arome_france"), "url: {}", url);
        assert!(!url.contains("models=meteofrance_arome_france_hd"), "url: {}", url);
        assert!(url.contains("forecast_days=2"), "url: {}", url);
        assert!(url.contains("wind_speed_unit=kn"), "url: {}", url);
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
        // Livorno → Capraia ≈ 35.9 nm at 5 kn → 7.18 h → 14 full 30-min steps + 1 partial + 1 departure = 16 points
        let wpts = vec![(43.55_f64, 10.29_f64), (43.05, 9.84)];
        let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, 0.0, None, &[]);
        assert_eq!(track.len(), 16, "Expected 16 points, got {}", track.len());
        assert!((track[0].lat - 43.55).abs() < 0.01);
        assert!((track[0].time - dep).num_seconds() == 0);
        let last = track.last().unwrap();
        assert!((last.lat - 43.05).abs() < 0.001, "Expected 43.05, got {}", last.lat);
        assert!((last.lon - 9.84).abs() < 0.001,  "Expected 9.84, got {}",  last.lon);
    }

    #[test]
    fn test_generate_route_track_timestamps_advance_hourly() {
        use chrono::TimeZone;
        let dep = Utc.with_ymd_and_hms(2026, 5, 14, 6, 0, 0).unwrap();
        let wpts = vec![(43.55_f64, 10.29_f64), (43.05, 9.84)];
        let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, 0.0, None, &[]);
        // All but the last step are exactly 30 min apart; last step may be a partial half-hour
        for i in 1..track.len() - 1 {
            let diff = (track[i].time - track[i - 1].time).num_seconds();
            assert_eq!(diff, 1800, "Expected 30-min steps at index {}", i);
        }
    }

    #[test]
    fn test_compute_route_overlay_returns_points_with_coords() {
        use chrono::TimeZone;
        use crate::db::operations::forecast::{FetchWithHourly, ForecastHourlyPoint};

        let dep = Utc.with_ymd_and_hms(2026, 5, 14, 9, 0, 0).unwrap();
        let wpts = vec![(43.5_f64, 9.0_f64), (43.5, 9.5)];
        let track = generate_route_track(&wpts, dep, 10.0, 1.0, 0.0, 0.0, None, &[]);
        // Build hourly points that span the route timestamps
        let hourly: Vec<ForecastHourlyPoint> = track.iter().map(|pt| ForecastHourlyPoint {
            timestamp: pt.time.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
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
            model: "ecmwf".to_string(),
            hourly,
        }];
        let overlay = compute_route_overlay(&track, &fetches);
        // Every point should have lat/lon
        for p in &overlay {
            assert!(p.lat >= 43.4 && p.lat <= 43.6, "lat out of range: {}", p.lat);
        }
        assert!(!overlay.is_empty());
        // Check weather fields are populated (not just lat/lon)
        assert!(overlay[0].wind_speed_kn.is_some(), "Expected wind_speed_kn to be interpolated");
    }

    #[test]
    fn test_generate_route_track_empty_and_single_waypoint() {
        use chrono::TimeZone;
        let dep = Utc.with_ymd_and_hms(2026, 5, 14, 6, 0, 0).unwrap();
        // 0 waypoints → empty track
        assert!(generate_route_track(&[], dep, 5.0, 1.0, 0.0, 0.0, None, &[]).is_empty());
        // 1 waypoint → empty track (no pair to form a leg)
        assert!(generate_route_track(&[(43.55, 10.29)], dep, 5.0, 1.0, 0.0, 0.0, None, &[]).is_empty());
    }

    #[test]
    fn test_generate_route_track_two_legs() {
        use chrono::TimeZone;
        let dep = Utc.with_ymd_and_hms(2026, 5, 14, 6, 0, 0).unwrap();
        let wpts = vec![(43.55_f64, 10.29_f64), (43.05, 9.84), (42.70, 9.45)];
        let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, 0.0, None, &[]);
        // First point at first waypoint
        assert!((track[0].lat - 43.55).abs() < 0.01, "first lat wrong");
        assert!((track[0].lon - 10.29).abs() < 0.01, "first lon wrong");
        // Last point at last waypoint
        let last = track.last().unwrap();
        assert!((last.lat - 42.70).abs() < 0.01, "last lat wrong: {}", last.lat);
        assert!((last.lon - 9.45).abs() < 0.01, "last lon wrong: {}", last.lon);
        // Timestamps strictly increasing (no duplicates at leg boundary)
        for i in 1..track.len() {
            assert!(track[i].time > track[i - 1].time,
                "Timestamps not strictly increasing at index {}", i);
        }
    }

    #[test]
    fn test_parse_waypoints_valid() {
        let wpts = parse_waypoints("43.55,10.29;43.05,9.84;42.70,9.45").unwrap();
        assert_eq!(wpts.len(), 3);
        assert!((wpts[0].0 - 43.55).abs() < 1e-9);
        assert!((wpts[0].1 - 10.29).abs() < 1e-9);
        assert!((wpts[2].0 - 42.70).abs() < 1e-9);
        assert!((wpts[2].1 - 9.45).abs() < 1e-9);
    }

    #[test]
    fn test_parse_waypoints_too_few() {
        assert!(parse_waypoints("43.55,10.29").is_err());
        assert!(parse_waypoints("").is_err());
    }

    #[test]
    fn test_parse_waypoints_invalid_format() {
        assert!(parse_waypoints("43.55;10.29;bad").is_err());   // missing comma in pair
        assert!(parse_waypoints("43.55,abc;10.29,9.0").is_err()); // non-numeric
    }

    #[test]
    fn test_parse_waypoints_out_of_range() {
        assert!(parse_waypoints("999.0,10.29;43.05,9.84").is_err());
        assert!(parse_waypoints("43.55,200.0;43.05,9.84").is_err());
    }

    #[test]
    fn test_compute_twa_upwind() {
        // Heading north (0°), wind from north (0°) → TWA = 0°
        assert!((compute_twa(0.0, 0.0) - 0.0).abs() < 0.01, "got {}", compute_twa(0.0, 0.0));
    }

    #[test]
    fn test_compute_twa_beam_reach_port() {
        // Heading north (0°), wind from east (90°) → TWA = 90°
        assert!((compute_twa(0.0, 90.0) - 90.0).abs() < 0.01, "got {}", compute_twa(0.0, 90.0));
    }

    #[test]
    fn test_compute_twa_beam_reach_starboard() {
        // Heading north (0°), wind from west (270°) → TWA = 90°
        assert!((compute_twa(0.0, 270.0) - 90.0).abs() < 0.01, "got {}", compute_twa(0.0, 270.0));
    }

    #[test]
    fn test_compute_twa_downwind() {
        // Heading north (0°), wind from south (180°) → TWA = 180°
        assert!((compute_twa(0.0, 180.0) - 180.0).abs() < 0.01, "got {}", compute_twa(0.0, 180.0));
    }

    #[test]
    fn test_compute_twa_reaching_on_easterly_heading() {
        // Heading east (90°), wind from north (0°) → TWA = 90°
        assert!((compute_twa(90.0, 0.0) - 90.0).abs() < 0.01, "got {}", compute_twa(90.0, 0.0));
    }

    #[test]
    fn test_generate_route_track_uses_polar_speed() {
        use crate::polars::PolarTable;
        let polars = PolarTable::constant_for_test(7.0);

        let ts_str = "2026-06-01T06:00:00Z";
        let hourly = vec![crate::db::operations::forecast::ForecastHourlyPoint {
            timestamp: ts_str.to_string(),
            wind_speed_kn: Some(12.0),
            wind_direction_deg: Some(180.0),
            wind_gust_kn: None, wave_height_m: None, wave_period_s: None,
            wave_direction_deg: None, cape_j_kg: None,
        }];
        let fetches = vec![crate::db::operations::forecast::FetchWithHourly {
            lat: 43.0, lon: 8.0, model: "ecmwf".to_string(), hourly,
        }];

        let dep = chrono::DateTime::parse_from_rfc3339(ts_str).unwrap().with_timezone(&chrono::Utc);
        let wpts = vec![(43.0_f64, 8.0_f64), (43.12_f64, 8.0_f64)];
        let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, 0.0, Some(&polars), &fetches);

        assert!(track.len() >= 2, "expected ≥2 points, got {}", track.len());
        let spd = track[1].speed_kn.expect("speed_kn should be set");
        assert!((spd - 7.0).abs() < 0.1, "expected polar speed 7.0, got {}", spd);
    }

    #[test]
    fn test_generate_route_track_falls_back_to_motoring_no_wind() {
        let polars = crate::polars::PolarTable::constant_for_test(7.0);
        let dep = chrono::Utc::now();
        let wpts = vec![(43.0_f64, 8.0_f64), (43.12_f64, 8.0_f64)];
        let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, 0.0, Some(&polars), &[]);

        assert!(track.len() >= 2);
        let spd = track[1].speed_kn.expect("speed_kn should be set");
        assert!((spd - 5.0).abs() < 0.1, "expected motoring speed 5.0, got {}", spd);
    }

    #[test]
    fn test_generate_route_track_min_twa_deg_forces_motor() {
        use crate::polars::PolarTable;
        let polars = PolarTable::constant_for_test(7.0);

        let ts_str = "2026-06-01T06:00:00Z";
        let hourly = vec![crate::db::operations::forecast::ForecastHourlyPoint {
            timestamp: ts_str.to_string(),
            wind_speed_kn: Some(12.0),
            // Leg bearing is due north (0°); wind from 45° → TWA 45°, which the polar itself
            // happily sails (its minimum is 42°) but which min_twa_deg = 60.0 must reject.
            wind_direction_deg: Some(45.0),
            wind_gust_kn: None, wave_height_m: None, wave_period_s: None,
            wave_direction_deg: None, cape_j_kg: None,
        }];
        let fetches = vec![crate::db::operations::forecast::FetchWithHourly {
            lat: 43.0, lon: 8.0, model: "ecmwf".to_string(), hourly,
        }];

        let dep = chrono::DateTime::parse_from_rfc3339(ts_str).unwrap().with_timezone(&chrono::Utc);
        let wpts = vec![(43.0_f64, 8.0_f64), (43.12_f64, 8.0_f64)];
        let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, 60.0, Some(&polars), &fetches);

        assert!(track.len() >= 2, "expected ≥2 points, got {}", track.len());
        assert_eq!(
            track[1].twa_deg, None,
            "TWA 45° is below min_twa_deg=60°, should not count as sailing"
        );
        let spd = track[1].speed_kn.expect("speed_kn should be set");
        assert!(
            (spd - 5.0).abs() < 0.1,
            "expected motoring speed 5.0 once min_twa_deg excludes this TWA, got {}",
            spd
        );
    }

    #[test]
    fn test_generate_route_track_relative_wind_deg_matches_sail_decision() {
        use chrono::TimeZone;
        use crate::db::operations::forecast::{FetchWithHourly, ForecastHourlyPoint};
        use crate::polars::PolarTable;

        let dep = Utc.with_ymd_and_hms(2026, 5, 14, 6, 0, 0).unwrap();
        let hourly = vec![ForecastHourlyPoint {
            timestamp: dep.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            wind_speed_kn: Some(12.0),
            wind_direction_deg: Some(90.0), // wind from due east
            wind_gust_kn: None, wave_height_m: None, wave_period_s: None,
            wave_direction_deg: None, cape_j_kg: None,
        }];
        let fetches = vec![FetchWithHourly { lat: 43.0, lon: 8.0, model: "ecmwf".to_string(), hourly }];
        let polars = PolarTable::constant_for_test(7.0);

        // Heading due north (0°) with wind from due east (90°) → TWA 90°, above min_twa_deg=60.
        let wpts = vec![(43.0_f64, 8.0_f64), (43.12_f64, 8.0_f64)];
        let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, 60.0, Some(&polars), &fetches);

        assert!(track.len() >= 2);
        assert_eq!(track[0].relative_wind_deg, None, "departure point has no leg/wind sample yet");

        let twa = track[1].twa_deg.expect("expected a sailing decision (TWA well above min_twa_deg=60)");
        let relative = track[1].relative_wind_deg
            .expect("relative_wind_deg should be set alongside a sail decision");
        assert_eq!(
            twa, relative,
            "relative_wind_deg must come from the exact same wind sample as the sail decision, \
             not a separately re-interpolated one"
        );
    }

    #[test]
    fn test_generate_route_track_heading_deg_matches_haversine_bearing() {
        use chrono::TimeZone;
        use crate::db::operations::forecast::{FetchWithHourly, ForecastHourlyPoint};

        let dep = Utc.with_ymd_and_hms(2026, 5, 14, 6, 0, 0).unwrap();
        let hourly = vec![ForecastHourlyPoint {
            timestamp: dep.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            wind_speed_kn: Some(12.0),
            wind_direction_deg: Some(90.0),
            wind_gust_kn: None, wave_height_m: None, wave_period_s: None,
            wave_direction_deg: None, cape_j_kg: None,
        }];
        let fetches = vec![FetchWithHourly { lat: 43.0, lon: 8.0, model: "ecmwf".to_string(), hourly }];

        // No polars → always motors, but heading_deg must still be recorded regardless of
        // the sail/motor decision (it's a property of the leg's course, not of sailing).
        let wpts = vec![(43.0_f64, 8.0_f64), (43.12_f64, 8.15_f64)];
        let track = generate_route_track(&wpts, dep, 5.0, 1.0, 0.0, 60.0, None, &fetches);

        assert!(track.len() >= 2);
        assert_eq!(track[0].heading_deg, None, "departure point has no incoming leg yet");

        let expected_bearing = crate::utilities::haversine_heading(43.0, 8.0, 43.12, 8.15);
        let heading = track[1].heading_deg.expect("heading_deg should be set once the boat has moved");
        assert!(
            (heading - expected_bearing).abs() < 0.01,
            "expected heading_deg to equal haversine_heading's bearing ({}), got {}",
            expected_bearing, heading
        );
    }

    #[test]
    fn test_compute_route_overlay_passes_through_heading_deg() {
        use chrono::TimeZone;
        use crate::db::operations::forecast::{FetchWithHourly, ForecastHourlyPoint};

        let dep = Utc.with_ymd_and_hms(2026, 5, 14, 9, 0, 0).unwrap();
        let wpts = vec![(43.5_f64, 9.0_f64), (43.5, 9.6)];
        let ts = dep.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let hourly = vec![ForecastHourlyPoint {
            timestamp: ts,
            wind_speed_kn: Some(12.0),
            wind_direction_deg: Some(180.0),
            wind_gust_kn: None, wave_height_m: None, wave_period_s: None,
            wave_direction_deg: None, cape_j_kg: None,
        }];
        let fetches = vec![
            FetchWithHourly { lat: 43.5, lon: 9.0, model: "ecmwf".to_string(), hourly: hourly.clone() },
            FetchWithHourly { lat: 43.5, lon: 9.6, model: "ecmwf".to_string(), hourly },
        ];
        let track = generate_route_track(&wpts, dep, 60.0, 1.0, 0.0, 0.0, None, &fetches);
        assert_eq!(track.len(), 2, "leg should complete in a single fast step");

        let overlay = compute_route_overlay(&track, &fetches);
        assert!(overlay.len() >= 2);
        assert_eq!(overlay[0].heading_deg, None);
        assert_eq!(
            overlay[1].heading_deg, track[1].heading_deg,
            "compute_route_overlay must pass heading_deg through unchanged"
        );
    }

    #[test]
    fn test_compute_route_overlay_passes_through_relative_wind_deg() {
        use chrono::TimeZone;
        use crate::db::operations::forecast::{FetchWithHourly, ForecastHourlyPoint};

        let dep = Utc.with_ymd_and_hms(2026, 5, 14, 9, 0, 0).unwrap();
        // A leg long enough (~26 nm) that these two forecast points are each in range of only
        // one endpoint (MAX_DISTANCE_NM = 25 nm), and fast enough (60 kn) that it completes in a
        // single step — so track[1] sits at the destination, on top of the "near destination"
        // point, while the step's *decision* wind sample (taken at track[0], the leg's start)
        // sees only the "near start" point. If compute_route_overlay ever recomputed
        // relative_wind_deg from pt's own (end-of-leg) position instead of copying the
        // decision-time value through, it would pick up the "near destination" wind direction
        // (90°) instead of the "near start" one (180°) — a very different angle, not a rounding
        // difference — so this setup actually distinguishes pass-through from recomputation.
        let wpts = vec![(43.5_f64, 9.0_f64), (43.5, 9.6)];
        let ts = dep.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let make_hourly = |dir: f64| vec![ForecastHourlyPoint {
            timestamp: ts.clone(),
            wind_speed_kn: Some(12.0),
            wind_direction_deg: Some(dir),
            wind_gust_kn: None, wave_height_m: None, wave_period_s: None,
            wave_direction_deg: None, cape_j_kg: None,
        }];
        let fetches = vec![
            FetchWithHourly { lat: 43.5, lon: 9.0, model: "ecmwf".to_string(), hourly: make_hourly(180.0) },
            FetchWithHourly { lat: 43.5, lon: 9.6, model: "ecmwf".to_string(), hourly: make_hourly(90.0) },
        ];
        // No polars → the boat always motors, but relative_wind_deg is still computed from the
        // wind sample regardless of the sail/motor decision.
        let track = generate_route_track(&wpts, dep, 60.0, 1.0, 0.0, 0.0, None, &fetches);
        assert_eq!(track.len(), 2, "leg should complete in a single fast step");
        assert_eq!(track[0].relative_wind_deg, None, "departure point has no leg/wind sample yet");
        let track_relative = track[1].relative_wind_deg
            .expect("relative_wind_deg should be set once wind data is available");
        let bearing = crate::utilities::haversine_heading(43.5, 9.0, 43.5, 9.6);
        let expected_near_start = compute_twa(bearing, 180.0);
        assert!(
            (track_relative - expected_near_start).abs() < 0.5,
            "expected the near-start wind sample ({}), got {}", expected_near_start, track_relative
        );

        let overlay = compute_route_overlay(&track, &fetches);
        assert!(overlay.len() >= 2);
        assert_eq!(overlay[0].relative_wind_deg, None);
        assert_eq!(
            overlay[1].relative_wind_deg, Some(track_relative),
            "compute_route_overlay must pass relative_wind_deg through unchanged, not recompute it \
             from a possibly different (end-of-leg) wind sample"
        );
    }
}
