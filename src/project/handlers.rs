use crate::project::types::{Project, Artifact};
// src/project/handlers.rs

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::sync::Arc;
use crate::state::AppState;
use crate::project::types::{
    CreateProjectRequest, UpdateProjectRequest, 
    CreateArtifactRequest, UpdateArtifactRequest,
    ProjectsResponse, ArtifactsResponse
};

// Project handlers

pub async fn create_project_handler(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<CreateProjectRequest>,
) -> impl IntoResponse {
    match app_state.project_store.create_project(
        payload.name,
        payload.description,
        payload.tags,
        None, // owner will be added when we have auth
    ).await {
        Ok(project) => (StatusCode::CREATED, Json(project)).into_response(),
        Err(e) => {
            eprintln!("Failed to create project: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create project").into_response()
        }
    }
}

pub async fn get_project_handler(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match app_state.project_store.get_project(&id).await {
        Ok(Some(project)) => Json::<Project>(project).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Project not found").into_response(),
        Err(e) => {
            eprintln!("Failed to get project: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get project").into_response()
        }
    }
}

pub async fn list_projects_handler(
    State(app_state): State<Arc<AppState>>,
) -> impl IntoResponse {
    match app_state.project_store.list_projects().await {
        Ok(projects) => {
            let response = ProjectsResponse {
                total: projects.len(),
                projects,
            };
            Json(response).into_response()
        }
        Err(e) => {
            eprintln!("Failed to list projects: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to list projects").into_response()
        }
    }
}

pub async fn update_project_handler(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateProjectRequest>,
) -> impl IntoResponse {
    match app_state.project_store.update_project(
        &id,
        payload.name,
        payload.description,
        payload.tags,
    ).await {
        Ok(Some(project)) => Json::<Project>(project).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Project not found").into_response(),
        Err(e) => {
            eprintln!("Failed to update project: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to update project").into_response()
        }
    }
}

pub async fn delete_project_handler(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match app_state.project_store.delete_project(&id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "Project not found").into_response(),
        Err(e) => {
            eprintln!("Failed to delete project: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to delete project").into_response()
        }
    }
}

// Artifact handlers

pub async fn create_artifact_handler(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<CreateArtifactRequest>,
) -> impl IntoResponse {
    match app_state.project_store.create_artifact(
        payload.project_id,
        payload.name,
        payload.artifact_type,
        payload.content,
    ).await {
        Ok(artifact) => (StatusCode::CREATED, Json(artifact)).into_response(),
        Err(e) => {
            eprintln!("Failed to create artifact: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create artifact").into_response()
        }
    }
}

pub async fn get_artifact_handler(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match app_state.project_store.get_artifact(&id).await {
        Ok(Some(artifact)) => Json::<Artifact>(artifact).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Artifact not found").into_response(),
        Err(e) => {
            eprintln!("Failed to get artifact: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get artifact").into_response()
        }
    }
}

pub async fn list_project_artifacts_handler(
    State(app_state): State<Arc<AppState>>,
    Path(project_id): Path<String>,
) -> impl IntoResponse {
    match app_state.project_store.list_project_artifacts(&project_id).await {
        Ok(artifacts) => {
            let response = ArtifactsResponse {
                total: artifacts.len(),
                artifacts,
            };
            Json(response).into_response()
        }
        Err(e) => {
            eprintln!("Failed to list artifacts: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to list artifacts").into_response()
        }
    }
}

pub async fn update_artifact_handler(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateArtifactRequest>,
) -> impl IntoResponse {
    match app_state.project_store.update_artifact(
        &id,
        payload.name,
        payload.content,
    ).await {
        Ok(Some(artifact)) => Json::<Artifact>(artifact).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Artifact not found").into_response(),
        Err(e) => {
            eprintln!("Failed to update artifact: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to update artifact").into_response()
        }
    }
}

pub async fn delete_artifact_handler(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match app_state.project_store.delete_artifact(&id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "Artifact not found").into_response(),
        Err(e) => {
            eprintln!("Failed to delete artifact: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to delete artifact").into_response()
        }
    }
}
