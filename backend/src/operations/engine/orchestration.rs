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
    deepseek_orchestrator::DeepSeekOrchestrator, events::OperationEngineEvent,
    lifecycle::LifecycleManager, tool_router::ToolRouter, skills::SkillRegistry,
    simple_mode::SimpleModeDetector,
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
    deepseek_orchestrator: Option<Arc<DeepSeekOrchestrator>>,
    memory_service: Arc<MemoryService>,
    context_builder: ContextBuilder,
    context_loader: ContextLoader,
    delegation_handler: DelegationHandler,
    tool_router: Option<Arc<ToolRouter>>, // File operation routing
    skill_registry: Arc<SkillRegistry>, // Skills system for specialized tasks
    artifact_manager: ArtifactManager,
    lifecycle_manager: LifecycleManager,
    task_manager: TaskManager,
}

impl Orchestrator {
    pub fn new(
        gpt5: Gpt5Provider,
        deepseek_orchestrator: Option<Arc<DeepSeekOrchestrator>>,
        memory_service: Arc<MemoryService>,
        context_builder: ContextBuilder,
        context_loader: ContextLoader,
        delegation_handler: DelegationHandler,
        tool_router: Option<Arc<ToolRouter>>,
        skill_registry: Arc<SkillRegistry>,
        artifact_manager: ArtifactManager,
        lifecycle_manager: LifecycleManager,
        task_manager: TaskManager,
    ) -> Self {
        Self {
            gpt5,
            deepseek_orchestrator,
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

        // Route to DeepSeek if enabled
        if crate::config::CONFIG.use_deepseek_codegen && self.deepseek_orchestrator.is_some() {
            info!("[ENGINE] DeepSeek orchestration enabled, using DeepSeek dual-model path");
            return self
                .execute_with_deepseek(
                    operation_id,
                    session_id,
                    user_content,
                    system_prompt,
                    event_tx,
                )
                .await;
        }

        info!("[ENGINE] Using GPT-5 orchestration path");

        // Check complexity and generate plan for complex operations
        let simplicity = SimpleModeDetector::simplicity_score(user_content);
        let (plan_text, _task_ids): (Option<String>, Vec<String>) = if simplicity <= 0.7 {
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
            let task_ids = task_records.into_iter().map(|t| t.id).collect();

            (Some(plan), task_ids)
        } else {
            info!(
                "[ENGINE] Simple operation detected (score: {:.2}), skipping planning",
                simplicity
            );
            (None, Vec::new())
        };

        // Build tools: delegation tools + create_artifact
        let mut tools = get_delegation_tools();
        tools.push(get_create_artifact_tool_schema());

        let tool_names: Vec<String> = tools
            .iter()
            .filter_map(|t| {
                t.get("name")
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

        // Build execution message: if we have a plan, tell GPT-5 to execute it
        let execution_message = if let Some(ref plan) = plan_text {
            info!("[ENGINE] Transitioning to execution phase with plan");
            self.lifecycle_manager
                .update_status(operation_id, "executing", event_tx)
                .await?;

            Message::user(format!(
                "Original request: {}\n\n\
                Execution Plan:\n{}\n\n\
                === EXECUTION MODE ACTIVATED ===\n\n\
                You are now in EXECUTION MODE. Your job is to CALL TOOLS to execute each step of the plan above.\n\n\
                CRITICAL EXECUTION RULES:\n\
                1. Make ONE tool call per response - the loop will continue automatically\n\
                2. DO NOT write explanations - just call the appropriate tool for the next step\n\
                3. For creating files: Use write_project_file with the file path and complete content\n\
                4. For editing files: Use edit_project_file or read_project_file first to see content\n\
                5. For searching code: Use search_codebase or find_function\n\
                6. Each tool call will be executed immediately and you'll see the results\n\
                7. After each tool executes, you'll be called again to make the NEXT tool call\n\n\
                Example - if step 1 is 'Create file /tmp/hello.txt':\n\
                Correct response: {{call write_project_file with path='/tmp/hello.txt', content='Hello World'}}\n\
                Wrong response: \"I'll create the file by calling write_project_file...\" (NO! Just call the tool)\n\n\
                Start executing step 1 of the plan NOW by making the appropriate tool call.",
                user_content, plan
            ))
        } else {
            messages[0].clone()
        };

        let mut conversation_messages = vec![execution_message];

        // Tool use loop: continue calling GPT-5 until no more tool calls are made
        let mut accumulated_text = String::new();
        let mut delegation_calls = Vec::new();
        let mut current_response_id = previous_response_id;
        let max_tool_iterations = 10; // Safety limit to prevent infinite loops
        let mut iteration = 0;

        loop {
            iteration += 1;
            if iteration > max_tool_iterations {
                warn!("[ENGINE] Max tool iterations ({}) reached, stopping", max_tool_iterations);
                break;
            }

            info!("[ENGINE] Tool use iteration {}/{}", iteration, max_tool_iterations);

            // Track tool calls and results in this iteration
            let mut tool_calls_in_iteration = Vec::new();
            let mut tool_results_for_next_iteration = Vec::new();

            // Stream GPT-5 responses and handle tool calls
            let mut stream = self
                .gpt5
                .create_stream_with_tools(conversation_messages.clone(), system_prompt.clone(), tools.clone(), current_response_id.clone(), None)
                .await
                .context("Failed to create GPT-5 stream")?;

            let mut iteration_text = String::new();
            let mut iteration_response_id = None;

            // Process tool calls during streaming
            while let Some(event) = stream.next().await {
            if let Some(token) = &cancel_token {
                if token.is_cancelled() {
                    return Err(anyhow::anyhow!("Operation cancelled"));
                }
            }

            match event? {
                Gpt5StreamEvent::TextDelta { delta } => {
                    iteration_text.push_str(&delta);
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

                    // Track this tool call for loop continuation
                    tool_calls_in_iteration.push((id.clone(), name.clone(), arguments.clone()));

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

                                    // Determine tool type and build summary
                                    let (tool_type, summary) = match name.as_str() {
                                        "write_project_file" => {
                                            let path = result.get("path").and_then(|p| p.as_str()).unwrap_or("unknown");
                                            let lines = result.get("lines_written").and_then(|l| l.as_u64()).unwrap_or(0);
                                            ("file_write", format!("Wrote {} ({} lines)", path, lines))
                                        },
                                        "edit_project_file" => {
                                            let path = result.get("path").and_then(|p| p.as_str()).unwrap_or("unknown");
                                            let replacements = result.get("replacements_made").and_then(|r| r.as_u64()).unwrap_or(0);
                                            ("file_edit", format!("Edited {} ({} replacements)", path, replacements))
                                        },
                                        "read_project_file" => {
                                            let file_count = result.get("files_read").and_then(|c| c.as_u64()).unwrap_or(1);
                                            ("file_read", format!("Read {} file(s)", file_count))
                                        },
                                        tool if tool.starts_with("git_") => {
                                            ("git", format!("Executed {}", name))
                                        },
                                        _ => ("other", format!("Executed {}", name))
                                    };

                                    // Emit ToolExecuted event
                                    let _ = event_tx
                                        .send(OperationEngineEvent::ToolExecuted {
                                            operation_id: operation_id.to_string(),
                                            tool_name: name.clone(),
                                            tool_type: tool_type.to_string(),
                                            summary,
                                            success: true,
                                            details: Some(result.clone()),
                                        })
                                        .await;

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

                                    // Store tool result for next iteration
                                    tool_results_for_next_iteration.push((
                                        id.clone(),
                                        serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string())
                                    ));
                                }
                                Err(e) => {
                                    warn!("[ENGINE] File operation failed: {}", e);

                                    // Emit failed ToolExecuted event
                                    let tool_type = if name.starts_with("write_") {
                                        "file_write"
                                    } else if name.starts_with("edit_") {
                                        "file_edit"
                                    } else if name.starts_with("git_") {
                                        "git"
                                    } else {
                                        "other"
                                    };

                                    let _ = event_tx
                                        .send(OperationEngineEvent::ToolExecuted {
                                            operation_id: operation_id.to_string(),
                                            tool_name: name.clone(),
                                            tool_type: tool_type.to_string(),
                                            summary: format!("Failed to execute {}: {}", name, e),
                                            success: false,
                                            details: None,
                                        })
                                        .await;

                                    let _ = event_tx
                                        .send(OperationEngineEvent::Streaming {
                                            operation_id: operation_id.to_string(),
                                            content: format!("\n[File Operation Error: {}]\n", e),
                                        })
                                        .await;

                                    // Store error result for next iteration
                                    tool_results_for_next_iteration.push((
                                        id.clone(),
                                        format!(r#"{{"error": "{}"}}"#, e.to_string().replace('"', "\\\""))
                                    ));
                                }
                            }
                        } else {
                            warn!("[ENGINE] ToolRouter not available for {}", name);
                            // Store error for missing router
                            tool_results_for_next_iteration.push((
                                id.clone(),
                                r#"{"error": "ToolRouter not available"}"#.to_string()
                            ));
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
                    iteration_response_id = Some(response_id);
                }
                _ => {}
            }
        }

        // After stream ends: accumulate text and check if we should continue loop
        accumulated_text.push_str(&iteration_text);

        info!("[ENGINE] Iteration {} complete: {} tool calls made", iteration, tool_calls_in_iteration.len());

        // If no tool calls were made, we're done - break the loop
        if tool_calls_in_iteration.is_empty() {
            info!("[ENGINE] No tool calls in iteration, execution complete");
            break;
        }

        // Update response_id and append tool results for next iteration
        if let Some(response_id) = iteration_response_id {
            info!("[ENGINE] Continuing to iteration {} with response_id: {}", iteration + 1, response_id);
            info!("[ENGINE] {} tool results to append", tool_results_for_next_iteration.len());

            // Append tool results to conversation messages for next iteration
            for (call_id, output) in tool_results_for_next_iteration {
                conversation_messages.push(Message::tool_result(call_id, output));
            }

            current_response_id = Some(response_id);
        } else {
            warn!("[ENGINE] No response_id captured, cannot continue loop");
            break;
        }
    } // End of tool use loop

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

    /// Execute operation using DeepSeek dual-model orchestration
    /// Simplified path that delegates to DeepSeekOrchestrator
    async fn execute_with_deepseek(
        &self,
        operation_id: &str,
        session_id: &str,
        user_content: &str,
        system_prompt: String,
        event_tx: &mpsc::Sender<OperationEngineEvent>,
    ) -> Result<()> {
        let deepseek = match &self.deepseek_orchestrator {
            Some(orch) => orch,
            None => return Err(anyhow::anyhow!("DeepSeek orchestrator not initialized")),
        };

        // Build messages with system prompt
        let messages = vec![
            Message::system(system_prompt),
            Message::user(user_content.to_string()),
        ];

        // Build tools for DeepSeek
        let tools = get_delegation_tools();

        // Execute with DeepSeek orchestrator
        let response = deepseek
            .execute(operation_id, messages, tools, event_tx)
            .await
            .context("DeepSeek orchestration failed")?;

        // Complete operation
        self.lifecycle_manager
            .complete_operation(
                operation_id,
                session_id,
                Some(response),
                event_tx,
                vec![], // Artifacts are handled by DeepSeek orchestrator
            )
            .await?;

        Ok(())
    }
}
