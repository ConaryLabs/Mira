// src/api/ws/chat_tools/mod.rs
// Orchestrates tool-enhanced chat functionality for WebSocket connections.
// Manages the flow from message receipt through tool execution to response delivery.

use crate::api::ws::message::{WsClientMessage, MessageMetadata};
use crate::state::AppState;
use crate::config::CONFIG;
use anyhow::Result;
use axum::extract::ws::Message;
use futures_util::stream::SplitSink;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

// Import the tools we need
use crate::tools::executor::ToolExecutor;
use crate::tools::prompt_builder::ToolPromptBuilder;
use crate::tools::message_handler::ToolMessageHandler;
use crate::tools::definitions::get_enabled_tools;

/// Main entry point for processing tool-enhanced chat messages.
/// Coordinates memory storage, context building, and tool execution.
pub async fn handle_chat_message_with_tools(
    content: String,
    project_id: Option<String>,
    metadata: Option<MessageMetadata>,
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<axum::extract::ws::WebSocket, Message>>>,
    session_id: String,
) -> Result<()> {
    info!("Processing tool-enabled chat message for session: {}", session_id);
    
    // Log project context if present
    if let Some(ref pid) = project_id {
        info!("Active project context: {}", pid);
    }

    // Save the user's message to memory for context persistence
    if let Err(e) = app_state
        .memory_service
        .save_user_message(&session_id, &content, project_id.as_deref())
        .await
    {
        warn!("Failed to save user message: {}", e);
    }

    info!(
        "Building context with parallel recall (history: {}, semantic: {})...",
        CONFIG.ws_history_cap, CONFIG.ws_vector_search_k
    );
    
    // Retrieve relevant conversation context from memory
    let context = app_state.memory_service.parallel_recall_context(
        &session_id,
        &content,
        CONFIG.ws_history_cap,
        CONFIG.ws_vector_search_k,
    )
    .await
    .unwrap_or_else(|e| {
        warn!("Failed to build context: {}. Using empty context.", e);
        crate::memory::recall::RecallContext::default()
    });

    // Initialize the tool executor with available services
    let executor = ToolExecutor::from_app_state(&app_state);

    // Build system prompt with tool descriptions and project context
    let system_prompt = ToolPromptBuilder::build_tool_aware_system_prompt(
        &context,
        &get_enabled_tools(),
        metadata.as_ref(),
        project_id.as_deref(),  // Pass project context for LLM awareness
    );

    // Create WebSocket connection handler for response streaming
    let connection = Arc::new(crate::api::ws::chat::connection::WebSocketConnection::new_with_parts(
        sender,
        Arc::new(Mutex::new(std::time::Instant::now())),
        Arc::new(Mutex::new(false)),
        Arc::new(Mutex::new(std::time::Instant::now())),
    ));

    // Initialize message handler with executor and connection
    let message_handler = ToolMessageHandler::new(
        Arc::new(executor),
        connection,
        app_state.clone(),
    );

    // Process the message with tool capabilities
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

/// Routes incoming WebSocket messages to appropriate tool handlers.
/// Provides a flexible routing layer for different message types.
pub async fn route_tool_message(
    msg: WsClientMessage,
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<axum::extract::ws::WebSocket, Message>>>,
) -> Result<()> {
    match msg {
        WsClientMessage::Chat { content, project_id, metadata } => {
            // Generate a unique session identifier for this conversation
            let session_id = format!("tool-session-{}", uuid::Uuid::new_v4());
            
            // Log project context for debugging
            if let Some(ref pid) = project_id {
                info!("Routing tool message with project context: {}", pid);
            }
            
            handle_chat_message_with_tools(
                content,
                project_id,
                metadata,
                app_state,
                sender,
                session_id,
            ).await
        }
        _ => {
            warn!("Unsupported message type for tool router");
            Ok(())
        }
    }
}
