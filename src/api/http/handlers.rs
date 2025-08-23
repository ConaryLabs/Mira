// src/api/http/handlers.rs
// Phase 1: Extract HTTP Handlers from mod.rs
// Updated to use centralized error handling and configuration

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;

use crate::state::AppState;
use crate::api::error::{ApiError, ApiResult, IntoApiError};
use crate::config::CONFIG;

/// Health check handler
pub async fn health_handler() -> impl IntoResponse {
    Json(json!({
        "status": "healthy", 
        "version": env!("CARGO_PKG_VERSION"),
        "model": CONFIG.model,
        "timestamp": Utc::now().to_rfc3339()
    }))
}

/// Project details handler
/// CRITICAL: Maintains compatibility with main.rs route expectations
pub async fn project_details_handler(
    State(app_state): State<Arc<AppState>>,
    Path(project_id): Path<String>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        // Get project details from store
        let project = app_state
            .project_store
            .get_project(&project_id)
            .await
            .into_api_error("Failed to retrieve project")?
            .ok_or_else(|| ApiError::not_found("Project not found"))?;

        Ok(Json(project))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}
