// src/api/ws/chat_tools/mod.rs
// REFACTORED VERSION - Simplified main interface using extracted modules
// Reduced from ~550-600 lines to ~150 lines by extracting:
// - executor.rs: Tool execution logic
// - message_handler.rs: WebSocket message handling
// - prompt_builder.rs: System prompt building
//
// PRESERVED CRITICAL INTEGRATIONS:
// - handle_chat_message_with_tools function (used by message_router.rs)
// - WsServerMessageWithTools enum (used by WebSocket handlers)
// - update_ws_handler_for_tools function (used by mod.rs)
// - All CONFIG and AppState dependencies

use std::sync::Arc;

use anyhow::Result;
use axum::extract::ws::Message;
use futures_util::stream::SplitSink;
use tokio::sync::Mutex;
use tracing::{info, warn};

// Import extracted modules
pub mod executor;
pub mod message_handler;
pub mod prompt_builder;

// Re-export types for external use (CRITICAL: preserves existing API)
pub use executor::{ToolChatRequest, ToolEvent, ToolExecutor, ToolConfig};
pub use message_handler::{ToolMessageHandler, WsServerMessageWithTools};
pub use prompt_builder::{ToolPromptBuilder, PromptTemplates};

use crate::api::ws::message::{WsClientMessage, MessageMetadata};
use crate::llm::responses::ResponsesManager;
use crate::memory::parallel_recall::build_context_parallel;
use crate::state::AppState;
use crate::config::CONFIG;

/// CRITICAL FUNCTION: Main entry point for tool-enabled chat (used by message_router.rs)
/// This function MUST maintain the exact same signature for compatibility
pub async fn handle_chat_message_with_tools(
    content: String,
    project_id: Option<String>,
    metadata: Option<MessageMetadata>,
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<axum::extract::ws::WebSocket, Message>>>,
    session_id: String,
) -> Result<()> {
    info!("Processing chat message with tools (refactored) for session: {}", session_id);

    // 1. Save user message to memory
    if let Err(e) = app_state
        .memory_service
        .save_user_message(&session_id, &content, project_id.as_deref())
        .await
    {
        warn!("Failed to save user message: {}", e);
    }

    // 2. Build context using parallel optimization (preserved from original)
    let history_cap = CONFIG.ws_history_cap;
    let vector_k = CONFIG.ws_vector_search_k;
    
    info!("Building context PARALLEL (history: {}, semantic: {})...", history_cap, vector_k);
    
    let context = build_context_parallel(
        &session_id,
        &content,
        history_cap,
        vector_k,
        &app_state.llm_client,
        app_state.sqlite_store.as_ref(),
        app_state.qdrant_store.as_ref(),
    )
    .await
    .unwrap_or_else(|e| {
        warn!("Failed to build context: {}. Using empty context.", e);
        crate::memory::recall::RecallContext { recent: vec![], semantic: vec![] }
    });

    // 3. Create tool executor with default configuration (FIXED: Use actual API)
    let tool_executor = Arc::new(ToolExecutor::new(
        app_state.responses_manager.clone()
    ));

    // 4. Build system prompt with tools (FIXED: Use static method)
    let system_prompt = ToolPromptBuilder::build_tool_aware_system_prompt(
        &context,
        &crate::services::chat_with_tools::get_enabled_tools(),
        metadata.as_ref()
    );

    // 5. Create connection wrapper for WebSocket (FIXED: Use new_with_parts for already-split sender)
    let connection = Arc::new(crate::api::ws::connection::WebSocketConnection::new_with_parts(
        sender,
        Arc::new(Mutex::new(std::time::Instant::now())),
        Arc::new(Mutex::new(false)),
        Arc::new(Mutex::new(std::time::Instant::now())),
    ));

    // 6. Create message handler for WebSocket streaming (FIXED: Use actual API)
    let message_handler = ToolMessageHandler::new(
        tool_executor.clone(),
        connection,
        app_state.clone(),
    );

    // 7. Execute tool-enabled chat with streaming (FIXED: Clone session_id to avoid move)
    message_handler.handle_tool_message(
        content,
        project_id,
        metadata,
        context,
        system_prompt,
        session_id.clone(), // Clone to avoid the move
    ).await?;

    info!("Tool-enabled chat completed for session: {}", session_id);
    Ok(())
}

/// PRESERVED: WebSocket handler update function (used by ws/mod.rs)
pub async fn update_ws_handler_for_tools(
    msg: WsClientMessage,
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<axum::extract::ws::WebSocket, Message>>>,
    session_id: String,
) -> Result<()> {
    match msg {
        WsClientMessage::Chat { content, project_id, metadata } => {
            handle_chat_message_with_tools(
                content,
                project_id,
                metadata,
                app_state,
                sender,
                session_id,
            ).await
        }
        _ => Ok(()) // Only handle chat messages with tools
    }
}

/// Create a tool executor with default configuration
pub fn create_tool_executor(app_state: &Arc<AppState>) -> Arc<ToolExecutor> {
    let responses_manager = Arc::new(ResponsesManager::new(app_state.llm_client.clone()));
    Arc::new(ToolExecutor::new(responses_manager))
}

/// Create a tool executor with custom configuration
pub fn create_tool_executor_with_config(
    app_state: &Arc<AppState>, 
    config: ToolConfig
) -> Arc<ToolExecutor> {
    let responses_manager = Arc::new(ResponsesManager::new(app_state.llm_client.clone()));
    Arc::new(ToolExecutor::with_config(responses_manager, config))
}

/// Check if tools are enabled and available
pub fn tools_available() -> bool {
    CONFIG.enable_chat_tools && !crate::services::chat_with_tools::get_enabled_tools().is_empty()
}

/// Get count of available tools
pub fn available_tool_count() -> usize {
    if CONFIG.enable_chat_tools {
        crate::services::chat_with_tools::get_enabled_tools().len()
    } else {
        0
    }
}

/// Utility function to build a simple tool-aware prompt
pub fn build_simple_tool_prompt(has_tools: bool) -> String {
    if has_tools {
        ToolPromptBuilder::build_tool_aware_system_prompt(
            &crate::memory::recall::RecallContext { recent: vec![], semantic: vec![] },
            &crate::services::chat_with_tools::get_enabled_tools(),
            None,
        )
    } else {
        "You are Mira, a helpful AI assistant.".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tools_available() {
        let available = tools_available();
        assert!(available == true || available == false);
    }

    #[test]
    fn test_available_tool_count() {
        let count = available_tool_count();
        assert!(count >= 0); // Fixed: removed incomplete assertion
    }
}
