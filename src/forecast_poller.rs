use crate::db::VesselDatabase;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::{Arc, Mutex, RwLock};
use tracing::{info, warn};

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
        let active_trip = {
            let db = db.read().unwrap_or_else(|e| e.into_inner());
            db.get_active_trip_id().unwrap_or(None)
        };
        let Some(trip_id) = active_trip else {
            tokio::time::sleep(tokio::time::Duration::from_secs(IDLE_CHECK_SECS)).await;
            continue;
        };

        let areas = {
            let db = db.read().unwrap_or_else(|e| e.into_inner());
            db.list_forecast_areas(trip_id).unwrap_or_default()
        };
        if areas.is_empty() {
            tokio::time::sleep(tokio::time::Duration::from_secs(IDLE_CHECK_SECS)).await;
            continue;
        }

        let last_fetch = {
            let db = db.read().unwrap_or_else(|e| e.into_inner());
            db.get_last_fetch_time(trip_id).unwrap_or(None)
        };
        let now = Utc::now();
        if let Some(last) = last_fetch {
            let elapsed = (now - last).num_seconds();
            if elapsed < FETCH_INTERVAL_SECS {
                let wait_secs = (FETCH_INTERVAL_SECS - elapsed) as u64;
                let next = last + chrono::Duration::seconds(FETCH_INTERVAL_SECS);
                {
                    let mut s = status.lock().unwrap();
                    s.next_fetch = Some(next);
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(wait_secs)).await;
                continue;
            }
        }

        let mut fetch_error = false;
        'areas: for area in &areas {
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
                            trip_id, area.id, f.lat, f.lon, fetched_at, &f.hourly,
                        ) {
                            warn!("Failed to store forecast point for trip {}: {}", trip_id, e);
                        }
                    }
                    info!(
                        "Forecast fetched for trip {} area {}: {} grid points",
                        trip_id, area.id, forecasts.len()
                    );
                }
                Err(e) => {
                    warn!("Forecast fetch failed for trip {} area {}: {}", trip_id, area.id, e);
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
