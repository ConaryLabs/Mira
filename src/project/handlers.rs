// src/project/handlers.rs
// Updated to use centralized error handling from src/api/error.rs
// Eliminates 8+ instances of duplicated error handling patterns

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
// FIXED: Removed unused imports Project and Artifact
use crate::api::error::{ApiError, ApiResult, IntoApiError, IntoApiErrorOption};

// Project handlers

pub async fn create_project_handler(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<CreateProjectRequest>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        let project = app_state.project_store.create_project(
            payload.name,
            payload.description,
            payload.tags,
            None, // owner will be added when we have auth
        ).await
        .into_api_error("Failed to create project")?;
        
        Ok((StatusCode::CREATED, Json(project)))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn get_project_handler(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        let project = app_state.project_store
            .get_project(&id)
            .await
            .into_api_error("Failed to get project")?
            .ok_or_not_found("Project not found")?;
        
        Ok(Json(project))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn list_projects_handler(
    State(app_state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        let projects = app_state.project_store
            .list_projects()
            .await
            .into_api_error("Failed to list projects")?;
        
        let response = ProjectsResponse {
            total: projects.len(),
            projects,
        };
        
        Ok(Json(response))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn update_project_handler(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateProjectRequest>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        let project = app_state.project_store
            .update_project(
                &id,
                payload.name,
                payload.description,
                payload.tags,
            )
            .await
            .into_api_error("Failed to update project")?
            .ok_or_not_found("Project not found")?;
        
        Ok(Json(project))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn delete_project_handler(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        let deleted = app_state.project_store
            .delete_project(&id)
            .await
            .into_api_error("Failed to delete project")?;
        
        if deleted {
            Ok(StatusCode::NO_CONTENT)
        } else {
            Err(ApiError::not_found("Project not found"))
        }
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}

// Artifact handlers

pub async fn create_artifact_handler(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<CreateArtifactRequest>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        let artifact = app_state.project_store
            .create_artifact(
                payload.project_id,
                payload.name,
                payload.artifact_type,
                payload.content,
            )
            .await
            .into_api_error("Failed to create artifact")?;
        
        Ok((StatusCode::CREATED, Json(artifact)))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn get_artifact_handler(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        let artifact = app_state.project_store
            .get_artifact(&id)
            .await
            .into_api_error("Failed to get artifact")?
            .ok_or_not_found("Artifact not found")?;
        
        Ok(Json(artifact))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn list_project_artifacts_handler(
    State(app_state): State<Arc<AppState>>,
    Path(project_id): Path<String>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        let artifacts = app_state.project_store
            .list_project_artifacts(&project_id)
            .await
            .into_api_error("Failed to list artifacts")?;
        
        let response = ArtifactsResponse {
            total: artifacts.len(),
            artifacts,
        };
        
        Ok(Json(response))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn update_artifact_handler(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateArtifactRequest>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        let artifact = app_state.project_store
            .update_artifact(
                &id,
                payload.name,
                payload.content,
            )
            .await
            .into_api_error("Failed to update artifact")?
            .ok_or_not_found("Artifact not found")?;
        
        Ok(Json(artifact))
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn delete_artifact_handler(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        let deleted = app_state.project_store
            .delete_artifact(&id)
            .await
            .into_api_error("Failed to delete artifact")?;
        
        if deleted {
            Ok(StatusCode::NO_CONTENT)
        } else {
            Err(ApiError::not_found("Artifact not found"))
        }
    }.await;
    
    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}
