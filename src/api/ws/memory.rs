// src/api/ws/memory.rs
// Thin WebSocket routing layer for memory commands - delegates to MemoryService

use std::sync::Arc;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{debug, error, info, warn};

use crate::{
    api::{
        error::{ApiError, ApiResult},
        ws::message::WsServerMessage,
    },
    config::CONFIG,
    state::AppState,
    memory::RecallContext,
};

const DEFAULT_SESSION: &str = "peter-eternal";

// Simple request structs for parsing
#[derive(Debug, Deserialize)]
struct ImportMemoryData {
    content: String,
    role: String,
    salience: Option<f32>,
    tags: Option<Vec<String>>,
    memory_type: Option<String>,
}

fn get_session_id(session_id: Option<&str>) -> String {
    session_id.map(String::from).unwrap_or_else(|| DEFAULT_SESSION.to_string())
}

/// Main router for memory commands - delegates to MemoryService
pub async fn handle_memory_command(
    method: &str,
    params: Value,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    info!("Processing memory command: {}", method);
    debug!("Parameters: {:?}", params);
    
    let memory = &app_state.memory_service;
    
    let result = match method {
        "memory.save" => save_memory(params, memory).await,
        "memory.search" => search_memory(params, memory).await,
        "memory.get_context" => get_context(params, memory).await,
        "memory.get_recent" => get_recent_memories(params, memory).await,
        "memory.get_stats" => get_memory_stats(params, memory).await,
        "memory.trigger_rolling_summary" => trigger_rolling_summary(params, memory).await,
        "memory.trigger_snapshot_summary" => trigger_snapshot_summary(params, memory).await,
        "memory.import" => import_memories(params, memory).await,
        "memory.export" => export_memories(params, memory).await,
        "memory.check_qdrant" => check_qdrant_status(app_state).await,
        
        // Not implemented yet - return pending status
        "memory.pin" | "memory.unpin" | "memory.delete" | "memory.update_salience" => {
            Ok(WsServerMessage::Data {
                data: json!({
                    "status": "pending_implementation",
                    "method": method
                }),
                request_id: None,
            })
        }
        
        _ => Err(ApiError::bad_request(format!("Unknown memory method: {}", method)))
    };
    
    result.map_err(|e| {
        error!("Memory command error ({}): {}", method, e);
        ApiError::internal(e.to_string())
    })
}

async fn save_memory(
    params: Value,
    memory: &Arc<crate::memory::MemoryService>
) -> ApiResult<WsServerMessage> {
    let session_id = get_session_id(params["session_id"].as_str());
    let content = params["content"].as_str()
        .ok_or_else(|| ApiError::bad_request("content is required"))?;
    let role = params["role"].as_str().unwrap_or("user");
    
    let entry_id = match role {
        "user" => {
            let id = memory.save_user_message(&session_id, content, params["project_id"].as_str())
                .await
                .map_err(|e| ApiError::internal(format!("Failed to save user message: {}", e)))?;
            info!("Saved user message {} for session: {}", id, session_id);
            id
        }
        "assistant" => {
            // Build ChatResponse from params
            use crate::llm::types::ChatResponse;
            
            let metadata = params.get("metadata");
            let response = if let Some(meta) = metadata {
                ChatResponse {
                    output: content.to_string(),
                    persona: meta["persona"].as_str().unwrap_or("assistant").to_string(),
                    mood: meta["mood"].as_str().unwrap_or("neutral").to_string(),
                    salience: meta["salience"].as_f64().unwrap_or(5.0) as f32,
                    summary: meta["summary"].as_str().unwrap_or(content).to_string(),
                    memory_type: meta["memory_type"].as_str().unwrap_or("other").to_string(),
                    tags: meta["tags"].as_array()
                        .map(|arr| arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect())
                        .unwrap_or_else(|| vec!["assistant".to_string()]),
                    intent: meta["intent"].as_str().map(String::from),
                    monologue: meta["monologue"].as_str().map(String::from),
                    reasoning_summary: meta["reasoning_summary"].as_str().map(String::from),
                }
            } else {
                ChatResponse {
                    output: content.to_string(),
                    persona: "assistant".to_string(),
                    mood: "neutral".to_string(),
                    salience: 5.0,
                    summary: content.to_string(),
                    memory_type: "other".to_string(),
                    tags: vec!["assistant".to_string()],
                    intent: None,
                    monologue: None,
                    reasoning_summary: None,
                }
            };
            
            let id = memory.save_assistant_response(&session_id, &response, params["project_id"].as_str())
                .await
                .map_err(|e| ApiError::internal(format!("Failed to save assistant response: {}", e)))?;
            info!("Saved assistant response {} for session: {}", id, session_id);
            id
        }
        _ => return Err(ApiError::bad_request(format!("Invalid role: {}. Must be 'user' or 'assistant'", role)))
    };
    
    Ok(WsServerMessage::Data {
        data: json!({
            "success": true,
            "session_id": session_id,
            "entry_id": entry_id,
            "message": format!("Memory {} saved for session {}", entry_id, session_id)
        }),
        request_id: None,
    })
}

async fn search_memory(
    params: Value,
    memory: &Arc<crate::memory::MemoryService>
) -> ApiResult<WsServerMessage> {
    let session_id = get_session_id(params["session_id"].as_str());
    let query = params["query"].as_str()
        .ok_or_else(|| ApiError::bad_request("query is required"))?;
    let max_results = params["max_results"].as_u64().unwrap_or(10) as usize;
    
    let results = memory.search_similar(&session_id, query, max_results).await
        .unwrap_or_else(|e| {
            warn!("Search failed for session {}: {}, returning empty results", session_id, e);
            Vec::new()
        });
    
    Ok(WsServerMessage::Data {
        data: json!({
            "memories": results,
            "count": results.len(),
            "session_id": session_id
        }),
        request_id: None,
    })
}

async fn get_context(
    params: Value,
    memory: &Arc<crate::memory::MemoryService>
) -> ApiResult<WsServerMessage> {
    let session_id = get_session_id(params["session_id"].as_str());
    let recent_count = params["recent_count"].as_u64().unwrap_or(10) as usize;
    let semantic_count = params["semantic_count"].as_u64().unwrap_or(5) as usize;
    
    let context = if let Some(user_text) = params["user_text"].as_str() {
        memory.parallel_recall_context(&session_id, user_text, recent_count, semantic_count).await
            .map_err(|e| ApiError::internal(format!("Failed to build context: {}", e)))?
    } else {
        let recent = memory.get_recent_context(&session_id, recent_count).await
            .map_err(|e| ApiError::internal(format!("Failed to get recent context: {}", e)))?;
        RecallContext {
            recent,
            semantic: Vec::new(),
        }
    };
    
    Ok(WsServerMessage::Data {
        data: json!({
            "context": {
                "recent": context.recent,
                "semantic": context.semantic
            },
            "session_id": session_id,
            "stats": {
                "recent_count": context.recent.len(),
                "semantic_count": context.semantic.len()
            }
        }),
        request_id: None,
    })
}

async fn get_recent_memories(
    params: Value,
    memory: &Arc<crate::memory::MemoryService>
) -> ApiResult<WsServerMessage> {
    let session_id = get_session_id(params["session_id"].as_str());
    let count = params["count"].as_u64().unwrap_or(20) as usize;
    
    let memories = memory.get_recent_context(&session_id, count).await
        .map_err(|e| ApiError::internal(format!("Failed to get recent memories: {}", e)))?;
    
    Ok(WsServerMessage::Data {
        data: json!({
            "memories": memories,
            "count": memories.len(),
            "session_id": session_id
        }),
        request_id: None,
    })
}

async fn get_memory_stats(
    params: Value,
    memory: &Arc<crate::memory::MemoryService>
) -> ApiResult<WsServerMessage> {
    let session_id = get_session_id(params["session_id"].as_str());
    let stats = memory.get_stats(&session_id).await
        .map_err(|e| ApiError::internal(format!("Failed to get stats: {}", e)))?;
    
    Ok(WsServerMessage::Data {
        data: json!({
            "session_id": session_id,
            "stats": stats
        }),
        request_id: None,
    })
}

async fn trigger_rolling_summary(
    params: Value,
    memory: &Arc<crate::memory::MemoryService>
) -> ApiResult<WsServerMessage> {
    let session_id = get_session_id(params["session_id"].as_str());
    let window_size = params["window_size"].as_u64().unwrap_or(10) as usize;
    
    let message = memory.create_rolling_summary(&session_id, window_size).await
        .map_err(|e| ApiError::internal(format!("Failed to create rolling summary: {}", e)))?;
    
    Ok(WsServerMessage::Data {
        data: json!({
            "success": true,
            "session_id": session_id,
            "message": message
        }),
        request_id: None,
    })
}

async fn trigger_snapshot_summary(
    params: Value,
    memory: &Arc<crate::memory::MemoryService>
) -> ApiResult<WsServerMessage> {
    let session_id = get_session_id(params["session_id"].as_str());
    
    let summary = memory.create_snapshot_summary(&session_id, None).await
        .map_err(|e| ApiError::internal(format!("Failed to create snapshot summary: {}", e)))?;
    
    Ok(WsServerMessage::Data {
        data: json!({
            "success": true,
            "session_id": session_id,
            "summary": summary
        }),
        request_id: None,
    })
}

async fn import_memories(
    params: Value,
    memory: &Arc<crate::memory::MemoryService>
) -> ApiResult<WsServerMessage> {
    let session_id = get_session_id(params["session_id"].as_str());
    let memories: Vec<ImportMemoryData> = serde_json::from_value(params["memories"].clone())
        .map_err(|e| ApiError::bad_request(format!("Invalid memories array: {}", e)))?;
    
    let mut imported = 0;
    let mut errors = Vec::new();
    let mut imported_ids = Vec::new();
    
    for (idx, mem) in memories.into_iter().enumerate() {
        let result = match mem.role.as_str() {
            "user" => {
                memory.save_user_message(&session_id, &mem.content, None)
                    .await
                    .map_err(|e| ApiError::internal(e.to_string()))
            }
            "assistant" => {
                use crate::llm::types::ChatResponse;
                let response = ChatResponse {
                    output: mem.content,
                    persona: "assistant".to_string(),
                    mood: "neutral".to_string(),
                    salience: mem.salience.unwrap_or(5.0),
                    summary: format!("Imported memory {}", idx + 1),
                    memory_type: mem.memory_type.unwrap_or_else(|| "other".to_string()),
                    tags: mem.tags.unwrap_or_default(),
                    intent: None,
                    monologue: None,
                    reasoning_summary: None,
                };
                memory.save_assistant_response(&session_id, &response, None)
                    .await
                    .map_err(|e| ApiError::internal(e.to_string()))
            }
            _ => Err(ApiError::bad_request(format!("Invalid role: {}", mem.role)))
        };
        
        match result {
            Ok(entry_id) => {
                imported += 1;
                imported_ids.push(entry_id);
            }
            Err(e) => errors.push(format!("Memory {}: {}", idx + 1, e))
        }
    }
    
    info!("Imported {} memories for session {}, {} errors", imported, session_id, errors.len());
    
    Ok(WsServerMessage::Data {
        data: json!({
            "imported": imported,
            "imported_ids": imported_ids,
            "errors": errors,
            "session_id": session_id
        }),
        request_id: None,
    })
}

async fn export_memories(
    params: Value,
    memory: &Arc<crate::memory::MemoryService>
) -> ApiResult<WsServerMessage> {
    let session_id = get_session_id(params["session_id"].as_str());
    let count = params["count"].as_u64().unwrap_or(1000) as usize;
    
    let memories = memory.get_recent_context(&session_id, count).await
        .map_err(|e| ApiError::internal(format!("Failed to export memories: {}", e)))?;
    
    info!("Exported {} memories for session {}", memories.len(), session_id);
    
    Ok(WsServerMessage::Data {
        data: json!({
            "memories": memories,
            "count": memories.len(),
            "session_id": session_id,
            "format": "json"
        }),
        request_id: None,
    })
}

async fn check_qdrant_status(app_state: Arc<AppState>) -> ApiResult<WsServerMessage> {
    let mut status = json!({
        "qdrant_url": CONFIG.qdrant_url.clone(),
        "qdrant_configured": !CONFIG.qdrant_url.is_empty(),
        "openai_embedding_key_configured": CONFIG.get_openai_key().is_some(),
        "embedding_heads": CONFIG.embed_heads.clone(),
        "collection_name": CONFIG.qdrant_collection.clone(),
    });
    
    // Test embedding generation
    match app_state.embedding_client.get_embedding("test").await {
        Ok(embedding) => {
            status["embedding_test"] = json!({
                "success": true,
                "dimension": embedding.len()
            });
        }
        Err(e) => {
            warn!("Embedding test failed: {}", e);
            status["embedding_test"] = json!({
                "success": false,
                "error": e.to_string()
            });
        }
    }
    
    Ok(WsServerMessage::Data {
        data: status,
        request_id: None,
    })
}
