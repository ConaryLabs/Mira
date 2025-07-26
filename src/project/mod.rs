// src/project/mod.rs

pub mod types;
pub mod store;
pub mod handlers;

use axum::{
    routing::{get, post, put, delete},
    Router,
};
use std::sync::Arc;
use crate::handlers::AppState;

pub fn project_router() -> Router<Arc<AppState>> {
    Router::new()
        // Project routes
        .route("/projects", post(handlers::create_project_handler))
        .route("/projects", get(handlers::list_projects_handler))
        .route("/project/:id", get(handlers::get_project_handler))
        .route("/project/:id", put(handlers::update_project_handler))
        .route("/project/:id", delete(handlers::delete_project_handler))
        // Artifact routes
        .route("/artifact", post(handlers::create_artifact_handler))
        .route("/artifact/:id", get(handlers::get_artifact_handler))
        .route("/artifact/:id", put(handlers::update_artifact_handler))
        .route("/artifact/:id", delete(handlers::delete_artifact_handler))
        .route("/project/:id/artifacts", get(handlers::list_project_artifacts_handler))
}
