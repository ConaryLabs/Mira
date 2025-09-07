// src/api/ws/chat_tools/mod.rs
// Organizes the modules related to tool execution in a WebSocket context.

pub mod executor;
pub mod message_handler;
pub mod prompt_builder;

// Re-export key components for easier access from other parts of the application.
pub use executor::ToolExecutor;
pub use message_handler::ToolMessageHandler;
pub use prompt_builder::ToolPromptBuilder;

use crate::api::ws::message::{WsClientMessage, MessageMetadata};
use crate::state::AppState;
use crate::config::CONFIG;
use anyhow::Result;
use axum::extract::ws::Message;
use futures_util::stream::SplitSink;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

/// Handles an incoming chat message by setting up the context and routing it to the tool handler.
pub async fn handle_chat_message_with_tools(
    content: String,
    project_id: Option<String>,
    metadata: Option<MessageMetadata>,
    app_state: Arc<AppState>,
    sender: Arc<Mutex<SplitSink<axum::extract::ws::WebSocket, Message>>>,
    session_id: String,
) -> Result<()> {
    info!("Processing tool-enabled chat message for session: {}", session_id);

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

    let executor = ToolExecutor::from_app_state(&app_state);

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
        Arc::new(executor),
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

/// A simplified router for WebSocket messages to dispatch to the tool handler.
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
        _ => {
            info!("Received non-chat message, ignoring in this handler.");
            Ok(())
        }
    }
}
