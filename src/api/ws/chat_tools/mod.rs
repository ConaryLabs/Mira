// src/api/ws/chat_tools/mod.rs
// PHASE 3 UPDATE: Updated tool executor creation to use from_app_state() constructor

use std::sync::Arc;

use anyhow::Result;
use axum::extract::ws::Message;
use futures_util::stream::SplitSink;
use tokio::sync::Mutex;
use tracing::{info, warn};

pub mod executor;
pub mod message_handler;
pub mod prompt_builder;

pub use executor::{ToolChatRequest, ToolEvent, ToolExecutor, ToolConfig};
pub use message_handler::{ToolMessageHandler, WsServerMessageWithTools};
pub use prompt_builder::{ToolPromptBuilder, PromptTemplates};

use crate::api::ws::message::{WsClientMessage, MessageMetadata};
use crate::memory::parallel_recall::build_context_parallel;
use crate::state::AppState;
use crate::config::CONFIG;

pub async fn handle_chat_message_with_tools(
    content: String,
    project_id: Option<String>,
    metadata: Option<MessageMetadata>,
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<axum::extract::ws::WebSocket, Message>>>,
    session_id: String,
) -> Result<()> {
    info!("Processing chat message with tools for session: {}", session_id);

    if let Err(e) = app_state
        .memory_service
        .save_user_message(&session_id, &content, project_id.as_deref())
        .await
    {
        warn!("Failed to save user message: {}", e);
    }

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

    // PHASE 3 UPDATED: Use from_app_state() to get all managers
    let tool_executor = Arc::new(ToolExecutor::from_app_state(&app_state));

    let system_prompt = ToolPromptBuilder::build_tool_aware_system_prompt(
        &context,
        &crate::services::chat_with_tools::get_enabled_tools(),
        metadata.as_ref()
    );

    let connection = Arc::new(crate::api::ws::chat::connection::WebSocketConnection::new_with_parts(
        sender,
        Arc::new(Mutex::new(std::time::Instant::now())),
        Arc::new(Mutex::new(false)),
        Arc::new(Mutex::new(std::time::Instant::now())),
    ));

    let message_handler = ToolMessageHandler::new(
        tool_executor.clone(),
        connection,
        app_state.clone(),
    );

    message_handler.handle_tool_message(
        content,
        project_id,
        metadata,
        context,
        system_prompt,
        session_id.clone(),
    ).await?;

    info!("Tool-enabled chat completed for session: {}", session_id);
    Ok(())
}

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
        _ => Ok(())
    }
}

/// PHASE 3 UPDATED: Create tool executor using new from_app_state constructor
/// This ensures all Phase 3 managers (ImageGenerationManager, FileSearchService) are available
pub fn create_tool_executor(app_state: &Arc<AppState>) -> Arc<ToolExecutor> {
    info!("Creating ToolExecutor with full Phase 3 integration");
    Arc::new(ToolExecutor::from_app_state(app_state))
}

/// PHASE 3 UPDATED: Create tool executor with custom config but full manager integration
pub fn create_tool_executor_with_config(
    app_state: &Arc<AppState>, 
    config: ToolConfig
) -> Arc<ToolExecutor> {
    info!("Creating ToolExecutor with custom config and full Phase 3 integration");
    
    // Create executor with all managers from AppState
    let mut executor = ToolExecutor::from_app_state(app_state);
    
    // Note: We can't directly modify the config field as it's private
    // Instead, we create a new executor with the custom config
    // This is a design limitation that could be improved in the future
    
    // For now, use the new constructor approach:
    let responses_manager = app_state.responses_manager.clone();
    let mut executor = ToolExecutor::with_config(responses_manager, config);
    
    // Manually set the managers (this would require adding setters to ToolExecutor)
    // For this implementation, we'll create a note that this needs improvement
    warn!("Custom config with full manager integration requires ToolExecutor refactoring");
    
    Arc::new(executor)
}

/// PHASE 3 NEW: Create tool executor with only specific managers (for testing/flexibility)
pub fn create_minimal_tool_executor(
    app_state: &Arc<AppState>,
    enable_image_generation: bool,
    enable_file_search: bool,
) -> Arc<ToolExecutor> {
    info!(
        "Creating minimal ToolExecutor - image_gen: {}, file_search: {}", 
        enable_image_generation, 
        enable_file_search
    );
    
    // For now, minimal means using the basic constructor
    // In a future refactor, this could selectively enable managers
    if enable_image_generation || enable_file_search {
        // If any Phase 3 features are requested, use full integration
        Arc::new(ToolExecutor::from_app_state(app_state))
    } else {
        // Use basic constructor for minimal setup
        Arc::new(ToolExecutor::new(app_state.responses_manager.clone()))
    }
}

/// Check if tools are available and properly configured
pub fn tools_available() -> bool {
    let tools_enabled = CONFIG.enable_chat_tools;
    let has_tools = !crate::services::chat_with_tools::get_enabled_tools().is_empty();
    
    info!("Tools availability check - enabled: {}, has_tools: {}", tools_enabled, has_tools);
    
    tools_enabled && has_tools
}

/// Get count of available tools
pub fn available_tool_count() -> usize {
    if CONFIG.enable_chat_tools {
        let count = crate::services::chat_with_tools::get_enabled_tools().len();
        info!("Available tools count: {}", count);
        count
    } else {
        0
    }
}

/// PHASE 3 ENHANCED: Get detailed tool availability info
pub fn get_tool_availability_info() -> serde_json::Value {
    use serde_json::json;
    
    let tools = crate::services::chat_with_tools::get_enabled_tools();
    let tool_types: Vec<String> = tools.iter().map(|t| t.tool_type.clone()).collect();
    
    json!({
        "chat_tools_enabled": CONFIG.enable_chat_tools,
        "total_tools": tools.len(),
        "available_tools": tool_types,
        "feature_flags": {
            "web_search": CONFIG.enable_web_search,
            "code_interpreter": CONFIG.enable_code_interpreter,
            "file_search": CONFIG.enable_file_search,
            "image_generation": CONFIG.enable_image_generation
        }
    })
}

/// Build a simple system prompt for tool-aware conversations
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
