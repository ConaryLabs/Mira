// src/api/ws/memory.rs
// WebSocket handlers for memory operations including save, search, context retrieval,
// pinning, import/export, and statistics.

use std::sync::Arc;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, error, info, warn};

use crate::{
    api::{
        error::{ApiError, ApiResult},
        ws::message::WsServerMessage,
    },
    config::CONFIG,
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
    #[allow(dead_code)]
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
        "memory.export" => export_memories(params, app_state).await,
        "memory.get_recent" => get_recent_memories(params, app_state).await,
        "memory.delete" => delete_memory(params, app_state).await,
        "memory.update_salience" => update_salience(params, app_state).await,
        "memory.get_stats" => get_memory_stats(params, app_state).await,
        "memory.check_qdrant" => check_qdrant_status(app_state).await,
        _ => Err(anyhow!("Unknown memory method: {}", method))
    };
    
    result.map_err(|e| {
        error!("Memory command error: {}", e);
        ApiError::internal(e.to_string())
    })
}

/// Saves a memory entry (user or assistant message)
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
    
    Ok(WsServerMessage::Data {
        data: json!({
            "success": true,
            "session_id": session_id,
            "message": format!("Memory saved for session {}", session_id)
        }),
        request_id: None,
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
    let min_salience = request.min_salience.unwrap_or(CONFIG.min_salience_for_qdrant);
    
    // Search with session ID parameter
    let search_results = match app_state.memory_service
        .search_similar(&session_id, &request.query, max_results * 2)
        .await
    {
        Ok(results) => {
            info!("Found {} results from semantic search", results.len());
            results
        }
        Err(e) => {
            warn!("Semantic search failed ({}), falling back to recent memories", e);
            app_state.memory_service
                .get_recent_context(&session_id, max_results)
                .await
                .unwrap_or_default()
        }
    };
    
    // Strict session isolation - only return memories from the requested session
    let filtered_results: Vec<_> = search_results.into_iter()
        .filter(|entry| entry.session_id == session_id)
        .filter(|entry| entry.salience.unwrap_or(0.0) >= min_salience)
        .take(max_results)
        .collect();
    
    debug!("Returning {} memories after filtering", filtered_results.len());
    
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
async fn pin_memory(params: Value, _app_state: Arc<AppState>) -> Result<WsServerMessage> {
    let request: PinMemoryRequest = serde_json::from_value(params)
        .map_err(|e| anyhow!("Invalid pin request: {}", e))?;
    
    info!("Pinning memory with id: {}, pinned: {}", request.memory_id, request.pinned);
    
    // TODO: Implement pin_memory in MemoryService
    warn!("Pin operation not yet implemented");
    
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

/// Imports multiple memories
async fn import_memories(params: Value, app_state: Arc<AppState>) -> Result<WsServerMessage> {
    let request: ImportMemoriesRequest = serde_json::from_value(params)
        .map_err(|e| anyhow!("Invalid import request: {}", e))?;
    
    let session_id = get_session_id(request.session_id);
    info!("Importing {} memories for session: {}", request.memories.len(), session_id);
    
    let mut imported_count = 0;
    let mut errors = Vec::new();
    
    for (idx, memory_data) in request.memories.into_iter().enumerate() {
        let save_params = json!({
            "session_id": session_id,
            "content": memory_data.content,
            "role": memory_data.role,
            "metadata": {
                "salience": memory_data.salience.unwrap_or(5.0),
                "tags": memory_data.tags.unwrap_or_default(),
                "memory_type": memory_data.memory_type.unwrap_or_else(|| "other".to_string()),
                "persona": "assistant",
                "mood": "neutral",
                "summary": format!("Imported memory {}", idx + 1)
            }
        });
        
        match save_memory(save_params, app_state.clone()).await {
            Ok(_) => imported_count += 1,
            Err(e) => errors.push(format!("Memory {}: {}", idx + 1, e))
        }
    }
    
    Ok(WsServerMessage::Data {
        data: json!({
            "imported": imported_count,
            "errors": errors,
            "session_id": session_id
        }),
        request_id: None,
    })
}

/// Exports memories for a session
async fn export_memories(params: Value, app_state: Arc<AppState>) -> Result<WsServerMessage> {
    let session_id = params["session_id"].as_str()
        .map(String::from)
        .unwrap_or_else(|| DEFAULT_SESSION.to_string());
    
    info!("Exporting memories for session: {}", session_id);
    
    let memories = app_state.memory_service
        .get_recent_context(&session_id, 1000)
        .await?;
    
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

/// Gets recent memories for a session
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
async fn delete_memory(params: Value, _app_state: Arc<AppState>) -> Result<WsServerMessage> {
    let request: DeleteMemoryRequest = serde_json::from_value(params)
        .map_err(|e| anyhow!("Invalid delete request: {}", e))?;
    
    info!("Deleting memory with id: {}", request.memory_id);
    
    // TODO: Implement delete_memory in MemoryService
    warn!("Delete operation not yet implemented");
    
    Ok(WsServerMessage::Data {
        data: json!({
            "memory_id": request.memory_id,
            "status": "pending_implementation"
        }),
        request_id: None,
    })
}

/// Updates the salience score of a memory
async fn update_salience(params: Value, _app_state: Arc<AppState>) -> Result<WsServerMessage> {
    let request: UpdateSalienceRequest = serde_json::from_value(params)
        .map_err(|e| anyhow!("Invalid salience update request: {}", e))?;
    
    info!("Updating salience for memory {}: {}", request.memory_id, request.salience);
    
    // TODO: Implement update_salience in MemoryService
    warn!("Salience update not yet implemented");
    
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
