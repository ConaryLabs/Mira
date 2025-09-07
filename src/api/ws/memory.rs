// src/api/ws/memory.rs
// WebSocket handlers for memory operations including save, search, context retrieval,
// pinning, import/export, and statistics.
// UPDATED: Return Data messages instead of Status for request_id support

use std::sync::Arc;
use std::str::FromStr;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, error, info, warn};

use crate::{
    api::error::{ApiError, ApiResult},
    api::ws::message::WsServerMessage,
    memory::types::{MemoryEntry, MemoryType},
    state::AppState,
};

// Default session ID for single-user mode
const DEFAULT_SESSION: &str = "peter-eternal";

// Request types for memory operations
#[derive(Debug, Deserialize)]
struct SaveMemoryRequest {
    session_id: Option<String>,
    content: String,
    project_id: Option<String>,
    role: Option<String>,
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct SearchMemoryRequest {
    session_id: Option<String>,
    query: String,
    max_results: Option<usize>,
    min_salience: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct GetContextRequest {
    session_id: Option<String>,
    user_text: Option<String>,
    recent_count: Option<usize>,
    semantic_count: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct PinMemoryRequest {
    memory_id: i64,
    pinned: bool,
}

#[derive(Debug, Deserialize)]
struct ImportMemoriesRequest {
    session_id: Option<String>,
    memories: Vec<MemoryImportData>,
}

#[derive(Debug, Deserialize)]
struct MemoryImportData {
    content: String,
    role: String,
    timestamp: Option<String>,
    salience: Option<f32>,
    tags: Option<Vec<String>>,
    memory_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GetRecentRequest {
    session_id: Option<String>,
    count: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct DeleteMemoryRequest {
    memory_id: i64,
}

#[derive(Debug, Deserialize)]
struct UpdateSalienceRequest {
    memory_id: i64,
    salience: f32,
}

#[derive(Debug, Deserialize)]
struct GetStatsRequest {
    session_id: Option<String>,
}

// Make MemoryServiceStats serializable
#[derive(Debug, Clone, Serialize)]
struct SerializableMemoryStats {
    total_messages: usize,
    recent_messages: usize,
    semantic_entries: usize,
    code_entries: usize,
    summary_entries: usize,
}

/// Returns the session ID, defaulting to the eternal session for single-user mode
fn get_session_id(session_id: Option<String>) -> String {
    session_id.unwrap_or_else(|| DEFAULT_SESSION.to_string())
}

/// Routes memory commands to appropriate handlers
pub async fn handle_memory_command(
    method: &str,
    params: Value,
    app_state: Arc<AppState>,
) -> ApiResult<WsServerMessage> {
    info!("Processing memory command: {}", method);
    debug!("Parameters: {:?}", params);
    
    let result = match method {
        "memory.save" => save_memory(params, app_state).await,
        "memory.search" => search_memory(params, app_state).await,
        "memory.get_context" => get_context(params, app_state).await,
        "memory.pin" => pin_memory(params, app_state).await,
        "memory.unpin" => unpin_memory(params, app_state).await,
        "memory.import" => import_memories(params, app_state).await,
        "memory.get_recent" => get_recent_memories(params, app_state).await,
        "memory.delete" => delete_memory(params, app_state).await,
        "memory.update_salience" => update_salience(params, app_state).await,
        "memory.get_stats" => get_memory_stats(params, app_state).await,
        "memory.check_status" => check_qdrant_status(app_state).await,
        _ => {
            error!("Unknown memory method: {}", method);
            return Err(ApiError::bad_request(format!("Unknown memory method: {}", method)));
        }
    };
    
    result.map_err(|e| {
        error!("Memory command {} failed: {}", method, e);
        ApiError::internal(format!("Memory operation failed: {}", e))
    })
}

/// Saves a user or assistant message to memory
async fn save_memory(params: Value, app_state: Arc<AppState>) -> Result<WsServerMessage> {
    let request: SaveMemoryRequest = serde_json::from_value(params)
        .map_err(|e| anyhow!("Invalid save memory request: {}", e))?;
    
    let session_id = get_session_id(request.session_id);
    debug!("Saving memory for session: {}", session_id);
    
    let role = request.role.as_deref().unwrap_or("user");
    
    match role {
        "user" => {
            app_state.memory_service
                .save_user_message(
                    &session_id,
                    &request.content,
                    request.project_id.as_deref()
                )
                .await?;
            
            info!("Saved user message for session: {}", session_id);
        }
        "assistant" => {
            if let Some(metadata) = request.metadata {
                use crate::services::chat::ChatResponse;
                
                let response = ChatResponse {
                    output: request.content.clone(),
                    persona: metadata["persona"].as_str().unwrap_or("assistant").to_string(),
                    mood: metadata["mood"].as_str().unwrap_or("neutral").to_string(),
                    salience: metadata["salience"].as_u64().unwrap_or(5) as usize,
                    summary: metadata["summary"].as_str().unwrap_or(&request.content).to_string(),
                    memory_type: metadata["memory_type"].as_str().unwrap_or("other").to_string(),
                    tags: metadata["tags"]
                        .as_array()
                        .map(|arr| arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect())
                        .unwrap_or_default(),
                    intent: metadata["intent"].as_str().map(String::from),
                    monologue: metadata["monologue"].as_str().map(String::from),
                    reasoning_summary: metadata["reasoning_summary"].as_str().map(String::from),
                };
                
                app_state.memory_service
                    .save_assistant_response(&session_id, &response)
                    .await?;
                
                info!("Saved assistant response for session: {}", session_id);
            } else {
                return Err(anyhow!("Assistant messages require metadata"));
            }
        }
        _ => return Err(anyhow!("Invalid role: {}. Must be 'user' or 'assistant'", role))
    }
    
    // Return Data message with request_id support
    Ok(WsServerMessage::Data {
        data: json!({
            "success": true,
            "session_id": session_id,
            "message": format!("Memory saved for session {}", session_id)
        }),
        request_id: None, // Will be filled by router
    })
}

/// Performs semantic search across memories
async fn search_memory(params: Value, app_state: Arc<AppState>) -> Result<WsServerMessage> {
    let request: SearchMemoryRequest = serde_json::from_value(params)
        .map_err(|e| anyhow!("Invalid search request: {}", e))?;
    
    let session_id = get_session_id(request.session_id);
    info!("Searching memories for session: {} with query: {}", 
          session_id, request.query);
    
    let max_results = request.max_results.unwrap_or(10);
    let min_salience = request.min_salience.unwrap_or(0.0);
    
    // Try semantic search first
    let search_results = match app_state.memory_service
        .search_similar(&request.query, max_results * 2)
        .await
    {
        Ok(results) => {
            info!("Found {} results from semantic search", results.len());
            results
        }
        Err(e) => {
            warn!("Semantic search failed ({}), falling back to recent memories", e);
            // Fallback: just get recent memories if semantic search fails
            app_state.memory_service
                .get_recent_context(&session_id, max_results)
                .await
                .unwrap_or_default()
        }
    };
    
    // Filter results
    let filtered_results: Vec<_> = search_results.into_iter()
        .filter(|entry| {
            // Include memories from the current session
            entry.session_id == session_id || 
            // Or memories with high salience from any session
            entry.salience.unwrap_or(0.0) >= min_salience
        })
        .take(max_results)
        .collect();
    
    debug!("Returning {} memories after filtering", filtered_results.len());
    
    // Return Data message
    Ok(WsServerMessage::Data {
        data: json!({
            "memories": filtered_results,
            "count": filtered_results.len(),
            "session_id": session_id
        }),
        request_id: None,
    })
}

/// Builds conversation context with recent and semantically relevant memories
async fn get_context(params: Value, app_state: Arc<AppState>) -> Result<WsServerMessage> {
    let request: GetContextRequest = serde_json::from_value(params)
        .map_err(|e| anyhow!("Invalid context request: {}", e))?;
    
    let session_id = get_session_id(request.session_id);
    info!("Building context for session: {}", session_id);
    
    let recent_count = request.recent_count.unwrap_or(10);
    let semantic_count = request.semantic_count.unwrap_or(5);
    
    let context = if let Some(user_text) = request.user_text {
        app_state.memory_service
            .parallel_recall_context(
                &session_id,
                &user_text,
                recent_count,
                semantic_count
            )
            .await?
    } else {
        let recent = app_state.memory_service
            .get_recent_context(&session_id, recent_count)
            .await?;
        
        crate::memory::recall::RecallContext::new(recent, Vec::new())
    };
    
    debug!("Built context with {} recent and {} semantic memories", 
           context.recent.len(), context.semantic.len());
    
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

/// Pins a memory to prevent decay
async fn pin_memory(params: Value, app_state: Arc<AppState>) -> Result<WsServerMessage> {
    let request: PinMemoryRequest = serde_json::from_value(params)
        .map_err(|e| anyhow!("Invalid pin request: {}", e))?;
    
    info!("Pinning memory with id: {}, pinned: {}", request.memory_id, request.pinned);
    
    // This would require adding a pin_memory method to MemoryService
    // For now, return a placeholder response
    warn!("Pin operation requires adding pin_memory method to MemoryService");
    
    Ok(WsServerMessage::Data {
        data: json!({
            "memory_id": request.memory_id,
            "pinned": request.pinned,
            "status": "pending_implementation"
        }),
        request_id: None,
    })
}

/// Unpins a memory
async fn unpin_memory(params: Value, app_state: Arc<AppState>) -> Result<WsServerMessage> {
    let memory_id = params["memory_id"].as_i64()
        .ok_or_else(|| anyhow!("memory_id is required"))?;
    
    let unpin_params = json!({
        "memory_id": memory_id,
        "pinned": false
    });
    
    pin_memory(unpin_params, app_state).await
}

// Continue with remaining functions updated to use Data messages...

/// Imports multiple memories in batch
async fn import_memories(params: Value, app_state: Arc<AppState>) -> Result<WsServerMessage> {
    let request: ImportMemoriesRequest = serde_json::from_value(params)
        .map_err(|e| anyhow!("Invalid import request: {}", e))?;
    
    let session_id = get_session_id(request.session_id);
    info!("Importing {} memories for session: {}", 
          request.memories.len(), session_id);
    
    let mut imported_count = 0;
    let mut failed_count = 0;
    let mut errors = Vec::new();
    
    for (idx, memory_data) in request.memories.iter().enumerate() {
        match app_state.memory_service
            .save_user_message(&session_id, &memory_data.content, None)
            .await
        {
            Ok(_) => imported_count += 1,
            Err(e) => {
                failed_count += 1;
                errors.push(format!("Memory {}: {}", idx, e));
                warn!("Failed to import memory {}: {}", idx, e);
            }
        }
    }
    
    let success = failed_count == 0;
    
    Ok(WsServerMessage::Data {
        data: json!({
            "success": success,
            "imported": imported_count,
            "failed": failed_count,
            "session_id": session_id,
            "errors": if !errors.is_empty() { Some(errors) } else { None }
        }),
        request_id: None,
    })
}

/// Retrieves recent memories for a session
async fn get_recent_memories(params: Value, app_state: Arc<AppState>) -> Result<WsServerMessage> {
    let request: GetRecentRequest = serde_json::from_value(params)
        .map_err(|e| anyhow!("Invalid get recent request: {}", e))?;
    
    let session_id = get_session_id(request.session_id);
    let count = request.count.unwrap_or(20);
    
    info!("Getting {} recent memories for session: {}", count, session_id);
    
    let memories = app_state.memory_service
        .get_recent_context(&session_id, count)
        .await?;
    
    Ok(WsServerMessage::Data {
        data: json!({
            "memories": memories,
            "count": memories.len(),
            "session_id": session_id
        }),
        request_id: None,
    })
}

/// Deletes a memory by ID
async fn delete_memory(params: Value, app_state: Arc<AppState>) -> Result<WsServerMessage> {
    let request: DeleteMemoryRequest = serde_json::from_value(params)
        .map_err(|e| anyhow!("Invalid delete request: {}", e))?;
    
    info!("Deleting memory with id: {}", request.memory_id);
    
    warn!("Delete operation requires adding delete_memory method to MemoryService");
    
    Ok(WsServerMessage::Data {
        data: json!({
            "memory_id": request.memory_id,
            "status": "pending_implementation"
        }),
        request_id: None,
    })
}

/// Updates the salience score of a memory
async fn update_salience(params: Value, app_state: Arc<AppState>) -> Result<WsServerMessage> {
    let request: UpdateSalienceRequest = serde_json::from_value(params)
        .map_err(|e| anyhow!("Invalid salience update request: {}", e))?;
    
    info!("Updating salience for memory {}: {}", request.memory_id, request.salience);
    
    warn!("Salience update requires adding update_salience method to MemoryService");
    
    Ok(WsServerMessage::Data {
        data: json!({
            "memory_id": request.memory_id,
            "new_salience": request.salience,
            "status": "pending_implementation"
        }),
        request_id: None,
    })
}

/// Retrieves memory statistics for a session
async fn get_memory_stats(params: Value, app_state: Arc<AppState>) -> Result<WsServerMessage> {
    let request: GetStatsRequest = serde_json::from_value(params)
        .map_err(|e| anyhow!("Invalid stats request: {}", e))?;
    
    let session_id = get_session_id(request.session_id);
    info!("Getting memory stats for session: {}", session_id);
    
    let stats = app_state.memory_service
        .get_service_stats(&session_id)
        .await?;
    
    let serializable_stats = SerializableMemoryStats {
        total_messages: stats.total_messages,
        recent_messages: stats.recent_messages,
        semantic_entries: stats.semantic_entries,
        code_entries: stats.code_entries,
        summary_entries: stats.summary_entries,
    };
    
    Ok(WsServerMessage::Data {
        data: json!({
            "session_id": session_id,
            "stats": serializable_stats
        }),
        request_id: None,
    })
}

/// Debug function to check if Qdrant is properly configured
async fn check_qdrant_status(app_state: Arc<AppState>) -> Result<WsServerMessage> {
    use crate::config::CONFIG;
    
    let mut status = json!({
        "qdrant_url": CONFIG.qdrant_url.clone(),
        "qdrant_configured": !CONFIG.qdrant_url.is_empty(),
        "openai_key_configured": CONFIG.openai_api_key.is_some(),
        "embedding_heads": CONFIG.embed_heads.clone(),
        "collection_name": CONFIG.qdrant_collection.clone(),
    });
    
    // Try to generate a test embedding
    match app_state.llm_client.get_embedding("test").await {
        Ok(embedding) => {
            status["embedding_test"] = json!({
                "success": true,
                "dimension": embedding.len()
            });
        }
        Err(e) => {
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
