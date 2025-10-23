// src/api/ws/chat/unified_handler.rs
// FIXED: Pass tools to GPT-5 and handle tool calls for artifacts

use std::sync::Arc;
use anyhow::Result;
use futures::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::api::ws::message::MessageMetadata;
use crate::api::ws::operations::OperationManager;
use crate::llm::provider::{Message, gpt5::Gpt5StreamEvent};
use crate::llm::structured::tool_schema::get_create_artifact_tool_schema;
use crate::persona::PersonaOverlay;
use crate::prompt::UnifiedPromptBuilder;
use crate::state::AppState;

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub content: String,
    pub project_id: Option<String>,
    pub metadata: Option<MessageMetadata>,
    pub session_id: String,
}

pub struct UnifiedChatHandler {
    app_state: Arc<AppState>,
    operation_manager: Arc<OperationManager>,
}

impl UnifiedChatHandler {
    pub fn new(app_state: Arc<AppState>) -> Self {
        let operation_manager = Arc::new(OperationManager::new(
            app_state.operation_engine.clone(),
        ));
        
        Self {
            app_state,
            operation_manager,
        }
    }
    
    /// Route message: check if it should go to OperationEngine or regular chat
    pub async fn route_and_handle(
        &self,
        request: ChatRequest,
        ws_tx: mpsc::Sender<Value>,
    ) -> Result<()> {
        // Use LLM to determine routing
        let should_route_to_ops = self.app_state
            .message_router
            .should_route_to_operation(&request.content)
            .await
            .unwrap_or(false);
        
        if should_route_to_ops {
            info!("Routing to OperationEngine: {}", request.content.chars().take(50).collect::<String>());
            
            // Route to OperationEngine
            let _op_id = self.operation_manager.start_operation(
                request.session_id,
                request.content,
                ws_tx,
            ).await?;
            Ok(())
        } else {
            info!("Routing to regular chat: {}", request.content.chars().take(50).collect::<String>());
            
            // Route to regular chat handler
            self.handle_regular_chat(request, ws_tx).await
        }
    }
    
    /// Handle regular conversational chat with tool support for artifacts
    async fn handle_regular_chat(
        &self,
        request: ChatRequest,
        ws_tx: mpsc::Sender<Value>,
    ) -> Result<()> {
        let session_id = request.session_id.clone();
        let content = request.content.clone();
        
        // Send typing indicator
        let _ = ws_tx.send(json!({
            "type": "status",
            "status": "thinking"
        })).await;
        
        // 1. Store user message (this triggers full message pipeline)
        debug!("Storing user message in memory with full pipeline");
        
        let user_id = self.app_state
            .memory_service
            .save_user_message(&session_id, &content, request.project_id.as_deref())
            .await
            .map_err(|e| {
                error!("Failed to store user message: {}", e);
                e
            })?;
        
        debug!("User message stored with ID: {} (pipeline complete)", user_id);
        
        // 2. Get relationship context (for future use)
        debug!("Loading relationship context");
        let _relationship_context = self.app_state
            .relationship_service
            .context_loader()
            .load_context(&session_id)
            .await
            .ok();
        
        // 3. Recall relevant context (semantic + recent)
        debug!("Recalling context for conversation");
        let recall_context = self.app_state
            .memory_service
            .parallel_recall_context(&session_id, &content, 10, 5)
            .await
            .map_err(|e| {
                error!("Failed to recall context: {}", e);
                e
            })?;
        
        debug!("Recalled {} recent + {} semantic entries", 
               recall_context.recent.len(), 
               recall_context.semantic.len());
        
        // 4. Build system prompt with full context
        let persona = PersonaOverlay::Default;
        let system_prompt = UnifiedPromptBuilder::build_system_prompt(
            &persona,
            &recall_context,
            None, // tools
            request.metadata.as_ref(),
            request.project_id.as_deref(),
        );
        
        // Build messages for GPT-5
        let messages = vec![Message::user(content.clone())];
        
        // 5. Get tools for artifact creation
        let tools = vec![get_create_artifact_tool_schema()];
        debug!("Passing {} tools to GPT-5", tools.len());
        
        // 6. Generate response with streaming and tool support
        debug!("Generating response with GPT-5 streaming");
        
        let mut stream = self.app_state
            .gpt5_provider
            .create_stream_with_tools(
                messages,
                system_prompt,
                tools,  // FIXED: Pass actual tools!
                None,   // no previous response
            )
            .await
            .map_err(|e| {
                error!("Failed to create stream: {}", e);
                e
            })?;
        
        let mut full_response = String::new();
        let mut artifacts_created = Vec::new();
        let tx_clone = ws_tx.clone();
        
        // Process stream events
        while let Some(event) = stream.next().await {
            match event? {
                Gpt5StreamEvent::TextDelta { delta } => {
                    full_response.push_str(&delta);
                    let _ = tx_clone.try_send(json!({
                        "type": "stream",
                        "delta": delta,
                    }));
                }
                Gpt5StreamEvent::ToolCallComplete { id: _, name, arguments } => {
                    // Handle tool calls
                    if name == "create_artifact" {
                        debug!("Tool call: create_artifact with args: {}", arguments);
                        
                        // Extract artifact data
                        let title = arguments.get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or("untitled");
                        let content = arguments.get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let language = arguments.get("language")
                            .and_then(|v| v.as_str())
                            .unwrap_or("text");
                        let path = arguments.get("path")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| title.to_string());
                        
                        // Generate artifact ID
                        let artifact_id = format!("artifact-{}-{}", 
                            chrono::Utc::now().timestamp(),
                            uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("xxx")
                        );
                        
                        // Send artifact event immediately
                        let artifact_event = json!({
                            "type": "data",
                            "data": {
                                "type": "artifact_created",
                                "artifact": {
                                    "id": artifact_id,
                                    "path": path,
                                    "content": content,
                                    "language": language,
                                }
                            }
                        });
                        
                        let _ = tx_clone.send(artifact_event).await;
                        
                        artifacts_created.push(json!({
                            "id": artifact_id,
                            "path": path,
                            "content": content,
                            "language": language,
                        }));
                        
                        info!("Created artifact: {} ({})", path, language);
                    } else {
                        warn!("Unknown tool call: {}", name);
                    }
                }
                Gpt5StreamEvent::Done { .. } => {
                    let _ = tx_clone.try_send(json!({
                        "type": "stream_end"
                    }));
                }
                Gpt5StreamEvent::Error { message } => {
                    error!("Stream error: {}", message);
                    return Err(anyhow::anyhow!("Stream error: {}", message));
                }
                _ => {
                    // Ignore other events
                }
            }
        }
        
        debug!("Response generated: {} chars, {} artifacts created", 
               full_response.len(), 
               artifacts_created.len());
        
        // 7. Store assistant response (this also goes through full pipeline)
        let assistant_id = self.app_state
            .memory_service
            .save_assistant_message(&session_id, &full_response, Some(user_id))
            .await
            .map_err(|e| {
                error!("Failed to store assistant message: {}", e);
                e
            })?;
        
        debug!("Assistant message stored with ID: {} (pipeline complete)", assistant_id);
        
        // 8. Send completion message
        let _ = ws_tx.send(json!({
            "type": "chat_complete",
            "user_message_id": user_id,
            "assistant_message_id": assistant_id,
            "content": full_response,
            "artifacts": artifacts_created,
        })).await;
        
        info!("Regular chat completed successfully with {} artifacts", artifacts_created.len());
        
        Ok(())
    }
    
    /// Cancel an operation
    pub async fn cancel_operation(&self, operation_id: &str) -> Result<()> {
        self.operation_manager.cancel_operation(operation_id).await
    }
}
