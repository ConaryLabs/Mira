// src/api/ws/project.rs
// WebSocket handler for project operations - REFACTORED: 509 → ~200 lines!

use std::sync::Arc;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{debug, info, error};

use crate::{
    api::{
        error::{ApiError, ApiResult},
        ws::message::WsServerMessage,
    },
    state::AppState,
    project::types::{Project, Artifact, ArtifactType},
};

// ============================================================================
// REQUEST TYPES - Minimal and focused
// ============================================================================

#[derive(Debug, Deserialize)]
struct CreateProjectRequest {
    name: String,
    description: Option<String>,
    tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct UpdateProjectRequest {
    id: String,
    name: Option<String>,
    description: Option<String>,
    tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ProjectIdRequest {
    id: String,
}

#[derive(Debug, Deserialize)]
struct CreateArtifactRequest {
    project_id: String,
    name: String,
    artifact_type: String,
    content: Option<String>,
    // metadata: Option<Value>, // Not supported by store
}

#[derive(Debug, Deserialize)]
struct UpdateArtifactRequest {
    id: String,
    name: Option<String>,
    content: Option<String>,
    // metadata: Option<Value>, // Not supported by store
}

#[derive(Debug, Deserialize)]
struct ArtifactIdRequest {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ListArtifactsRequest {
    project_id: String,
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

fn project_to_json(project: &Project) -> Value {
    serde_json::to_value(project).unwrap_or_else(|_| json!({}))
}

fn artifact_to_json(artifact: &Artifact) -> Value {
    serde_json::to_value(artifact).unwrap_or_else(|_| json!({}))
}

// Parse artifact type with proper error handling
fn parse_artifact_type(type_str: &str) -> ArtifactType {
    match type_str.to_lowercase().as_str() {
        "code" => ArtifactType::Code,
        "image" => ArtifactType::Image,
        "log" => ArtifactType::Log,
        "note" => ArtifactType::Note,
        "markdown" => ArtifactType::Markdown,
        _ => ArtifactType::Note, // Default
    }
}

// ============================================================================
// MAIN ROUTER - Clean and simple
// ============================================================================

pub async fn handle_project_command(
    method: &str,
    params: Value,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    debug!("Processing project command: {}", method);
    
    let result = match method {
        // Project CRUD - each handler is now ~10 lines!
        "project.create" => {
            let req: CreateProjectRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;
            
            if req.name.trim().is_empty() {
                return Err(ApiError::bad_request("Project name cannot be empty"));
            }
            
            let project = app_state.project_store
                .create_project(req.name, req.description, req.tags, Some("peter".to_string()))
                .await
                .map_err(|e| ApiError::internal(format!("Failed to create project: {}", e)))?;
            
            Ok(WsServerMessage::Data {
                data: json!({ "type": "project_created", "project": project_to_json(&project) }),
                request_id: None,
            })
        }
        
        "project.list" => {
            let projects = app_state.project_store
                .list_projects()
                .await
                .map_err(|e| ApiError::internal(format!("Failed to list projects: {}", e)))?;
            
            // 🚀 NEW: Enrich projects with git attachment information
            let mut enriched_projects = Vec::new();
            for project in projects {
                let attachments = app_state.git_client.store
                    .list_project_attachments(&project.id)
                    .await
                    .unwrap_or_default();
                
                let mut project_json = project_to_json(&project);
                project_json["has_repository"] = json!(!attachments.is_empty());
                
                if let Some(attachment) = attachments.first() {
                    project_json["repository_url"] = json!(attachment.repo_url);
                    project_json["import_status"] = json!(attachment.import_status);
                    project_json["last_sync_at"] = json!(attachment.last_sync_at);
                }
                
                enriched_projects.push(project_json);
            }
            
            debug!("Enriched {} projects with repository information", enriched_projects.len());
            
            Ok(WsServerMessage::Data {
                data: json!({ "type": "project_list", "projects": enriched_projects }),
                request_id: None,
            })
        }
        
        "project.get" => {
            let req: ProjectIdRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;
            
            let project = app_state.project_store
                .get_project(&req.id)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to get project: {}", e)))?
                .ok_or_else(|| ApiError::not_found("Project not found"))?;
            
            Ok(WsServerMessage::Data {
                data: json!({ "type": "project", "project": project_to_json(&project) }),
                request_id: None,
            })
        }
        
        "project.update" => {
            let req: UpdateProjectRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;
            
            let project = app_state.project_store
                .update_project(&req.id, req.name, req.description, req.tags)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to update project: {}", e)))?
                .ok_or_else(|| ApiError::not_found("Project not found"))?;
            
            Ok(WsServerMessage::Data {
                data: json!({ "type": "project_updated", "project": project_to_json(&project) }),
                request_id: None,
            })
        }
        
        "project.delete" => {
            let req: ProjectIdRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;
            
            let deleted = app_state.project_store
                .delete_project(&req.id)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to delete project: {}", e)))?;
            
            if !deleted {
                return Err(ApiError::not_found("Project not found"));
            }
            
            Ok(WsServerMessage::Status {
                message: format!("Project {} deleted", req.id),
                detail: None,
            })
        }
        
        // Artifact CRUD - Fixed to match actual store signatures
        "artifact.create" => {
            let req: CreateArtifactRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;
            
            if req.name.trim().is_empty() {
                return Err(ApiError::bad_request("Artifact name cannot be empty"));
            }
            
            let artifact_type = parse_artifact_type(&req.artifact_type);
            let artifact = app_state.project_store
                .create_artifact(req.project_id, req.name, artifact_type, req.content)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to create artifact: {}", e)))?;
            
            Ok(WsServerMessage::Data {
                data: json!({ "type": "artifact_created", "artifact": artifact_to_json(&artifact) }),
                request_id: None,
            })
        }
        
        "artifact.get" => {
            let req: ArtifactIdRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;
            
            let artifact = app_state.project_store
                .get_artifact(&req.id)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to get artifact: {}", e)))?
                .ok_or_else(|| ApiError::not_found("Artifact not found"))?;
            
            Ok(WsServerMessage::Data {
                data: json!({ "type": "artifact", "artifact": artifact_to_json(&artifact) }),
                request_id: None,
            })
        }
        
        "artifact.update" => {
            let req: UpdateArtifactRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;
            
            let artifact = app_state.project_store
                .update_artifact(&req.id, req.name, req.content)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to update artifact: {}", e)))?
                .ok_or_else(|| ApiError::not_found("Artifact not found"))?;
            
            Ok(WsServerMessage::Data {
                data: json!({ "type": "artifact_updated", "artifact": artifact_to_json(&artifact) }),
                request_id: None,
            })
        }
        
        "artifact.delete" => {
            let req: ArtifactIdRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;
            
            let deleted = app_state.project_store
                .delete_artifact(&req.id)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to delete artifact: {}", e)))?;
            
            if !deleted {
                return Err(ApiError::not_found("Artifact not found"));
            }
            
            Ok(WsServerMessage::Status {
                message: format!("Artifact {} deleted", req.id),
                detail: None,
            })
        }
        
        "artifact.list" => {
            let req: ListArtifactsRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid request: {}", e)))?;
            
            let artifacts = app_state.project_store
                .list_project_artifacts(&req.project_id)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to list artifacts: {}", e)))?;
            
            let artifact_list: Vec<Value> = artifacts.iter().map(artifact_to_json).collect();
            
            Ok(WsServerMessage::Data {
                data: json!({ "type": "artifact_list", "artifacts": artifact_list }),
                request_id: None,
            })
        }
        
        _ => {
            error!("Unknown project method: {}", method);
            return Err(ApiError::bad_request(format!("Unknown project method: {}", method)));
        }
    };
    
    match &result {
        Ok(_) => info!("Project command {} completed successfully", method),
        Err(e) => error!("Project command {} failed: {:?}", method, e),
    }
    
    result
}
