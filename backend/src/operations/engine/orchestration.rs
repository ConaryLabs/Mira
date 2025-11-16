// src/operations/engine/orchestration.rs
// Main operation orchestration: run_operation method
// FIXED: Added error handling wrapper to emit Failed events on any error

use crate::llm::provider::Message;
use crate::llm::provider::gpt5::{Gpt5Provider, Gpt5StreamEvent};
use crate::llm::structured::tool_schema::get_create_artifact_tool_schema;
use crate::memory::service::MemoryService;
use crate::operations::ContextLoader;
use crate::operations::delegation_tools::{get_delegation_tools, parse_tool_call};
use crate::operations::engine::{
    artifacts::ArtifactManager, context::ContextBuilder, delegation::DelegationHandler,
    events::OperationEngineEvent, lifecycle::LifecycleManager, tool_router::ToolRouter,
    skills::SkillRegistry,
};

use anyhow::{Context, Result};
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

pub struct Orchestrator {
    gpt5: Gpt5Provider,
    memory_service: Arc<MemoryService>,
    context_builder: ContextBuilder,
    context_loader: ContextLoader,
    delegation_handler: DelegationHandler,
    tool_router: Option<ToolRouter>, // File operation routing
    skill_registry: Arc<SkillRegistry>, // Skills system for specialized tasks
    artifact_manager: ArtifactManager,
    lifecycle_manager: LifecycleManager,
}

impl Orchestrator {
    pub fn new(
        gpt5: Gpt5Provider,
        memory_service: Arc<MemoryService>,
        context_builder: ContextBuilder,
        context_loader: ContextLoader,
        delegation_handler: DelegationHandler,
        tool_router: Option<ToolRouter>,
        skill_registry: Arc<SkillRegistry>,
        artifact_manager: ArtifactManager,
        lifecycle_manager: LifecycleManager,
    ) -> Self {
        Self {
            gpt5,
            memory_service,
            context_builder,
            context_loader,
            delegation_handler,
            tool_router,
            skill_registry,
            artifact_manager,
            lifecycle_manager,
        }
    }

    /// Main operation orchestration with error handling wrapper
    ///
    /// This wrapper ensures that ANY error (cancellation, API failures, etc.)
    /// properly emits a Failed event before propagating the error.
    pub async fn run_operation(
        &self,
        operation_id: &str,
        session_id: &str,
        user_content: &str,
        project_id: Option<&str>,
        cancel_token: Option<CancellationToken>,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        // Run the inner operation logic
        let result = self
            .run_operation_inner(
                operation_id,
                session_id,
                user_content,
                project_id,
                cancel_token,
                event_tx,
            )
            .await;

        // If ANY error occurred, emit Failed event
        if let Err(e) = &result {
            let error_msg = e.to_string();
            warn!("[ENGINE] Operation {} failed: {}", operation_id, error_msg);

            // Emit failed event (ignore errors from this since we're already failing)
            let _ = self
                .lifecycle_manager
                .fail_operation(operation_id, error_msg, event_tx)
                .await;
        }

        result
    }

    /// Internal operation orchestration logic
    async fn run_operation_inner(
        &self,
        operation_id: &str,
        session_id: &str,
        user_content: &str,
        project_id: Option<&str>,
        cancel_token: Option<CancellationToken>,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        if let Some(token) = &cancel_token {
            if token.is_cancelled() {
                return Err(anyhow::anyhow!("Operation cancelled before start"));
            }
        }

        // Store user message in memory
        let user_msg_id = self
            .memory_service
            .save_user_message(session_id, user_content, project_id)
            .await?;

        info!("Stored user message in memory: message_id={}", user_msg_id);

        // Load memory context
        let recall_context = self
            .context_builder
            .load_memory_context(session_id, user_content)
            .await?;

        // Load project context (file tree + code intelligence) using shared loader
        let (file_tree, code_context) = self
            .context_loader
            .load_project_context(user_content, project_id, 10)
            .await;

        // Build system prompt with full context
        let system_prompt = self
            .context_builder
            .build_system_prompt(
                session_id,
                &recall_context,
                code_context.as_ref(),
                file_tree.as_ref(),
            )
            .await;

        let messages = vec![Message::user(user_content.to_string())];

        self.lifecycle_manager
            .start_operation(operation_id, event_tx)
            .await?;

        // Build tools: delegation tools + create_artifact
        let mut tools = get_delegation_tools();
        tools.push(get_create_artifact_tool_schema());

        let tool_names: Vec<String> = tools
            .iter()
            .filter_map(|t| {
                t.get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string())
            })
            .collect();
        info!(
            "[ENGINE] Passing {} tools to GPT-5: {:?}",
            tools.len(),
            tool_names
        );

        let previous_response_id = None;

        if let Some(token) = &cancel_token {
            if token.is_cancelled() {
                return Err(anyhow::anyhow!("Operation cancelled"));
            }
        }

        // Stream GPT-5 responses and handle tool calls
        let mut stream = self
            .gpt5
            .create_stream_with_tools(messages, system_prompt, tools, previous_response_id)
            .await
            .context("Failed to create GPT-5 stream")?;

        let mut accumulated_text = String::new();
        let mut delegation_calls = Vec::new();
        let mut _final_response_id = None;

        // Process tool calls during streaming
        while let Some(event) = stream.next().await {
            if let Some(token) = &cancel_token {
                if token.is_cancelled() {
                    return Err(anyhow::anyhow!("Operation cancelled"));
                }
            }

            match event? {
                Gpt5StreamEvent::TextDelta { delta } => {
                    accumulated_text.push_str(&delta);
                    let _ = event_tx
                        .send(OperationEngineEvent::Streaming {
                            operation_id: operation_id.to_string(),
                            content: delta,
                        })
                        .await;
                }
                Gpt5StreamEvent::ToolCallComplete {
                    id,
                    name,
                    arguments,
                } => {
                    info!("[ENGINE] GPT-5 tool call: {} (id: {})", name, id);

                    // Handle create_artifact immediately
                    if name == "create_artifact" {
                        info!("[ENGINE] GPT-5 called create_artifact, creating immediately");
                        if let Err(e) = self
                            .artifact_manager
                            .create_artifact(operation_id, arguments.clone(), event_tx)
                            .await
                        {
                            warn!("[ENGINE] Failed to create artifact: {}", e);
                        }
                    } else if matches!(
                        name.as_str(),
                        "read_project_file" | "search_codebase" | "list_project_files"
                        | "get_file_summary" | "get_file_structure"
                    ) {
                        // Handle file operation meta-tools via ToolRouter
                        info!("[ENGINE] Routing {} to DeepSeek file operations", name);
                        if let Some(ref router) = self.tool_router {
                            match router.route_tool_call(&name, arguments.clone()).await {
                                Ok(result) => {
                                    info!(
                                        "[ENGINE] File operation completed: {}",
                                        serde_json::to_string(&result).unwrap_or_default()
                                    );
                                    // Send result as streaming content
                                    let _ = event_tx
                                        .send(OperationEngineEvent::Streaming {
                                            operation_id: operation_id.to_string(),
                                            content: format!(
                                                "\n[File Operation Result: {}]\n{}\n",
                                                name,
                                                serde_json::to_string_pretty(&result)
                                                    .unwrap_or_default()
                                            ),
                                        })
                                        .await;
                                }
                                Err(e) => {
                                    warn!("[ENGINE] File operation failed: {}", e);
                                    let _ = event_tx
                                        .send(OperationEngineEvent::Streaming {
                                            operation_id: operation_id.to_string(),
                                            content: format!("\n[File Operation Error: {}]\n", e),
                                        })
                                        .await;
                                }
                            }
                        } else {
                            warn!("[ENGINE] ToolRouter not available for {}", name);
                        }
                    } else if name == "activate_skill" {
                        // Handle skill activation
                        info!("[ENGINE] Activating skill with arguments: {:?}", arguments);

                        let skill_name = arguments
                            .get("skill_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");

                        let task_description = arguments
                            .get("task_description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        let context = arguments
                            .get("context")
                            .and_then(|v| v.as_str());

                        // Load the skill
                        match self.skill_registry.get(skill_name) {
                            Some(skill) => {
                                info!(
                                    "[ENGINE] Skill '{}' activated (preferred model: {:?})",
                                    skill_name, skill.preferred_model
                                );

                                // Build the skill prompt
                                let skill_prompt = skill.build_prompt(task_description, context);

                                // Stream the skill activation notice
                                let _ = event_tx
                                    .send(OperationEngineEvent::Streaming {
                                        operation_id: operation_id.to_string(),
                                        content: format!(
                                            "\n**Activating {} skill...**\n\n",
                                            skill_name
                                        ),
                                    })
                                    .await;

                                // Queue for delegation (skill will be passed in args)
                                delegation_calls.push((
                                    id.clone(),
                                    "activate_skill_internal".to_string(),
                                    serde_json::json!({
                                        "skill_name": skill_name,
                                        "skill_prompt": skill_prompt,
                                        "task_description": task_description,
                                        "preferred_model": format!("{:?}", skill.preferred_model),
                                        "allowed_tools": skill.allowed_tools,
                                    }),
                                ));
                            }
                            None => {
                                warn!("[ENGINE] Skill '{}' not found", skill_name);
                                let _ = event_tx
                                    .send(OperationEngineEvent::Streaming {
                                        operation_id: operation_id.to_string(),
                                        content: format!(
                                            "\n**Error: Skill '{}' not found**\n",
                                            skill_name
                                        ),
                                    })
                                    .await;
                            }
                        }
                    } else {
                        // Queue other tools for DeepSeek delegation
                        info!("[ENGINE] Queueing {} for DeepSeek delegation", name);
                        delegation_calls.push((id, name, arguments));
                    }
                }
                Gpt5StreamEvent::Done { response_id, .. } => {
                    _final_response_id = Some(response_id);
                }
                _ => {}
            }
        }

        // Handle delegation calls (everything except create_artifact)
        if !delegation_calls.is_empty() {
            if let Some(token) = &cancel_token {
                if token.is_cancelled() {
                    return Err(anyhow::anyhow!("Operation cancelled"));
                }
            }

            self.lifecycle_manager
                .update_status(operation_id, "delegating", event_tx)
                .await?;

            for (_tool_id, tool_name, tool_args) in delegation_calls {
                if let Some(token) = &cancel_token {
                    if token.is_cancelled() {
                        return Err(anyhow::anyhow!("Operation cancelled"));
                    }
                }

                let tool_call_json = serde_json::json!({
                    "function": {
                        "name": tool_name,
                        "arguments": serde_json::to_string(&tool_args)?,
                    }
                });

                let (parsed_name, parsed_args) = parse_tool_call(&tool_call_json)?;

                let _ = event_tx
                    .send(OperationEngineEvent::Delegated {
                        operation_id: operation_id.to_string(),
                        delegated_to: "deepseek".to_string(),
                        reason: format!("Tool call: {}", parsed_name),
                    })
                    .await;

                // Delegate to DeepSeek with rich context
                let result = self
                    .delegation_handler
                    .delegate_to_deepseek(
                        &parsed_name,
                        parsed_args,
                        cancel_token.clone(),
                        file_tree.as_ref(),
                        code_context.as_ref(),
                        &recall_context,
                    )
                    .await?;

                // DeepSeek can also return artifacts
                if let Some(artifact_data) = result.get("artifact") {
                    self.artifact_manager
                        .create_artifact(operation_id, artifact_data.clone(), event_tx)
                        .await?;
                }
            }

            self.lifecycle_manager
                .update_status(operation_id, "generating", event_tx)
                .await?;
        }

        // Complete the operation
        let artifacts = self
            .artifact_manager
            .get_artifacts_for_operation(operation_id)
            .await?;
        self.lifecycle_manager
            .complete_operation(
                operation_id,
                session_id,
                Some(accumulated_text),
                event_tx,
                artifacts,
            )
            .await?;

        Ok(())
    }
}
