use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
    routing::get,
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};
use tracing::{info, error};
use std::sync::Arc;

use crate::db::{VesselDatabase, TripSummary, TrackPoint, WebMetricData};

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<VesselDatabase>,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub status: String,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            status: "ok".to_string(),
            data: Some(data),
            error: None,
        }
    }

    pub fn error(message: String) -> Self {
        Self {
            status: "error".to_string(),
            data: None,
            error: Some(message),
        }
    }
}

// Query parameters
#[derive(Debug, Deserialize)]
pub struct TripIdQuery {
    pub id: u32,
}

// Query parameters
#[derive(Debug, Deserialize)]
pub struct TripDescriptionQuery {
    pub id: u32,
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct TrackQuery {
    pub trip_id: Option<u32>,
    pub start: Option<String>,
    pub end: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MetricsQuery {
    pub metric: String,
    pub trip_id: Option<u32>,
    pub start: Option<String>,
    pub end: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TripsQuery {
    pub year: Option<i32>,
    pub last_months: Option<u32>,
}

pub async fn get_trips(
    State(state): State<AppState>,
    Query(params): Query<TripsQuery>,
) -> Result<Json<ApiResponse<Vec<TripSummary>>>, StatusCode> {
    info!(?params, "GET /api/trips called");
    match state.db.fetch_trips(params.year, params.last_months) {
        Ok(trips) => Ok(Json(ApiResponse::ok(trips))),
        Err(e) => {
            error!(error = %e, "Failed to fetch trips");
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

pub async fn get_trip(
    State(state): State<AppState>,
    Query(params): Query<TripIdQuery>,
) -> Result<Json<ApiResponse<TripSummary>>, StatusCode> {
    info!(?params, "GET /api/trip called");
    match state.db.fetch_trip(params.id) {
        Ok(res_trip) => {
            if let Some(trip) = res_trip {
                Ok(Json(ApiResponse::ok(trip)))
            } else {
                error!(trip_id = params.id, "Trip not found");
                Ok(Json(ApiResponse::error(format!("Trip {} not found", params.id))))
            }
        }
        Err(e) => {
            error!(error = %e, "Failed to fetch trip");
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

pub async fn get_track(
    State(state): State<AppState>,
    Query(params): Query<TrackQuery>,
) -> Result<Json<ApiResponse<Vec<TrackPoint>>>, StatusCode> {
    info!(?params, "GET /api/track called");
    match state.db.fetch_track(
        params.trip_id,
        params.start.as_deref(),
        params.end.as_deref(),
    ) {
        Ok(track) => Ok(Json(ApiResponse::ok(track))),
        Err(e) => {
            error!(error = %e, "Failed to fetch track");
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

pub async fn get_metrics(
    State(state): State<AppState>,
    Query(params): Query<MetricsQuery>,
) -> Result<Json<ApiResponse<Vec<WebMetricData>>>, StatusCode> {
    info!(?params, "GET /api/metrics called");
    match state.db.fetch_metrics(
        &params.metric,
        params.trip_id,
        params.start.as_deref(),
        params.end.as_deref(),
    ) {
        Ok(metrics) => Ok(Json(ApiResponse::ok(metrics))),
        Err(e) => {
            error!(error = %e, "Failed to fetch metrics");
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

pub async fn update_trip_description(
    State(state): State<AppState>,
    Json(params): Json<TripDescriptionQuery>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {

    info!(?params, "POST /api/trip_description called");
    
    match state.db.update_trip_description(params.id as i64, &params.description) {
        Ok(()) => Ok(Json(ApiResponse::ok(()))),
        Err(e) => {
            error!(error = %e, "Failed to update trip description");
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

pub fn create_api_router(state: AppState) -> Router {
    Router::new()
        .route("/trip_description", post(update_trip_description))
        .route("/trips", get(get_trips))
        .route("/trip", get(get_trip))
        .route("/track", get(get_track))
        .route("/metrics", get(get_metrics))
        .with_state(state)
}
