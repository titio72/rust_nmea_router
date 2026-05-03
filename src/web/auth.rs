use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
    Json,
};
use chrono::Utc;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use super::api::AppState;

pub struct JwtSecret([u8; 32]);

impl JwtSecret {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        JwtSecret(bytes)
    }

    fn encoding_key(&self) -> EncodingKey {
        EncodingKey::from_secret(&self.0)
    }

    fn decoding_key(&self) -> DecodingKey {
        DecodingKey::from_secret(&self.0)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    exp: usize,
}

pub fn create_token(
    secret: &JwtSecret,
    duration_secs: u64,
) -> Result<String, jsonwebtoken::errors::Error> {
    let exp = (Utc::now() + chrono::Duration::seconds(duration_secs as i64)).timestamp() as usize;
    encode(&Header::default(), &Claims { exp }, &secret.encoding_key())
}

pub fn validate_token(secret: &JwtSecret, token: &str) -> bool {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    decode::<Claims>(token, &secret.decoding_key(), &validation).is_ok()
}

pub fn build_session_cookie(token: &str, duration_secs: u64, secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    format!(
        "session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={duration_secs}{secure_flag}"
    )
}

pub fn build_clear_cookie(secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    format!("session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0{secure_flag}")
}

fn extract_session_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    let cookie_header = headers.get("cookie")?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(val) = part.strip_prefix("session=") {
            return Some(val.to_string());
        }
    }
    None
}

const PUBLIC_PATHS: &[&str] = &[
    "/login.html",
    "/api/auth/login",
    "/api/auth/logout",
    "/api/sync/manifest",
    "/api/sync/trip",
];

const PUBLIC_EXTENSIONS: &[&str] = &[
    ".js", ".css", ".png", ".svg", ".ico", ".jpg", ".jpeg", ".woff", ".woff2", ".ttf",
];

fn is_public_path(path: &str) -> bool {
    PUBLIC_PATHS.contains(&path) || PUBLIC_EXTENSIONS.iter().any(|ext| path.ends_with(ext))
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if state.config.web.auth_password.as_deref().unwrap_or("").is_empty() {
        return next.run(request).await;
    }

    let path = request.uri().path().to_string();

    if is_public_path(&path) {
        return next.run(request).await;
    }

    let token = extract_session_cookie(request.headers());
    let authenticated = token
        .as_deref()
        .map(|t| validate_token(&state.jwt_secret, t))
        .unwrap_or(false);

    if authenticated {
        return next.run(request).await;
    }

    if path.starts_with("/api/") || path.starts_with("/signalk/") {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"status": "error", "error": "Unauthorized"})),
        )
            .into_response()
    } else {
        let encoded_path = urlencoding_encode(&path);
        Redirect::to(&format!("/login.html?next={encoded_path}")).into_response()
    }
}

fn urlencoding_encode(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/') {
                vec![c]
            } else {
                format!("%{:02X}", c as u32).chars().collect()
            }
        })
        .collect()
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub password: String,
}

pub async fn login_handler(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Response {
    let expected = state.config.web.auth_password.as_deref().unwrap_or("");
    if !expected.is_empty() && body.password != expected {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"status": "error", "error": "Invalid password"})),
        )
            .into_response();
    }

    match create_token(&state.jwt_secret, state.config.web.session_duration_secs) {
        Ok(token) => {
            let cookie = build_session_cookie(
                &token,
                state.config.web.session_duration_secs,
                state.config.web.secure_cookies,
            );
            (
                StatusCode::OK,
                [(axum::http::header::SET_COOKIE, cookie)],
                Json(serde_json::json!({"status": "ok"})),
            )
                .into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn logout_handler(State(state): State<AppState>) -> Response {
    let cookie = build_clear_cookie(state.config.web.secure_cookies);
    (
        StatusCode::OK,
        [(axum::http::header::SET_COOKIE, cookie)],
        Json(serde_json::json!({"status": "ok"})),
    )
        .into_response()
}
