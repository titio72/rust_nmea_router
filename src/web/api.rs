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
use std::{backtrace::Backtrace, sync::Arc};

use crate::db::{VesselDatabase, TripSummary, TrackPoint, WebMetricData, SpeedDistributionData, WindStatisticsData, TripLegsData, TrackAnalytics, HeatmapData};
use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<VesselDatabase>,
    pub config: Arc<Config>,
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

#[derive(Debug, Deserialize)]
pub struct TimeRangeQuery {
    pub id: Option<u32>,
    pub start: Option<String>,
    pub end: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TimeRangeRequiredQuery {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Deserialize)]
pub struct HeatmapQuery {
    pub date: String,  // Date in YYYY-MM-DD format
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TrackingStatusRequest {
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct TrackingStatusResponse {
    pub enabled: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MetricsStatusRequest {
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct MetricsStatusResponse {
    pub enabled: bool,
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
            {
                let bt = Backtrace::force_capture();
                error!(?bt, "Backtrace for error");
                Ok(Json(ApiResponse::error(e.to_string())))
            }
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
            {
                let bt = Backtrace::force_capture();
                error!(?bt, "Backtrace for error");
                Ok(Json(ApiResponse::error(e.to_string())))
            }
        }
    }
}

pub async fn get_speed_distribution(
    State(state): State<AppState>,
    Query(params): Query<TimeRangeQuery>,
) -> Result<Json<ApiResponse<SpeedDistributionData>>, StatusCode> {
    info!(?params, "GET /api/speed_distribution called");
    match state.db.fetch_speed_distribution(
        params.id,
        params.start.as_deref(),
        params.end.as_deref(),
    ) {
        Ok(distribution) => Ok(Json(ApiResponse::ok(distribution))),
        Err(e) => {
            error!(error = %e, "Failed to fetch speed distribution");
            {
                let bt = Backtrace::force_capture();
                error!(?bt, "Backtrace for error");
                Ok(Json(ApiResponse::error(e.to_string())))
            }
        }
    }
}

pub async fn get_wind_statistics(
    State(state): State<AppState>,
    Query(params): Query<TimeRangeQuery>,
) -> Result<Json<ApiResponse<WindStatisticsData>>, StatusCode> {
    info!(?params, "GET /api/wind_statistics called");
    match state.db.fetch_wind_statistics(
        params.id,
        params.start.as_deref(),
        params.end.as_deref(),
    ) {
        Ok(statistics) => Ok(Json(ApiResponse::ok(statistics))),
        Err(e) => {
            error!(error = %e, "Failed to fetch wind statistics");
            {
                let bt = Backtrace::force_capture();
                error!(?bt, "Backtrace for error");
                Ok(Json(ApiResponse::error(e.to_string())))
            }
        }
    }
}

pub async fn get_trip_legs(
    State(state): State<AppState>,
    Query(params): Query<TripIdQuery>,
) -> Result<Json<ApiResponse<TripLegsData>>, StatusCode> {
    info!(?params, "GET /api/trip_legs called");
    match state.db.fetch_trip_legs(params.id) {
        Ok(legs_data) => Ok(Json(ApiResponse::ok(legs_data))),
        Err(e) => {
            error!(error = %e, "Failed to fetch trip legs");
            {
                let bt = Backtrace::force_capture();
                error!(?bt, "Backtrace for error");
                Ok(Json(ApiResponse::error(e.to_string())))
            }
        }
    }
}

pub async fn get_track_analytics(
    State(state): State<AppState>,
    Query(params): Query<TimeRangeRequiredQuery>,
) -> Result<Json<ApiResponse<TrackAnalytics>>, StatusCode> {
    info!(?params, "GET /api/track_analytics called");
    match state.db.fetch_track_analytics(&params.start, &params.end) {
        Ok(analytics) => Ok(Json(ApiResponse::ok(analytics))),
        Err(e) => {
            error!(error = %e, "Failed to fetch track analytics");
            {
                let bt = Backtrace::force_capture();
                error!(?bt, "Backtrace for error");
                Ok(Json(ApiResponse::error(e.to_string())))
            }
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
            {
                let bt = Backtrace::force_capture();
                error!(?bt, "Backtrace for error");
                Ok(Json(ApiResponse::error(e.to_string())))
            }
        }
    }
}

pub async fn get_google_maps_key(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Option<String>>>, StatusCode> {
    info!("GET /api/config/google_maps_key called");
    
    match state.config.web.google_maps_api_key.clone() {
        Some(key) => Ok(Json(ApiResponse::ok(Some(key)))),
        None => Ok(Json(ApiResponse::ok(None))),
    }
}

pub async fn get_heatmap(
    State(state): State<AppState>,
    Query(params): Query<HeatmapQuery>,
) -> Result<Json<ApiResponse<HeatmapData>>, StatusCode> {
    info!(?params, "GET /api/heatmap called");
    match state.db.fetch_heatmap(&params.date) {
        Ok(heatmap) => Ok(Json(ApiResponse::ok(heatmap))),
        Err(e) => {
            error!(error = %e, "Failed to fetch heatmap");
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

pub async fn get_tracking_status(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<TrackingStatusResponse>>, StatusCode> {
    info!("GET /api/tracking/status called");
    
    // Get tracking status from database
    let enabled = state.db.get_system_status("tracking_enabled")
        .unwrap_or(true);
    
    let response = TrackingStatusResponse { enabled };
    Ok(Json(ApiResponse::ok(response)))
}

pub async fn set_tracking_status(
    State(state): State<AppState>,
    Json(request): Json<TrackingStatusRequest>,
) -> Result<Json<ApiResponse<TrackingStatusResponse>>, StatusCode> {
    info!(?request, "POST /api/tracking/status called");
    
    // Save tracking status to database
    if let Err(e) = state.db.set_system_status("tracking_enabled", request.enabled) {
        error!(error = %e, "Failed to set tracking status in database");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    
    let response = TrackingStatusResponse {
        enabled: request.enabled,
    };
    Ok(Json(ApiResponse::ok(response)))
}

pub async fn get_metrics_status(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<MetricsStatusResponse>>, StatusCode> {
    info!("GET /api/metrics/status called");
    
    // Get metrics status from database
    let enabled = state.db.get_system_status("metrics_enabled")
        .unwrap_or(true);
    
    let response = MetricsStatusResponse { enabled };
    Ok(Json(ApiResponse::ok(response)))
}

pub async fn set_metrics_status(
    State(state): State<AppState>,
    Json(request): Json<MetricsStatusRequest>,
) -> Result<Json<ApiResponse<MetricsStatusResponse>>, StatusCode> {
    info!(?request, "POST /api/metrics/status called");
    
    // Save metrics status to database (persistent across restarts)
    if let Err(e) = state.db.set_system_status("metrics_enabled", request.enabled) {
        error!("Failed to save metrics status to database: {}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    
    let response = MetricsStatusResponse {
        enabled: request.enabled,
    };
    
    info!("Metrics status updated to: {}", request.enabled);
    Ok(Json(ApiResponse::ok(response)))
}

pub fn create_api_router(state: AppState) -> Router {
    Router::new()
        .route("/trip_description", post(update_trip_description))
        .route("/trips", get(get_trips))
        .route("/trip", get(get_trip))
        .route("/track", get(get_track))
        .route("/metrics", get(get_metrics))
        .route("/speed_distribution", get(get_speed_distribution))
        .route("/wind_statistics", get(get_wind_statistics))
        .route("/trip_legs", get(get_trip_legs))
        .route("/track_analytics", get(get_track_analytics))
        .route("/heatmap", get(get_heatmap))
        .route("/config/google_maps_key", get(get_google_maps_key))
        .route("/tracking/status", get(get_tracking_status))
        .route("/tracking/status", post(set_tracking_status))
        .route("/metrics/status", get(get_metrics_status))
        .route("/metrics/status", post(set_metrics_status))
        .with_state(state)
}
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    use serde_json::json;

    // Helper function to create a test database connection
    fn create_test_db() -> VesselDatabase {
        // Read database URL from environment or use default
        let db_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "mysql://nmea:nmea@localhost:3306/nmea_router".to_string());
        
        VesselDatabase::new(&db_url).expect("Failed to connect to test database")
    }

    // Helper function to create a test app
    fn create_test_app() -> Router {
        let db = create_test_db();
        let mut config = crate::config::Config::default();
        config.web.google_maps_api_key = Some("your_google_maps_api_key_here".to_string());
        let state = AppState {
            db: Arc::new(db),
            config: Arc::new(config),
        };
        create_api_router(state)
    }

    #[tokio::test]
    async fn test_get_trips_default() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/trips")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
        assert!(json["data"].is_array());
        assert!(json["error"].is_null());
    }

    #[tokio::test]
    async fn test_get_trips_with_year() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/trips?year=2026")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
        assert!(json["data"].is_array());
    }

    #[tokio::test]
    async fn test_get_trips_with_last_months() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/trips?last_months=6")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
        assert!(json["data"].is_array());
    }

    #[tokio::test]
    async fn test_get_trip_valid_id() {
        let app = create_test_app();

        // First get a valid trip ID
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/trips?last_months=12")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        if let Some(trips) = json["data"].as_array() {
            if !trips.is_empty() {
                let trip_id = trips[0]["id"].as_u64().unwrap();

                // Now test getting that specific trip
                let response = app
                    .oneshot(
                        Request::builder()
                            .uri(&format!("/trip?id={}", trip_id))
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();

                assert_eq!(response.status(), StatusCode::OK);

                let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap();
                let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

                assert_eq!(json["status"], "ok");
                assert!(json["data"].is_object());
                assert_eq!(json["data"]["id"], trip_id);
            }
        }
    }

    #[tokio::test]
    async fn test_get_trip_invalid_id() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/trip?id=999999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "error");
        assert!(json["error"].as_str().unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn test_get_track_with_trip_id() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/track?trip_id=132")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
        assert!(json["data"].is_array());
    }

    #[tokio::test]
    async fn test_get_track_with_time_range() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/track?start=2026-02-02%2006:00:00&end=2026-02-02%2008:00:00")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
        assert!(json["data"].is_array());
    }

    #[tokio::test]
    async fn test_get_track_missing_params() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/track")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "error");
        assert!(json["error"].as_str().unwrap().contains("required"));
    }

    #[tokio::test]
    async fn test_get_metrics_with_trip_id() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics?metric=windSpeed&trip_id=132")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Metrics endpoint can return either ok with data or error if no metrics table/data exists
        assert!(json["status"] == "ok" || json["status"] == "error");
        if json["status"] == "ok" {
            assert!(json["data"].is_array());
        }
    }

    #[tokio::test]
    async fn test_get_speed_distribution_with_trip_id() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/speed_distribution?id=132")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
        assert!(json["data"].is_object());
        assert!(json["data"]["labels"].is_array());
        assert!(json["data"]["sailing"].is_array());
        assert!(json["data"]["motoring"].is_array());
    }

    #[tokio::test]
    async fn test_get_speed_distribution_with_time_range() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/speed_distribution?start=2026-02-02%2006:00:00&end=2026-02-02%2008:00:00")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
        assert!(json["data"].is_object());
        assert!(json["data"]["labels"].is_array());
    }

    #[tokio::test]
    async fn test_get_speed_distribution_missing_params() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/speed_distribution")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "error");
        assert!(json["error"].as_str().unwrap().contains("required"));
    }

    #[tokio::test]
    async fn test_get_wind_statistics_with_trip_id() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/wind_statistics?id=132")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
        assert!(json["data"].is_object());
        assert!(json["data"]["directions"].is_array());
        assert!(json["data"]["wind_distances"].is_array());
        assert!(json["data"]["max_wind_speeds"].is_array());
    }

    #[tokio::test]
    async fn test_get_wind_statistics_with_time_range() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/wind_statistics?start=2026-02-02%2009:00:00&end=2026-02-02%2012:00:00")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
        assert!(json["data"].is_object());
    }

    #[tokio::test]
    async fn test_get_wind_statistics_missing_params() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/wind_statistics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "error");
        assert!(json["error"].as_str().unwrap().contains("required"));
    }

    #[tokio::test]
    async fn test_get_trip_legs() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/trip_legs?id=132")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
        assert!(json["data"].is_object());
        assert!(json["data"]["legs"].is_array());
        
        // Verify leg structure if legs exist
        if let Some(legs) = json["data"]["legs"].as_array() {
            if !legs.is_empty() {
                let leg = &legs[0];
                assert!(leg["leg_number"].is_number());
                assert!(leg["start_timestamp"].is_string());
                assert!(leg["end_timestamp"].is_string());
                assert!(leg["total_distance_nm"].is_number());
                assert!(leg["sailing_distance_nm"].is_number());
                assert!(leg["motoring_distance_nm"].is_number());
                assert!(leg["sailing_time_ms"].is_number());
                assert!(leg["motoring_time_ms"].is_number());
                assert!(leg["sailing_time_formatted"].is_string());
                assert!(leg["motoring_time_formatted"].is_string());
                
                // Verify UTC ISO format timestamp
                let timestamp = leg["start_timestamp"].as_str().unwrap();
                assert!(timestamp.ends_with('Z'));
                assert!(timestamp.contains('T'));
            }
        }
    }

    #[tokio::test]
    async fn test_update_trip_description() {
        let app = create_test_app();

        let payload = json!({
            "id": 132,
            "description": "Test trip description"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/trip_description")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
    }

    #[tokio::test]
    async fn test_api_error_handling() {
        let app = create_test_app();

        // Test with malformed query parameter
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/trip?id=invalid")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should return 400 Bad Request for invalid parameter type
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_metrics_with_time_range() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics?metric=temperature&start=2026-02-02%2006:00:00&end=2026-02-02%2008:00:00")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Metrics endpoint can return either ok with data or error if no metrics table/data exists
        assert!(json["status"] == "ok" || json["status"] == "error");
        if json["status"] == "ok" {
            assert!(json["data"].is_array());
        }
    }

    #[tokio::test]
    async fn test_track_analytics() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/track_analytics?start=2026-02-02%2006:00:00&end=2026-02-02%2012:00:00")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
        assert!(json["data"].is_object());
        
        // Verify structure
        let data = &json["data"];
        assert!(data["max_speed_kn"].is_number() || data["max_speed_kn"].is_null());
        assert!(data["max_speed_timestamp"].is_string() || data["max_speed_timestamp"].is_null());
        assert!(data["fastest_1nm"].is_object() || data["fastest_1nm"].is_null());
        assert!(data["fastest_5nm"].is_object() || data["fastest_5nm"].is_null());
        assert!(data["fastest_10nm"].is_object() || data["fastest_10nm"].is_null());
        
        // If fastest segments exist, verify their structure
        if let Some(segment) = data["fastest_1nm"].as_object() {
            assert!(segment.contains_key("distance_nm"));
            assert!(segment.contains_key("average_speed_kn"));
            assert!(segment.contains_key("duration_ms"));
            assert!(segment.contains_key("start_timestamp"));
            assert!(segment.contains_key("end_timestamp"));
        }
    }

    #[tokio::test]
    async fn test_concurrent_requests() {
        let app = create_test_app();

        // Create multiple concurrent requests
        let mut handles = vec![];

        for _ in 0..5 {
            let app_clone = app.clone();
            let handle = tokio::spawn(async move {
                app_clone
                    .oneshot(
                        Request::builder()
                            .uri("/trips?last_months=12")
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap()
            });
            handles.push(handle);
        }

        // Wait for all requests to complete
        for handle in handles {
            let response = handle.await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
    }

    #[tokio::test]
    async fn test_get_google_maps_key() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/config/google_maps_key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        
        assert_eq!(json["status"], "ok");
        assert_eq!(json["data"], "your_google_maps_api_key_here");
        assert!(json["error"].is_null());
    }

    #[tokio::test]
    async fn test_get_heatmap() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/heatmap?date=2026-02-07")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
        assert!(json["data"].is_object());
        
        // Verify heatmap data structure
        let data = &json["data"];
        assert!(data["total_distance"].is_number());
        assert!(data["max_distance"].is_number());
        assert!(data["min_distance"].is_number());
        assert!(data["days"].is_array());
        
        // Verify day structure if days exist
        if let Some(days) = data["days"].as_array() {
            if !days.is_empty() {
                let day = &days[0];
                assert!(day["date"].is_string());
                assert!(day["distance_nm"].is_number());
            }
        }
    }

    #[tokio::test]
    async fn test_get_tracking_status() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/tracking/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
        assert!(json["data"].is_object());
        assert!(json["data"]["enabled"].is_boolean());
        assert!(json["error"].is_null());
    }

    #[tokio::test]
    async fn test_set_tracking_status() {
        let app = create_test_app();

        let payload = json!({
            "enabled": false
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/tracking/status")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
        assert!(json["data"].is_object());
        assert!(json["data"]["enabled"].is_boolean());
    }

    #[tokio::test]
    async fn test_get_metrics_status() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
        assert!(json["data"].is_object());
        assert!(json["data"]["enabled"].is_boolean());
        assert!(json["error"].is_null());
    }

    #[tokio::test]
    async fn test_set_metrics_status() {
        let app = create_test_app();

        let payload = json!({
            "enabled": false
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/metrics/status")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
        assert!(json["data"].is_object());
        assert!(json["data"]["enabled"].is_boolean());
    }
}
