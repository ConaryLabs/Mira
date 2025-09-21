// src/tools/message_handler.rs
// Handles tool-enabled chat processing with structured responses.

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, warn};

use crate::api::ws::chat::connection::WebSocketConnection;
use crate::api::ws::message::{MessageMetadata, WsServerMessage};
use crate::tools::executor::{ToolExecutor, ToolExecutorExt, ToolChatRequest};
use crate::memory::RecallContext;
use crate::state::AppState;

/// Processes tool-enhanced messages and sends structured responses.
pub struct ToolMessageHandler {
    executor: Arc<ToolExecutor>,
    connection: Arc<WebSocketConnection>,
    app_state: Arc<AppState>,
}

impl ToolMessageHandler {
    pub fn new(
        executor: Arc<ToolExecutor>,
        connection: Arc<WebSocketConnection>,
        app_state: Arc<AppState>,
    ) -> Self {
        Self { executor, connection, app_state }
    }

    /// Process a tool-enabled message and return complete structured response.
    pub async fn handle_tool_message(
        &self,
        content: String,
        project_id: Option<String>,
        metadata: Option<MessageMetadata>,
        context: RecallContext,
        system_prompt: String,
        session_id: String,
    ) -> Result<()> {
        info!("Handling tool message for session {}: {}", session_id, content.chars().take(80).collect::<String>());

        // Check if tools are enabled
        if !self.executor.tools_enabled() {
            warn!("Tools are not enabled, falling back to simple response.");
            return self.handle_simple_response(content).await;
        }

        // Package request for tool executor
        let request = ToolChatRequest { 
            content: content.clone(),
            project_id: project_id.clone(),
            metadata: metadata.clone(),
            session_id: session_id.clone(),
            context,
            system_prompt,
        };

        // For now, we'll process tools synchronously and build a structured response
        // This is where you'd analyze the content for tool needs and execute them
        
        let mut tool_results = Vec::new();
        let mut response_content = format!("Processing your request: {}", content);

        // Check for tool triggers in content (simplified logic)
        if content.contains("search") || content.contains("find") {
            // Execute file search tool
            if let Some(ref pid) = project_id {
                match self.executor.execute_tool("file_search", serde_json::json!({
                    "query": content,
                    "project_id": pid
                })).await {
                    Ok(result) => {
                        tool_results.push(("file_search", result));
                        response_content.push_str("\n\nI searched your files and found relevant results.");
                    }
                    Err(e) => {
                        warn!("File search failed: {}", e);
                        response_content.push_str("\n\nFile search encountered an error.");
                    }
                }
            }
        }

        if content.contains("image") || content.contains("generate") || content.contains("picture") {
            // Execute image generation
            match self.executor.execute_tool("image_generation", serde_json::json!({
                "prompt": content
            })).await {
                Ok(result) => {
                    tool_results.push(("image_generation", result));
                    response_content.push_str("\n\nI've generated an image based on your request.");
                }
                Err(e) => {
                    warn!("Image generation failed: {}", e);
                    response_content.push_str("\n\nImage generation encountered an error.");
                }
            }
        }

        // Build structured response with tool results
        let mut response_data = serde_json::json!({
            "content": response_content,
            "analysis": {
                "salience": if tool_results.is_empty() { 3.0 } else { 7.0 },
                "topics": ["tools", "processing"],
                "mood": "helpful",
                "contains_code": false
            },
            "metadata": {
                "tool_enabled": true,
                "session_id": session_id,
                "project_id": project_id,
                "tools_executed": tool_results.len()
            }
        });

        // Add tool results to response
        if !tool_results.is_empty() {
            response_data["tool_results"] = serde_json::json!(tool_results);
        }

        self.connection.send_message(WsServerMessage::Response {
            data: response_data,
        }).await?;

        Ok(())
    }

    /// Fallback response when tools are disabled.
    async fn handle_simple_response(&self, content: String) -> Result<()> {
        info!("Handling simple response because tools are disabled.");
        
        let response_data = serde_json::json!({
            "content": format!("Tools are currently disabled. You said: {}", content),
            "analysis": {
                "salience": 0.5,
                "topics": ["system", "notification"],
                "mood": "informative",
                "contains_code": false
            },
            "metadata": {
                "tool_enabled": false
            }
        });

        self.connection.send_message(WsServerMessage::Response {
            data: response_data,
        }).await?;

        Ok(())
    }
}
