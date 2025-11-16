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
    skills::SkillRegistry, simple_mode::SimpleModeDetector,
};
use crate::operations::TaskManager;

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
    task_manager: TaskManager,
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
        task_manager: TaskManager,
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
            task_manager,
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

        // Check complexity and generate plan for complex operations
        let simplicity = SimpleModeDetector::simplicity_score(user_content);
        let _task_ids: Vec<String> = if simplicity <= 0.7 {
            info!(
                "[ENGINE] Complex operation detected (score: {:.2}), generating plan",
                simplicity
            );

            // Generate execution plan
            let plan = self
                .generate_plan(operation_id, user_content, system_prompt.clone(), event_tx)
                .await?;

            // Parse plan into tasks
            let _tasks = self
                .parse_plan_into_tasks(operation_id, &plan, event_tx)
                .await?;

            // Get task IDs for tracking
            let task_records = self.task_manager.get_tasks(operation_id).await?;
            task_records.into_iter().map(|t| t.id).collect()
        } else {
            info!(
                "[ENGINE] Simple operation detected (score: {:.2}), skipping planning",
                simplicity
            );
            Vec::new()
        };

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
            .create_stream_with_tools(messages, system_prompt, tools, previous_response_id, None) // Use default reasoning
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
                        "read_project_file" | "write_project_file" | "edit_project_file"
                        | "search_codebase" | "list_project_files"
                        | "get_file_summary" | "get_file_structure"
                        | "web_search" | "fetch_url" | "execute_command"
                        | "git_history" | "git_blame" | "git_diff" | "git_file_history"
                        | "git_branches" | "git_show_commit" | "git_file_at_commit"
                        | "git_recent_changes" | "git_contributors" | "git_status"
                        | "find_function" | "find_class_or_struct" | "search_code_semantic"
                        | "find_imports" | "analyze_dependencies" | "get_complexity_hotspots"
                        | "get_quality_issues" | "get_file_symbols" | "find_tests_for_code"
                        | "get_codebase_stats" | "find_callers" | "get_element_definition"
                    ) {
                        // Handle file operation, external, git, and code intelligence meta-tools via ToolRouter
                        info!("[ENGINE] Routing {} via ToolRouter", name);
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

    /// Generate execution plan for complex operations using GPT-5 reasoning
    async fn generate_plan(
        &self,
        operation_id: &str,
        user_content: &str,
        system_prompt: String,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<String> {
        info!("[ENGINE] Generating execution plan for complex operation");

        // Call GPT-5 with reasoning enabled, NO tools
        let plan_prompt = format!(
            "You are planning how to accomplish this task:\n\n{}\n\n\
            Generate a step-by-step execution plan. Be specific and break down the work into clear, actionable tasks.\n\
            Format your plan as a numbered list of tasks. Each task should be a single, focused action.\n\n\
            Example format:\n\
            1. Task description here\n\
            2. Another task description\n\
            3. Final task description",
            user_content
        );

        let messages = vec![Message::user(plan_prompt)];

        // Stream the plan generation (with empty tools array to get reasoning)
        // Use HIGH reasoning for better planning quality
        let mut stream = self
            .gpt5
            .create_stream_with_tools(messages, system_prompt, vec![], None, Some("high".to_string()))
            .await
            .context("Failed to create GPT-5 planning stream")?;

        let mut plan_text = String::new();
        let mut reasoning_tokens: Option<i32> = None;

        while let Some(event) = stream.next().await {
            match event? {
                Gpt5StreamEvent::TextDelta { delta } => {
                    plan_text.push_str(&delta);
                    // Stream plan as it's being generated
                    let _ = event_tx
                        .send(OperationEngineEvent::Streaming {
                            operation_id: operation_id.to_string(),
                            content: delta,
                        })
                        .await;
                }
                Gpt5StreamEvent::Done {
                    reasoning_tokens: rt,
                    ..
                } => {
                    reasoning_tokens = Some(rt as i32);
                }
                _ => {}
            }
        }

        if plan_text.is_empty() {
            return Err(anyhow::anyhow!("Generated plan was empty"));
        }

        // Record plan in database and emit event
        self.lifecycle_manager
            .record_plan(operation_id, plan_text.clone(), reasoning_tokens, event_tx)
            .await?;

        Ok(plan_text)
    }

    /// Parse plan text into individual tasks
    async fn parse_plan_into_tasks(
        &self,
        operation_id: &str,
        plan_text: &str,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<Vec<String>> {
        info!("[ENGINE] Parsing plan into trackable tasks");

        let mut tasks = Vec::new();

        // Parse numbered list format: "1. Task description"
        for line in plan_text.lines() {
            let trimmed = line.trim();

            // Match patterns like "1. ", "2. ", "1) ", etc.
            if let Some(task_desc) = trimmed
                .strip_prefix(|c: char| c.is_numeric())
                .and_then(|s| s.strip_prefix('.').or(s.strip_prefix(')')))
                .map(|s| s.trim())
            {
                if !task_desc.is_empty() {
                    tasks.push(task_desc.to_string());
                }
            }
        }

        // Fallback: if no numbered tasks found, treat each non-empty line as a task
        if tasks.is_empty() {
            tasks = plan_text
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty() && l.len() > 3)
                .map(|l| l.to_string())
                .collect();
        }

        if tasks.is_empty() {
            warn!("[ENGINE] No tasks extracted from plan, creating single task");
            tasks.push("Execute the planned operation".to_string());
        }

        info!("[ENGINE] Extracted {} tasks from plan", tasks.len());

        // Create task records and emit events
        for (i, task_desc) in tasks.iter().enumerate() {
            let active_form = if task_desc.starts_with(char::is_uppercase) {
                // "Analyze code" → "Analyzing code"
                let mut chars = task_desc.chars();
                let first = chars.next().unwrap();
                let rest: String = chars.collect();
                format!("{}ing {}", first, rest.to_lowercase())
            } else {
                format!("Working on: {}", task_desc)
            };

            self.task_manager
                .create_task(
                    operation_id,
                    i as i32,
                    task_desc.clone(),
                    active_form,
                    event_tx,
                )
                .await?;
        }

        Ok(tasks)
    }
}
