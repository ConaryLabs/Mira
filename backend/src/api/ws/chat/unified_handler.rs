// src/api/ws/chat/unified_handler.rs
// Gemini 3 chat handler - routes messages to OperationEngine with slash command support

use anyhow::Result;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

use crate::api::ws::message::MessageMetadata;
use crate::api::ws::operations::OperationManager;
use crate::commands::CommandRegistry;
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
}

impl UnifiedChatHandler {
    pub fn new(app_state: Arc<AppState>) -> Self {
        let operation_manager = Arc::new(OperationManager::new(app_state.operation_engine.clone()));

        Self {
            operation_manager,
            command_registry: app_state.command_registry.clone(),
        }
    }

    /// Route messages - check for slash commands first, then to OperationEngine
    pub async fn route_and_handle(
        &self,
        request: ChatRequest,
        ws_tx: mpsc::Sender<Value>,
    ) -> Result<()> {
        // Check for slash commands
        if let Some(expanded) = self.try_expand_command(&request).await {
            info!(
                "[SlashCommand] Expanded /{} to prompt",
                expanded.command_name
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
            let _ = ws_tx.send(response).await;
            return Ok(());
        }

        // Regular message - route to OperationEngine
        info!(
            "[Gemini3] Routing to OperationEngine: {}",
            request.content.chars().take(50).collect::<String>()
        );

        let _op_id = self
            .operation_manager
            .start_operation(request.session_id, request.content, ws_tx)
            .await?;

        Ok(())
    }

    /// Try to expand a slash command, returns None if not a command
    async fn try_expand_command(&self, request: &ChatRequest) -> Option<ExpandedCommand> {
        let registry = self.command_registry.read().await;

        if let Some((command_name, arguments)) = registry.parse_command(&request.content) {
            if let Some(prompt) = registry.execute(&command_name, &arguments) {
                return Some(ExpandedCommand {
                    command_name,
                    arguments,
                    prompt,
                });
            }
        }

        None
    }

    /// Handle built-in commands like /commands, /reload-commands
    async fn handle_builtin_command(&self, request: &ChatRequest) -> Option<Value> {
        let content = request.content.trim();

        // List available commands
        if content == "/commands" || content == "/help-commands" {
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

        None
    }


    /// Cancel an operation
    pub async fn cancel_operation(&self, operation_id: &str) -> Result<()> {
        self.operation_manager.cancel_operation(operation_id).await
    }
}
