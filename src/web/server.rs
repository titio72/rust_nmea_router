use axum::{
    Router,
    routing::{get, get_service, post},
    extract::DefaultBodyLimit,
    middleware,
    http::{Method, header},
};
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use tower_http::services::ServeDir;
use tower_http::cors::{CorsLayer, Any};
use tracing::info;

use std::sync::atomic::AtomicBool;
use crate::db::VesselDatabase;
use crate::config::Config;
use crate::utilities::cleanup_old_exports;
use super::api::{AppState, create_api_router};
use super::auth::{JwtSecret, auth_middleware, login_handler, logout_handler};
use super::broadcast_manager::get_signalk_channels;

const MIN_PASSWORD_LEN: usize = 12;

pub async fn start_web_server(
    db: Arc<RwLock<VesselDatabase>>,
    config: Arc<Config>,
    port: u16,
    startup_signal: std::sync::mpsc::Sender<Result<(), String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    match config.web.auth_password.as_deref() {
        None | Some("") => {
            tracing::warn!("Web UI has no auth_password set — all routes are unprotected!");
        }
        Some(pw) if pw.len() < MIN_PASSWORD_LEN => {
            tracing::warn!(
                "Web UI auth_password is only {} characters — use at least {} for internet exposure",
                pw.len(), MIN_PASSWORD_LEN
            );
        }
        _ => {}
    }

    let signalk_broadcast = get_signalk_channels();

    let jwt_secret = Arc::new(JwtSecret::generate());

    let state = AppState {
        db,
        config,
        signalk_broadcast,
        backup_in_progress: Arc::new(AtomicBool::new(false)),
        jwt_secret,
    };

    tokio::spawn(async {
        cleanup_old_exports().await;
    });

    let api_router = create_api_router(state.clone());

    let auth_routes = Router::new()
        .route("/api/auth/login", post(login_handler))
        .route("/api/auth/logout", post(logout_handler))
        .with_state(state.clone());

    let app = Router::new()
        .merge(auth_routes)
        .nest("/api", api_router)
        .route("/signalk/v1/stream", get(super::signalk::signalk_stream).with_state(state.clone()))
        .nest_service("/", get_service(ServeDir::new("static")).handle_error(|error| async move {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Unhandled internal error: {}", error),
            )
        }))
        .layer(middleware::from_fn_with_state(state.clone(), auth_middleware))
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST, Method::DELETE])
                .allow_headers([header::CONTENT_TYPE, header::COOKIE]),
        );

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Web server binding to http://{}", addr);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => {
            let _ = startup_signal.send(Ok(()));
            l
        }
        Err(e) => {
            let _ = startup_signal.send(Err(format!("Failed to bind to port {}: {}", port, e)));
            return Err(e.into());
        }
    };

    axum::serve(listener, app)
        .await
        .map_err(|e| format!("Server error: {}", e).into())
}
