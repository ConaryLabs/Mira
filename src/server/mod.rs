//! Mira MCP Server - Core server implementation
//!
//! NOTE: Some items are infrastructure for future features or external use.

#![allow(dead_code)] // Server infrastructure (some items for future use)

mod db;
mod handlers;

use anyhow::Result;
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::*,
    tool, tool_router, tool_handler,
};
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use crate::tools::*;

// Re-export database utilities
pub use db::{create_optimized_pool, run_migrations};

/// Macro to extract a required field from a request, returning an error if missing.
/// Usage: `require!(req.title, "title required for create")`
macro_rules! require {
    ($field:expr, $msg:expr) => {
        $field.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!($msg)))?
    };
}

/// Helper to create an unknown action error response
fn unknown_action(action: &str, valid_actions: &str) -> CallToolResult {
    CallToolResult::error(vec![Content::text(format!(
        "Unknown action: {}. Use {}", action, valid_actions
    ))])
}

// === Project Context ===

/// Active project context for scoping data
#[derive(Clone, Debug)]
pub struct ProjectContext {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub project_type: Option<String>,
}

// === Mira MCP Server ===

#[derive(Clone)]
pub struct MiraServer {
    pub db: Arc<SqlitePool>,
    pub semantic: Arc<SemanticSearch>,
    pub tool_router: ToolRouter<Self>,
    pub active_project: Arc<RwLock<Option<ProjectContext>>>,
}

impl MiraServer {
    pub async fn new(database_url: &str, qdrant_url: Option<&str>, gemini_key: Option<String>) -> Result<Self> {
        info!("Connecting to database: {}", database_url);
        let db = create_optimized_pool(database_url).await?;
        info!("Database connected successfully");

        let semantic = SemanticSearch::new(qdrant_url, gemini_key).await;
        if semantic.is_available() {
            info!("Semantic search enabled (Qdrant + Gemini)");
        } else {
            info!("Semantic search disabled (using text-based fallback)");
        }

        Ok(Self {
            db: Arc::new(db),
            semantic: Arc::new(semantic),
            tool_router: Self::tool_router(),
            active_project: Arc::new(RwLock::new(None)),
        })
    }

    /// Get the active project context (if set)
    pub async fn get_active_project(&self) -> Option<ProjectContext> {
        self.active_project.read().await.clone()
    }

    /// Set the active project context
    pub async fn set_active_project(&self, ctx: Option<ProjectContext>) {
        *self.active_project.write().await = ctx;
    }

    /// Get the tool router (public wrapper for macro-generated function)
    pub fn get_tool_router() -> ToolRouter<Self> {
        Self::tool_router()
    }
}

#[tool_router]
impl MiraServer {
    // === Admin/Analytics ===

    #[tool(description = "List database tables with row counts.")]
    async fn list_tables(&self) -> Result<CallToolResult, McpError> {
        let result = analytics::list_tables(self.db.as_ref()).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Execute read-only SQL SELECT query.")]
    async fn query(&self, Parameters(req): Parameters<QueryRequest>) -> Result<CallToolResult, McpError> {
        match analytics::query(self.db.as_ref(), req).await {
            Ok(result) => Ok(json_response(result)),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }

    // === Hotline - Talk to Mira (GPT-5.2) ===

    #[tool(description = "Talk to Mira (GPT-5.2) for advice or collaboration.")]
    // Note: Provider param allows switching to DeepSeek V3.2. See HotlineRequest schema.
    async fn hotline(&self, Parameters(req): Parameters<HotlineRequest>) -> Result<CallToolResult, McpError> {
        let project = self.get_active_project().await;
        let project_id = project.as_ref().map(|p| p.id);
        let project_name = project.as_ref().map(|p| p.name.as_str());
        let project_type = project.as_ref().and_then(|p| p.project_type.as_deref());
        let message_preview: String = req.message.chars().take(100).collect();
        let provider = req.provider.clone().unwrap_or_else(|| "openai".to_string());
        let start = std::time::Instant::now();

        match hotline::call_mira(
            req,
            self.db.as_ref(),
            &self.semantic,
            project_id,
            project_name,
            project_type,
        ).await {
            Ok(result) => {
                let _ = mcp_history::log_call_semantic(
                    self.db.as_ref(),
                    &self.semantic,
                    None,
                    project_id,
                    "hotline",
                    Some(&serde_json::json!({"message": &message_preview, "provider": &provider})),
                    &format!("Asked {}: {}", provider, message_preview),
                    true,
                    Some(start.elapsed().as_millis() as i64),
                ).await;
                Ok(json_response(result))
            }
            Err(e) => {
                let _ = mcp_history::log_call_semantic(
                    self.db.as_ref(),
                    &self.semantic,
                    None,
                    project_id,
                    "hotline",
                    Some(&serde_json::json!({"message": &message_preview, "provider": &provider})),
                    &format!("Failed {}: {}", provider, e),
                    false,
                    Some(start.elapsed().as_millis() as i64),
                ).await;
                Ok(CallToolResult::error(vec![Content::text(format!("Hotline error: {}", e))]))
            }
        }
    }

    #[tool(description = "Manage advisory sessions. Actions: list/get/close/pin/decide")]
    async fn advisory_session(&self, Parameters(req): Parameters<AdvisorySessionRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        match handlers::advisory::handle(self.db.as_ref(), project_id, &req).await {
            Ok(result) => Ok(json_response(result)),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }

    // === Memory (core - high usage) ===

    #[tool(description = "Store a fact/decision/preference for future recall. Scoped to active project.")]
    async fn remember(&self, Parameters(req): Parameters<RememberRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let content_preview: String = req.content.chars().take(100).collect();
        let category = req.category.clone();
        let start = std::time::Instant::now();

        let result = memory::remember(self.db.as_ref(), &self.semantic, req, project_id).await.map_err(to_mcp_err)?;

        // Log to MCP history with semantic embedding
        let _ = mcp_history::log_call_semantic(
            self.db.as_ref(),
            &self.semantic,
            None, // session_id not available at tool level
            project_id,
            "remember",
            Some(&serde_json::json!({"content": &content_preview, "category": category})),
            &format!("Stored: {}", content_preview),
            true,
            Some(start.elapsed().as_millis() as i64),
        ).await;

        Ok(json_response(result))
    }

    #[tool(description = "Search memories using semantic similarity.")]
    async fn recall(&self, Parameters(req): Parameters<RecallRequest>) -> Result<CallToolResult, McpError> {
        let query = req.query.clone();
        let project_id = self.get_active_project().await.map(|p| p.id);
        let start = std::time::Instant::now();

        let result = memory::recall(self.db.as_ref(), &self.semantic, req, project_id).await.map_err(to_mcp_err)?;

        // Log to MCP history with semantic embedding
        let _ = mcp_history::log_call_semantic(
            self.db.as_ref(),
            &self.semantic,
            None,
            project_id,
            "recall",
            Some(&serde_json::json!({"query": &query})),
            &format!("Searched: {} ({} results)", query, result.len()),
            true,
            Some(start.elapsed().as_millis() as i64),
        ).await;

        Ok(vec_response(result, format!("No memories found matching '{}'", query)))
    }

    #[tool(description = "Delete a memory by ID.")]
    async fn forget(&self, Parameters(req): Parameters<ForgetRequest>) -> Result<CallToolResult, McpError> {
        let result = memory::forget(self.db.as_ref(), &self.semantic, req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    // === Session Context ===

    #[tool(description = "Initialize session: sets project, loads persona, context, corrections, goals. Call once at session start.")]
    async fn session_start(&self, Parameters(req): Parameters<SessionStartRequest>) -> Result<CallToolResult, McpError> {
        let result = sessions::session_start(self.db.as_ref(), req).await.map_err(to_mcp_err)?;

        // Set active project context so subsequent calls are scoped
        let ctx = ProjectContext {
            id: result.project_id,
            path: result.project_path.clone(),
            name: result.project_name.clone(),
            project_type: result.project_type.clone(),
        };
        self.set_active_project(Some(ctx)).await;

        // Return formatted output
        Ok(text_response(format::session_start(&result)))
    }

    #[tool(description = "Get context from previous sessions. Call at session start.")]
    async fn get_session_context(&self, Parameters(req): Parameters<GetSessionContextRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let result = sessions::get_session_context(self.db.as_ref(), req, project_id).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Store session summary. Call at session end.")]
    async fn store_session(&self, Parameters(req): Parameters<StoreSessionRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let summary_preview = req.summary.chars().take(100).collect::<String>();
        let start = std::time::Instant::now();

        let result = sessions::store_session(self.db.as_ref(), self.semantic.as_ref(), req, project_id).await.map_err(to_mcp_err)?;

        // Log to MCP history with semantic embedding
        let _ = mcp_history::log_call_semantic(
            self.db.as_ref(),
            &self.semantic,
            None,
            project_id,
            "store_session",
            None,
            &format!("Session saved: {}", summary_preview),
            true,
            Some(start.elapsed().as_millis() as i64),
        ).await;

        Ok(json_response(result))
    }

    #[tool(description = "Search past sessions semantically.")]
    async fn search_sessions(&self, Parameters(req): Parameters<SearchSessionsRequest>) -> Result<CallToolResult, McpError> {
        let query = req.query.clone();
        let project_id = self.get_active_project().await.map(|p| p.id);
        let result = sessions::search_sessions(self.db.as_ref(), self.semantic.as_ref(), req, project_id).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No sessions found matching '{}'", query)))
    }

    #[tool(description = "Search MCP tool call history. Useful for recalling what tools were used and their results.")]
    async fn search_mcp_history(&self, Parameters(req): Parameters<SearchMcpHistoryRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let limit = req.limit.unwrap_or(20) as usize;

        // Use semantic search when query is provided, otherwise use regular search
        let result = if let Some(query) = &req.query {
            mcp_history::semantic_search(
                self.db.as_ref(),
                &self.semantic,
                query,
                project_id,
                limit,
            ).await.map_err(to_mcp_err)?
        } else {
            mcp_history::search_history(
                self.db.as_ref(),
                project_id,
                req.tool_name.as_deref(),
                None,
                limit as i64,
            ).await.map_err(to_mcp_err)?
        };

        Ok(vec_response(result, "No MCP history found".to_string()))
    }

    #[tool(description = "Store an important decision with context.")]
    async fn store_decision(&self, Parameters(req): Parameters<StoreDecisionRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let decision_key = req.key.clone();
        let start = std::time::Instant::now();

        let result = sessions::store_decision(self.db.as_ref(), self.semantic.as_ref(), req, project_id).await.map_err(to_mcp_err)?;

        // Log to MCP history with semantic embedding
        let _ = mcp_history::log_call_semantic(
            self.db.as_ref(),
            &self.semantic,
            None,
            project_id,
            "store_decision",
            Some(&serde_json::json!({"key": &decision_key})),
            &format!("Decision: {}", decision_key),
            true,
            Some(start.elapsed().as_millis() as i64),
        ).await;

        Ok(json_response(result))
    }

    #[tool(description = "Sync work state for session resume.")]
    async fn sync_work_state(&self, Parameters(req): Parameters<SyncWorkStateRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let result = sessions::sync_work_state(self.db.as_ref(), req, project_id).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Get work state for session resume.")]
    async fn get_work_state(&self, Parameters(req): Parameters<GetWorkStateRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let result = sessions::get_work_state(self.db.as_ref(), req, project_id).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, "No work state found".to_string()))
    }

    // === Project ===

    #[tool(description = "Set active project.")]
    async fn set_project(&self, Parameters(req): Parameters<SetProjectRequest>) -> Result<CallToolResult, McpError> {
        let result = project::set_project(self.db.as_ref(), req).await.map_err(to_mcp_err)?;

        if let Some(id) = result.get("id").and_then(|v| v.as_i64()) {
            let ctx = ProjectContext {
                id,
                path: result.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                name: result.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                project_type: result.get("project_type").and_then(|v| v.as_str()).map(|s| s.to_string()),
            };
            self.set_active_project(Some(ctx)).await;
        }

        Ok(json_response(result))
    }

    #[tool(description = "Get currently active project.")]
    async fn get_project(&self, Parameters(_req): Parameters<GetProjectRequest>) -> Result<CallToolResult, McpError> {
        match self.get_active_project().await {
            Some(ctx) => Ok(json_response(serde_json::json!({
                "id": ctx.id,
                "path": ctx.path,
                "name": ctx.name,
                "project_type": ctx.project_type,
            }))),
            None => Ok(json_response(serde_json::json!({
                "active": false,
                "message": "No project set. Call set_project() first."
            }))),
        }
    }

    #[tool(description = "Get coding guidelines.")]
    async fn get_guidelines(&self, Parameters(req): Parameters<GetGuidelinesRequest>) -> Result<CallToolResult, McpError> {
        let result = project::get_guidelines(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, "No guidelines found."))
    }

    #[tool(description = "Add a coding guideline.")]
    async fn add_guideline(&self, Parameters(req): Parameters<AddGuidelineRequest>) -> Result<CallToolResult, McpError> {
        let result = project::add_guideline(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    // === Consolidated Task Tool (6→1) ===

    #[tool(description = "Manage tasks. Actions: create/list/get/update/complete/delete")]
    async fn task(&self, Parameters(req): Parameters<TaskRequest>) -> Result<CallToolResult, McpError> {
        match req.action.as_str() {
            "create" => {
                let title = require!(req.title, "title required for create");
                let project_id = self.get_active_project().await.map(|p| p.id);
                let result = tasks::create_task(self.db.as_ref(), tasks::CreateTaskParams {
                    title: title.clone(),
                    description: req.description.clone(),
                    priority: req.priority.clone(),
                    parent_id: req.parent_id.clone(),
                }).await.map_err(to_mcp_err)?;

                let _ = mcp_history::log_call_semantic(
                    self.db.as_ref(),
                    &self.semantic,
                    None,
                    project_id,
                    "task",
                    Some(&serde_json::json!({"action": "create", "title": &title})),
                    &format!("Created task: {}", title),
                    true,
                    None,
                ).await;

                Ok(json_response(result))
            }
            "list" => {
                let result = tasks::list_tasks(self.db.as_ref(), tasks::ListTasksParams {
                    status: req.status.clone(),
                    parent_id: req.parent_id.clone(),
                    include_completed: req.include_completed,
                    limit: req.limit,
                }).await.map_err(to_mcp_err)?;
                Ok(vec_response(result, "No tasks found."))
            }
            "get" => {
                let task_id = require!(req.task_id, "task_id required");
                let result = tasks::get_task(self.db.as_ref(), &task_id).await.map_err(to_mcp_err)?;
                Ok(option_response(result, format!("Task {} not found", task_id)))
            }
            "update" => {
                let result = tasks::update_task(self.db.as_ref(), tasks::UpdateTaskParams {
                    task_id: require!(req.task_id, "task_id required"),
                    title: req.title.clone(),
                    description: req.description.clone(),
                    status: req.status.clone(),
                    priority: req.priority.clone(),
                }).await.map_err(to_mcp_err)?;
                Ok(option_response(result, "Task not found"))
            }
            "complete" => {
                let task_id = require!(req.task_id, "task_id required");
                let project_id = self.get_active_project().await.map(|p| p.id);
                let result = tasks::complete_task(self.db.as_ref(), &task_id, req.notes.clone()).await.map_err(to_mcp_err)?;

                if let Some(ref task) = result {
                    let title = task.get("title").and_then(|t| t.as_str()).unwrap_or("unknown");
                    let _ = mcp_history::log_call_semantic(
                        self.db.as_ref(),
                        &self.semantic,
                        None,
                        project_id,
                        "task",
                        Some(&serde_json::json!({"action": "complete", "task_id": &task_id, "title": title})),
                        &format!("Completed task: {}", title),
                        true,
                        None,
                    ).await;
                }

                Ok(option_response(result, format!("Task {} not found", task_id)))
            }
            "delete" => {
                let task_id = require!(req.task_id, "task_id required");
                match tasks::delete_task(self.db.as_ref(), &task_id).await.map_err(to_mcp_err)? {
                    Some(title) => Ok(json_response(serde_json::json!({
                        "status": "deleted",
                        "task_id": task_id,
                        "title": title,
                    }))),
                    None => Ok(text_response(format!("Task {} not found", task_id))),
                }
            }
            action => Ok(unknown_action(action, "create/list/get/update/complete/delete")),
        }
    }

    // === Consolidated Goal Tool (7→1) ===

    #[tool(description = "Manage goals/milestones. Actions: create/list/get/update/delete/add_milestone/complete_milestone/progress")]
    async fn goal(&self, Parameters(req): Parameters<GoalRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        match req.action.as_str() {
            "create" => {
                let title = require!(req.title, "title required");
                let result = goals::create_goal(self.db.as_ref(), goals::CreateGoalParams {
                    title: title.clone(),
                    description: req.description.clone(),
                    success_criteria: req.success_criteria.clone(),
                    priority: req.priority.clone(),
                }, project_id).await.map_err(to_mcp_err)?;

                let _ = mcp_history::log_call_semantic(
                    self.db.as_ref(),
                    &self.semantic,
                    None,
                    project_id,
                    "goal",
                    Some(&serde_json::json!({"action": "create", "title": &title})),
                    &format!("Created goal: {}", title),
                    true,
                    None,
                ).await;

                Ok(json_response(result))
            }
            "list" => {
                let result = goals::list_goals(self.db.as_ref(), goals::ListGoalsParams {
                    status: req.status.clone(),
                    include_finished: req.include_finished,
                    limit: req.limit,
                }, project_id).await.map_err(to_mcp_err)?;
                Ok(vec_response(result, "No goals found."))
            }
            "get" => {
                let goal_id = require!(req.goal_id, "goal_id required");
                let result = goals::get_goal(self.db.as_ref(), &goal_id).await.map_err(to_mcp_err)?;
                Ok(option_response(result, format!("Goal '{}' not found", goal_id)))
            }
            "update" => {
                let result = goals::update_goal(self.db.as_ref(), goals::UpdateGoalParams {
                    goal_id: require!(req.goal_id, "goal_id required"),
                    title: req.title.clone(),
                    description: req.description.clone(),
                    status: req.status.clone(),
                    priority: req.priority.clone(),
                    progress_percent: req.progress_percent,
                }).await.map_err(to_mcp_err)?;
                Ok(option_response(result, "Goal not found"))
            }
            "add_milestone" => {
                let result = goals::add_milestone(self.db.as_ref(), goals::AddMilestoneParams {
                    goal_id: require!(req.goal_id, "goal_id required"),
                    title: require!(req.title, "title required"),
                    description: req.description.clone(),
                    weight: req.weight,
                }).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            "complete_milestone" => {
                let milestone_id = require!(req.milestone_id, "milestone_id required");
                let result = goals::complete_milestone(self.db.as_ref(), &milestone_id).await.map_err(to_mcp_err)?;
                Ok(option_response(result, format!("Milestone '{}' not found", milestone_id)))
            }
            "progress" => {
                let result = goals::get_goal_progress(self.db.as_ref(), req.goal_id.clone(), project_id).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            "delete" => {
                let goal_id = require!(req.goal_id, "goal_id required");
                match goals::delete_goal(self.db.as_ref(), &goal_id).await.map_err(to_mcp_err)? {
                    Some(title) => Ok(json_response(serde_json::json!({
                        "status": "deleted",
                        "goal_id": goal_id,
                        "title": title,
                    }))),
                    None => Ok(text_response(format!("Goal {} not found", goal_id))),
                }
            }
            action => Ok(unknown_action(action, "create/list/get/update/delete/add_milestone/complete_milestone/progress")),
        }
    }

    // === Proposal Tool (Proactive Organization System) ===

    #[tool(description = "Manage proposals (auto-extracted goals/tasks/decisions). Actions: extract/list/confirm/reject/review")]
    async fn proposal(&self, Parameters(req): Parameters<types::ProposalRequest>) -> Result<CallToolResult, McpError> {
        use crate::core::ops::proposals;

        let ctx = crate::core::OpContext::new(std::env::current_dir().unwrap_or_default())
            .with_db(self.db.as_ref().clone());

        match req.action.as_str() {
            "extract" => {
                let text = require!(req.text, "text required for extract");
                let base_confidence = req.base_confidence.unwrap_or(0.5);

                // First try pattern-based extraction
                let matches = proposals::extract_from_text(&ctx, &text, base_confidence)
                    .await.map_err(|e| to_mcp_err(e.into()))?;

                let mut created = Vec::new();
                let mut extraction_method = "pattern";

                let mut skipped_dupes = 0;

                if matches.is_empty() {
                    // Fallback to LLM-based extraction
                    extraction_method = "llm";
                    match proposals::extract_with_llm(&text).await {
                        Ok(llm_results) if !llm_results.is_empty() => {
                            for r in llm_results {
                                // Check for duplicate before creating
                                if let Ok(Some(existing_id)) = proposals::find_duplicate(&ctx, &r.content).await {
                                    tracing::debug!("Skipping duplicate proposal, matches: {}", existing_id);
                                    skipped_dupes += 1;
                                    continue;
                                }

                                let ptype: proposals::ProposalType = r.proposal_type.parse()
                                    .unwrap_or(proposals::ProposalType::Task);

                                let evidence = serde_json::json!({
                                    "source": "llm",
                                    "llm_confidence": r.confidence,
                                });

                                let proposal = proposals::create_proposal(
                                    &ctx,
                                    ptype,
                                    &r.content,
                                    r.title.as_deref(),
                                    r.confidence.min(0.7), // Cap LLM confidence at 0.7
                                    Some(&evidence.to_string()),
                                    Some("extract_llm"),
                                    None,
                                ).await.map_err(|e| to_mcp_err(e.into()))?;

                                created.push(serde_json::json!({
                                    "id": proposal.id,
                                    "type": proposal.proposal_type.to_string(),
                                    "content": proposal.content,
                                    "confidence": proposal.confidence,
                                    "status": proposal.status.to_string(),
                                }));
                            }
                        }
                        Ok(_) => {
                            return Ok(text_response("No proposals detected in text."));
                        }
                        Err(e) => {
                            return Ok(text_response(format!("Pattern extraction found nothing, LLM fallback failed: {}", e)));
                        }
                    }
                } else {
                    // Process pattern matches
                    for m in matches {
                        // Check for duplicate before creating
                        if let Ok(Some(existing_id)) = proposals::find_duplicate(&ctx, &m.full_context).await {
                            tracing::debug!("Skipping duplicate proposal, matches: {}", existing_id);
                            skipped_dupes += 1;
                            continue;
                        }

                        let evidence = serde_json::json!({
                            "pattern_id": m.pattern_id,
                            "matched_text": m.matched_text,
                            "context": m.full_context,
                        });

                        let proposal = proposals::create_proposal(
                            &ctx,
                            m.proposal_type,
                            &m.full_context,
                            None,
                            m.confidence,
                            Some(&evidence.to_string()),
                            Some("extract"),
                            None,
                        ).await.map_err(|e| to_mcp_err(e.into()))?;

                        created.push(serde_json::json!({
                            "id": proposal.id,
                            "type": proposal.proposal_type.to_string(),
                            "content": proposal.content,
                            "confidence": proposal.confidence,
                            "status": proposal.status.to_string(),
                        }));
                    }
                }

                let mut response = serde_json::json!({
                    "extracted": created.len(),
                    "method": extraction_method,
                    "proposals": created,
                });
                if skipped_dupes > 0 {
                    response["skipped_duplicates"] = serde_json::json!(skipped_dupes);
                }
                Ok(json_response(response))
            }
            "list" => {
                let status = req.status.as_ref().and_then(|s| s.parse().ok());
                let ptype = req.proposal_type.as_ref().and_then(|t| t.parse().ok());
                let limit = req.limit.unwrap_or(20);

                let props = proposals::list_proposals(&ctx, status, ptype, limit)
                    .await.map_err(|e| to_mcp_err(e.into()))?;

                if props.is_empty() {
                    return Ok(text_response("No proposals found."));
                }

                let results: Vec<_> = props.iter().map(|p| serde_json::json!({
                    "id": p.id,
                    "type": p.proposal_type.to_string(),
                    "content": if p.content.len() > 100 { format!("{}...", &p.content[..100]) } else { p.content.clone() },
                    "confidence": p.confidence,
                    "status": p.status.to_string(),
                })).collect();

                Ok(vec_response(results, "No proposals found."))
            }
            "confirm" => {
                let proposal_id = require!(req.proposal_id, "proposal_id required");
                match proposals::confirm_proposal(&ctx, &proposal_id).await.map_err(|e| to_mcp_err(e.into()))? {
                    Some(msg) => Ok(text_response(msg)),
                    None => Ok(text_response(format!("Proposal {} not found", proposal_id))),
                }
            }
            "reject" => {
                let proposal_id = require!(req.proposal_id, "proposal_id required");
                match proposals::reject_proposal(&ctx, &proposal_id).await.map_err(|e| to_mcp_err(e.into()))? {
                    Some(msg) => Ok(text_response(msg)),
                    None => Ok(text_response(format!("Proposal {} not found or already processed", proposal_id))),
                }
            }
            "review" => {
                // Get pending proposals for batch review
                let limit = req.limit.unwrap_or(10);
                let pending = proposals::get_pending_review(&ctx, limit)
                    .await.map_err(|e| to_mcp_err(e.into()))?;

                if pending.is_empty() {
                    return Ok(text_response("No pending proposals to review."));
                }

                let mut output = format!("{} pending proposals:\n\n", pending.len());
                for p in &pending {
                    output.push_str(&format!(
                        "- [{}] {} ({:.0}% confidence)\n  {}\n\n",
                        p.id,
                        p.proposal_type,
                        p.confidence * 100.0,
                        if p.content.len() > 80 { format!("{}...", &p.content[..80]) } else { p.content.clone() },
                    ));
                }
                output.push_str("Use confirm/reject with proposal_id to process.");

                Ok(text_response(output))
            }
            action => Ok(unknown_action(action, "extract/list/confirm/reject/review")),
        }
    }

    // === Consolidated Correction Tool (4→1) ===

    #[tool(description = "Manage corrections. Actions: record/get/validate/list")]
    async fn correction(&self, Parameters(req): Parameters<CorrectionRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        match req.action.as_str() {
            "record" => {
                let what_was_wrong = require!(req.what_was_wrong, "what_was_wrong required");
                let what_is_right = require!(req.what_is_right, "what_is_right required");
                let result = corrections::record_correction(self.db.as_ref(), &self.semantic, corrections::RecordCorrectionParams {
                    correction_type: require!(req.correction_type, "correction_type required"),
                    what_was_wrong: what_was_wrong.clone(),
                    what_is_right: what_is_right.clone(),
                    rationale: req.rationale.clone(),
                    scope: req.scope.clone(),
                    keywords: req.keywords.clone(),
                }, project_id).await.map_err(to_mcp_err)?;

                let _ = mcp_history::log_call_semantic(
                    self.db.as_ref(),
                    &self.semantic,
                    None,
                    project_id,
                    "correction",
                    Some(&serde_json::json!({"action": "record", "wrong": &what_was_wrong, "right": &what_is_right})),
                    &format!("Correction: {} → {}", what_was_wrong.chars().take(50).collect::<String>(), what_is_right.chars().take(50).collect::<String>()),
                    true,
                    None,
                ).await;

                Ok(json_response(result))
            }
            "get" => {
                let result = corrections::get_corrections(self.db.as_ref(), &self.semantic, corrections::GetCorrectionsParams {
                    file_path: req.file_path.clone(),
                    topic: req.topic.clone(),
                    correction_type: req.correction_type.clone(),
                    context: req.keywords.clone(),
                    limit: req.limit,
                }, project_id).await.map_err(to_mcp_err)?;
                Ok(vec_response(result, "No corrections found."))
            }
            "validate" => {
                let result = corrections::validate_correction(
                    self.db.as_ref(),
                    &require!(req.correction_id, "correction_id required"),
                    &require!(req.outcome, "outcome required"),
                ).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            "list" => {
                let result = corrections::list_corrections(self.db.as_ref(), corrections::ListCorrectionsParams {
                    correction_type: req.correction_type.clone(),
                    scope: req.scope.clone(),
                    status: None,
                    limit: req.limit,
                }, project_id).await.map_err(to_mcp_err)?;
                Ok(vec_response(result, "No corrections found."))
            }
            action => Ok(unknown_action(action, "record/get/validate/list")),
        }
    }

    // === Consolidated Document Tool (3→1) ===

    #[tool(description = "Manage documents. Actions: list/search/get/ingest/delete")]
    async fn document(&self, Parameters(req): Parameters<DocumentRequest>) -> Result<CallToolResult, McpError> {
        match req.action.as_str() {
            "list" => {
                let result = documents::list_documents(self.db.as_ref(), documents::ListDocumentsParams {
                    doc_type: req.doc_type.clone(),
                    limit: req.limit,
                }).await.map_err(to_mcp_err)?;
                Ok(vec_response(result, "No documents found."))
            }
            "search" => {
                let query = require!(req.query, "query required");
                let result = documents::search_documents(self.db.as_ref(), self.semantic.clone(), &query, req.limit).await.map_err(to_mcp_err)?;
                Ok(vec_response(result, format!("No documents found matching '{}'", query)))
            }
            "get" => {
                let document_id = require!(req.document_id, "document_id required");
                let result = documents::get_document(self.db.as_ref(), &document_id, req.include_content.unwrap_or(false)).await.map_err(to_mcp_err)?;
                Ok(option_response(result, format!("Document '{}' not found", document_id)))
            }
            "ingest" => {
                let path = require!(req.path, "path required for ingest");
                let result = ingest::ingest_document(
                    self.db.as_ref(),
                    Some(self.semantic.as_ref()),
                    &path,
                    req.name.as_deref(),
                ).await.map_err(to_mcp_err)?;
                Ok(json_response(serde_json::json!({
                    "status": "ingested",
                    "document_id": result.document_id,
                    "name": result.name,
                    "doc_type": result.doc_type,
                    "chunk_count": result.chunk_count,
                    "total_tokens": result.total_tokens,
                })))
            }
            "delete" => {
                let document_id = require!(req.document_id, "document_id required for delete");
                let deleted = ingest::delete_document(self.db.as_ref(), Some(self.semantic.as_ref()), &document_id).await.map_err(to_mcp_err)?;
                if deleted {
                    Ok(text_response(format!("Document '{}' deleted", document_id)))
                } else {
                    Ok(text_response(format!("Document '{}' not found", document_id)))
                }
            }
            action => Ok(unknown_action(action, "list/search/get/ingest/delete")),
        }
    }

    // === Consolidated Permission Tool (3→1) ===

    #[tool(description = "Manage permission rules. Actions: save/list/delete.")]
    async fn permission(&self, Parameters(req): Parameters<PermissionRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        match req.action.as_str() {
            "save" => {
                let result = permissions::save_permission(self.db.as_ref(), permissions::SavePermissionParams {
                    tool_name: require!(req.tool_name, "tool_name required"),
                    input_field: req.input_field.clone(),
                    input_pattern: req.input_pattern.clone(),
                    match_type: req.match_type.clone(),
                    scope: req.scope.clone(),
                    description: req.description.clone(),
                }, project_id).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            "list" => {
                let result = permissions::list_permissions(self.db.as_ref(), permissions::ListPermissionsParams {
                    tool_name: req.tool_name.clone(),
                    scope: req.scope.clone(),
                    limit: req.limit,
                }, project_id).await.map_err(to_mcp_err)?;
                Ok(vec_response(result, "No permission rules found."))
            }
            "delete" => {
                let result = permissions::delete_permission(self.db.as_ref(), &require!(req.rule_id, "rule_id required")).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            action => Ok(unknown_action(action, "save/list/delete")),
        }
    }

    // === Consolidated Build Tool (4→1) ===

    #[tool(description = "Manage build tracking. Actions: record/record_error/get_errors/resolve")]
    async fn build(&self, Parameters(req): Parameters<BuildRequest>) -> Result<CallToolResult, McpError> {
        match req.action.as_str() {
            "record" => {
                let result = build_intel::record_build(self.db.as_ref(), build_intel::RecordBuildParams {
                    command: require!(req.command, "command required"),
                    success: require!(req.success, "success required"),
                    duration_ms: req.duration_ms,
                }).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            "record_error" => {
                let result = build_intel::record_build_error(self.db.as_ref(), build_intel::RecordBuildErrorParams {
                    message: require!(req.message, "message required"),
                    category: req.category.clone(),
                    severity: req.severity.clone(),
                    file_path: req.file_path.clone(),
                    line_number: req.line_number,
                    code: req.code.clone(),
                }).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            "get_errors" => {
                let result = build_intel::get_build_errors(self.db.as_ref(), build_intel::GetBuildErrorsParams {
                    file_path: req.file_path.clone(),
                    category: req.category.clone(),
                    include_resolved: req.include_resolved,
                    limit: req.limit,
                }).await.map_err(to_mcp_err)?;
                Ok(vec_response(result, "No build errors found."))
            }
            "resolve" => {
                let result = build_intel::resolve_error(self.db.as_ref(), require!(req.error_id, "error_id required")).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            action => Ok(unknown_action(action, "record/record_error/get_errors/resolve")),
        }
    }

    // === Code Intelligence (keep separate - distinct use cases) ===

    #[tool(description = "Get symbols from a file.")]
    async fn get_symbols(&self, Parameters(req): Parameters<GetSymbolsRequest>) -> Result<CallToolResult, McpError> {
        let file_path = req.file_path.clone();
        let result = code_intel::get_symbols(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No symbols in '{}'", file_path)))
    }

    #[tool(description = "Get call graph for a function.")]
    async fn get_call_graph(&self, Parameters(req): Parameters<GetCallGraphRequest>) -> Result<CallToolResult, McpError> {
        let result = code_intel::get_call_graph(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Find related files via imports or co-change patterns.")]
    async fn get_related_files(&self, Parameters(req): Parameters<GetRelatedFilesRequest>) -> Result<CallToolResult, McpError> {
        let result = code_intel::get_related_files(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Search code by meaning.")]
    async fn semantic_code_search(&self, Parameters(req): Parameters<SemanticCodeSearchRequest>) -> Result<CallToolResult, McpError> {
        let query = req.query.clone();
        let result = code_intel::semantic_code_search(self.db.as_ref(), self.semantic.clone(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No code found for '{}'", query)))
    }

    #[tool(description = "Get codebase style metrics.")]
    async fn get_codebase_style(&self, Parameters(req): Parameters<GetCodebaseStyleRequest>) -> Result<CallToolResult, McpError> {
        let project_path = match req.project_path {
            Some(p) => p,
            None => self.get_active_project().await.map(|p| p.path).unwrap_or_else(|| ".".to_string()),
        };
        let report = code_intel::analyze_codebase_style(self.db.as_ref(), &project_path).await.map_err(to_mcp_err)?;
        Ok(text_response(format::style_report(&report)))
    }

    // === Git Intelligence ===

    #[tool(description = "Get recent commits.")]
    async fn get_recent_commits(&self, Parameters(req): Parameters<GetRecentCommitsRequest>) -> Result<CallToolResult, McpError> {
        let result = git_intel::get_recent_commits(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, "No commits found"))
    }

    #[tool(description = "Search commits by message.")]
    async fn search_commits(&self, Parameters(req): Parameters<SearchCommitsRequest>) -> Result<CallToolResult, McpError> {
        let query = req.query.clone();
        let result = git_intel::search_commits(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No commits for '{}'", query)))
    }

    #[tool(description = "Find files that change together.")]
    async fn find_cochange_patterns(&self, Parameters(req): Parameters<FindCochangeRequest>) -> Result<CallToolResult, McpError> {
        let file_path = req.file_path.clone();
        let result = git_intel::find_cochange_patterns(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No co-changes for '{}'", file_path)))
    }

    #[tool(description = "Find similar past error fixes.")]
    async fn find_similar_fixes(&self, Parameters(req): Parameters<FindSimilarFixesRequest>) -> Result<CallToolResult, McpError> {
        let error = req.error.clone();
        let result = git_intel::find_similar_fixes(self.db.as_ref(), &self.semantic, req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No fixes for: {}", error)))
    }

    #[tool(description = "Record an error fix.")]
    async fn record_error_fix(&self, Parameters(req): Parameters<RecordErrorFixRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let error_preview: String = req.error_pattern.chars().take(80).collect();
        let fix_preview: String = req.fix_description.chars().take(80).collect();

        let result = git_intel::record_error_fix(self.db.as_ref(), &self.semantic, req).await.map_err(to_mcp_err)?;

        let _ = mcp_history::log_call_semantic(
            self.db.as_ref(),
            &self.semantic,
            None,
            project_id,
            "record_error_fix",
            Some(&serde_json::json!({"error": &error_preview, "fix": &fix_preview})),
            &format!("Error fix: {} → {}", error_preview, fix_preview),
            true,
            None,
        ).await;

        Ok(json_response(result))
    }

    // === Proactive Context ===

    #[tool(description = "Get all context for current work: corrections, decisions, goals, errors.")]
    async fn get_proactive_context(&self, Parameters(req): Parameters<GetProactiveContextRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let result = proactive::get_proactive_context(self.db.as_ref(), &self.semantic, req, project_id).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Record a rejected approach.")]
    async fn record_rejected_approach(&self, Parameters(req): Parameters<RecordRejectedApproachRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let approach_preview: String = req.approach.chars().take(80).collect();
        let reason_preview: String = req.rejection_reason.chars().take(80).collect();

        let result = goals::record_rejected_approach(self.db.as_ref(), &self.semantic, req, project_id).await.map_err(to_mcp_err)?;

        let _ = mcp_history::log_call_semantic(
            self.db.as_ref(),
            &self.semantic,
            None,
            project_id,
            "record_rejected_approach",
            Some(&serde_json::json!({"approach": &approach_preview, "reason": &reason_preview})),
            &format!("Rejected: {} ({})", approach_preview, reason_preview),
            true,
            None,
        ).await;

        Ok(json_response(result))
    }

    // === Indexing ===

    #[tool(description = "Index code and git history. Actions: project/file/status/cleanup")]
    async fn index(&self, Parameters(req): Parameters<IndexRequest>) -> Result<CallToolResult, McpError> {
        use handlers::indexing;

        let result = match req.action.as_str() {
            "project" => indexing::index_project(self.db.as_ref(), self.semantic.clone(), &req).await,
            "file" => indexing::index_file(self.db.as_ref(), self.semantic.clone(), &req).await,
            "status" => indexing::index_status(self.db.as_ref()).await,
            "cleanup" => indexing::index_cleanup(self.db.as_ref()).await,
            action => return Ok(unknown_action(action, "project/file/status/cleanup")),
        };

        match result {
            Ok(r) => Ok(json_response(r)),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }
}

#[tool_handler]
impl ServerHandler for MiraServer {
    fn get_info(&self) -> ServerInfo {
        // Include current date/time so Claude always knows the actual date
        let now = chrono::Local::now();
        let date_str = now.format("%Y-%m-%d %H:%M:%S %Z").to_string();

        let instructions = format!(
            "Mira Power Suit - Memory and Intelligence Layer for Claude Code.\n\n\
            CURRENT DATE/TIME: {}\n\n\
            Features: semantic memory (remember/recall), cross-session context, persistent tasks, \
            code intelligence, git intelligence, and document search. All search tools use semantic \
            similarity when Qdrant + Gemini are configured.",
            date_str
        );

        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_06_18,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "mira".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                ..Default::default()
            },
            instructions: Some(instructions),
        }
    }
}
