// src/api/ws/chat_tools/mod.rs
// PHASE 3 UPDATE: Enhanced tool executor creation with proper config management
// FIXED: Proper integration of custom config with AppState managers

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
    
    info!("Building context with parallel recall (history: {}, semantic: {})...", history_cap, vector_k);
    
    let context = app_state.memory_service.parallel_recall_context(
        &session_id,
        &content,
        history_cap,
        vector_k,
    )
    .await
    .unwrap_or_else(|e| {
        warn!("Failed to build context: {}. Using empty context.", e);
        crate::memory::recall::RecallContext { recent: vec![], semantic: vec![] }
    });

    // ENHANCED: Use from_app_state() to get all managers with proper integration
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

/// ENHANCED: Create tool executor using new from_app_state constructor
/// This ensures all Phase 3 managers are available with default configuration
pub fn create_tool_executor(app_state: &Arc<AppState>) -> Arc<ToolExecutor> {
    info!("Creating ToolExecutor with full Phase 3 integration");
    Arc::new(ToolExecutor::from_app_state(app_state))
}

/// ENHANCED: Create tool executor with custom config and full manager integration
/// FIXED: Now properly integrates custom config with all Phase 3 managers
pub fn create_tool_executor_with_config(
    app_state: &Arc<AppState>, 
    config: ToolConfig
) -> Arc<ToolExecutor> {
    info!("Creating ToolExecutor with custom config and full Phase 3 integration");
    
    // FIXED: Use the new from_app_state_with_config method for proper integration
    Arc::new(ToolExecutor::from_app_state_with_config(app_state, config))
}

/// ENHANCED: Create tool executor with builder pattern for maximum flexibility
pub struct ToolExecutorBuilder {
    app_state: Arc<AppState>,
    config: Option<ToolConfig>,
    enable_image_generation: bool,
    enable_file_search: bool,
}

impl ToolExecutorBuilder {
    /// Start building a new tool executor
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self {
            app_state,
            config: None,
            enable_image_generation: true,
            enable_file_search: true,
        }
    }
    
    /// Set custom configuration
    pub fn with_config(mut self, config: ToolConfig) -> Self {
        self.config = Some(config);
        self
    }
    
    /// Enable or disable image generation
    pub fn with_image_generation(mut self, enabled: bool) -> Self {
        self.enable_image_generation = enabled;
        self
    }
    
    /// Enable or disable file search
    pub fn with_file_search(mut self, enabled: bool) -> Self {
        self.enable_file_search = enabled;
        self
    }
    
    /// Build the final tool executor
    pub fn build(self) -> Arc<ToolExecutor> {
        let config = self.config.unwrap_or_default();
        
        info!(
            "Building ToolExecutor - model: {}, image_gen: {}, file_search: {}", 
            config.model,
            self.enable_image_generation,
            self.enable_file_search
        );
        
        // Create base executor with config and all managers
        let mut executor = ToolExecutor::from_app_state_with_config(&self.app_state, config);
        
        // Selectively disable managers if requested
        if !self.enable_image_generation {
            executor = ToolExecutor::with_config(
                self.app_state.responses_manager.clone(), 
                executor.get_config().clone()
            );
        }
        
        if self.enable_file_search && executor.file_search_service.is_none() {
            executor = executor.with_file_search_service(self.app_state.file_search_service.clone());
        }
        
        if self.enable_image_generation && executor.image_generation_manager.is_none() {
            executor = executor.with_image_generation_manager(self.app_state.image_generation_manager.clone());
        }
        
        Arc::new(executor)
    }
}

/// ENHANCED: Create tool executor with selective manager enabling
pub fn create_custom_tool_executor(
    app_state: &Arc<AppState>,
    enable_image_generation: bool,
    enable_file_search: bool,
) -> Arc<ToolExecutor> {
    ToolExecutorBuilder::new(app_state.clone())
        .with_image_generation(enable_image_generation)
        .with_file_search(enable_file_search)
        .build()
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

/// ENHANCED: Get detailed tool availability info
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

/// ENHANCED: Validate tool executor configuration
pub fn validate_tool_configuration(executor: &ToolExecutor) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    
    if let Err(e) = executor.validate_configuration() {
        return Err(e);
    }
    
    if executor.get_config().enable_tools && !CONFIG.enable_chat_tools {
        warnings.push("Tools enabled in executor config but disabled globally".to_string());
    }
    
    if CONFIG.enable_image_generation && executor.image_generation_manager.is_none() {
        warnings.push("Image generation enabled globally but manager not available in executor".to_string());
    }
    
    if CONFIG.enable_file_search && executor.file_search_service.is_none() {
        warnings.push("File search enabled globally but service not available in executor".to_string());
    }
    
    if !warnings.is_empty() {
        for warning in &warnings {
            warn!("Tool configuration warning: {}", warning);
        }
    }
    
    Ok(warnings)
}

/// ENHANCED: Create tool executor with configuration validation
pub fn create_validated_tool_executor(
    app_state: &Arc<AppState>,
    config: Option<ToolConfig>,
) -> Result<Arc<ToolExecutor>> {
    let executor = match config {
        Some(config) => create_tool_executor_with_config(app_state, config),
        None => create_tool_executor(app_state),
    };
    
    // Validate the configuration
    let warnings = validate_tool_configuration(&executor)?;
    
    if !warnings.is_empty() {
        info!("ToolExecutor created with {} configuration warnings", warnings.len());
    } else {
        info!("ToolExecutor created with valid configuration");
    }
    
    Ok(executor)
}
