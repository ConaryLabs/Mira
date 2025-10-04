// src/api/ws/code_intelligence.rs
use std::sync::Arc;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{debug, error, info};

use crate::{
    api::{
        error::{ApiError, ApiResult},
        ws::message::WsServerMessage,
    },
    state::AppState,
    memory::features::code_intelligence::CodeIntelligenceService,
};

#[derive(Debug, Deserialize)]
struct SearchElementsRequest {
    pattern: String,
    project_id: String,  // Now required for project-scoped search
    limit: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct RepoStatsRequest {
    attachment_id: String,
}

#[derive(Debug, Deserialize)]
struct ComplexityHotspotsRequest {
    project_id: String,  // Now required for project-scoped search
    limit: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct ElementsByTypeRequest {
    element_type: String,
    project_id: String,  // Now required for project-scoped search
    limit: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct DeleteRepositoryDataRequest {
    project_id: String,
}

// NEW: Dependency analysis request types
#[derive(Debug, Deserialize)]
struct DependencyAnalyzeRequest {
    project_id: String,
}

#[derive(Debug, Deserialize)]
struct DependencyFindCallersRequest {
    project_id: String,
    message_type: String,
    method: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DependencyFindHandlerRequest {
    project_id: String,
    message_type: String,
    method: Option<String>,
}

pub async fn handle_code_intelligence_command(
    method: &str,
    params: Value,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    debug!("Processing code intelligence command: {}", method);
    
    match method {
        "code.search" => {
            let req: SearchElementsRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid search request: {}", e)))?;
            
            info!("Searching code elements for pattern: {} in project: {}", req.pattern, req.project_id);
            let elements = app_state.code_intelligence
                .search_elements_for_project(&req.pattern, &req.project_id, req.limit)
                .await
                .map_err(|e| ApiError::internal(format!("Search failed: {}", e)))?;
            
            let elements_json: Vec<_> = elements.into_iter().map(|element| {
                json!({
                    "type": element.element_type,
                    "name": element.name,
                    "full_path": element.full_path,
                    "visibility": element.visibility,
                    "start_line": element.start_line,
                    "end_line": element.end_line,
                    "complexity_score": element.complexity_score,
                    "is_test": element.is_test,
                    "is_async": element.is_async,
                    "documentation": element.documentation,
                })
            }).collect();
            
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "code_search_results",
                    "pattern": req.pattern,
                    "project_id": req.project_id,
                    "elements": elements_json,
                    "count": elements_json.len()
                }),
                request_id: None,
            })
        }

        "code.repo_stats" => {
            let req: RepoStatsRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid repo stats request: {}", e)))?;
            
            info!("Getting repo stats for: {}", req.attachment_id);
            let stats = app_state.code_intelligence
                .get_repo_stats(&req.attachment_id)
                .await
                .map_err(|e| ApiError::internal(format!("Repo stats failed: {}", e)))?;
            
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "repo_stats",
                    "attachment_id": req.attachment_id,
                    "total_files": stats.total_files,
                    "analyzed_files": stats.analyzed_files,
                    "total_elements": stats.total_elements,
                    "avg_complexity": stats.avg_complexity,
                    "total_quality_issues": stats.total_quality_issues,
                    "critical_issues": stats.critical_issues,
                    "high_issues": stats.high_issues
                }),
                request_id: None,
            })
        }

        "code.complexity_hotspots" => {
            let req: ComplexityHotspotsRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid hotspots request: {}", e)))?;
            
            info!("Getting complexity hotspots for project: {} (limit: {:?})", req.project_id, req.limit);
            let hotspots = app_state.code_intelligence
                .get_complexity_hotspots_for_project(&req.project_id, req.limit)
                .await
                .map_err(|e| ApiError::internal(format!("Hotspots query failed: {}", e)))?;
            
            let hotspots_json: Vec<_> = hotspots.into_iter().map(|element| {
                json!({
                    "name": element.name,
                    "full_path": element.full_path,
                    "complexity_score": element.complexity_score,
                    "start_line": element.start_line,
                    "end_line": element.end_line,
                    "element_type": element.element_type,
                })
            }).collect();
            
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "complexity_hotspots",
                    "project_id": req.project_id,
                    "hotspots": hotspots_json,
                    "count": hotspots_json.len()
                }),
                request_id: None,
            })
        }

        "code.elements_by_type" => {
            let req: ElementsByTypeRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid elements by type request: {}", e)))?;
            
            info!("Getting {} elements for project: {} (limit: {:?})", req.element_type, req.project_id, req.limit);
            let elements = app_state.code_intelligence
                .get_elements_by_type(&req.element_type, req.limit)
                .await
                .map_err(|e| ApiError::internal(format!("Elements query failed: {}", e)))?;
            
            let elements_json: Vec<_> = elements.into_iter().map(|element| {
                json!({
                    "name": element.name,
                    "full_path": element.full_path,
                    "visibility": element.visibility,
                    "complexity_score": element.complexity_score,
                    "is_test": element.is_test,
                    "is_async": element.is_async,
                    "documentation": element.documentation,
                })
            }).collect();
            
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "elements_by_type",
                    "element_type": req.element_type,
                    "project_id": req.project_id,
                    "elements": elements_json,
                    "count": elements_json.len()
                }),
                request_id: None,
            })
        }

        "code.supported_languages" => {
            // For now, just Rust - will add TypeScript/JavaScript later
            let languages = vec!["rust".to_string()];
            
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "supported_languages",
                    "languages": languages
                }),
                request_id: None,
            })
        }

        "code.delete_repository_data" => {
            let req: DeleteRepositoryDataRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid delete request: {}", e)))?;
            
            info!("Deleting code intelligence data for project: {}", req.project_id);
            
            // Get all repository attachments for this project
            let attachments = app_state.git_client.store.get_attachments_for_project(&req.project_id).await
                .map_err(|e| ApiError::internal(format!("Failed to get attachments: {}", e)))?;
            
            let mut deleted_elements = 0;
            for attachment in attachments {
                let count = app_state.code_intelligence
                    .delete_repository_data(&attachment.id)
                    .await
                    .map_err(|e| ApiError::internal(format!("Delete failed: {}", e)))?;
                deleted_elements += count;
            }
            
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "repository_data_deleted",
                    "project_id": req.project_id,
                    "deleted_elements": deleted_elements
                }),
                request_id: None,
            })
        }

        // NEW: WebSocket dependency analysis
        "dependencies.analyze" => {
            let req: DependencyAnalyzeRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid dependency analyze request: {}", e)))?;
            
            info!("Analyzing WebSocket dependencies for project: {}", req.project_id);
            
            let service = CodeIntelligenceService::new(app_state.sqlite_pool.clone());
            service.link_websocket_dependencies(&req.project_id).await
                .map_err(|e| ApiError::internal(format!("Dependency linking failed: {}", e)))?;
            
            let report = service.get_dependency_report(&req.project_id).await
                .map_err(|e| ApiError::internal(format!("Failed to get dependency report: {}", e)))?;
            
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "dependency_report",
                    "project_id": req.project_id,
                    "orphaned_calls": report.orphaned_calls,
                    "unused_handlers": report.unused_handlers,
                    "orphaned_count": report.orphaned_calls.len(),
                    "unused_count": report.unused_handlers.len()
                }),
                request_id: None,
            })
        }

        "dependencies.find_callers" => {
            let req: DependencyFindCallersRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid find callers request: {}", e)))?;
            
            info!("Finding callers for message_type: {}, method: {:?} in project: {}", 
                  req.message_type, req.method, req.project_id);
            
            let callers = sqlx::query!(
                "SELECT c.frontend_element, c.call_line, rf.file_path
                 FROM websocket_calls c
                 JOIN code_elements ce ON c.frontend_file_id = ce.id
                 JOIN repository_files rf ON ce.file_id = rf.id
                 WHERE c.project_id = ? 
                   AND c.message_type = ? 
                   AND (c.method = ? OR ? IS NULL)",
                req.project_id,
                req.message_type,
                req.method,
                req.method
            )
            .fetch_all(&app_state.sqlite_pool)
            .await
            .map_err(|e| ApiError::internal(format!("Database query failed: {}", e)))?;
            
            let result: Vec<_> = callers.into_iter().map(|row| {
                json!({
                    "frontend_element": row.frontend_element,
                    "call_line": row.call_line,
                    "file_path": row.file_path
                })
            }).collect();
            
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "dependency_callers",
                    "message_type": req.message_type,
                    "method": req.method,
                    "project_id": req.project_id,
                    "callers": result,
                    "count": result.len()
                }),
                request_id: None,
            })
        }

        "dependencies.find_handler" => {
            let req: DependencyFindHandlerRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid find handler request: {}", e)))?;
            
            info!("Finding handler for message_type: {}, method: {:?} in project: {}", 
                  req.message_type, req.method, req.project_id);
            
            let handler = sqlx::query!(
                "SELECT h.handler_function, h.handler_line, rf.file_path
                 FROM websocket_handlers h
                 JOIN code_elements ce ON h.backend_file_id = ce.id
                 JOIN repository_files rf ON ce.file_id = rf.id
                 WHERE h.project_id = ? 
                   AND h.message_type = ? 
                   AND (h.method = ? OR ? IS NULL)
                 LIMIT 1",
                req.project_id,
                req.message_type,
                req.method,
                req.method
            )
            .fetch_optional(&app_state.sqlite_pool)
            .await
            .map_err(|e| ApiError::internal(format!("Database query failed: {}", e)))?;
            
            let result = handler.map(|row| {
                json!({
                    "handler_function": row.handler_function,
                    "handler_line": row.handler_line,
                    "file_path": row.file_path
                })
            });
            
            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "dependency_handler",
                    "message_type": req.message_type,
                    "method": req.method,
                    "project_id": req.project_id,
                    "handler": result
                }),
                request_id: None,
            })
        }

        _ => {
            error!("Unknown code intelligence method: {}", method);
            Err(ApiError::bad_request(format!("Unknown method: {}", method)))
        }
    }
}

pub fn is_code_intelligence_enabled(_app_state: &AppState) -> bool {
    true
}

pub async fn get_code_intelligence_status(app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    // For now, just Rust - will add TypeScript/JavaScript later
    let supported_languages = vec!["rust".to_string()];
    let git_has_code_intel = app_state.git_client.has_code_intelligence();
    
    Ok(WsServerMessage::Data {
        data: json!({
            "type": "code_intelligence_status",
            "enabled": true,
            "supported_languages": supported_languages,
            "git_integration": git_has_code_intel
        }),
        request_id: None,
    })
}
