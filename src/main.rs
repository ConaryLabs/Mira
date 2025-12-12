// backend/src/main.rs
// Mira Power Suit - MCP Server for Claude Code
// Consolidated tools for minimal token footprint (55→30 tools)

use anyhow::Result;
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::*,
    tool, tool_router, tool_handler,
    transport::{StreamableHttpService, StreamableHttpServerConfig},
    transport::streamable_http_server::session::local::LocalSessionManager,
};
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

mod tools;
mod indexer;
mod hooks;
use tools::*;
use indexer::{CodeIndexer, GitIndexer};

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
    db: Arc<SqlitePool>,
    semantic: Arc<SemanticSearch>,
    tool_router: ToolRouter<Self>,
    active_project: Arc<RwLock<Option<ProjectContext>>>,
}

impl MiraServer {
    pub async fn new(database_url: &str, qdrant_url: Option<&str>, gemini_key: Option<String>) -> Result<Self> {
        info!("Connecting to database: {}", database_url);
        let db = SqlitePool::connect(database_url).await?;
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

    // === Memory (core - high usage) ===

    #[tool(description = "Store a fact/decision/preference for future recall. Scoped to active project.")]
    async fn remember(&self, Parameters(req): Parameters<RememberRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let result = memory::remember(self.db.as_ref(), self.semantic.as_ref(), req, project_id).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Search memories using semantic similarity.")]
    async fn recall(&self, Parameters(req): Parameters<RecallRequest>) -> Result<CallToolResult, McpError> {
        let query = req.query.clone();
        let project_id = self.get_active_project().await.map(|p| p.id);
        let result = memory::recall(self.db.as_ref(), self.semantic.as_ref(), req, project_id).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No memories found matching '{}'", query)))
    }

    #[tool(description = "Delete a memory by ID.")]
    async fn forget(&self, Parameters(req): Parameters<ForgetRequest>) -> Result<CallToolResult, McpError> {
        let result = memory::forget(self.db.as_ref(), self.semantic.as_ref(), req).await.map_err(to_mcp_err)?;
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
        let result = sessions::store_session(self.db.as_ref(), self.semantic.as_ref(), req, project_id).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Search past sessions semantically.")]
    async fn search_sessions(&self, Parameters(req): Parameters<SearchSessionsRequest>) -> Result<CallToolResult, McpError> {
        let query = req.query.clone();
        let project_id = self.get_active_project().await.map(|p| p.id);
        let result = sessions::search_sessions(self.db.as_ref(), self.semantic.as_ref(), req, project_id).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No sessions found matching '{}'", query)))
    }

    #[tool(description = "Store an important decision with context.")]
    async fn store_decision(&self, Parameters(req): Parameters<StoreDecisionRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let result = sessions::store_decision(self.db.as_ref(), self.semantic.as_ref(), req, project_id).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    // === Project ===

    #[tool(description = "Set active project. Call at session start for scoped data.")]
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

    #[tool(description = "Get coding guidelines. Use category='mira_usage' for tool guidance.")]
    async fn get_guidelines(&self, Parameters(req): Parameters<GetGuidelinesRequest>) -> Result<CallToolResult, McpError> {
        let result = project::get_guidelines(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, "No guidelines found."))
    }

    #[tool(description = "Add a coding guideline or convention.")]
    async fn add_guideline(&self, Parameters(req): Parameters<AddGuidelineRequest>) -> Result<CallToolResult, McpError> {
        let result = project::add_guideline(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    // === Consolidated Task Tool (6→1) ===

    #[tool(description = "Manage tasks. Actions: create/list/get/update/complete/delete")]
    async fn task(&self, Parameters(req): Parameters<TaskRequest>) -> Result<CallToolResult, McpError> {
        let action = req.action.as_str();
        match action {
            "create" => {
                let title = req.title.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("title required for create")))?;
                let result = tasks::create_task(self.db.as_ref(), tasks::CreateTaskParams {
                    title,
                    description: req.description.clone(),
                    priority: req.priority.clone(),
                    parent_id: req.parent_id.clone(),
                }).await.map_err(to_mcp_err)?;
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
                let task_id = req.task_id.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("task_id required")))?;
                let result = tasks::get_task(self.db.as_ref(), &task_id).await.map_err(to_mcp_err)?;
                Ok(option_response(result, format!("Task {} not found", task_id)))
            }
            "update" => {
                let task_id = req.task_id.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("task_id required")))?;
                let result = tasks::update_task(self.db.as_ref(), tasks::UpdateTaskParams {
                    task_id,
                    title: req.title.clone(),
                    description: req.description.clone(),
                    status: req.status.clone(),
                    priority: req.priority.clone(),
                }).await.map_err(to_mcp_err)?;
                Ok(option_response(result, "Task not found"))
            }
            "complete" => {
                let task_id = req.task_id.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("task_id required")))?;
                let result = tasks::complete_task(self.db.as_ref(), &task_id, req.notes.clone()).await.map_err(to_mcp_err)?;
                Ok(option_response(result, format!("Task {} not found", task_id)))
            }
            "delete" => {
                let task_id = req.task_id.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("task_id required")))?;
                match tasks::delete_task(self.db.as_ref(), &task_id).await.map_err(to_mcp_err)? {
                    Some(title) => Ok(json_response(serde_json::json!({
                        "status": "deleted",
                        "task_id": task_id,
                        "title": title,
                    }))),
                    None => Ok(text_response(format!("Task {} not found", task_id))),
                }
            }
            _ => Ok(CallToolResult::error(vec![Content::text(format!("Unknown action: {}. Use create/list/get/update/complete/delete", action))])),
        }
    }

    // === Consolidated Goal Tool (7→1) ===

    #[tool(description = "Manage goals/milestones. Actions: create/list/get/update/add_milestone/complete_milestone/progress")]
    async fn goal(&self, Parameters(req): Parameters<GoalRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let action = req.action.as_str();
        match action {
            "create" => {
                let title = req.title.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("title required")))?;
                let result = goals::create_goal(self.db.as_ref(), goals::CreateGoalParams {
                    title,
                    description: req.description.clone(),
                    success_criteria: req.success_criteria.clone(),
                    priority: req.priority.clone(),
                }, project_id).await.map_err(to_mcp_err)?;
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
                let goal_id = req.goal_id.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("goal_id required")))?;
                let result = goals::get_goal(self.db.as_ref(), &goal_id).await.map_err(to_mcp_err)?;
                Ok(option_response(result, format!("Goal '{}' not found", goal_id)))
            }
            "update" => {
                let goal_id = req.goal_id.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("goal_id required")))?;
                let result = goals::update_goal(self.db.as_ref(), goals::UpdateGoalParams {
                    goal_id,
                    title: req.title.clone(),
                    description: req.description.clone(),
                    status: req.status.clone(),
                    priority: req.priority.clone(),
                    progress_percent: req.progress_percent,
                }).await.map_err(to_mcp_err)?;
                Ok(option_response(result, "Goal not found"))
            }
            "add_milestone" => {
                let goal_id = req.goal_id.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("goal_id required")))?;
                let title = req.title.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("title required")))?;
                let result = goals::add_milestone(self.db.as_ref(), goals::AddMilestoneParams {
                    goal_id,
                    title,
                    description: req.description.clone(),
                    weight: req.weight,
                }).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            "complete_milestone" => {
                let milestone_id = req.milestone_id.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("milestone_id required")))?;
                let result = goals::complete_milestone(self.db.as_ref(), &milestone_id).await.map_err(to_mcp_err)?;
                Ok(option_response(result, format!("Milestone '{}' not found", milestone_id)))
            }
            "progress" => {
                let result = goals::get_goal_progress(self.db.as_ref(), req.goal_id.clone(), project_id).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            _ => Ok(CallToolResult::error(vec![Content::text(format!("Unknown action: {}. Use create/list/get/update/add_milestone/complete_milestone/progress", action))])),
        }
    }

    // === Consolidated Correction Tool (4→1) ===

    #[tool(description = "Manage corrections. Actions: record/get/validate/list. Record when user corrects you.")]
    async fn correction(&self, Parameters(req): Parameters<CorrectionRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let action = req.action.as_str();
        match action {
            "record" => {
                let correction_type = req.correction_type.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("correction_type required")))?;
                let what_was_wrong = req.what_was_wrong.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("what_was_wrong required")))?;
                let what_is_right = req.what_is_right.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("what_is_right required")))?;
                let result = corrections::record_correction(self.db.as_ref(), self.semantic.as_ref(), corrections::RecordCorrectionParams {
                    correction_type,
                    what_was_wrong,
                    what_is_right,
                    rationale: req.rationale.clone(),
                    scope: req.scope.clone(),
                    keywords: req.keywords.clone(),
                }, project_id).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            "get" => {
                let result = corrections::get_corrections(self.db.as_ref(), self.semantic.as_ref(), corrections::GetCorrectionsParams {
                    file_path: req.file_path.clone(),
                    topic: req.topic.clone(),
                    correction_type: req.correction_type.clone(),
                    context: req.keywords.clone(), // Use keywords as context for semantic search
                    limit: req.limit,
                }, project_id).await.map_err(to_mcp_err)?;
                Ok(vec_response(result, "No corrections found."))
            }
            "validate" => {
                let correction_id = req.correction_id.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("correction_id required")))?;
                let outcome = req.outcome.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("outcome required")))?;
                let result = corrections::validate_correction(self.db.as_ref(), &correction_id, &outcome).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            "list" => {
                let result = corrections::list_corrections(self.db.as_ref(), corrections::ListCorrectionsParams {
                    correction_type: req.correction_type.clone(),
                    scope: req.scope.clone(),
                    status: None, // Default to active
                    limit: req.limit,
                }, project_id).await.map_err(to_mcp_err)?;
                Ok(vec_response(result, "No corrections found."))
            }
            _ => Ok(CallToolResult::error(vec![Content::text(format!("Unknown action: {}. Use record/get/validate/list", action))])),
        }
    }

    // === Consolidated Document Tool (3→1) ===

    #[tool(description = "Manage documents. Actions: list/search/get")]
    async fn document(&self, Parameters(req): Parameters<DocumentRequest>) -> Result<CallToolResult, McpError> {
        let action = req.action.as_str();
        match action {
            "list" => {
                let result = documents::list_documents(self.db.as_ref(), documents::ListDocumentsParams {
                    doc_type: req.doc_type.clone(),
                    limit: req.limit,
                }).await.map_err(to_mcp_err)?;
                Ok(vec_response(result, "No documents found."))
            }
            "search" => {
                let query = req.query.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("query required")))?;
                let result = documents::search_documents(self.db.as_ref(), self.semantic.as_ref(), &query, req.limit).await.map_err(to_mcp_err)?;
                Ok(vec_response(result, format!("No documents found matching '{}'", query)))
            }
            "get" => {
                let document_id = req.document_id.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("document_id required")))?;
                let result = documents::get_document(self.db.as_ref(), &document_id, req.include_content.unwrap_or(false)).await.map_err(to_mcp_err)?;
                Ok(option_response(result, format!("Document '{}' not found", document_id)))
            }
            _ => Ok(CallToolResult::error(vec![Content::text(format!("Unknown action: {}. Use list/search/get", action))])),
        }
    }

    // === Consolidated Permission Tool (3→1) ===

    #[tool(description = "Manage permission rules. Actions: save/list/delete. Save when user approves a tool.")]
    async fn permission(&self, Parameters(req): Parameters<PermissionRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let action = req.action.as_str();
        match action {
            "save" => {
                let tool_name = req.tool_name.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("tool_name required")))?;
                let result = permissions::save_permission(self.db.as_ref(), permissions::SavePermissionParams {
                    tool_name,
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
                let rule_id = req.rule_id.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("rule_id required")))?;
                let result = permissions::delete_permission(self.db.as_ref(), &rule_id).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            _ => Ok(CallToolResult::error(vec![Content::text(format!("Unknown action: {}. Use save/list/delete", action))])),
        }
    }

    // === Consolidated Build Tool (4→1) ===

    #[tool(description = "Manage build tracking. Actions: record/record_error/get_errors/resolve")]
    async fn build(&self, Parameters(req): Parameters<BuildRequest>) -> Result<CallToolResult, McpError> {
        let action = req.action.as_str();
        match action {
            "record" => {
                let command = req.command.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("command required")))?;
                let success = req.success.ok_or_else(|| to_mcp_err(anyhow::anyhow!("success required")))?;
                let result = build_intel::record_build(self.db.as_ref(), build_intel::RecordBuildParams {
                    command,
                    success,
                    duration_ms: req.duration_ms,
                }).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            "record_error" => {
                let message = req.message.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("message required")))?;
                let result = build_intel::record_build_error(self.db.as_ref(), build_intel::RecordBuildErrorParams {
                    message,
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
                let error_id = req.error_id.ok_or_else(|| to_mcp_err(anyhow::anyhow!("error_id required")))?;
                let result = build_intel::resolve_error(self.db.as_ref(), error_id).await.map_err(to_mcp_err)?;
                Ok(json_response(result))
            }
            _ => Ok(CallToolResult::error(vec![Content::text(format!("Unknown action: {}. Use record/record_error/get_errors/resolve", action))])),
        }
    }

    // === Code Intelligence (keep separate - distinct use cases) ===

    #[tool(description = "Get symbols (functions/classes/structs) from a file.")]
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

    #[tool(description = "Search code by meaning (semantic search).")]
    async fn semantic_code_search(&self, Parameters(req): Parameters<SemanticCodeSearchRequest>) -> Result<CallToolResult, McpError> {
        let query = req.query.clone();
        let result = code_intel::semantic_code_search(self.db.as_ref(), self.semantic.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No code found for '{}'", query)))
    }

    // === Git Intelligence ===

    #[tool(description = "Get recent commits, optionally filtered.")]
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

    #[tool(description = "Find similar past error fixes (semantic search).")]
    async fn find_similar_fixes(&self, Parameters(req): Parameters<FindSimilarFixesRequest>) -> Result<CallToolResult, McpError> {
        let error = req.error.clone();
        let result = git_intel::find_similar_fixes(self.db.as_ref(), self.semantic.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No fixes for: {}", error)))
    }

    #[tool(description = "Record an error fix for future learning.")]
    async fn record_error_fix(&self, Parameters(req): Parameters<RecordErrorFixRequest>) -> Result<CallToolResult, McpError> {
        let result = git_intel::record_error_fix(self.db.as_ref(), self.semantic.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    // === Proactive Context ===

    #[tool(description = "Get all context for current work: corrections, decisions, goals, errors.")]
    async fn get_proactive_context(&self, Parameters(req): Parameters<GetProactiveContextRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let result = proactive::get_proactive_context(self.db.as_ref(), self.semantic.as_ref(), req, project_id).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Record a rejected approach to avoid re-suggesting it.")]
    async fn record_rejected_approach(&self, Parameters(req): Parameters<RecordRejectedApproachRequest>) -> Result<CallToolResult, McpError> {
        let project_id = self.get_active_project().await.map(|p| p.id);
        let result = goals::record_rejected_approach(self.db.as_ref(), req, project_id).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    // === Indexing ===

    #[tool(description = "Index code and git history. Actions: project/file/status/cleanup")]
    async fn index(&self, Parameters(req): Parameters<IndexRequest>) -> Result<CallToolResult, McpError> {
        let action = req.action.as_str();
        match action {
            "project" => {
                let path = req.path.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("path required")))?;
                let path = std::path::Path::new(&path);

                let mut code_indexer = CodeIndexer::with_semantic(
                    self.db.as_ref().clone(),
                    Some(self.semantic.clone())
                ).map_err(to_mcp_err)?;
                let mut stats = code_indexer.index_directory(path).await.map_err(to_mcp_err)?;

                // Index git if requested (default: true)
                if req.include_git.unwrap_or(true) {
                    let git_indexer = GitIndexer::new(self.db.as_ref().clone());
                    let commit_limit = req.commit_limit.unwrap_or(500) as usize;
                    let git_stats = git_indexer.index_repository(path, commit_limit).await.map_err(to_mcp_err)?;
                    stats.merge(git_stats);
                }

                Ok(json_response(serde_json::json!({
                    "status": "indexed",
                    "files_processed": stats.files_processed,
                    "symbols_found": stats.symbols_found,
                    "imports_found": stats.imports_found,
                    "embeddings_generated": stats.embeddings_generated,
                    "commits_indexed": stats.commits_indexed,
                    "cochange_patterns": stats.cochange_patterns,
                    "errors": stats.errors,
                })))
            }
            "file" => {
                let path = req.path.clone().ok_or_else(|| to_mcp_err(anyhow::anyhow!("path required")))?;
                let path = std::path::Path::new(&path);

                let mut code_indexer = CodeIndexer::with_semantic(
                    self.db.as_ref().clone(),
                    Some(self.semantic.clone())
                ).map_err(to_mcp_err)?;
                let stats = code_indexer.index_file(path).await.map_err(to_mcp_err)?;

                Ok(json_response(serde_json::json!({
                    "status": "indexed",
                    "file": req.path,
                    "symbols_found": stats.symbols_found,
                    "imports_found": stats.imports_found,
                    "embeddings_generated": stats.embeddings_generated,
                })))
            }
            "status" => {
                // Get indexing status from database
                let symbols: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM code_symbols")
                    .fetch_one(self.db.as_ref())
                    .await
                    .map_err(|e| to_mcp_err(e.into()))?;
                let imports: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM imports")
                    .fetch_one(self.db.as_ref())
                    .await
                    .map_err(|e| to_mcp_err(e.into()))?;
                let commits: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM git_commits")
                    .fetch_one(self.db.as_ref())
                    .await
                    .map_err(|e| to_mcp_err(e.into()))?;
                let cochange: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM cochange_patterns")
                    .fetch_one(self.db.as_ref())
                    .await
                    .map_err(|e| to_mcp_err(e.into()))?;

                Ok(json_response(serde_json::json!({
                    "symbols_indexed": symbols.0,
                    "imports_indexed": imports.0,
                    "commits_indexed": commits.0,
                    "cochange_patterns": cochange.0,
                })))
            }
            "cleanup" => {
                // Remove stale data from excluded directories and orphaned entries
                let excluded_patterns = vec![
                    "%/target/%",
                    "%/node_modules/%",
                    "%/__pycache__/%",
                    "%/.git/%",
                ];

                let mut symbols_removed = 0i64;
                let mut calls_removed = 0i64;
                let mut imports_removed = 0i64;

                for pattern in &excluded_patterns {
                    // Remove call_graph entries first (foreign key constraints)
                    let result = sqlx::query(
                        "DELETE FROM call_graph WHERE caller_id IN (SELECT id FROM code_symbols WHERE file_path LIKE $1)"
                    )
                    .bind(pattern)
                    .execute(self.db.as_ref())
                    .await
                    .map_err(|e| to_mcp_err(e.into()))?;
                    calls_removed += result.rows_affected() as i64;

                    let result = sqlx::query(
                        "DELETE FROM call_graph WHERE callee_id IN (SELECT id FROM code_symbols WHERE file_path LIKE $1)"
                    )
                    .bind(pattern)
                    .execute(self.db.as_ref())
                    .await
                    .map_err(|e| to_mcp_err(e.into()))?;
                    calls_removed += result.rows_affected() as i64;

                    // Remove symbols
                    let result = sqlx::query("DELETE FROM code_symbols WHERE file_path LIKE $1")
                        .bind(pattern)
                        .execute(self.db.as_ref())
                        .await
                        .map_err(|e| to_mcp_err(e.into()))?;
                    symbols_removed += result.rows_affected() as i64;

                    // Remove imports
                    let result = sqlx::query("DELETE FROM imports WHERE file_path LIKE $1")
                        .bind(pattern)
                        .execute(self.db.as_ref())
                        .await
                        .map_err(|e| to_mcp_err(e.into()))?;
                    imports_removed += result.rows_affected() as i64;
                }

                // Also clean up orphaned call_graph entries (where caller or callee no longer exists)
                let result = sqlx::query(
                    "DELETE FROM call_graph WHERE caller_id NOT IN (SELECT id FROM code_symbols) OR callee_id NOT IN (SELECT id FROM code_symbols)"
                )
                .execute(self.db.as_ref())
                .await
                .map_err(|e| to_mcp_err(e.into()))?;
                let orphans_removed = result.rows_affected() as i64;

                Ok(json_response(serde_json::json!({
                    "status": "cleaned",
                    "symbols_removed": symbols_removed,
                    "calls_removed": calls_removed + orphans_removed,
                    "imports_removed": imports_removed,
                    "patterns_cleaned": excluded_patterns,
                })))
            }
            _ => Ok(CallToolResult::error(vec![Content::text(format!(
                "Unknown action: {}. Use project/file/status/cleanup", action
            ))])),
        }
    }
}

#[tool_handler]
impl ServerHandler for MiraServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some("Mira Power Suit - Memory and Intelligence Layer for Claude Code. Features: semantic memory (remember/recall), cross-session context, persistent tasks, code intelligence, git intelligence, and document search. All search tools use semantic similarity when Qdrant + Gemini are configured.".to_string()),
        }
    }
}

mod daemon;

use clap::{Parser, Subcommand};
use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};

/// Auth middleware that checks for Bearer token
async fn auth_middleware(
    req: Request<Body>,
    next: Next,
    expected_token: String,
) -> Result<Response, StatusCode> {
    // Check Authorization header
    if let Some(auth_header) = req.headers().get("authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if auth_str.starts_with("Bearer ") {
                let token = &auth_str[7..];
                if token == expected_token {
                    return Ok(next.run(req).await);
                }
            }
        }
    }

    // Also check X-Auth-Token header for simpler clients
    if let Some(token_header) = req.headers().get("x-auth-token") {
        if let Ok(token) = token_header.to_str() {
            if token == expected_token {
                return Ok(next.run(req).await);
            }
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}

/// Graceful shutdown signal handler
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install CTRL+C signal handler");
    info!("Shutdown signal received, stopping server...");
}

#[derive(Parser)]
#[command(name = "mira")]
#[command(about = "Memory and Intelligence Layer for Claude Code")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the MCP server over stdio (default)
    Serve,
    /// Run the MCP server over HTTP/SSE
    ServeHttp {
        /// Port to listen on
        #[arg(short, long, default_value = "3000")]
        port: u16,
        /// Auth token (required for connections)
        #[arg(short, long, env = "MIRA_AUTH_TOKEN")]
        auth_token: Option<String>,
    },
    /// Daemon management commands
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// Claude Code hook handlers (for use in settings.json)
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
}

#[derive(Subcommand)]
enum HookAction {
    /// Handle PermissionRequest hooks - auto-approve based on saved rules
    Permission,
    /// Handle PreCompact hooks - save context before conversation compaction
    Precompact,
    /// Handle PostToolCall hooks - auto-remember significant actions
    Posttool,
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the background watcher daemon
    Start {
        /// Project path to watch (defaults to current directory)
        #[arg(short, long)]
        path: Option<String>,
    },
    /// Stop the daemon
    Stop {
        /// Project path (defaults to current directory)
        #[arg(short, long)]
        path: Option<String>,
    },
    /// Check daemon status
    Status {
        /// Project path (defaults to current directory)
        #[arg(short, long)]
        path: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    match cli.command {
        None | Some(Commands::Serve) => {
            // Default: run MCP server over stdio
            info!("Starting Mira MCP Server (stdio)...");

            let database_url = std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://data/mira.db".to_string());
            let qdrant_url = std::env::var("QDRANT_URL").ok();
            let gemini_key = std::env::var("GEMINI_API_KEY")
                .or_else(|_| std::env::var("GOOGLE_API_KEY"))
                .ok();

            let server = MiraServer::new(&database_url, qdrant_url.as_deref(), gemini_key).await?;
            info!("Server initialized");

            let service = server.serve(rmcp::transport::stdio()).await?;
            service.waiting().await?;
        }
        Some(Commands::ServeHttp { port, auth_token }) => {
            // Run MCP server over HTTP/SSE
            info!("Starting Mira MCP Server (HTTP/SSE) on port {}...", port);

            let database_url = std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://data/mira.db".to_string());
            let qdrant_url = std::env::var("QDRANT_URL").ok();
            let gemini_key = std::env::var("GEMINI_API_KEY")
                .or_else(|_| std::env::var("GOOGLE_API_KEY"))
                .ok();

            // Create shared state that will be cloned for each session
            let db = Arc::new(SqlitePool::connect(&database_url).await?);
            let semantic = Arc::new(SemanticSearch::new(qdrant_url.as_deref(), gemini_key).await);
            info!("Database connected");

            // Optional auth token validation
            let expected_token = auth_token.clone();

            // Create the MCP service with StreamableHttpService
            let mcp_service = StreamableHttpService::new(
                {
                    let db = db.clone();
                    let semantic = semantic.clone();
                    move || {
                        Ok(MiraServer {
                            db: db.clone(),
                            semantic: semantic.clone(),
                            tool_router: MiraServer::tool_router(),
                            active_project: Arc::new(RwLock::new(None)),
                        })
                    }
                },
                Arc::new(LocalSessionManager::default()),
                StreamableHttpServerConfig::default(),
            );

            // Build router with optional auth middleware
            let app = if let Some(token) = expected_token {
                info!("Auth token required for connections");
                axum::Router::new()
                    .nest_service("/mcp", mcp_service)
                    .layer(axum::middleware::from_fn(move |req, next| {
                        let token = token.clone();
                        auth_middleware(req, next, token)
                    }))
            } else {
                info!("Warning: No auth token set, server is open");
                axum::Router::new().nest_service("/mcp", mcp_service)
            };

            let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
            info!("Listening on http://0.0.0.0:{}/mcp", port);

            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal())
                .await?;
        }
        Some(Commands::Daemon { action }) => {
            let database_url = std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://data/mira.db".to_string());
            let qdrant_url = std::env::var("QDRANT_URL").ok();
            let gemini_key = std::env::var("GEMINI_API_KEY")
                .or_else(|_| std::env::var("GOOGLE_API_KEY"))
                .ok();

            match action {
                DaemonAction::Start { path } => {
                    let project_path = path
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|| std::env::current_dir().unwrap());

                    // Check if already running
                    if let Some(pid) = daemon::is_running(&project_path) {
                        println!("Daemon already running with PID {}", pid);
                        return Ok(());
                    }

                    info!("Starting Mira daemon for {}", project_path.display());
                    let d = daemon::Daemon::new(
                        &project_path,
                        &database_url,
                        qdrant_url.as_deref(),
                        gemini_key,
                    ).await?;
                    d.run().await?;
                }
                DaemonAction::Stop { path } => {
                    let project_path = path
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|| std::env::current_dir().unwrap());

                    if daemon::stop(&project_path)? {
                        println!("Daemon stopped");
                    } else {
                        println!("No daemon running");
                    }
                }
                DaemonAction::Status { path } => {
                    let project_path = path
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|| std::env::current_dir().unwrap());

                    if let Some(pid) = daemon::is_running(&project_path) {
                        println!("Daemon running with PID {}", pid);
                    } else {
                        println!("Daemon not running");
                    }
                }
            }
        }
        Some(Commands::Hook { action }) => {
            match action {
                HookAction::Permission => {
                    hooks::permission::run().await?;
                }
                HookAction::Precompact => {
                    hooks::precompact::run().await?;
                }
                HookAction::Posttool => {
                    hooks::posttool::run().await?;
                }
            }
        }
    }

    Ok(())
}
