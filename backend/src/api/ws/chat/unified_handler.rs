// src/api/ws/chat/unified_handler.rs
// FIXED: Pass tools to GPT-5 and handle tool calls for artifacts
// UPDATED: Inject code intelligence context automatically

use anyhow::Result;
use futures::StreamExt;
use serde_json::{Value, json};
use std::sync::Arc;
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
        let operation_manager = Arc::new(OperationManager::new(app_state.operation_engine.clone()));

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
        let should_route_to_ops = self
            .app_state
            .message_router
            .should_route_to_operation(&request.content)
            .await
            .unwrap_or(false);

        if should_route_to_ops {
            info!(
                "Routing to OperationEngine: {}",
                request.content.chars().take(50).collect::<String>()
            );

            // Route to OperationEngine
            let _op_id = self
                .operation_manager
                .start_operation(request.session_id, request.content, ws_tx)
                .await?;
            Ok(())
        } else {
            info!(
                "Routing to regular chat: {}",
                request.content.chars().take(50).collect::<String>()
            );

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
        let _ = ws_tx
            .send(json!({"type": "status", "status": "thinking"}))
            .await;

        // Prepare all context
        let (user_id, recall_context, file_tree, code_context) = self
            .prepare_context(&session_id, &content, request.project_id.as_deref())
            .await?;

        // Build prompt and get tools
        let (system_prompt, tools) = self.build_prompt_and_tools(
            &recall_context,
            request.metadata.as_ref(),
            request.project_id.as_deref(),
            code_context.as_ref(),
            file_tree.as_ref(),
        );

        // Stream response and process tool calls
        let (full_response, artifacts) = self
            .process_stream_with_tools(&content, system_prompt, tools, &ws_tx)
            .await?;

        // Finalize response
        self.finalize_response(&session_id, user_id, &full_response, artifacts, &ws_tx)
            .await?;

        Ok(())
    }

    /// Prepare all context needed for chat
    async fn prepare_context(
        &self,
        session_id: &str,
        content: &str,
        project_id: Option<&str>,
    ) -> Result<(
        i64,
        crate::memory::features::recall_engine::RecallContext,
        Option<Vec<crate::git::client::FileNode>>,
        Option<Vec<crate::memory::core::types::MemoryEntry>>,
    )> {
        // Store user message
        debug!("Storing user message in memory");
        let user_id = self
            .app_state
            .memory_service
            .save_user_message(session_id, content, project_id)
            .await
            .map_err(|e| {
                error!("Failed to store user message: {}", e);
                e
            })?;
        debug!("User message stored with ID: {}", user_id);

        // Load relationship context (for future use)
        debug!("Loading relationship context");
        if let Err(e) = self
            .app_state
            .relationship_service
            .context_loader()
            .load_context(session_id)
            .await
        {
            debug!("Failed to load relationship context: {}", e);
        }

        // Recall relevant context
        debug!("Recalling context for conversation");
        let recall_context = self
            .app_state
            .memory_service
            .parallel_recall_context(session_id, content, 10, 5)
            .await
            .map_err(|e| {
                error!("Failed to recall context: {}", e);
                e
            })?;
        debug!(
            "Recalled {} recent + {} semantic entries",
            recall_context.recent.len(),
            recall_context.semantic.len()
        );

        // Load project context
        let (file_tree, code_context) = self
            .app_state
            .context_loader
            .load_project_context(content, project_id, 10)
            .await;

        Ok((user_id, recall_context, file_tree, code_context))
    }

    /// Build system prompt and get tools
    fn build_prompt_and_tools(
        &self,
        recall_context: &crate::memory::features::recall_engine::RecallContext,
        metadata: Option<&MessageMetadata>,
        project_id: Option<&str>,
        code_context: Option<&Vec<crate::memory::core::types::MemoryEntry>>,
        file_tree: Option<&Vec<crate::git::client::FileNode>>,
    ) -> (String, Vec<Value>) {
        let persona = PersonaOverlay::Default;
        let system_prompt = UnifiedPromptBuilder::build_system_prompt(
            &persona,
            recall_context,
            None,
            metadata,
            project_id,
            code_context.map(|v| v.as_slice()),
            file_tree.map(|v| v.as_slice()),
        );

        let tools = vec![get_create_artifact_tool_schema()];
        debug!("Passing {} tools to GPT-5", tools.len());

        (system_prompt, tools)
    }

    /// Stream response and process tool calls
    async fn process_stream_with_tools(
        &self,
        content: &str,
        system_prompt: String,
        tools: Vec<Value>,
        ws_tx: &mpsc::Sender<Value>,
    ) -> Result<(String, Vec<Value>)> {
        debug!("Generating response with GPT-5 streaming");

        let messages = vec![Message::user(content.to_string())];
        let mut stream = self
            .app_state
            .gpt5_provider
            .create_stream_with_tools(messages, system_prompt, tools, None)
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
                    let _ = tx_clone.try_send(json!({"type": "stream", "delta": delta}));
                }
                Gpt5StreamEvent::ToolCallComplete {
                    id: _,
                    name,
                    arguments,
                } => {
                    if name == "create_artifact" {
                        let artifact = self.handle_artifact_creation(&arguments, &tx_clone).await?;
                        artifacts_created.push(artifact);
                    } else {
                        warn!("Unknown tool call: {}", name);
                    }
                }
                Gpt5StreamEvent::Done { .. } => {
                    let _ = tx_clone.try_send(json!({"type": "stream_end"}));
                }
                Gpt5StreamEvent::Error { message } => {
                    error!("Stream error: {}", message);
                    return Err(anyhow::anyhow!("Stream error: {}", message));
                }
                _ => {}
            }
        }

        debug!(
            "Response generated: {} chars, {} artifacts",
            full_response.len(),
            artifacts_created.len()
        );

        Ok((full_response, artifacts_created))
    }

    /// Handle artifact creation from tool call
    async fn handle_artifact_creation(
        &self,
        arguments: &Value,
        ws_tx: &mpsc::Sender<Value>,
    ) -> Result<Value> {
        debug!("Tool call: create_artifact with args: {}", arguments);

        // Extract artifact data
        let title = arguments
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("untitled");
        let content = arguments
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let language = arguments
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("text");
        let path = arguments
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| title.to_string());

        // Generate artifact ID
        let uuid_part = uuid::Uuid::new_v4().to_string();
        let artifact_id = format!(
            "artifact-{}-{}",
            chrono::Utc::now().timestamp(),
            &uuid_part[..8]
        );

        // Send artifact event immediately
        let artifact = json!({
            "id": artifact_id,
            "path": path,
            "content": content,
            "language": language,
        });

        let _ = ws_tx
            .send(json!({
                "type": "data",
                "data": {
                    "type": "artifact_created",
                    "artifact": artifact.clone(),
                }
            }))
            .await;

        info!("Created artifact: {} ({})", path, language);
        Ok(artifact)
    }

    /// Finalize response by storing and sending completion
    async fn finalize_response(
        &self,
        session_id: &str,
        user_id: i64,
        full_response: &str,
        artifacts: Vec<Value>,
        ws_tx: &mpsc::Sender<Value>,
    ) -> Result<()> {
        // Store assistant response
        let assistant_id = self
            .app_state
            .memory_service
            .save_assistant_message(session_id, full_response, Some(user_id))
            .await
            .map_err(|e| {
                error!("Failed to store assistant message: {}", e);
                e
            })?;
        debug!("Assistant message stored with ID: {}", assistant_id);

        // Send completion message
        let _ = ws_tx
            .send(json!({
                "type": "chat_complete",
                "user_message_id": user_id,
                "assistant_message_id": assistant_id,
                "content": full_response,
                "artifacts": artifacts,
            }))
            .await;

        info!(
            "Chat completed successfully with {} artifacts",
            artifacts.len()
        );
        Ok(())
    }

    /// Cancel an operation
    pub async fn cancel_operation(&self, operation_id: &str) -> Result<()> {
        self.operation_manager.cancel_operation(operation_id).await
    }
}
