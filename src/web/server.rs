use axum::{
    Router,
    routing::get_service,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::services::ServeDir;
use tower_http::cors::{CorsLayer, Any};

use crate::db::VesselDatabase;
use super::api::{AppState, create_api_router};

pub async fn start_web_server(
    db: Arc<VesselDatabase>,
    port: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let state = AppState { db };

    // Create API router
    let api_router = create_api_router(state);

    // Create main app router with static file serving
    let app = Router::new()
        .nest("/api", api_router)
        .nest_service("/", get_service(ServeDir::new("static")))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Web server starting on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("Server error: {}", e).into())
}
