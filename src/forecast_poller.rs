use crate::db::VesselDatabase;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::{Arc, Mutex, RwLock};
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize)]
pub struct ForecastPollerStatus {
    pub online: bool,
    pub last_fetch: Option<DateTime<Utc>>,
    pub next_fetch: Option<DateTime<Utc>>,
}

impl Default for ForecastPollerStatus {
    fn default() -> Self {
        Self { online: true, last_fetch: None, next_fetch: None }
    }
}

const FETCH_INTERVAL_SECS: i64 = 3 * 3600;
const IDLE_CHECK_SECS: u64 = 300;
const RETRY_SECS: u64 = 900;

pub async fn run_poller(
    db: Arc<RwLock<VesselDatabase>>,
    status: Arc<Mutex<ForecastPollerStatus>>,
) {
    info!("Forecast poller started");
    loop {
        let areas = {
            let db = db.read().unwrap_or_else(|e| e.into_inner());
            db.list_forecast_areas().unwrap_or_default()
        };
        if areas.is_empty() {
            debug!("Forecast poller: no areas defined, sleeping {}s", IDLE_CHECK_SECS);
            tokio::time::sleep(tokio::time::Duration::from_secs(IDLE_CHECK_SECS)).await;
            continue;
        }

        let last_fetch = {
            let db = db.read().unwrap_or_else(|e| e.into_inner());
            db.get_last_fetch_time().unwrap_or(None)
        };
        let now = Utc::now();
        if let Some(last) = last_fetch {
            let elapsed = (now - last).num_seconds();
            if elapsed < FETCH_INTERVAL_SECS {
                let wait_secs = (FETCH_INTERVAL_SECS - elapsed) as u64;
                let next = last + chrono::Duration::seconds(FETCH_INTERVAL_SECS);
                info!(
                    last_fetch = %last.format("%Y-%m-%dT%H:%M:%SZ"),
                    next_fetch = %next.format("%Y-%m-%dT%H:%M:%SZ"),
                    wait_secs,
                    "Forecast poller: not due yet, sleeping"
                );
                {
                    let mut s = status.lock().unwrap();
                    s.next_fetch = Some(next);
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(wait_secs)).await;
                continue;
            }
        }

        info!(
            area_count = areas.len(),
            "Forecast poller: triggering fetch"
        );

        let mut fetch_error = false;
        'areas: for area in &areas {
            info!(
                area_id = area.id,
                lat_min = area.lat_min, lat_max = area.lat_max,
                lon_min = area.lon_min, lon_max = area.lon_max,
                "Fetching area forecast"
            );
            match crate::forecast::fetch_area_forecast(
                area.lat_min, area.lat_max, area.lon_min, area.lon_max,
            )
            .await
            {
                Ok(forecasts) => {
                    {
                        let mut s = status.lock().unwrap();
                        s.online = true;
                    }
                    let fetched_at = Utc::now();
                    let db = db.read().unwrap_or_else(|e| e.into_inner());
                    for f in &forecasts {
                        if let Err(e) = db.insert_forecast(
                            area.id, f.lat, f.lon, fetched_at, &f.hourly,
                        ) {
                            warn!(area_id = area.id, error = %e, "Failed to store forecast point");
                        }
                    }
                    info!(
                        area_id = area.id,
                        grid_points = forecasts.len(),
                        "Area forecast stored"
                    );
                }
                Err(e) => {
                    warn!(
                        area_id = area.id,
                        error = %e,
                        retry_secs = RETRY_SECS,
                        "Area forecast fetch failed"
                    );
                    {
                        let mut s = status.lock().unwrap();
                        s.online = false;
                    }
                    fetch_error = true;
                    break 'areas;
                }
            }
        }

        if fetch_error {
            tokio::time::sleep(tokio::time::Duration::from_secs(RETRY_SECS)).await;
            continue;
        }

        let next = Utc::now() + chrono::Duration::seconds(FETCH_INTERVAL_SECS);
        {
            let mut s = status.lock().unwrap();
            s.last_fetch = Some(Utc::now());
            s.next_fetch = Some(next);
            s.online = true;
        }
        info!(
            area_count = areas.len(),
            next_fetch = %next.format("%Y-%m-%dT%H:%M:%SZ"),
            "Forecast poller: all areas fetched successfully"
        );
        tokio::time::sleep(tokio::time::Duration::from_secs(FETCH_INTERVAL_SECS as u64)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_status_serialises() {
        let s = ForecastPollerStatus::default();
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"online\":true"));
        assert!(json.contains("\"last_fetch\":null"));
    }
}
