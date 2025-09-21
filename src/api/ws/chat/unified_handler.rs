// src/api/ws/chat/unified_handler.rs
// Updated to use structured responses instead of streaming

use std::sync::Arc;
use anyhow::Result;
use futures::Stream;
use serde_json::json;
use tracing::{debug, info, warn};

use crate::api::ws::message::MessageMetadata;
use crate::config::CONFIG;
use crate::llm::responses::thread::ThreadManager;
use crate::llm::structured::CompleteResponse; // NEW: Import structured response types
use crate::memory::RecallContext;
use crate::persona::PersonaOverlay;
use crate::prompt::unified_builder::UnifiedPromptBuilder;
use crate::state::AppState;
use crate::tools::executor::ToolExecutor;

#[derive(Debug, Clone)]
pub enum ChatEvent {
    Content { text: String },
    ToolExecution { 
        tool_name: String, 
        status: String 
    },
    ToolResult {
        tool_name: String,
        result: serde_json::Value,
    },
    Complete {
        // Complete doesn't need mood/salience/tags - those are internal metadata
        // Frontend gets the full response content in Content events
    },
    Done,
    Error { message: String },
}

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
        
        // Build system prompt - FORCE JSON STRUCTURE
        let persona = self.select_persona(&request.metadata);
        let system_prompt = UnifiedPromptBuilder::build_system_prompt(
            &persona,
            &context,
            tools.as_deref(),
            request.metadata.as_ref(),
            request.project_id.as_deref(),
            true,  // CRITICAL: require_json for structured responses
        );
        debug!("System prompt built: {} chars", system_prompt.len());
        
        // Save user message to memory
        if let Err(e) = self.app_state.memory_service.save_user_message(
            &request.session_id,
            &request.content,
            request.project_id.as_deref()
        ).await {
            warn!("Failed to save user message to memory: {}", e);
        }
        
        // NEW: Get STRUCTURED response instead of streaming
        info!("Getting structured response for session: {}", request.session_id);
        let complete_response = self.app_state.llm_client
            .get_structured_response(
                &request.content,
                system_prompt,
                self.build_context_messages(&context).await?,
                &request.session_id,
            )
            .await?;
        
        // NEW: Save to database atomically (all 3 tables)
        let message_id = self.app_state.sqlite_store
            .save_structured_response(
                &request.session_id,
                &complete_response,
                None,  // parent_id
            )
            .await?;
        
        info!("Saved response with id={}, tokens={:?}", 
              message_id, complete_response.metadata.total_tokens);
        
        // NEW: Create event stream from complete response
        let events = self.create_event_stream(complete_response);
        Ok(events)
    }
    
    // NEW: Convert complete response to event stream for compatibility
    fn create_event_stream(
        &self,
        response: CompleteResponse,
    ) -> impl Stream<Item = Result<ChatEvent>> {
        // Convert complete response to stream of events
        let mut events = vec![];
        
        // Send content
        events.push(Ok(ChatEvent::Content {
            text: response.structured.output.clone()
        }));
        
        // Send metadata
        events.push(Ok(ChatEvent::Complete {
            mood: response.structured.analysis.mood.clone(),
            salience: Some(response.structured.analysis.salience),
            tags: Some(response.structured.analysis.topics.clone()),
        }));
        
        // Send done
        events.push(Ok(ChatEvent::Done));
        
        futures::stream::iter(events)
    }
    
    
    // Helper method to build context messages for GPT-5
    async fn build_context_messages(&self, context: &RecallContext) -> Result<Vec<serde_json::Value>> {
        let mut messages = Vec::new();
        
        // Add recent messages
        for entry in &context.recent {
            messages.push(json!({
                "role": entry.role,
                "content": [{
                    "type": "input_text",
                    "text": entry.content
                }]
            }));
        }
        
        // Add semantic matches
        for entry in &context.semantic {
            messages.push(json!({
                "role": entry.role,
                "content": [{
                    "type": "input_text",
                    "text": entry.content
                }]
            }));
        }
        
        Ok(messages)
    }
    
    // EXISTING METHODS PRESERVED
    
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
    
    // REMOVED: build_gpt5_request method (no longer needed with structured responses)
    // REMOVED: streaming-related methods
}
