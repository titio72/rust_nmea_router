use axum::{
    Router,
    routing::{get, get_service},
    extract::DefaultBodyLimit,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::services::ServeDir;
use tower_http::cors::{CorsLayer, Any};
use tracing::info;

use crate::db::VesselDatabase;
use crate::config::Config;
use crate::utilities::cleanup_old_exports;
use super::api::{AppState, create_api_router};
use super::broadcast_manager::get_signalk_channels;

pub async fn start_web_server(
    db: Arc<VesselDatabase>,
    config: Arc<Config>,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    // Get the global SignalK broadcast channels
    let signalk_broadcast = get_signalk_channels();
    
    let state = AppState { db, config, signalk_broadcast };

    // Spawn cleanup task to remove old exports every 24 hours
    tokio::spawn(async {
        cleanup_old_exports().await;
    });

    // Create API router
    let api_router = create_api_router(state.clone());

    // Create main app router with static file serving
    let app = Router::new()
        .nest("/api", api_router)
        .route("/signalk/v1/stream", get(super::signalk::signalk_stream).with_state(state.clone()))
        .nest_service("/", get_service(ServeDir::new("static")).handle_error(|error| async move {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Unhandled internal error: {}", error),
            )
        }))
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024)) // 10MB limit for file uploads
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Web server starting on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("Server error: {}", e).into())
}
