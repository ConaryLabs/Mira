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
        // Project routes (using plural for consistency)
        .route("/projects", post(handlers::create_project_handler))
        .route("/projects", get(handlers::list_projects_handler))
        .route("/projects/:id", get(handlers::get_project_handler))
        .route("/projects/:id", put(handlers::update_project_handler))
        .route("/projects/:id", delete(handlers::delete_project_handler))
        // Artifact routes (using plural for collection)
        .route("/artifacts", post(handlers::create_artifact_handler))
        .route("/artifacts/:id", get(handlers::get_artifact_handler))
        .route("/artifacts/:id", put(handlers::update_artifact_handler))
        .route("/artifacts/:id", delete(handlers::delete_artifact_handler))
        .route("/projects/:id/artifacts", get(handlers::list_project_artifacts_handler))
}

// Re-export for easy use elsewhere
