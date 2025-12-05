// src/api/ws/chat/unified_handler.rs
// Gemini 3 chat handler - routes messages to OperationEngine with slash command support

use anyhow::Result;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::api::ws::message::MessageMetadata;
use crate::api::ws::operations::OperationManager;
use crate::checkpoint::CheckpointManager;
use crate::commands::CommandRegistry;
use crate::mcp::McpManager;
use crate::state::AppState;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub content: String,
    pub project_id: Option<String>,
    pub metadata: Option<MessageMetadata>,
    pub session_id: String,
}

/// Result of expanding a slash command
struct ExpandedCommand {
    command_name: String,
    #[allow(dead_code)]
    arguments: String,
    prompt: String,
}

pub struct UnifiedChatHandler {
    operation_manager: Arc<OperationManager>,
    command_registry: Arc<RwLock<CommandRegistry>>,
    checkpoint_manager: Arc<CheckpointManager>,
    mcp_manager: Arc<McpManager>,
}

impl UnifiedChatHandler {
    pub fn new(app_state: Arc<AppState>) -> Self {
        let operation_manager = Arc::new(OperationManager::new(
            app_state.operation_engine.clone(),
            app_state.sqlite_pool.clone(),
            app_state.project_store.clone(),
        ));

        Self {
            operation_manager,
            command_registry: app_state.command_registry.clone(),
            checkpoint_manager: app_state.checkpoint_manager.clone(),
            mcp_manager: app_state.mcp_manager.clone(),
        }
    }

    /// Route messages - check for slash commands first, then to OperationEngine
    pub async fn route_and_handle(
        &self,
        request: ChatRequest,
        ws_tx: mpsc::Sender<Value>,
    ) -> Result<()> {
        let content_preview: String = request.content.chars().take(50).collect();
        debug!(
            session_id = %request.session_id,
            content_len = request.content.len(),
            project_id = ?request.project_id,
            content_preview = %content_preview,
            "Routing chat request"
        );

        // Check for slash commands
        if let Some(expanded) = self.try_expand_command(&request).await {
            info!(
                session_id = %request.session_id,
                command = %expanded.command_name,
                "Slash command expanded"
            );

            // Send status update about command expansion
            let _ = ws_tx
                .send(json!({
                    "type": "status",
                    "status": format!("Executing /{} command...", expanded.command_name)
                }))
                .await;

            // Route the expanded prompt to OperationEngine
            let _op_id = self
                .operation_manager
                .start_operation(request.session_id, expanded.prompt, ws_tx)
                .await?;

            return Ok(());
        }

        // Check for built-in commands
        if let Some(response) = self.handle_builtin_command(&request).await {
            debug!(
                session_id = %request.session_id,
                "Handled as builtin command"
            );
            let _ = ws_tx.send(response).await;
            return Ok(());
        }

        // Regular message - route to OperationEngine
        info!(
            session_id = %request.session_id,
            content_preview = %content_preview,
            "Routing to OperationEngine"
        );

        let op_id = self
            .operation_manager
            .start_operation(request.session_id.clone(), request.content, ws_tx)
            .await?;

        debug!(
            session_id = %request.session_id,
            operation_id = %op_id,
            "Operation started"
        );

        Ok(())
    }

    /// Try to expand a slash command, returns None if not a command
    async fn try_expand_command(&self, request: &ChatRequest) -> Option<ExpandedCommand> {
        let registry = self.command_registry.read().await;

        if let Some((command_name, arguments)) = registry.parse_command(&request.content) {
            debug!(
                session_id = %request.session_id,
                command = %command_name,
                args_len = arguments.len(),
                "Parsed slash command"
            );

            if let Some(prompt) = registry.execute(&command_name, &arguments) {
                return Some(ExpandedCommand {
                    command_name,
                    arguments,
                    prompt,
                });
            } else {
                warn!(
                    session_id = %request.session_id,
                    command = %command_name,
                    "Slash command not found in registry"
                );
            }
        }

        None
    }

    /// Handle built-in commands like /commands, /reload-commands
    async fn handle_builtin_command(&self, request: &ChatRequest) -> Option<Value> {
        let content = request.content.trim();

        // Check if it's a builtin command pattern
        if !content.starts_with('/') {
            return None;
        }

        debug!(
            session_id = %request.session_id,
            command = %content,
            "Checking builtin command"
        );

        // List available commands
        if content == "/commands" || content == "/help-commands" {
            debug!(session_id = %request.session_id, "Listing available commands");
            let registry = self.command_registry.read().await;
            let commands: Vec<_> = registry
                .list()
                .iter()
                .map(|cmd| {
                    json!({
                        "name": cmd.name,
                        "description": cmd.description,
                        "scope": format!("{:?}", cmd.scope),
                        "takes_arguments": cmd.takes_arguments()
                    })
                })
                .collect();

            return Some(json!({
                "type": "chat_complete",
                "user_message_id": "",
                "assistant_message_id": "",
                "content": if commands.is_empty() {
                    "No custom slash commands found.\n\nTo add commands, create markdown files in:\n- `.mira/commands/` (project-specific)\n- `~/.mira/commands/` (user-global)\n\nExample: `.mira/commands/review.md`\n```markdown\n# Review Code\nReview the following code for issues:\n\n$ARGUMENTS\n```\n\nThen use: `/review <your code here>`".to_string()
                } else {
                    format!("**Available Slash Commands:**\n\n{}",
                        commands.iter()
                            .map(|c| format!("- `/{}`{}",
                                c["name"].as_str().unwrap_or(""),
                                c["description"].as_str()
                                    .map(|d| format!(" - {}", d))
                                    .unwrap_or_default()
                            ))
                            .collect::<Vec<_>>()
                            .join("\n")
                    )
                },
                "artifacts": [],
                "thinking": null
            }));
        }

        // Reload commands
        if content == "/reload-commands" {
            let mut registry = self.command_registry.write().await;

            // Get project root from metadata if available
            let project_root = request
                .metadata
                .as_ref()
                .and_then(|m| m.file_path.as_ref())
                .map(|f| PathBuf::from(f).parent().map(|p| p.to_path_buf()))
                .flatten();

            match registry.reload(project_root.as_deref()).await {
                Ok(()) => {
                    let count = registry.len();
                    return Some(json!({
                        "type": "chat_complete",
                        "user_message_id": "",
                        "assistant_message_id": "",
                        "content": format!("Reloaded {} custom slash commands.", count),
                        "artifacts": [],
                        "thinking": null
                    }));
                }
                Err(e) => {
                    return Some(json!({
                        "type": "chat_complete",
                        "user_message_id": "",
                        "assistant_message_id": "",
                        "content": format!("Failed to reload commands: {}", e),
                        "artifacts": [],
                        "thinking": null
                    }));
                }
            }
        }

        // List checkpoints for current session
        if content == "/checkpoints" {
            match self.checkpoint_manager.list_checkpoints(&request.session_id, 20).await {
                Ok(checkpoints) => {
                    if checkpoints.is_empty() {
                        return Some(json!({
                            "type": "chat_complete",
                            "user_message_id": "",
                            "assistant_message_id": "",
                            "content": "No checkpoints found for this session.\n\nCheckpoints are automatically created before file modifications.",
                            "artifacts": [],
                            "thinking": null
                        }));
                    }

                    let mut output = String::from("**Checkpoints (most recent first):**\n\n");
                    for (i, cp) in checkpoints.iter().enumerate() {
                        let time = chrono::DateTime::from_timestamp(cp.created_at, 0)
                            .map(|dt| dt.format("%H:%M:%S").to_string())
                            .unwrap_or_else(|| "unknown".to_string());

                        let tool_info = cp.tool_name.as_deref().unwrap_or("manual");
                        let desc = cp.description.as_deref().unwrap_or("");

                        output.push_str(&format!(
                            "{}. `{}` - {} ({} files) {}\n",
                            i + 1,
                            &cp.id[..8],
                            time,
                            cp.file_count,
                            if !desc.is_empty() { format!("- {}", desc) } else { format!("[{}]", tool_info) }
                        ));
                    }
                    output.push_str("\nUse `/rewind <checkpoint-id>` to restore to a checkpoint.");

                    return Some(json!({
                        "type": "chat_complete",
                        "user_message_id": "",
                        "assistant_message_id": "",
                        "content": output,
                        "artifacts": [],
                        "thinking": null
                    }));
                }
                Err(e) => {
                    return Some(json!({
                        "type": "chat_complete",
                        "user_message_id": "",
                        "assistant_message_id": "",
                        "content": format!("Failed to list checkpoints: {}", e),
                        "artifacts": [],
                        "thinking": null
                    }));
                }
            }
        }

        // List MCP servers and tools
        if content == "/mcp" {
            let servers = self.mcp_manager.list_servers().await;
            let tools = self.mcp_manager.get_all_tools().await;

            if servers.is_empty() {
                return Some(json!({
                    "type": "chat_complete",
                    "user_message_id": "",
                    "assistant_message_id": "",
                    "content": "No MCP servers connected.\n\nTo configure MCP servers, create `.mira/mcp.json`:\n```json\n{\n  \"servers\": [\n    {\n      \"name\": \"example\",\n      \"command\": \"npx\",\n      \"args\": [\"-y\", \"@anthropic/mcp-server-example\"]\n    }\n  ]\n}\n```",
                    "artifacts": [],
                    "thinking": null
                }));
            }

            let mut output = format!("**MCP Servers ({} connected):**\n\n", servers.len());
            for server in &servers {
                let server_tools: Vec<_> = tools.iter().filter(|(s, _)| s == server).collect();
                output.push_str(&format!("**{}** ({} tools)\n", server, server_tools.len()));
                for (_, tool) in server_tools {
                    let desc = tool.description.as_deref().unwrap_or("No description");
                    output.push_str(&format!("  - `{}`: {}\n", tool.name, desc));
                }
                output.push('\n');
            }

            return Some(json!({
                "type": "chat_complete",
                "user_message_id": "",
                "assistant_message_id": "",
                "content": output,
                "artifacts": [],
                "thinking": null
            }));
        }

        // Rewind to a checkpoint
        if content.starts_with("/rewind ") {
            let checkpoint_id_prefix = content.strip_prefix("/rewind ").unwrap().trim();

            if checkpoint_id_prefix.is_empty() {
                return Some(json!({
                    "type": "chat_complete",
                    "user_message_id": "",
                    "assistant_message_id": "",
                    "content": "Usage: `/rewind <checkpoint-id>`\n\nUse `/checkpoints` to see available checkpoints.",
                    "artifacts": [],
                    "thinking": null
                }));
            }

            // Find matching checkpoint
            match self.checkpoint_manager.list_checkpoints(&request.session_id, 100).await {
                Ok(checkpoints) => {
                    let matching: Vec<_> = checkpoints
                        .iter()
                        .filter(|cp| cp.id.starts_with(checkpoint_id_prefix))
                        .collect();

                    if matching.is_empty() {
                        return Some(json!({
                            "type": "chat_complete",
                            "user_message_id": "",
                            "assistant_message_id": "",
                            "content": format!("No checkpoint found matching '{}'.\n\nUse `/checkpoints` to see available checkpoints.", checkpoint_id_prefix),
                            "artifacts": [],
                            "thinking": null
                        }));
                    }

                    if matching.len() > 1 {
                        return Some(json!({
                            "type": "chat_complete",
                            "user_message_id": "",
                            "assistant_message_id": "",
                            "content": format!("Multiple checkpoints match '{}'. Please be more specific.", checkpoint_id_prefix),
                            "artifacts": [],
                            "thinking": null
                        }));
                    }

                    let checkpoint = matching[0];
                    match self.checkpoint_manager.restore_checkpoint(&checkpoint.id).await {
                        Ok(result) => {
                            let mut output = format!("**Restored checkpoint `{}`**\n\n", &checkpoint.id[..8]);

                            if !result.files_restored.is_empty() {
                                output.push_str(&format!("Restored {} file(s):\n", result.files_restored.len()));
                                for f in &result.files_restored {
                                    output.push_str(&format!("- {}\n", f));
                                }
                            }

                            if !result.files_deleted.is_empty() {
                                output.push_str(&format!("\nDeleted {} file(s) (didn't exist at checkpoint):\n", result.files_deleted.len()));
                                for f in &result.files_deleted {
                                    output.push_str(&format!("- {}\n", f));
                                }
                            }

                            if !result.errors.is_empty() {
                                output.push_str(&format!("\nErrors:\n"));
                                for e in &result.errors {
                                    output.push_str(&format!("- {}\n", e));
                                }
                            }

                            return Some(json!({
                                "type": "chat_complete",
                                "user_message_id": "",
                                "assistant_message_id": "",
                                "content": output,
                                "artifacts": [],
                                "thinking": null
                            }));
                        }
                        Err(e) => {
                            return Some(json!({
                                "type": "chat_complete",
                                "user_message_id": "",
                                "assistant_message_id": "",
                                "content": format!("Failed to restore checkpoint: {}", e),
                                "artifacts": [],
                                "thinking": null
                            }));
                        }
                    }
                }
                Err(e) => {
                    return Some(json!({
                        "type": "chat_complete",
                        "user_message_id": "",
                        "assistant_message_id": "",
                        "content": format!("Failed to find checkpoint: {}", e),
                        "artifacts": [],
                        "thinking": null
                    }));
                }
            }
        }

        None
    }


    /// Cancel an operation
    pub async fn cancel_operation(&self, operation_id: &str) -> Result<()> {
        info!(operation_id = %operation_id, "Cancelling operation");
        self.operation_manager.cancel_operation(operation_id).await
    }
}
