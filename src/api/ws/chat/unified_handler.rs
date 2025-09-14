// src/api/ws/chat/unified_handler.rs

use std::sync::Arc;
use anyhow::Result;
use futures::Stream;
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::api::ws::message::MessageMetadata;
use crate::config::CONFIG;
use crate::llm::responses::thread::ThreadManager;
use crate::memory::recall::RecallContext;
use crate::persona::PersonaOverlay;
use crate::prompt::unified_builder::UnifiedPromptBuilder;
use crate::state::AppState;
use crate::tools::executor::ToolExecutor;

// Re-export ChatEvent from streaming module
pub use crate::llm::client::streaming::ChatEvent;

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub content: String,
    pub project_id: Option<String>,
    pub metadata: Option<MessageMetadata>,
    pub session_id: String,
    pub require_json: bool,
}

pub struct UnifiedChatHandler {
    app_state: Arc<AppState>,
    tool_executor: ToolExecutor,
    thread_manager: Arc<ThreadManager>,
}

impl UnifiedChatHandler {
    pub fn new(app_state: Arc<AppState>) -> Self {
        let thread_manager = Arc::new(ThreadManager::new(
            CONFIG.history_message_cap,
            CONFIG.history_token_limit,
        ));
        
        Self {
            app_state,
            tool_executor: ToolExecutor::new(),
            thread_manager,
        }
    }
    
    pub async fn handle_message(
        &self,
        request: ChatRequest,
    ) -> Result<impl Stream<Item = Result<ChatEvent>> + Send> {
        let use_tools = self.tool_executor.should_use_tools(&request.metadata);
        
        if use_tools {
            info!("Processing tool-enabled chat for session: {}", request.session_id);
        } else {
            info!("Processing simple chat message: {}", 
                request.content.chars().take(80).collect::<String>());
        }
        
        // Build context from memory
        let context = self.build_context(&request.session_id, &request.content).await?;
        debug!("Context built: {} recent, {} semantic", 
            context.recent.len(), 
            context.semantic.len()
        );
        
        // Get tools if enabled
        let tools = if use_tools {
            Some(crate::tools::definitions::get_enabled_tools())
        } else {
            None
        };
        debug!("Tools enabled: {} (found {} tools)", 
            use_tools, 
            tools.as_ref().map_or(0, |t| t.len())
        );
        
        // Build system prompt
        let persona = self.select_persona(&request.metadata);
        let system_prompt = UnifiedPromptBuilder::build_system_prompt(
            &persona,
            &context,
            tools.as_deref(),
            request.metadata.as_ref(),
            request.project_id.as_deref(),
            request.require_json,
        );
        debug!("System prompt built: {} chars", system_prompt.len());
        
        // Build request body for GPT-5
        let request_body = self.build_gpt5_request(
            request.content.clone(),
            system_prompt,
            tools,
            request.session_id.clone(),
        ).await?;
        
        // Save user message to memory
        if let Err(e) = self.app_state.memory_service.save_user_message(
            &request.session_id,
            &request.content,
            request.project_id.as_deref()
        ).await {
            warn!("Failed to save user message to memory: {}", e);
        }
        
        // Create the stream
        info!("Creating response stream for session: {}", request.session_id);
        let stream = self.app_state.llm_client
            .post_response_stream(request_body)
            .await?;
        info!("Response stream created successfully");
        
        // Process through the streaming module
        info!("Processing stream events...");
        let event_stream = crate::llm::client::streaming::process_gpt5_stream(
            stream,
            use_tools,
            request.session_id,
            self.app_state.clone(),
            request.project_id,
        );
        
        Ok(Box::pin(event_stream))
    }
    
    async fn build_context(&self, session_id: &str, content: &str) -> Result<RecallContext> {
        self.app_state.memory_service.parallel_recall_context(
            session_id,
            content,
            CONFIG.context_recent_messages,
            CONFIG.context_semantic_matches,
        ).await
    }
    
    fn select_persona(&self, _metadata: &Option<MessageMetadata>) -> PersonaOverlay {
        PersonaOverlay::Default
    }
    
    async fn build_gpt5_request(
        &self,
        user_content: String,
        system_prompt: String,
        tools: Option<Vec<crate::llm::responses::types::Tool>>,
        session_id: String,
    ) -> Result<Value> {
        // Build the input messages
        let input = vec![
            json!({
                "role": "system",
                "content": [{
                    "type": "input_text",
                    "text": system_prompt
                }]
            }),
            json!({
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": user_content
                }]
            })
        ];
        
        // Build the request body
        let mut request_body = json!({
            "model": CONFIG.gpt5_model,
            "input": input,
            "stream": true,
            "instructions": "Respond helpfully using available tools when appropriate.",
            "max_output_tokens": CONFIG.max_output_tokens,
            "text": {
                "verbosity": CONFIG.verbosity
            },
            "reasoning": {
                "effort": CONFIG.reasoning_effort
            }
        });
        
        // Format tools for GPT-5 if provided
        if let Some(_tool_list) = tools {
            let mut valid_tools = Vec::new();
            
            // Only add web_search if it's actually enabled
            // GPT-5 wants just {"type": "web_search"} - no nested object
            if CONFIG.enable_web_search {
                valid_tools.push(json!({
                    "type": "web_search"
                }));
                info!("Added web_search tool to request");
            }
            
            // Add other built-in tools that don't require special setup
            // Note: code_interpreter requires container management, so we skip it
            
            // Only set tools if we have at least one valid tool
            if !valid_tools.is_empty() {
                request_body["tools"] = json!(valid_tools);
                request_body["tool_choice"] = json!("auto");
                info!("Sending {} tools with request", valid_tools.len());
            }
        }
        
        // Add previous response ID for thread continuity
        let previous_response_id = self.thread_manager
            .get_previous_response_id(&session_id)
            .await;
        
        if let Some(prev_id) = previous_response_id {
            request_body["previous_response_id"] = json!(prev_id);
            debug!("Using previous_response_id: {}", prev_id);
        }
        
        Ok(request_body)
    }
}

impl Clone for ToolExecutor {
    fn clone(&self) -> Self {
        Self::new()
    }
}
