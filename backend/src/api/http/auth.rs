// backend/src/api/http/auth.rs

use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use std::sync::Arc;
use tracing::error;

use crate::auth::{LoginRequest, RegisterRequest, AuthResponse, verify_token};
use crate::state::AppState;

pub fn create_auth_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/login", post(login))
        .route("/register", post(register))
        .route("/verify", post(verify))
}

async fn login(
    State(app_state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, AuthError> {
    let response = app_state.auth_service
        .login(req)
        .await
        .map_err(|e| AuthError::InvalidCredentials(e.to_string()))?;

    Ok(Json(response))
}

async fn register(
    State(app_state): State<Arc<AppState>>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<AuthResponse>, AuthError> {
    let response = app_state.auth_service
        .register(req)
        .await
        .map_err(|e| AuthError::RegistrationFailed(e.to_string()))?;

    Ok(Json(response))
}

#[derive(serde::Deserialize)]
struct VerifyRequest {
    token: String,
}

#[derive(serde::Serialize)]
struct VerifyResponse {
    valid: bool,
    user_id: Option<String>,
    username: Option<String>,
}

async fn verify(
    State(app_state): State<Arc<AppState>>,
    Json(req): Json<VerifyRequest>,
) -> Result<Json<VerifyResponse>, AuthError> {
    match verify_token(&req.token) {
        Ok(claims) => {
            match app_state.auth_service.verify_user_id(&claims.sub).await {
                Ok(_) => Ok(Json(VerifyResponse {
                    valid: true,
                    user_id: Some(claims.sub),
                    username: Some(claims.username),
                })),
                Err(_) => Ok(Json(VerifyResponse {
                    valid: false,
                    user_id: None,
                    username: None,
                })),
            }
        }
        Err(_) => Ok(Json(VerifyResponse {
            valid: false,
            user_id: None,
            username: None,
        })),
    }
}

#[derive(Debug)]
enum AuthError {
    InvalidCredentials(String),
    RegistrationFailed(String),
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AuthError::InvalidCredentials(msg) => (StatusCode::UNAUTHORIZED, msg),
            AuthError::RegistrationFailed(msg) => (StatusCode::BAD_REQUEST, msg),
        };

        error!("Auth error: {}", message);

        (status, Json(serde_json::json!({
            "error": message
        }))).into_response()
    }
}
