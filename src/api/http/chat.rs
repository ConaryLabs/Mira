// src/api/http/chat.rs - Updated REST handler with complete tool integration

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, error};

use crate::api::error::{ApiResult, IntoApiError};
use crate::config::CONFIG;
use crate::services::chat_with_tools::{ChatServiceToolExt, ChatResponseWithTools, ToolResult, Citation};
use crate::state::AppState;

// ---------- REST Chat Request/Response with Tool Support ----------

#[derive(Deserialize)]
pub struct RestChatRequest {
    pub message: String,
    pub project_id: Option<String>,
    pub persona_override: Option<String>,
    pub file_context: Option<serde_json::Value>,
    pub enable_tools: Option<bool>, // Allow per-request tool control
}

#[derive(Serialize)]
pub struct RestChatResponse {
    pub response: String,
    pub persona: String,
    pub mood: String,
    pub tags: Vec<String>,
    pub summary: String,
    // COMPLETED: Real tool results and citations instead of placeholders
    pub tool_results: Option<Vec<RestToolResult>>,
    pub citations: Option<Vec<RestCitation>>,
    pub previous_response_id: Option<String>,
    pub tools_used: usize,
}

#[derive(Serialize)]
pub struct RestToolResult {
    pub tool_type: String,
    pub tool_id: String,
    pub status: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct RestCitation {
    pub file_id: Option<String>,
    pub filename: Option<String>,
    pub url: Option<String>,
    pub snippet: Option<String>,
    pub title: Option<String>,
    pub source_type: String,
}

// ---------- History endpoints (unchanged) ----------

#[derive(Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Serialize)]
pub struct ChatHistoryMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: i64,
    pub tags: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct ChatHistoryResponse {
    pub messages: Vec<ChatHistoryMessage>,
}

pub async fn get_chat_history(
    State(app_state): State<Arc<AppState>>,
    Query(params): Query<HistoryQuery>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        let limit = params.limit.unwrap_or(50).min(100);
        let skip = params.offset.unwrap_or(0);
        let take = limit;

        info!("Fetching chat history: limit={}, offset={}", limit, skip);

        let messages: Vec<ChatHistoryMessage> = app_state.memory_service
            .get_recent_messages(&CONFIG.session_id, take + skip)
            .await
            .into_api_error("Failed to fetch chat history")?
            .into_iter()
            .skip(skip)
            .take(take)
            .enumerate()
            .map(|(i, m)| ChatHistoryMessage {
                id: m.id.unwrap_or_else(|| format!("msg_{}", i)),
                role: m.role,
                content: m.content,
                timestamp: m.timestamp.timestamp(),
                tags: m.tags.filter(|t| !t.is_empty()),
            })
            .collect();

        info!("Returning {} messages (skipped: {}, took: {})", messages.len(), skip, take);
        Ok(Json(ChatHistoryResponse { messages }))
    }.await;

    match result {
        Ok(response) => response.into_response(),
        Err(error) => error.into_response(),
    }
}

// ---------- REST chat handler with COMPLETE tool integration ----------

pub async fn rest_chat_handler(
    State(app_state): State<Arc<AppState>>,
    Json(request): Json<RestChatRequest>,
) -> impl IntoResponse {
    let result: ApiResult<_> = async {
        let session_id = &CONFIG.session_id;
        info!("REST chat request for session: {} (tools: {})", 
              session_id,
              request.enable_tools.unwrap_or(CONFIG.enable_chat_tools));

        // Determine if tools should be used
        let use_tools = request.enable_tools.unwrap_or(CONFIG.enable_chat_tools) && CONFIG.enable_chat_tools;

        if use_tools {
            info!("Using tool-enabled chat for enhanced capabilities");
            
            // Use the COMPLETED tool-enabled chat
            let tool_response = app_state.chat_service.chat_with_tools(
                session_id,
                &request.message,
                request.project_id.as_deref(),
                request.file_context,
            ).await
            .into_api_error("Tool-enabled chat service failed")?;

            // Convert to REST response format with REAL data
            let rest_tool_results = tool_response.tool_results.map(|tools| {
                tools.into_iter().map(|tool| RestToolResult {
                    tool_type: tool.tool_type,
                    tool_id: tool.tool_id,
                    status: tool.status,
                    result: tool.result,
                    error: tool.error,
                }).collect()
            });

            let rest_citations = tool_response.citations.map(|citations| {
                citations.into_iter().map(|citation| RestCitation {
                    file_id: citation.file_id,
                    filename: citation.filename,
                    url: citation.url,
                    snippet: citation.snippet,
                    title: citation.title,
                    source_type: citation.source_type,
                }).collect()
            });

            let tools_used = rest_tool_results.as_ref().map_or(0, |t| t.len());

            info!("Tool-enabled chat completed: {} tools used, {} citations", 
                  tools_used,
                  rest_citations.as_ref().map_or(0, |c| c.len()));

            Ok(Json(RestChatResponse {
                response: tool_response.base.output,
                persona: tool_response.base.persona,
                mood: tool_response.base.mood,
                tags: tool_response.base.tags,
                summary: tool_response.base.summary,
                tool_results: rest_tool_results,
                citations: rest_citations,
                previous_response_id: tool_response.previous_response_id,
                tools_used,
            }))
        } else {
            info!("Using regular chat (tools disabled)");
            
            // Use regular chat method without tools
            let response = app_state.chat_service.chat(
                session_id,
                &request.message,
                request.project_id.as_deref(),
            ).await
            .into_api_error("Chat service failed")?;

            // Add basic file context citation if provided
            let citations = if let Some(file_context) = &request.file_context {
                if let Some(file_path) = file_context.get("file_path").and_then(|p| p.as_str()) {
                    Some(vec![RestCitation {
                        file_id: Some("context_file".to_string()),
                        filename: Some(file_path.to_string()),
                        url: None,
                        snippet: file_context.get("content").and_then(|c| c.as_str()).map(|s| {
                            if s.len() > 200 { format!("{}...", &s[..200]) } else { s.to_string() }
                        }),
                        title: Some(format!("File: {}", file_path)),
                        source_type: "file".to_string(),
                    }])
                } else {
                    None
                }
            } else {
                None
            };

            Ok(Json(RestChatResponse {
                response: response.output,
                persona: response.persona,
                mood: response.mood,
                tags: response.tags,
                summary: response.summary,
                tool_results: None, // No tools used
                citations,
                previous_response_id: None,
                tools_used: 0,
            }))
        }
    }.await;

    match result {
        Ok(response) => response.into_response(),
        Err(error) => {
            error!("Chat service error: {}", error.message);
            error.into_response()
        }
    }
}
