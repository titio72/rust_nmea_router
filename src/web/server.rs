use axum::{
    Router,
    routing::{get, get_service, post},
    extract::DefaultBodyLimit,
    middleware,
    http::{Method, header, HeaderValue},
};
use tower::ServiceBuilder;
use tower_http::set_header::SetResponseHeaderLayer;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use tower_http::services::ServeDir;
use tower_http::cors::{CorsLayer, Any};
use tower_http::catch_panic::CatchPanicLayer;
use tracing::{info, error};

use crate::forecast_poller::{ForecastPollerStatus, run_poller};
use std::sync::atomic::AtomicBool;
use crate::ais_target_cache::AisTargetCache;
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
    ais_cache: Arc<std::sync::Mutex<AisTargetCache>>,
    port: u16,
    startup_signal: std::sync::mpsc::Sender<Result<(), String>>,
) -> Result<(), crate::error::AppError> {
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

    let poller_status = Arc::new(std::sync::Mutex::new(ForecastPollerStatus::default()));

    let polars = config.polars_file_path.as_deref().and_then(|path| {
        match crate::polars::PolarTable::from_csv(path) {
            Ok(t) => {
                tracing::info!(path, "Polar table loaded");
                Some(std::sync::Arc::new(t))
            }
            Err(e) => {
                tracing::warn!(path, error = %e, "Failed to load polar table — fixed motoring speed will be used");
                None
            }
        }
    });

    let state = AppState {
        db: db.clone(),
        config,
        signalk_broadcast,
        backup_in_progress: Arc::new(AtomicBool::new(false)),
        jwt_secret,
        ais_cache,
        poller_status: poller_status.clone(),
        polars,
    };

    tokio::spawn(async {
        cleanup_old_exports().await;
    });

    let poller_db = db.clone();
    let poller_status_arc = poller_status.clone();
    tokio::spawn(async move {
        run_poller(poller_db, poller_status_arc).await;
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
        .nest_service("/", get_service(
            ServiceBuilder::new()
                .layer(SetResponseHeaderLayer::overriding(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("no-cache"),
                ))
                .service(ServeDir::new("static"))
        ).handle_error(|error| async move {
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
        )
        .layer(CatchPanicLayer::custom(|err: Box<dyn std::any::Any + Send>| {
            let msg = err.downcast_ref::<&str>().copied()
                .or_else(|| err.downcast_ref::<String>().map(|s| s.as_str()))
                .unwrap_or("unknown panic");
            error!(panic = msg, "Handler panicked");
            axum::response::Response::builder()
                .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    format!(r#"{{"status":"error","error":"Internal server error: {}"}}"#, msg)
                ))
                .unwrap()
        }));

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Web server binding to http://{}", addr);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => {
            let _ = startup_signal.send(Ok(()));
            l
        }
        Err(e) => {
            let _ = startup_signal.send(Err(format!("Failed to bind to port {}: {}", port, e)));
            return Err(crate::error::AppError::Io(format!("Failed to bind to port {}: {}", port, e)));
        }
    };

    axum::serve(listener, app)
        .await
        .map_err(|e| crate::error::AppError::Io(format!("Server error: {}", e)))
}
