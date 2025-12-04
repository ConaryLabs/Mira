// backend/src/api/http/auth.rs

use axum::{
    extract::{Json, State},
    http::{StatusCode, HeaderMap},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use std::sync::Arc;
use tracing::error;

use crate::auth::{LoginRequest, RegisterRequest, AuthResponse, ChangePasswordRequest, UpdatePreferencesRequest, User, verify_token};
use crate::state::AppState;

pub fn create_auth_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/login", post(login))
        .route("/register", post(register))
        .route("/verify", post(verify))
        .route("/change-password", post(change_password))
        .route("/preferences", post(update_preferences))
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

async fn change_password(
    State(app_state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<Json<serde_json::Value>, AuthError> {
    // Extract token from Authorization header
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AuthError::Unauthorized("Missing authorization header".to_string()))?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| AuthError::Unauthorized("Invalid authorization format".to_string()))?;

    // Verify token and get user ID
    let claims = verify_token(token)
        .map_err(|_| AuthError::Unauthorized("Invalid token".to_string()))?;

    // Change password
    app_state.auth_service
        .change_password(&claims.sub, req)
        .await
        .map_err(|e| AuthError::PasswordChangeFailed(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Password changed successfully"
    })))
}

async fn update_preferences(
    State(app_state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<UpdatePreferencesRequest>,
) -> Result<Json<User>, AuthError> {
    // Extract token from Authorization header
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AuthError::Unauthorized("Missing authorization header".to_string()))?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| AuthError::Unauthorized("Invalid authorization format".to_string()))?;

    // Verify token and get user ID
    let claims = verify_token(token)
        .map_err(|_| AuthError::Unauthorized("Invalid token".to_string()))?;

    // Update preferences
    let user = app_state.auth_service
        .update_preferences(&claims.sub, req)
        .await
        .map_err(|e| AuthError::PreferencesUpdateFailed(e.to_string()))?;

    Ok(Json(user))
}

#[derive(Debug)]
enum AuthError {
    InvalidCredentials(String),
    RegistrationFailed(String),
    Unauthorized(String),
    PasswordChangeFailed(String),
    PreferencesUpdateFailed(String),
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AuthError::InvalidCredentials(msg) => (StatusCode::UNAUTHORIZED, msg),
            AuthError::RegistrationFailed(msg) => (StatusCode::BAD_REQUEST, msg),
            AuthError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg),
            AuthError::PasswordChangeFailed(msg) => (StatusCode::BAD_REQUEST, msg),
            AuthError::PreferencesUpdateFailed(msg) => (StatusCode::BAD_REQUEST, msg),
        };

        error!("Auth error: {}", message);

        (status, Json(serde_json::json!({
            "error": message
        }))).into_response()
    }
}
