use axum::{
    extract::{State, Path},
    response::IntoResponse,
    Json, routing::get, Router,
};
use std::sync::Arc;
use crate::handlers::AppState;
use crate::project::types::Project;
use crate::git::GitRepoAttachment;
use serde::Serialize;

#[derive(Serialize)]
pub struct ProjectDetailsResponse {
    pub project: Project,
    pub attached_repos: Vec<GitRepoAttachment>,
}

pub async fn project_details_handler(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<String>,
) -> impl IntoResponse {
    let project_store = &state.project_store;
    let git_store = &state.git_store;

    let project = match project_store.get_project(&project_id).await {
        Ok(Some(p)) => p,
        _ => {
            return (axum::http::StatusCode::NOT_FOUND, "Project not found").into_response();
        }
    };

    let attached_repos = match git_store.get_attachments_for_project(&project_id).await {
        Ok(repos) => repos,
        Err(_) => Vec::new(),
    };

    Json(ProjectDetailsResponse { project, attached_repos }).into_response()
}

// Only use this router if you want a *separate* subrouter for project details.
// Otherwise, register `project_details_handler` directly in http_router() as you are now.
pub fn project_router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/projects/:project_id/details",
            get(project_details_handler),
        )
}
