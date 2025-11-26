// src/api/ws/code_intelligence.rs
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use tracing::{debug, error, info};

use crate::{
    api::{
        error::{ApiError, ApiResult},
        ws::message::WsServerMessage,
    },
    state::AppState,
};

#[derive(Debug, Deserialize)]
struct SearchElementsRequest {
    pattern: String,
    project_id: String,
    limit: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct RepoStatsRequest {
    attachment_id: String,
}

#[derive(Debug, Deserialize)]
struct ComplexityHotspotsRequest {
    project_id: String,
    limit: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct ElementsByTypeRequest {
    element_type: String,
    project_id: String,
    limit: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct DeleteRepositoryDataRequest {
    project_id: String,
}

#[derive(Debug, Deserialize)]
struct BudgetStatusRequest {
    user_id: String,
}

#[derive(Debug, Deserialize)]
struct SemanticSearchRequest {
    query: String,
    project_id: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct CochangeRequest {
    project_id: String,
    file_path: String,
}

#[derive(Debug, Deserialize)]
struct ExpertiseRequest {
    project_id: String,
    file_path: Option<String>,
    limit: Option<i64>,
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

            info!(
                "Searching code elements for pattern: {} in project: {}",
                req.pattern, req.project_id
            );
            let elements = app_state
                .code_intelligence
                .search_elements_for_project(&req.pattern, &req.project_id, req.limit)
                .await
                .map_err(|e| ApiError::internal(format!("Search failed: {}", e)))?;

            let elements_json: Vec<_> = elements
                .into_iter()
                .map(|element| {
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
                })
                .collect();

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
            let stats = app_state
                .code_intelligence
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

            info!(
                "Getting complexity hotspots for project: {} (limit: {:?})",
                req.project_id, req.limit
            );
            let hotspots = app_state
                .code_intelligence
                .get_complexity_hotspots_for_project(&req.project_id, req.limit)
                .await
                .map_err(|e| ApiError::internal(format!("Hotspots query failed: {}", e)))?;

            let hotspots_json: Vec<_> = hotspots
                .into_iter()
                .map(|element| {
                    json!({
                        "name": element.name,
                        "full_path": element.full_path,
                        "complexity_score": element.complexity_score,
                        "start_line": element.start_line,
                        "end_line": element.end_line,
                        "element_type": element.element_type,
                    })
                })
                .collect();

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
            let req: ElementsByTypeRequest = serde_json::from_value(params).map_err(|e| {
                ApiError::bad_request(format!("Invalid elements by type request: {}", e))
            })?;

            info!(
                "Getting {} elements for project: {} (limit: {:?})",
                req.element_type, req.project_id, req.limit
            );
            let elements = app_state
                .code_intelligence
                .get_elements_by_type(&req.element_type, req.limit)
                .await
                .map_err(|e| ApiError::internal(format!("Elements query failed: {}", e)))?;

            let elements_json: Vec<_> = elements
                .into_iter()
                .map(|element| {
                    json!({
                        "name": element.name,
                        "full_path": element.full_path,
                        "visibility": element.visibility,
                        "complexity_score": element.complexity_score,
                        "is_test": element.is_test,
                        "is_async": element.is_async,
                        "documentation": element.documentation,
                    })
                })
                .collect();

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

            info!(
                "Deleting code intelligence data for project: {}",
                req.project_id
            );

            // Get all repository attachments for this project
            let attachments = app_state
                .git_client
                .store
                .get_attachments_for_project(&req.project_id)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to get attachments: {}", e)))?;

            let mut deleted_elements = 0;
            for attachment in attachments {
                let count = app_state
                    .code_intelligence
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

        "code.budget_status" => {
            let req: BudgetStatusRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid budget request: {}", e)))?;

            info!("Getting budget status for user: {}", req.user_id);
            let status = app_state
                .budget_tracker
                .get_budget_status(&req.user_id)
                .await
                .map_err(|e| ApiError::internal(format!("Budget query failed: {}", e)))?;

            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "budget_status",
                    "daily_usage_percent": status.daily_usage_percent,
                    "monthly_usage_percent": status.monthly_usage_percent,
                    "daily_spent_usd": status.daily_spent_usd,
                    "daily_limit_usd": status.daily_limit_usd,
                    "monthly_spent_usd": status.monthly_spent_usd,
                    "monthly_limit_usd": status.monthly_limit_usd,
                    "daily_remaining": status.daily_remaining(),
                    "monthly_remaining": status.monthly_remaining(),
                    "is_critical": status.is_critical(),
                    "is_low": status.is_low(),
                    "last_updated": chrono::Utc::now().timestamp_millis()
                }),
                request_id: None,
            })
        }

        "code.semantic_search" => {
            let req: SemanticSearchRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid semantic search request: {}", e)))?;

            info!("Semantic search for query: {} (project: {:?})", req.query, req.project_id);

            let project_id = req.project_id.as_deref().unwrap_or("default");
            let limit = req.limit.unwrap_or(10);
            let results = app_state
                .code_intelligence
                .search_code(&req.query, project_id, limit)
                .await
                .map_err(|e| ApiError::internal(format!("Semantic search failed: {}", e)))?;

            // Transform MemoryEntry results into search result format
            let results_json: Vec<_> = results
                .into_iter()
                .enumerate()
                .map(|(idx, entry)| {
                    // Extract file path from tags
                    let file_path = entry.tags.as_ref()
                        .and_then(|tags| tags.iter()
                            .find(|t| t.starts_with("path:"))
                            .and_then(|t| t.strip_prefix("path:"))
                            .map(|s| s.to_string()))
                        .unwrap_or_default();

                    // Extract line info from tags if available
                    let line_start = entry.tags.as_ref()
                        .and_then(|tags| tags.iter()
                            .find(|t| t.starts_with("start_line:"))
                            .and_then(|t| t.strip_prefix("start_line:"))
                            .and_then(|s| s.parse::<i32>().ok()))
                        .unwrap_or(0);

                    let line_end = entry.tags.as_ref()
                        .and_then(|tags| tags.iter()
                            .find(|t| t.starts_with("end_line:"))
                            .and_then(|t| t.strip_prefix("end_line:"))
                            .and_then(|s| s.parse::<i32>().ok()))
                        .unwrap_or(0);

                    // Use salience as a proxy for score if available
                    let score = entry.salience.unwrap_or(0.5) as f64;

                    json!({
                        "id": entry.id.map(|id| id.to_string()).unwrap_or_else(|| idx.to_string()),
                        "file_path": file_path,
                        "content": entry.content,
                        "score": score,
                        "line_start": line_start,
                        "line_end": line_end,
                        "language": entry.programming_lang,
                    })
                })
                .collect();

            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "semantic_search_results",
                    "query": req.query,
                    "results": results_json,
                    "count": results_json.len()
                }),
                request_id: None,
            })
        }

        "code.cochange" => {
            let req: CochangeRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid cochange request: {}", e)))?;

            info!("Getting co-change suggestions for: {} in project: {}", req.file_path, req.project_id);

            let suggestions = app_state
                .cochange_service
                .get_suggestions(&req.project_id, &req.file_path)
                .await
                .map_err(|e| ApiError::internal(format!("Co-change query failed: {}", e)))?;

            let suggestions_json: Vec<_> = suggestions
                .into_iter()
                .map(|s| {
                    json!({
                        "file_path": s.file_path,
                        "confidence": s.confidence,
                        "reason": s.reason,
                        "co_change_count": s.cochange_count,
                    })
                })
                .collect();

            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "cochange_suggestions",
                    "file_path": req.file_path,
                    "project_id": req.project_id,
                    "suggestions": suggestions_json,
                    "count": suggestions_json.len()
                }),
                request_id: None,
            })
        }

        "code.expertise" => {
            let req: ExpertiseRequest = serde_json::from_value(params)
                .map_err(|e| ApiError::bad_request(format!("Invalid expertise request: {}", e)))?;

            info!("Getting expertise for project: {} (file: {:?})", req.project_id, req.file_path);

            let limit = req.limit.unwrap_or(10);
            let experts = if let Some(file_path) = req.file_path {
                app_state
                    .expertise_service
                    .find_experts_for_file(&req.project_id, &file_path, limit)
                    .await
                    .map_err(|e| ApiError::internal(format!("Expertise query failed: {}", e)))?
            } else {
                app_state
                    .expertise_service
                    .get_top_experts(&req.project_id, limit)
                    .await
                    .map_err(|e| ApiError::internal(format!("Expertise query failed: {}", e)))?
            };

            let experts_json: Vec<_> = experts
                .into_iter()
                .map(|e| {
                    json!({
                        "author": e.author_name,
                        "email": e.author_email,
                        "total_commits": e.commit_count,
                        "last_active": e.last_active,
                        "expertise_areas": e.matching_patterns,
                        "overall_score": e.expertise_score
                    })
                })
                .collect();

            Ok(WsServerMessage::Data {
                data: json!({
                    "type": "expertise_results",
                    "project_id": req.project_id,
                    "experts": experts_json,
                    "count": experts_json.len()
                }),
                request_id: None,
            })
        }

        // REMOVED: WebSocket dependency tracking (Phase 1 - tables deleted)
        // - dependencies.analyze
        // - dependencies.find_callers
        // - dependencies.find_handler
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
