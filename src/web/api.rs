use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Query, State, Multipart},
    http::{header, StatusCode},
    response::{Json, Response},
    routing::get,
    routing::post,
    routing::delete,
    Router,
};
use serde::{Deserialize, Serialize};
use tracing::{info, error, Span};
use std::{backtrace::Backtrace, sync::Arc, sync::atomic::{AtomicBool, Ordering}, sync::RwLock, time::Duration};
use tower_http::trace::TraceLayer;

use crate::db::{VesselDatabase, TripSummary, TrackPoint, WebMetricData, MultiMetricData, SpeedDistributionData, WindStatisticsData, TripLegsData, TrackAnalytics, HeatmapData};
use crate::config::Config;
use crate::web::broadcast_manager::SignalKBroadcastChannels;
use chrono::{DateTime, NaiveDate, Utc};

const MAX_IMPORT_TRIP_UPLOAD_BYTES: usize = 100 * 1024 * 1024; // 100 MiB

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<RwLock<VesselDatabase>>,
    pub config: Arc<Config>,
    pub signalk_broadcast: Arc<SignalKBroadcastChannels>,
    pub backup_in_progress: Arc<AtomicBool>,
}

impl AppState {
    /// Get a read guard for the database. Handles poisoned locks gracefully.
    pub fn db(&self) -> std::sync::RwLockReadGuard<'_, VesselDatabase> {
        self.db.read().unwrap_or_else(|e| e.into_inner())
    }
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

#[derive(Debug, Deserialize)]
pub struct TripUuidQuery {
    pub uuid: String,
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
    pub max_points: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct MetricsQuery {
    pub metric: String,
    pub trip_id: Option<u32>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub max_points: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct BatchMetricsQuery {
    pub metrics: String,  // comma-separated metric_id values, e.g. "1,2,4,5,6"
    pub trip_id: Option<u32>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub max_points: Option<usize>,
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
pub struct ExportTripQuery {
    pub id: u32,
    pub path: Option<String>,
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

#[derive(Debug, Serialize)]
pub struct ExportFileInfo {
    pub name: String,
    pub size: u64,
    pub modified: String,
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

#[derive(Debug, Deserialize, Serialize)]
pub struct SignalKStatusRequest {
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct SignalKStatusResponse {
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct BackupResponse {
    pub file: String,
}

#[derive(Debug, Serialize)]
pub struct BackupFileInfo {
    pub name: String,
    pub size: u64,
    pub modified: String,
}

#[derive(Debug, Deserialize)]
pub struct DeleteBackupQuery {
    pub file: Option<String>,
    pub all: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct DownloadBackupQuery {
    pub file: String,
}

fn parse_datetime_str(s: &str) -> Result<DateTime<Utc>, StatusCode> {
    // Try RFC3339 first (e.g. "2026-01-20T10:00:00Z")
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    // Fall back to SQL-style UTC datetime (e.g. "2026-01-20 10:00:00")
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .map(|ndt| DateTime::from_naive_utc_and_offset(ndt, Utc))
        .map_err(|_| StatusCode::BAD_REQUEST)
}

fn parse_optional_datetime(s: &Option<String>) -> Result<Option<DateTime<Utc>>, StatusCode> {
    match s {
        None => Ok(None),
        Some(s) => parse_datetime_str(s).map(Some),
    }
}

fn parse_required_datetime(s: &str) -> Result<DateTime<Utc>, StatusCode> {
    parse_datetime_str(s)
}

pub async fn get_trips(
    State(state): State<AppState>,
    Query(params): Query<TripsQuery>,
) -> Result<Json<ApiResponse<Vec<TripSummary>>>, StatusCode> {
    match state.db().fetch_trips(params.year, params.last_months) {
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
    match state.db().fetch_trip(params.id) {
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

pub async fn get_trip_by_uuid(
    State(state): State<AppState>,
    Query(params): Query<TripUuidQuery>,
) -> Result<Json<ApiResponse<TripSummary>>, StatusCode> {
    match state.db().fetch_trip_by_uuid(&params.uuid) {
        Ok(Some(trip)) => Ok(Json(ApiResponse::ok(trip))),
        Ok(None) => {
            error!(uuid = %params.uuid, "Trip not found by UUID");
            Ok(Json(ApiResponse::error(format!("Trip with UUID {} not found", params.uuid))))
        }
        Err(e) => {
            error!(error = %e, "Failed to fetch trip by UUID");
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

pub async fn get_track(
    State(state): State<AppState>,
    Query(params): Query<TrackQuery>,
) -> Result<Json<ApiResponse<Vec<TrackPoint>>>, StatusCode> {
    let start = parse_optional_datetime(&params.start)?;
    let end = parse_optional_datetime(&params.end)?;
    match state.db().fetch_track(
        params.trip_id,
        start,
        end,
        params.max_points,
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
    let start = parse_optional_datetime(&params.start)?;
    let end = parse_optional_datetime(&params.end)?;
    match state.db().fetch_metrics(
        &params.metric,
        params.trip_id,
        start,
        end,
        params.max_points,
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

pub async fn get_metrics_batch(
    State(state): State<AppState>,
    Query(params): Query<BatchMetricsQuery>,
) -> Result<Json<ApiResponse<MultiMetricData>>, StatusCode> {
    // Parse comma-separated metric ids into u8 values
    let metric_ids: Result<Vec<u8>, _> = params.metrics
        .split(',')
        .map(|s| s.trim().parse::<u8>())
        .collect();
    let metric_ids = match metric_ids {
        Ok(ids) if !ids.is_empty() => ids,
        _ => return Ok(Json(ApiResponse::error("Invalid or empty metrics parameter".to_string()))),
    };
    let start = parse_optional_datetime(&params.start)?;
    let end = parse_optional_datetime(&params.end)?;
    match state.db().fetch_metrics_batch(
        &metric_ids,
        params.trip_id,
        start,
        end,
        params.max_points,
    ) {
        Ok(data) => Ok(Json(ApiResponse::ok(data))),
        Err(e) => {
            error!(error = %e, "Failed to fetch metrics batch");
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

pub async fn get_speed_distribution(
    State(state): State<AppState>,
    Query(params): Query<TimeRangeQuery>,
) -> Result<Json<ApiResponse<SpeedDistributionData>>, StatusCode> {
    let start = parse_optional_datetime(&params.start)?;
    let end = parse_optional_datetime(&params.end)?;
    match state.db().fetch_speed_distribution(
        params.id,
        start,
        end,
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
    let start = parse_optional_datetime(&params.start)?;
    let end = parse_optional_datetime(&params.end)?;
    match state.db().fetch_wind_statistics(
        params.id,
        start,
        end,
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
    match state.db().fetch_trip_legs(params.id) {
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
    let start = parse_required_datetime(&params.start)?;
    let end = parse_required_datetime(&params.end)?;
    match state.db().fetch_track_analytics(start, end) {
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

pub async fn get_monthly_statistics(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<crate::db::MonthlyStatistics>>, StatusCode> {
    match state.db().fetch_monthly_statistics() {
        Ok(stats) => Ok(Json(ApiResponse::ok(stats))),
        Err(e) => {
            error!(error = %e, "Failed to fetch monthly statistics");
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
    
    match state.db().update_trip_description(params.id as i64, &params.description) {
        Ok(()) => {
            info!(trip_id = params.id, description = %params.description, "Trip description updated successfully");
            Ok(Json(ApiResponse::ok(())))
        },
        Err(e) => {
            error!(error = %e, trip_id = params.id, "Failed to update trip description");
            {
                let bt = Backtrace::force_capture();
                error!(?bt, "Backtrace for error");
                Ok(Json(ApiResponse::error(e.to_string())))
            }
        }
    }
}

pub async fn delete_trip(
    State(state): State<AppState>,
    Query(params): Query<TripIdQuery>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    match state.db().delete_trip(params.id) {
        Ok(()) => {
            info!(trip_id = params.id, "Trip deleted successfully");
            Ok(Json(ApiResponse::ok(())))
        },
        Err(e) => {
            error!(error = %e, trip_id = params.id, "Failed to delete trip");
            {
                let bt = Backtrace::force_capture();
                error!(?bt, "Backtrace for error");
                Ok(Json(ApiResponse::error(e.to_string())))
            }
        }
    }
}

pub async fn trim_trip(
    State(state): State<AppState>,
    Query(params): Query<TripIdQuery>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    match state.db().trim_trip(params.id) {
        Ok(()) => {
            info!(trip_id = params.id, "Trip trimmed successfully");
            Ok(Json(ApiResponse::ok(())))
        },
        Err(e) => {
            error!(error = %e, trip_id = params.id, "Failed to trim trip");
            {
                let bt = Backtrace::force_capture();
                error!(?bt, "Backtrace for error");
                Ok(Json(ApiResponse::error(e.to_string())))
            }
        }
    }
}

pub async fn export_trip(
    State(state): State<AppState>,
    Query(params): Query<ExportTripQuery>,
) -> Result<Json<ApiResponse<String>>, StatusCode> {
    // Determine the export path
    let export_path = params.path.clone().unwrap_or_else(|| {
        format!("static/exports/trip_{}.json", params.id)
    });
    
    info!(trip_id = params.id, path = %export_path, "Exporting trip");
    
    match state.db().export_trip(params.id as i64, &export_path) {
        Ok(()) => {
            info!(trip_id = params.id, path = %export_path, "Trip exported successfully");
            Ok(Json(ApiResponse::ok(format!("Trip {} exported to {}", params.id, export_path))))
        }
        Err(e) => {
            error!(error = %e, trip_id = params.id, "Failed to export trip");
            {
                let bt = Backtrace::force_capture();
                error!(?bt, "Backtrace for error");
                Ok(Json(ApiResponse::error(e.to_string())))
            }
        }
    }
}

pub async fn import_trip(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<ApiResponse<String>>, StatusCode> {
    // Process multipart fields with better error logging
    loop {
        match multipart.next_field().await {
            Ok(Some(field)) => {
                let field_name = field.name().unwrap_or("unknown").to_string();
                info!("Received field: {}", field_name);
                
                // Only process the "file" field
                if field_name != "file" {
                    continue;
                }
                
                // Read the field as bytes to better handle large files
                match field.bytes().await {
                    Ok(file_bytes) => {
                        info!("Processing uploaded JSON file for trip import, content size: {} bytes", file_bytes.len());
                        
                        // Convert bytes to UTF-8 string
                        let json_content = match std::str::from_utf8(&file_bytes) {
                            Ok(content) => content,
                            Err(e) => {
                                error!(error = %e, "Uploaded file is not valid UTF-8 JSON");
                                return Ok(Json(ApiResponse::error(
                                    "Uploaded file is not valid UTF-8 JSON".to_string(),
                                )));
                            }
                        };
                        
                        match state.db().import_trip(json_content) {
                            Ok(trip_id) => {
                                info!(trip_id = trip_id, "Trip imported successfully");
                                return Ok(Json(ApiResponse::ok(format!("Trip imported successfully with ID: {}", trip_id))));
                            }
                            Err(e) => {
                                error!(error = %e, "Failed to import trip");
                                let error_msg = e.to_string();
                                return Ok(Json(ApiResponse::error(error_msg)));
                            }
                        }
                    }
                    Err(read_err) => {
                        error!(error = %read_err, "Failed to read uploaded file content");
                        let error_msg = format!("Failed to read file: {}", read_err);
                        return Ok(Json(ApiResponse::error(error_msg)));
                    }
                }
            }
            Ok(None) => {
                info!("No more multipart fields");
                break;
            }
            Err(multipart_err) => {
                error!(error = %multipart_err, "Multipart parsing error");
                let error_msg = format!("Multipart parsing error: {}", multipart_err);
                return Ok(Json(ApiResponse::error(error_msg)));
            }
        }
    }
    
    Ok(Json(ApiResponse::error("No file uploaded".to_string())))
}

pub async fn list_exports() -> Result<Json<ApiResponse<Vec<ExportFileInfo>>>, StatusCode> {
    use std::fs;
    use std::path::Path;
    
    let export_dir = Path::new("static/exports");
    
    // Create the directory if it doesn't exist
    if !export_dir.exists() {
        match fs::create_dir_all(export_dir) {
            Ok(_) => {},
            Err(e) => {
                error!(error = %e, "Failed to create exports directory");
                return Ok(Json(ApiResponse::error(e.to_string())));
            }
        }
    }
    
    match fs::read_dir(export_dir) {
        Ok(entries) => {
            let mut files = Vec::new();
            
            for entry in entries {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if path.is_file() {
                        if let Ok(metadata) = path.metadata() {
                            if let Some(name) = path.file_name() {
                                if let Some(name_str) = name.to_str() {
                                    // Only include JSON files
                                    if name_str.ends_with(".json") {
                                        let modified = match metadata.modified() {
                                            Ok(time) => {
                                                match chrono::DateTime::<chrono::Utc>::from(time)
                                                    .format("%Y-%m-%d %H:%M:%S UTC")
                                                    .to_string()
                                                {
                                                    s => s,
                                                }
                                            }
                                            Err(_) => "Unknown".to_string(),
                                        };
                                        
                                        files.push(ExportFileInfo {
                                            name: name_str.to_string(),
                                            size: metadata.len(),
                                            modified,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
            
            // Sort by name (most recent first, assuming naming convention)
            files.sort_by(|a, b| b.name.cmp(&a.name));
            
            info!("Found {} export files", files.len());
            Ok(Json(ApiResponse::ok(files)))
        }
        Err(e) => {
            error!(error = %e, "Failed to read exports directory");
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

pub async fn get_google_maps_key(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Option<String>>>, StatusCode> {
    match state.config.web.google_maps_api_key.clone() {
        Some(key) => Ok(Json(ApiResponse::ok(Some(key)))),
        None => Ok(Json(ApiResponse::ok(None))),
    }
}

pub async fn get_heatmap(
    State(state): State<AppState>,
    Query(params): Query<HeatmapQuery>,
) -> Result<Json<ApiResponse<HeatmapData>>, StatusCode> {
    let date = NaiveDate::parse_from_str(&params.date, "%Y-%m-%d")
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    match state.db().fetch_heatmap(date) {
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
    // Get tracking status from database
    let enabled = state.db().get_system_status("tracking_enabled")
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
    if let Err(e) = state.db().set_system_status("tracking_enabled", request.enabled) {
        error!(error = %e, "Failed to set tracking status in database");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    
    let response = TrackingStatusResponse {
        enabled: request.enabled,
    };
    info!("Tracking status updated to: {}", request.enabled);
    Ok(Json(ApiResponse::ok(response)))
}

pub async fn get_metrics_status(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<MetricsStatusResponse>>, StatusCode> {
    // Get metrics status from database
    let enabled = state.db().get_system_status("metrics_enabled")
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
    if let Err(e) = state.db().set_system_status("metrics_enabled", request.enabled) {
        error!("Failed to save metrics status to database: {}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    
    let response = MetricsStatusResponse {
        enabled: request.enabled,
    };
    
    info!("Metrics status updated to: {}", request.enabled);
    Ok(Json(ApiResponse::ok(response)))
}

pub async fn get_signalk_status(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<SignalKStatusResponse>>, StatusCode> {
    // Get signalk status from database
    let enabled = state.db().get_system_status("signalk_enabled")
        .unwrap_or(false);
    
    let response = SignalKStatusResponse { enabled };
    Ok(Json(ApiResponse::ok(response)))
}

pub async fn set_udp_broadcast_status(
    State(state): State<AppState>,
    Json(request): Json<SignalKStatusRequest>,
) -> Result<Json<ApiResponse<SignalKStatusResponse>>, StatusCode> {
    info!(?request, "POST /api/udp_broadcast/status called");
    
    // Save UDP broadcast status to database
    if let Err(e) = state.db().set_system_status("udp_broadcast_enabled", request.enabled) {
        error!(error = %e, "Failed to set UDP broadcast status in database");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    
    let response = SignalKStatusResponse {
        enabled: request.enabled,
    };
    
    info!("UDP broadcast status updated to: {}", request.enabled);
    Ok(Json(ApiResponse::ok(response)))
}

pub async fn get_udp_broadcast_status(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<SignalKStatusResponse>>, StatusCode> {
    // Get UDP broadcast status from database
    let enabled = state.db().get_system_status("udp_broadcast_enabled")
        .unwrap_or(false);
    
    let response = SignalKStatusResponse { enabled };
    Ok(Json(ApiResponse::ok(response)))
}


pub async fn set_signalk_status(
    State(state): State<AppState>,
    Json(request): Json<SignalKStatusRequest>,
) -> Result<Json<ApiResponse<SignalKStatusResponse>>, StatusCode> {
    info!(?request, "POST /api/signalk/status called");
    
    // Save signalk status to database
    if let Err(e) = state.db().set_system_status("signalk_enabled", request.enabled) {
        error!(error = %e, "Failed to set signalk status");
        return Ok(Json(ApiResponse::error(e.to_string())));
    }
    
    let response = SignalKStatusResponse {
        enabled: request.enabled,
    };
    
    info!("SignalK status updated to: {}", request.enabled);
    Ok(Json(ApiResponse::ok(response)))
}

pub async fn download_backup(
    Query(params): Query<DownloadBackupQuery>,
) -> Result<Response<Body>, StatusCode> {
    let filename = &params.file;

    // Prevent path traversal
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err(StatusCode::BAD_REQUEST);
    }

    let path = std::path::Path::new("backups").join(filename);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!(error = %e, file = %filename, "Failed to read backup file");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let disposition = format!("attachment; filename=\"{}\"", filename);
    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/gzip")
        .header(header::CONTENT_DISPOSITION, disposition)
        .header(header::CONTENT_LENGTH, bytes.len())
        .body(Body::from(bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    info!(file = %filename, "Backup download served");
    Ok(response)
}

pub async fn list_backups() -> Result<Json<ApiResponse<Vec<BackupFileInfo>>>, StatusCode> {
    use std::fs;

    let backup_dir = std::path::Path::new("backups");
    if !backup_dir.exists() {
        return Ok(Json(ApiResponse::ok(vec![])));
    }

    match fs::read_dir(backup_dir) {
        Ok(entries) => {
            let mut files = Vec::new();
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let (Ok(metadata), Some(name_str)) = (
                        path.metadata(),
                        path.file_name().and_then(|n| n.to_str()),
                    ) {
                        let modified = metadata
                            .modified()
                            .map(|t| {
                                chrono::DateTime::<chrono::Utc>::from(t)
                                    .format("%Y-%m-%d %H:%M:%S UTC")
                                    .to_string()
                            })
                            .unwrap_or_else(|_| "Unknown".to_string());
                        files.push(BackupFileInfo {
                            name: name_str.to_string(),
                            size: metadata.len(),
                            modified,
                        });
                    }
                }
            }
            files.sort_by(|a, b| b.name.cmp(&a.name));
            Ok(Json(ApiResponse::ok(files)))
        }
        Err(e) => {
            error!(error = %e, "Failed to read backups directory");
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

pub async fn delete_backup(
    Query(params): Query<DeleteBackupQuery>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    use std::fs;

    let backup_dir = std::path::Path::new("backups");

    if params.all.unwrap_or(false) {
        // Delete all backup files
        match fs::read_dir(backup_dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        if let Err(e) = fs::remove_file(&path) {
                            error!(path = %path.display(), error = %e, "Failed to delete backup file");
                            return Ok(Json(ApiResponse::error(format!(
                                "Failed to delete {}: {}",
                                path.display(),
                                e
                            ))));
                        }
                    }
                }
                info!("All backups deleted");
                Ok(Json(ApiResponse::ok(())))
            }
            Err(e) => {
                error!(error = %e, "Failed to read backups directory");
                Ok(Json(ApiResponse::error(e.to_string())))
            }
        }
    } else if let Some(filename) = params.file {
        // Validate filename to prevent path traversal
        if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
            return Ok(Json(ApiResponse::error("Invalid filename".to_string())));
        }
        let path = backup_dir.join(&filename);
        match fs::remove_file(&path) {
            Ok(()) => {
                info!(file = %filename, "Backup deleted");
                Ok(Json(ApiResponse::ok(())))
            }
            Err(e) => {
                error!(error = %e, file = %filename, "Failed to delete backup");
                Ok(Json(ApiResponse::error(format!("Failed to delete {}: {}", filename, e))))
            }
        }
    } else {
        Ok(Json(ApiResponse::error(
            "Specify 'file' or 'all=true'".to_string(),
        )))
    }
}

pub async fn post_backup(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<BackupResponse>>, StatusCode> {
    // Reject concurrent backup requests atomically
    if state.backup_in_progress
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return Ok(Json(ApiResponse::error(
            "A backup is already in progress".to_string(),
        )));
    }

    let backup_dir = std::path::Path::new("backups");
    if !backup_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(backup_dir) {
            state.backup_in_progress.store(false, Ordering::Release);
            error!(error = %e, "Failed to create backup directory");
            return Ok(Json(ApiResponse::error(format!(
                "Failed to create backup directory: {}", e
            ))));
        }
    }

    let filename = format!(
        "backup_{}.gz",
        chrono::Utc::now().format("%Y%m%d_%H%M%S")
    );
    let output_path = backup_dir.join(&filename);
    let output_str = output_path.to_string_lossy().to_string();

    let db_cfg = &state.config.database.connection;
    let user = db_cfg.username.clone();
    let password = db_cfg.password.clone();

    info!(output = %output_str, "Starting database backup");

    let result = tokio::process::Command::new("./scripts/backup.sh")
        .arg(&user)
        .arg(&password)
        .arg(&output_str)
        .output()
        .await;

    state.backup_in_progress.store(false, Ordering::Release);

    match result {
        Ok(output) if output.status.success() => {
            info!(file = %filename, "Backup completed successfully");
            Ok(Json(ApiResponse::ok(BackupResponse { file: filename })))
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            error!(stderr = %stderr, "Backup script failed");
            Ok(Json(ApiResponse::error(format!("Backup failed: {}", stderr))))
        }
        Err(e) => {
            error!(error = %e, "Failed to execute backup script");
            Ok(Json(ApiResponse::error(format!(
                "Failed to execute backup script: {}", e
            ))))
        }
    }
}

pub async fn system_shutdown() -> Result<Json<ApiResponse<String>>, StatusCode> {
    info!("System shutdown requested via API");

    // Spawn shutdown in background so we can return a response first
    tokio::spawn(async {
        // Brief delay to allow the HTTP response to be sent
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        let shutdown_script = std::path::Path::new("./shutdown.sh");
        if shutdown_script.exists() {
            info!("Executing shutdown.sh");
            let _ = tokio::process::Command::new("./shutdown.sh").spawn();
        } else {
            info!("Executing system shutdown via systemctl");
            let _ = tokio::process::Command::new("systemctl")
                .arg("poweroff")
                .spawn();
        }
    });

    Ok(Json(ApiResponse::ok("Shutdown initiated".to_string())))
}

pub fn create_api_router(state: AppState) -> Router {
    Router::new()
        .route("/trip_description", post(update_trip_description))
        .route("/delete_trip", delete(delete_trip))
        .route("/trim_trip", post(trim_trip))
        .route("/export_trip", get(export_trip))
        .route(
            "/import_trip",
            post(import_trip).layer(DefaultBodyLimit::max(MAX_IMPORT_TRIP_UPLOAD_BYTES)),
        )
        .route("/list_exports", get(list_exports))
        .route("/trips", get(get_trips))
        .route("/trip", get(get_trip))
        .route("/trip_by_uuid", get(get_trip_by_uuid))
        .route("/track", get(get_track))
        .route("/metrics", get(get_metrics))
        .route("/metrics/batch", get(get_metrics_batch))
        .route("/speed_distribution", get(get_speed_distribution))
        .route("/wind_statistics", get(get_wind_statistics))
        .route("/trip_legs", get(get_trip_legs))
        .route("/track_analytics", get(get_track_analytics))
        .route("/monthly_statistics", get(get_monthly_statistics))
        .route("/heatmap", get(get_heatmap))
        .route("/config/google_maps_key", get(get_google_maps_key))
        .route("/tracking/status", get(get_tracking_status))
        .route("/tracking/status", post(set_tracking_status))
        .route("/metrics/status", get(get_metrics_status))
        .route("/metrics/status", post(set_metrics_status))
        .route("/signalk/status", get(get_signalk_status))
        .route("/signalk/status", post(set_signalk_status))
        .route("/udp_broadcast/status", get(get_udp_broadcast_status))
        .route("/udp_broadcast/status", post(set_udp_broadcast_status))
        .route("/backup", get(list_backups))
        .route("/backup", post(post_backup))
        .route("/backup", delete(delete_backup))
        .route("/backup/download", get(download_backup))
        .route("/system/shutdown", post(system_shutdown))
        .layer(
            TraceLayer::new_for_http()
                .on_request(|request: &axum::http::Request<_>, _span: &Span| {
                    info!(method = %request.method(), uri = %request.uri(), "API call");
                })
                .on_response(|response: &axum::http::Response<_>, latency: Duration, _span: &Span| {
                    let bytes = response
                        .headers()
                        .get(axum::http::header::CONTENT_LENGTH)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    info!(
                        status = %response.status(),
                        latency_ms = latency.as_millis(),
                        bytes,
                        "API response"
                    );
                })
        )
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
        let signalk_broadcast = Arc::new(SignalKBroadcastChannels::new());
        let state = AppState {
            db: Arc::new(RwLock::new(db)),
            config: Arc::new(config),
            signalk_broadcast,
            backup_in_progress: Arc::new(AtomicBool::new(false)),
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

    #[tokio::test]
    async fn test_get_trip_by_uuid() {
        let app = create_test_app();

        // Get a list of trips and pick the first one that has a uuid
        let list_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/trips?last_months=12")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let list_body = axum::body::to_bytes(list_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let list_json: serde_json::Value = serde_json::from_slice(&list_body).unwrap();

        if let Some(trips) = list_json["data"].as_array() {
            if let Some(trip) = trips.iter().find(|t| !t["uuid"].is_null()) {
                let uuid = trip["uuid"].as_str().unwrap().to_string();
                let trip_id = trip["id"].as_u64().unwrap();

                let response = app
                    .oneshot(
                        Request::builder()
                            .uri(&format!("/trip_by_uuid?uuid={}", uuid))
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
                assert_eq!(json["data"]["id"], trip_id, "UUID lookup must return the correct trip");
                assert_eq!(json["data"]["uuid"], uuid, "Returned uuid must match queried uuid");
            }
        }
    }

    #[tokio::test]
    async fn test_get_trip_by_uuid_not_found() {
        let app = create_test_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/trip_by_uuid?uuid=00000000-0000-0000-0000-000000000000")
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
        assert!(
            json["error"].as_str().unwrap().contains("not found"),
            "Error message should indicate trip not found"
        );
    }

    // ---- Seeded integration tests (require test_config.json + live test DB) ----
    //
    // Run with: cargo test web::api::tests -- --test-threads=1 --include-ignored

    fn create_clean_test_app() -> (Router, Arc<RwLock<crate::db::types::VesselDatabase>>) {
        use crate::db::test_helpers::setup_db;
        let db = Arc::new(RwLock::new(setup_db()));
        let config = Arc::new(crate::config::Config::load_for_context().unwrap());
        let signalk_broadcast = Arc::new(SignalKBroadcastChannels::new());
        let state = AppState {
            db: db.clone(),
            config,
            signalk_broadcast,
            backup_in_progress: Arc::new(AtomicBool::new(false)),
        };
        (create_api_router(state), db)
    }

    async fn call_api(app: Router, req: axum::http::Request<Body>) -> (StatusCode, serde_json::Value) {
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_trips_seeded() {
        use crate::db::test_helpers::add_test_trip;
        use std::time::{SystemTime, Duration};
        use std::ops::Add;
        let (app, db) = create_clean_test_app();
        let now = SystemTime::now();
        {
            let db = db.read().unwrap();
            add_test_trip(&db, "Trip A".to_string(), now, now.add(Duration::from_secs(3600)), 10.0, 2.0, 3600000, 600000, 0).unwrap();
            add_test_trip(&db, "Trip B".to_string(), now.add(Duration::from_secs(7200)), now.add(Duration::from_secs(10800)), 5.0, 0.0, 1800000, 0, 0).unwrap();
        }
        let (status, json) = call_api(app, axum::http::Request::builder().uri("/trips").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["data"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_trip_by_id_seeded() {
        use crate::db::test_helpers::add_test_trip;
        use std::time::{SystemTime, Duration};
        use std::ops::Add;
        let (app, db) = create_clean_test_app();
        let now = SystemTime::now();
        let trip_id = {
            let db = db.read().unwrap();
            add_test_trip(&db, "Seeded Trip".to_string(), now, now.add(Duration::from_secs(7200)), 12.5, 3.0, 7200000, 1800000, 0).unwrap()
        };
        let (status, json) = call_api(app, axum::http::Request::builder().uri(&format!("/trip?id={}", trip_id)).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["data"]["id"].as_u64().unwrap(), trip_id as u64);
        assert_eq!(json["data"]["description"].as_str().unwrap(), "Seeded Trip");
    }

    #[tokio::test]
    #[ignore]
    async fn test_delete_trip_seeded() {
        use crate::db::test_helpers::add_test_trip;
        use std::time::{SystemTime, Duration};
        use std::ops::Add;
        let (app, db) = create_clean_test_app();
        let now = SystemTime::now();
        let trip_id = {
            let db = db.read().unwrap();
            add_test_trip(&db, "To Delete".to_string(), now, now.add(Duration::from_secs(3600)), 1.0, 0.0, 3600000, 0, 0).unwrap()
        };
        let (status, json) = call_api(app.clone(), axum::http::Request::builder().method("DELETE").uri(&format!("/delete_trip?id={}", trip_id)).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["status"], "ok");
        let (_, json2) = call_api(app, axum::http::Request::builder().uri(&format!("/trip?id={}", trip_id)).body(Body::empty()).unwrap()).await;
        assert_eq!(json2["status"], "error");
    }

    #[tokio::test]
    #[ignore]
    async fn test_update_trip_description_seeded() {
        use crate::db::test_helpers::add_test_trip;
        use std::time::{SystemTime, Duration};
        use std::ops::Add;
        let (app, db) = create_clean_test_app();
        let now = SystemTime::now();
        let trip_id = {
            let db = db.read().unwrap();
            add_test_trip(&db, "Original".to_string(), now, now.add(Duration::from_secs(3600)), 1.0, 0.0, 3600000, 0, 0).unwrap()
        };
        let body = json!({"id": trip_id, "description": "Updated Description"}).to_string();
        let (status, json) = call_api(app.clone(), axum::http::Request::builder().method("POST").uri("/trip_description").header("content-type", "application/json").body(Body::from(body)).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["status"], "ok");
        let (_, json2) = call_api(app, axum::http::Request::builder().uri(&format!("/trip?id={}", trip_id)).body(Body::empty()).unwrap()).await;
        assert_eq!(json2["data"]["description"].as_str().unwrap(), "Updated Description");
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_track_seeded() {
        use crate::db::test_helpers::{add_test_trip, add_test_vessel_status};
        use crate::utilities::EngineStatus;
        use std::time::{SystemTime, Duration};
        use std::ops::Add;
        let (app, db) = create_clean_test_app();
        let now = SystemTime::now();
        let trip_id = {
            let db = db.read().unwrap();
            let tid = add_test_trip(&db, "Track Test".to_string(), now, now.add(Duration::from_secs(3600)), 5.0, 0.0, 3600000, 0, 0).unwrap();
            for i in 0..5u64 {
                add_test_vessel_status(&db, now.add(Duration::from_secs(i * 600)), 51.5 + (i as f64) * 0.01, -0.1, 6.0, 7.0, None, None, false, EngineStatus::Off, 1.0, 600000, Some(90.0), None).unwrap();
            }
            tid
        };
        let (status, json) = call_api(app, axum::http::Request::builder().uri(&format!("/track?trip_id={}", trip_id)).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["data"].as_array().unwrap().len(), 5);
        let lat = json["data"][0]["latitude"].as_f64().unwrap();
        assert!((lat - 51.5).abs() < 0.05, "Expected lat ~51.5, got {}", lat);
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_track_bad_datetime() {
        let (app, _) = create_clean_test_app();
        let resp = app.oneshot(axum::http::Request::builder().uri("/track?start=not-a-date").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_metrics_seeded() {
        use crate::db::test_helpers::{add_test_trip, add_test_env};
        use std::time::{SystemTime, Duration};
        use std::ops::Add;
        let (app, db) = create_clean_test_app();
        let now = SystemTime::now();
        let trip_id = {
            let db = db.read().unwrap();
            let tid = add_test_trip(&db, "Metrics Trip".to_string(), now, now.add(Duration::from_secs(3600)), 1.0, 0.0, 3600000, 0, 0).unwrap();
            for i in 0..3u64 {
                add_test_env(&db, now.add(Duration::from_secs(i * 300)), 2, Some(20.0 + i as f64), Some(22.0), Some(18.0), "C").unwrap();
            }
            tid
        };
        let (status, json) = call_api(app, axum::http::Request::builder().uri(&format!("/metrics?metric=2&trip_id={}", trip_id)).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["status"], "ok");
        assert!(!json["data"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_metrics_batch_invalid_param() {
        let (app, _) = create_clean_test_app();
        let (status, json) = call_api(app, axum::http::Request::builder().uri("/metrics/batch?metrics=not-ids").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["status"], "error");
        assert!(json["error"].as_str().unwrap().to_lowercase().contains("invalid"));
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_signalk_status_defaults_true() {
        // All system_status keys (including signalk_enabled) default to true when absent from cache/DB
        let (app, _) = create_clean_test_app();
        let (status, json) = call_api(app, axum::http::Request::builder().uri("/signalk/status").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["enabled"], true);
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_tracking_status_defaults_true() {
        let (app, _) = create_clean_test_app();
        let (status, json) = call_api(app, axum::http::Request::builder().uri("/tracking/status").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["enabled"], true);
    }

    #[tokio::test]
    #[ignore]
    async fn test_set_and_get_tracking_status_seeded() {
        let (app, _) = create_clean_test_app();
        let body = json!({"enabled": false}).to_string();
        let (status, json) = call_api(app.clone(), axum::http::Request::builder().method("POST").uri("/tracking/status").header("content-type", "application/json").body(Body::from(body)).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["enabled"], false);
        let (_, json2) = call_api(app, axum::http::Request::builder().uri("/tracking/status").body(Body::empty()).unwrap()).await;
        assert_eq!(json2["data"]["enabled"], false);
    }

    #[tokio::test]
    #[ignore]
    async fn test_list_exports_returns_array() {
        let (app, _) = create_clean_test_app();
        let (status, json) = call_api(app, axum::http::Request::builder().uri("/list_exports").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["status"], "ok");
        assert!(json["data"].is_array());
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_monthly_statistics_empty_db() {
        let (app, _) = create_clean_test_app();
        let (status, json) = call_api(app, axum::http::Request::builder().uri("/monthly_statistics").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["status"], "ok");
        assert!(json["data"]["months"].is_array());
    }

    #[tokio::test]
    #[ignore]
    async fn test_delete_backup_path_traversal() {
        let (app, _) = create_clean_test_app();
        let (status, json) = call_api(app, axum::http::Request::builder().method("DELETE").uri("/backup?file=../../../etc/passwd").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["status"], "error");
        assert_eq!(json["error"].as_str().unwrap(), "Invalid filename");
    }

    #[tokio::test]
    #[ignore]
    async fn test_download_backup_not_found() {
        let (app, _) = create_clean_test_app();
        let resp = app.oneshot(axum::http::Request::builder().uri("/backup/download?file=nonexistent_test_file.gz").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    #[ignore]
    async fn test_download_backup_path_traversal() {
        let (app, _) = create_clean_test_app();
        let resp = app.oneshot(axum::http::Request::builder().uri("/backup/download?file=../config.json").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
