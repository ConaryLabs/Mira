// src/api/ws/project.rs
// WebSocket handler implementation for project and artifact operations

use std::sync::Arc;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{debug, error, info, warn};

use crate::{
    api::{
        error::{ApiError, ApiResult},
        ws::message::WsServerMessage,
    },
    project::{Project, Artifact, ArtifactType},
    state::AppState,
};

// Request types for project operations

#[derive(Debug, Deserialize)]
struct CreateProjectRequest {
    name: String,
    description: Option<String>,
    tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct GetProjectRequest {
    id: String,
}

#[derive(Debug, Deserialize)]
struct UpdateProjectRequest {
    id: String,
    name: Option<String>,
    description: Option<String>,
    tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct DeleteProjectRequest {
    id: String,
}

#[derive(Debug, Deserialize)]
struct CreateArtifactRequest {
    project_id: String,
    name: String,
    artifact_type: String,
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GetArtifactRequest {
    id: String,
}

#[derive(Debug, Deserialize)]
struct UpdateArtifactRequest {
    id: String,
    name: Option<String>,
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeleteArtifactRequest {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ListArtifactsRequest {
    project_id: String,
}

/// Main entry point for all project-related WebSocket commands
pub async fn handle_project_command(
    method: &str,
    params: Value,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    debug!("Processing project command: {} with params: {:?}", method, params);
    
    let result = match method {
        // Project operations
        "project.create" => create_project(params, app_state).await,
        "project.list" => list_projects(app_state).await,
        "project.get" => get_project(params, app_state).await,
        "project.update" => update_project(params, app_state).await,
        "project.delete" => delete_project(params, app_state).await,
        
        // Artifact operations
        "artifact.create" => create_artifact(params, app_state).await,
        "artifact.get" => get_artifact(params, app_state).await,
        "artifact.update" => update_artifact(params, app_state).await,
        "artifact.delete" => delete_artifact(params, app_state).await,
        "artifact.list" => list_artifacts(params, app_state).await,
        
        _ => {
            error!("Unknown project method: {}", method);
            return Err(ApiError::bad_request(format!("Unknown project method: {}", method)));
        }
    };
    
    match &result {
        Ok(_) => info!("Successfully processed project command: {}", method),
        Err(e) => error!("Failed to process project command {}: {:?}", method, e),
    }
    
    result
}

// Project Operations

async fn create_project(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: CreateProjectRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid create project request: {}", e)))?;
    
    info!("Creating project: {}", request.name);
    
    if request.name.trim().is_empty() {
        return Err(ApiError::bad_request("Project name cannot be empty"));
    }
    
    let project = app_state.project_store
        .create_project(
            request.name,
            request.description,
            request.tags,
            Some("peter".to_string()), // Single-user mode
        )
        .await
        .map_err(|e| ApiError::internal(format!("Failed to create project: {}", e)))?;
    
    info!("Project created successfully: {}", project.id);
    
    Ok(WsServerMessage::Data {
        data: json!({
            "type": "project_created",
            "project": project_to_json(&project)
        }),
        request_id: None,
    })
}

async fn list_projects(app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    debug!("Listing all projects");
    
    let projects = app_state.project_store
        .list_projects()
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list projects: {}", e)))?;
    
    let mut project_list = Vec::new();
    for project in projects {
        let artifacts = app_state.project_store
            .list_project_artifacts(&project.id)
            .await
            .unwrap_or_else(|_| Vec::new());
        
        let mut project_json = project_to_json(&project);
        project_json["artifact_count"] = json!(artifacts.len());
        project_list.push(project_json);
    }
    
    info!("Found {} projects", project_list.len());
    
    Ok(WsServerMessage::Data {
        data: json!({
            "type": "project_list",
            "projects": project_list
        }),
        request_id: None,
    })
}

async fn get_project(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: GetProjectRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid get project request: {}", e)))?;
    
    debug!("Getting project: {}", request.id);
    
    let project = app_state.project_store
        .get_project(&request.id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to get project: {}", e)))?;
    
    match project {
        Some(p) => {
            let artifacts = app_state.project_store
                .list_project_artifacts(&p.id)
                .await
                .unwrap_or_else(|_| Vec::new());
            
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "project_details",
                    "project": project_to_json(&p),
                    "artifacts": artifacts.iter().map(artifact_to_json).collect::<Vec<_>>()
                }),
                request_id: None,
            })
        }
        None => Err(ApiError::not_found(format!("Project not found: {}", request.id)))
    }
}

async fn update_project(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: UpdateProjectRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid update project request: {}", e)))?;
    
    info!("Updating project: {}", request.id);
    
    if request.name.is_none() && request.description.is_none() && request.tags.is_none() {
        return Err(ApiError::bad_request("No fields to update"));
    }
    
    if let Some(ref name) = request.name {
        if name.trim().is_empty() {
            return Err(ApiError::bad_request("Project name cannot be empty"));
        }
    }
    
    let project = app_state.project_store
        .update_project(
            &request.id,
            request.name,
            request.description,
            request.tags,
        )
        .await
        .map_err(|e| ApiError::internal(format!("Failed to update project: {}", e)))?;
    
    match project {
        Some(p) => {
            info!("Project updated successfully: {}", p.id);
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "project_updated",
                    "project": project_to_json(&p)
                }),
                request_id: None,
            })
        }
        None => Err(ApiError::not_found(format!("Project not found: {}", request.id)))
    }
}

async fn delete_project(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: DeleteProjectRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid delete project request: {}", e)))?;
    
    info!("Deleting project: {}", request.id);
    
    let artifacts = app_state.project_store
        .list_project_artifacts(&request.id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to check artifacts: {}", e)))?;
    
    if !artifacts.is_empty() {
        warn!("Deleting project {} with {} artifacts", request.id, artifacts.len());
    }
    
    let deleted = app_state.project_store
        .delete_project(&request.id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to delete project: {}", e)))?;
    
    if deleted {
        info!("Project deleted successfully: {}", request.id);
        Ok(WsServerMessage::Data {
            data: json!({
                "type": "project_deleted",
                "id": request.id
            }),
            request_id: None,
        })
    } else {
        Err(ApiError::not_found(format!("Project not found: {}", request.id)))
    }
}

// Artifact Operations

async fn create_artifact(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: CreateArtifactRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid create artifact request: {}", e)))?;
    
    info!("Creating artifact: {} for project: {}", request.name, request.project_id);
    
    if request.name.trim().is_empty() {
        return Err(ApiError::bad_request("Artifact name cannot be empty"));
    }
    
    let project = app_state.project_store
        .get_project(&request.project_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to verify project: {}", e)))?;
    
    if project.is_none() {
        return Err(ApiError::not_found(format!("Project not found: {}", request.project_id)));
    }
    
    let artifact_type = match request.artifact_type.to_lowercase().as_str() {
        "code" => ArtifactType::Code,
        "image" => ArtifactType::Image,
        "log" => ArtifactType::Log,
        "note" => ArtifactType::Note,
        "markdown" => ArtifactType::Markdown,
        _ => {
            warn!("Unknown artifact type: {}, defaulting to Note", request.artifact_type);
            ArtifactType::Note
        }
    };
    
    let artifact = app_state.project_store
        .create_artifact(
            request.project_id,
            request.name,
            artifact_type,
            request.content,
        )
        .await
        .map_err(|e| ApiError::internal(format!("Failed to create artifact: {}", e)))?;
    
    info!("Artifact created successfully: {}", artifact.id);
    
    Ok(WsServerMessage::Data {
        data: json!({
            "type": "artifact_created",
            "artifact": artifact_to_json(&artifact)
        }),
        request_id: None,
    })
}

async fn get_artifact(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: GetArtifactRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid get artifact request: {}", e)))?;
    
    debug!("Getting artifact: {}", request.id);
    
    let artifact = app_state.project_store
        .get_artifact(&request.id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to get artifact: {}", e)))?;
    
    match artifact {
        Some(a) => {
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "artifact_details",
                    "artifact": artifact_to_json(&a)
                }),
                request_id: None,
            })
        }
        None => Err(ApiError::not_found(format!("Artifact not found: {}", request.id)))
    }
}

async fn update_artifact(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: UpdateArtifactRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid update artifact request: {}", e)))?;
    
    info!("Updating artifact: {}", request.id);
    
    if request.name.is_none() && request.content.is_none() {
        return Err(ApiError::bad_request("No fields to update"));
    }
    
    if let Some(ref name) = request.name {
        if name.trim().is_empty() {
            return Err(ApiError::bad_request("Artifact name cannot be empty"));
        }
    }
    
    let artifact = app_state.project_store
        .update_artifact(
            &request.id,
            request.name,
            request.content,
        )
        .await
        .map_err(|e| ApiError::internal(format!("Failed to update artifact: {}", e)))?;
    
    match artifact {
        Some(a) => {
            info!("Artifact updated successfully: {} (version: {})", a.id, a.version);
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "artifact_updated",
                    "artifact": artifact_to_json(&a)
                }),
                request_id: None,
            })
        }
        None => Err(ApiError::not_found(format!("Artifact not found: {}", request.id)))
    }
}

async fn delete_artifact(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: DeleteArtifactRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid delete artifact request: {}", e)))?;
    
    info!("Deleting artifact: {}", request.id);
    
    let deleted = app_state.project_store
        .delete_artifact(&request.id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to delete artifact: {}", e)))?;
    
    if deleted {
        info!("Artifact deleted successfully: {}", request.id);
        Ok(WsServerMessage::Data {
            data: json!({
                "type": "artifact_deleted",
                "id": request.id
            }),
            request_id: None,
        })
    } else {
        Err(ApiError::not_found(format!("Artifact not found: {}", request.id)))
    }
}

async fn list_artifacts(params: Value, app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let request: ListArtifactsRequest = serde_json::from_value(params)
        .map_err(|e| ApiError::bad_request(format!("Invalid list artifacts request: {}", e)))?;
    
    debug!("Listing artifacts for project: {}", request.project_id);
    
    let project = app_state.project_store
        .get_project(&request.project_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to verify project: {}", e)))?;
    
    if project.is_none() {
        return Err(ApiError::not_found(format!("Project not found: {}", request.project_id)));
    }
    
    let artifacts = app_state.project_store
        .list_project_artifacts(&request.project_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list artifacts: {}", e)))?;
    
    info!("Found {} artifacts for project {}", artifacts.len(), request.project_id);
    
    Ok(WsServerMessage::Data {
        data: json!({
            "type": "artifact_list",
            "project_id": request.project_id,
            "artifacts": artifacts.iter().map(artifact_to_json).collect::<Vec<_>>()
        }),
        request_id: None,
    })
}

// Helper functions

fn project_to_json(project: &Project) -> Value {
    json!({
        "id": project.id,
        "name": project.name,
        "description": project.description,
        "tags": project.tags,
        "owner": project.owner,
        "created_at": project.created_at.to_rfc3339(),
        "updated_at": project.updated_at.to_rfc3339()
    })
}

fn artifact_to_json(artifact: &Artifact) -> Value {
    json!({
        "id": artifact.id,
        "project_id": artifact.project_id,
        "name": artifact.name,
        "type": artifact.artifact_type.to_string(),
        "content": artifact.content,
        "version": artifact.version,
        "created_at": artifact.created_at.to_rfc3339(),
        "updated_at": artifact.updated_at.to_rfc3339()
    })
}

/// Validates project ownership for future multi-user support
#[allow(dead_code)]
async fn validate_project_access(
    project_id: &str,
    _user: &str,
    app_state: &AppState,
) -> ApiResult<Project> {
    let project = app_state.project_store
        .get_project(project_id)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to get project: {}", e)))?;
    
    match project {
        Some(p) => {
            // In single-user mode, always allow access
            // In multi-user mode, check p.owner == user
            Ok(p)
        }
        None => Err(ApiError::not_found(format!("Project not found: {}", project_id)))
    }
}
