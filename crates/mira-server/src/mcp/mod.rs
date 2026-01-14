// crates/mira-server/src/mcp/mod.rs
// MCP Server implementation

mod extraction;

use crate::tools::core as tools;

use std::collections::HashMap;
use tokio::sync::oneshot;

use crate::background::watcher::WatcherHandle;
use crate::db::Database;
use crate::embeddings::Embeddings;
use crate::llm::DeepSeekClient;
use mira_types::{AgentRole, ProjectContext, WsEvent};
use rmcp::{
    handler::server::{router::tool::ToolRouter, tool::ToolCallContext, wrapper::Parameters},
    model::{
        CallToolRequestParam, CallToolResult, ListToolsResult, PaginatedRequestParam,
        ServerCapabilities, ServerInfo,
    },
    schemars,
    service::{RequestContext, RoleServer},
    tool, tool_router, ErrorData, ServerHandler,
};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;

/// MCP Server state
#[derive(Clone)]
pub struct MiraServer {
    pub db: Arc<Database>,
    pub embeddings: Option<Arc<Embeddings>>,
    pub deepseek: Option<Arc<DeepSeekClient>>,
    pub project: Arc<RwLock<Option<ProjectContext>>>,
    /// Current session ID (generated on first tool call or session_start)
    pub session_id: Arc<RwLock<Option<String>>>,
    /// WebSocket broadcaster (unused in MCP-only mode)
    pub ws_tx: Option<tokio::sync::broadcast::Sender<mira_types::WsEvent>>,
    /// File watcher handle for registering projects
    pub watcher: Option<WatcherHandle>,
    /// Pending responses for agent collaboration (message_id -> response sender)
    pub pending_responses: Arc<RwLock<HashMap<String, oneshot::Sender<String>>>>,
    tool_router: ToolRouter<Self>,
}

impl MiraServer {
    pub fn new(db: Arc<Database>, embeddings: Option<Arc<Embeddings>>) -> Self {
        // Try to create DeepSeek client from env
        let deepseek = std::env::var("DEEPSEEK_API_KEY")
            .ok()
            .map(|key| Arc::new(DeepSeekClient::new(key)));

        Self {
            db,
            embeddings,
            deepseek,
            project: Arc::new(RwLock::new(None)),
            session_id: Arc::new(RwLock::new(None)),
            ws_tx: None,
            watcher: None,
            pending_responses: Arc::new(RwLock::new(HashMap::new())),
            tool_router: Self::tool_router(),
        }
    }

    /// Create with a file watcher for incremental indexing
    pub fn with_watcher(
        db: Arc<Database>,
        embeddings: Option<Arc<Embeddings>>,
        watcher: WatcherHandle,
    ) -> Self {
        let deepseek = std::env::var("DEEPSEEK_API_KEY")
            .ok()
            .map(|key| Arc::new(DeepSeekClient::new(key)));

        Self {
            db,
            embeddings,
            deepseek,
            project: Arc::new(RwLock::new(None)),
            session_id: Arc::new(RwLock::new(None)),
            ws_tx: None,
            watcher: Some(watcher),
            pending_responses: Arc::new(RwLock::new(HashMap::new())),
            tool_router: Self::tool_router(),
        }
    }

    /// Create with a broadcast channel and watcher (for future embedding scenarios)
    #[allow(dead_code)]
    pub fn with_broadcaster(
        db: Arc<Database>,
        embeddings: Option<Arc<Embeddings>>,
        deepseek: Option<Arc<DeepSeekClient>>,
        ws_tx: tokio::sync::broadcast::Sender<mira_types::WsEvent>,
        session_id: Arc<RwLock<Option<String>>>,
        project: Arc<RwLock<Option<ProjectContext>>>,
        pending_responses: Arc<RwLock<HashMap<String, oneshot::Sender<String>>>>,
        watcher: Option<WatcherHandle>,
    ) -> Self {
        Self {
            db,
            embeddings,
            deepseek,
            project,
            session_id,
            ws_tx: Some(ws_tx),
            watcher,
            pending_responses,
            tool_router: Self::tool_router(),
        }
    }

    /// Get or create the current session ID
    pub async fn get_or_create_session(&self) -> String {
        let mut session_guard = self.session_id.write().await;
        if let Some(ref id) = *session_guard {
            return id.clone();
        }

        // Generate new session ID
        let new_id = uuid::Uuid::new_v4().to_string();

        // Get project_id if available
        let project_id = self.project.read().await.as_ref().map(|p| p.id);

        // Create session in database
        if let Err(e) = self.db.create_session(&new_id, project_id) {
            eprintln!("[SESSION] Failed to create session: {}", e);
        }

        *session_guard = Some(new_id.clone());
        new_id
    }

    /// Broadcast an event (no-op in MCP-only mode)
    pub fn broadcast(&self, event: mira_types::WsEvent) {
        if let Some(tx) = &self.ws_tx {
            let receiver_count = tx.receiver_count();
            eprintln!("[BROADCAST] Sending {:?} to {} receivers", event, receiver_count);
            match tx.send(event) {
                Ok(n) => eprintln!("[BROADCAST] Sent to {} receivers", n),
                Err(e) => eprintln!("[BROADCAST] Error: {:?}", e),
            }
        } else {
            eprintln!("[BROADCAST] No ws_tx configured!");
        }
    }
}

// Request types for tools with parameters
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SessionStartRequest {
    #[schemars(description = "Project root path")]
    pub project_path: String,
    #[schemars(description = "Project name")]
    pub name: Option<String>,
    #[schemars(description = "Session ID (from Claude Code). If not provided, one will be generated.")]
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetProjectRequest {
    #[schemars(description = "Project root path")]
    pub project_path: String,
    #[schemars(description = "Project name")]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RememberRequest {
    #[schemars(description = "Content to remember")]
    pub content: String,
    #[schemars(description = "Key for upsert")]
    pub key: Option<String>,
    #[schemars(description = "Type: preference/decision/context/general")]
    pub fact_type: Option<String>,
    #[schemars(description = "Category")]
    pub category: Option<String>,
    #[schemars(description = "Confidence/truthiness (0.0-1.0, default 1.0). Use 0.8 for compaction summaries.")]
    pub confidence: Option<f64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RecallRequest {
    #[schemars(description = "Search query")]
    pub query: String,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
    #[schemars(description = "Filter by category")]
    pub category: Option<String>,
    #[schemars(description = "Filter by type")]
    pub fact_type: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ForgetRequest {
    #[schemars(description = "Memory ID to delete")]
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetSymbolsRequest {
    #[schemars(description = "File path")]
    pub file_path: String,
    #[schemars(description = "Symbol type")]
    pub symbol_type: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SemanticCodeSearchRequest {
    #[schemars(description = "Query")]
    pub query: String,
    #[schemars(description = "Language")]
    pub language: Option<String>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FindCallersRequest {
    #[schemars(description = "Function name to find callers for")]
    pub function_name: String,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FindCalleesRequest {
    #[schemars(description = "Function name to find callees for")]
    pub function_name: String,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CheckCapabilityRequest {
    #[schemars(description = "Description of the capability/feature to check for (e.g., 'semantic search', 'git change tracking')")]
    pub description: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TaskRequest {
    #[schemars(description = "Action: create/bulk_create/list/get/update/complete/delete")]
    pub action: String,
    #[schemars(description = "Task ID")]
    pub task_id: Option<String>,
    #[schemars(description = "Title")]
    pub title: Option<String>,
    #[schemars(description = "Description")]
    pub description: Option<String>,
    #[schemars(description = "Status: pending/in_progress/completed/blocked")]
    pub status: Option<String>,
    #[schemars(description = "Priority: low/medium/high/urgent")]
    pub priority: Option<String>,
    #[schemars(description = "Parent task ID")]
    pub parent_id: Option<String>,
    #[schemars(description = "Completion notes")]
    pub notes: Option<String>,
    #[schemars(description = "Include completed")]
    pub include_completed: Option<bool>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
    #[schemars(description = "For bulk_create: JSON array of tasks [{title, description?, priority?}, ...]")]
    pub tasks: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GoalRequest {
    #[schemars(description = "Action: create/bulk_create/list/get/update/delete/add_milestone/complete_milestone/progress")]
    pub action: String,
    #[schemars(description = "Goal ID")]
    pub goal_id: Option<String>,
    #[schemars(description = "Title")]
    pub title: Option<String>,
    #[schemars(description = "Description")]
    pub description: Option<String>,
    #[schemars(description = "Status: planning/in_progress/blocked/completed/abandoned")]
    pub status: Option<String>,
    #[schemars(description = "Priority: low/medium/high/critical")]
    pub priority: Option<String>,
    #[schemars(description = "Success criteria")]
    pub success_criteria: Option<String>,
    #[schemars(description = "Progress percent (0-100)")]
    pub progress_percent: Option<i32>,
    #[schemars(description = "Include finished goals")]
    pub include_finished: Option<bool>,
    #[schemars(description = "Milestone ID")]
    pub milestone_id: Option<String>,
    #[schemars(description = "Milestone weight")]
    pub weight: Option<i32>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
    #[schemars(description = "For bulk_create: JSON array of goals [{title, description?, priority?}, ...]")]
    pub goals: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct IndexRequest {
    #[schemars(description = "Action: project/file/status")]
    pub action: String,
    #[schemars(description = "Path")]
    pub path: Option<String>,
    #[schemars(description = "Commit limit")]
    pub commit_limit: Option<i64>,
    #[schemars(description = "Parallel")]
    pub parallel: Option<bool>,
    #[schemars(description = "Max workers")]
    pub max_workers: Option<i64>,
    #[schemars(description = "Skip embedding generation (faster indexing)")]
    pub skip_embed: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SessionHistoryRequest {
    #[schemars(description = "Action: list_sessions/get_history/current")]
    pub action: String,
    #[schemars(description = "Session ID (for get_history)")]
    pub session_id: Option<String>,
    #[schemars(description = "Max results")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReplyToMiraRequest {
    #[schemars(description = "The message_id you are replying to")]
    pub in_reply_to: String,
    #[schemars(description = "Your response content")]
    pub content: String,
    #[schemars(description = "Is your response complete? Set to false if you need more information.")]
    pub complete: Option<bool>,
}

// Expert consultation request types
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConsultArchitectRequest {
    #[schemars(description = "Code, design, or situation to analyze")]
    pub context: String,
    #[schemars(description = "Specific question to answer (optional)")]
    pub question: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConsultPlanReviewerRequest {
    #[schemars(description = "Implementation plan to review")]
    pub context: String,
    #[schemars(description = "Specific concern to address (optional)")]
    pub question: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConsultScopeAnalystRequest {
    #[schemars(description = "Requirements or plan to analyze for gaps")]
    pub context: String,
    #[schemars(description = "Specific area to focus on (optional)")]
    pub question: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConsultCodeReviewerRequest {
    #[schemars(description = "Code to review")]
    pub context: String,
    #[schemars(description = "Specific aspect to focus on (optional)")]
    pub question: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConsultSecurityRequest {
    #[schemars(description = "Code or design to analyze for security")]
    pub context: String,
    #[schemars(description = "Specific security concern (optional)")]
    pub question: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConfigureExpertRequest {
    #[schemars(description = "Action: set/get/delete/list")]
    pub action: String,
    #[schemars(description = "Expert role: architect/plan_reviewer/scope_analyst/code_reviewer/security")]
    pub role: Option<String>,
    #[schemars(description = "Custom system prompt (for 'set' action)")]
    pub prompt: Option<String>,
}

#[tool_router]
impl MiraServer {
    #[tool(description = "Initialize session: sets project, loads persona, context, corrections, goals. Call once at session start.")]
    async fn session_start(
        &self,
        Parameters(req): Parameters<SessionStartRequest>,
    ) -> Result<String, String> {
        tools::session_start(self, req.project_path, req.name, req.session_id).await
    }

    #[tool(description = "Set active project.")]
    async fn set_project(
        &self,
        Parameters(req): Parameters<SetProjectRequest>,
    ) -> Result<String, String> {
        tools::set_project(self, req.project_path, req.name).await
    }

    #[tool(description = "Get currently active project.")]
    async fn get_project(&self) -> Result<String, String> {
        tools::get_project(self).await
    }

    #[tool(description = "Store a fact/decision/preference for future recall. Scoped to active project.")]
    async fn remember(
        &self,
        Parameters(req): Parameters<RememberRequest>,
    ) -> Result<String, String> {
        tools::remember(
            self,
            req.content,
            req.key,
            req.fact_type,
            req.category,
            req.confidence,
        )
        .await
    }

    #[tool(description = "Search memories using semantic similarity.")]
    async fn recall(
        &self,
        Parameters(req): Parameters<RecallRequest>,
    ) -> Result<String, String> {
        tools::recall(self, req.query, req.limit, req.category, req.fact_type).await
    }

    #[tool(description = "Delete a memory by ID.")]
    async fn forget(
        &self,
        Parameters(req): Parameters<ForgetRequest>,
    ) -> Result<String, String> {
        tools::forget(self, req.id).await
    }

    #[tool(description = "Get symbols from a file.")]
    async fn get_symbols(
        &self,
        Parameters(req): Parameters<GetSymbolsRequest>,
    ) -> Result<String, String> {
        tools::get_symbols(req.file_path, req.symbol_type)
    }

    #[tool(description = "Search code by meaning.")]
    async fn search_code(
        &self,
        Parameters(req): Parameters<SemanticCodeSearchRequest>,
    ) -> Result<String, String> {
        tools::search_code(self, req.query, req.language, req.limit).await
    }

    #[tool(description = "Find all functions that call a given function.")]
    async fn find_callers(
        &self,
        Parameters(req): Parameters<FindCallersRequest>,
    ) -> Result<String, String> {
        tools::find_function_callers(self, req.function_name, req.limit).await
    }

    #[tool(description = "Find all functions called by a given function.")]
    async fn find_callees(
        &self,
        Parameters(req): Parameters<FindCalleesRequest>,
    ) -> Result<String, String> {
        tools::find_function_callees(self, req.function_name, req.limit).await
    }

    #[tool(description = "Check if a capability/feature exists in the codebase. Searches cached capabilities first, then falls back to live code search.")]
    async fn check_capability(
        &self,
        Parameters(req): Parameters<CheckCapabilityRequest>,
    ) -> Result<String, String> {
        tools::check_capability(self, req.description).await
    }

    #[tool(description = "Manage tasks. Actions: create/bulk_create/list/get/update/complete/delete. Use bulk_create with tasks param for multiple tasks in one call.")]
    async fn task(
        &self,
        Parameters(req): Parameters<TaskRequest>,
    ) -> Result<String, String> {
        tools::task(
            self,
            req.action,
            req.task_id,
            req.title,
            req.description,
            req.status,
            req.priority,
            req.include_completed,
            req.limit,
            req.tasks,
        )
        .await
    }

    #[tool(description = "Manage goals/milestones. Actions: create/bulk_create/list/get/update/delete/add_milestone/complete_milestone/progress. Use bulk_create with goals param for multiple goals in one call.")]
    async fn goal(
        &self,
        Parameters(req): Parameters<GoalRequest>,
    ) -> Result<String, String> {
        tools::goal(
            self,
            req.action,
            req.goal_id,
            req.title,
            req.description,
            req.status,
            req.priority,
            req.progress_percent,
            req.include_finished,
            req.limit,
            req.goals,
        )
        .await
    }

    #[tool(description = "Index code and git history. Actions: project/file/status")]
    async fn index(
        &self,
        Parameters(req): Parameters<IndexRequest>,
    ) -> Result<String, String> {
        tools::index(self, req.action, req.path, req.skip_embed.unwrap_or(false)).await
    }

    #[tool(description = "Generate LLM-powered summaries for codebase modules. Uses DeepSeek to analyze code and create descriptions.")]
    async fn summarize_codebase(&self) -> Result<String, String> {
        tools::summarize_codebase(self).await
    }

    #[tool(description = "Get session recap (preferences, recent context, goals).")]
    async fn get_session_recap(&self) -> Result<String, String> {
        tools::get_session_recap(self).await
    }

    #[tool(description = "Query session history. Actions: list_sessions, get_history, current")]
    async fn session_history(
        &self,
        Parameters(req): Parameters<SessionHistoryRequest>,
    ) -> Result<String, String> {
        let limit = req.limit.unwrap_or(20) as usize;

        match req.action.as_str() {
            "current" => {
                let session_id = self.session_id.read().await;
                match session_id.as_ref() {
                    Some(id) => Ok(format!("Current session: {}", id)),
                    None => Ok("No active session".to_string()),
                }
            }
            "list_sessions" => {
                let project = self.project.read().await;
                let project_id = project.as_ref().map(|p| p.id).ok_or("No active project")?;

                let sessions = self.db.get_recent_sessions(project_id, limit)
                    .map_err(|e| e.to_string())?;

                if sessions.is_empty() {
                    return Ok("No sessions found.".to_string());
                }

                let mut output = format!("{} sessions:\n", sessions.len());
                for s in sessions {
                    output.push_str(&format!(
                        "  [{}] {} - {} ({} tool calls)\n",
                        &s.id[..8],
                        s.started_at,
                        s.status,
                        s.summary.as_deref().unwrap_or("no summary")
                    ));
                }
                Ok(output)
            }
            "get_history" => {
                // Use provided session_id or fall back to current session
                let session_id = match req.session_id {
                    Some(id) => id,
                    None => self.session_id.read().await.clone()
                        .ok_or("No session_id provided and no active session")?,
                };

                let history = self.db.get_session_history(&session_id, limit)
                    .map_err(|e| e.to_string())?;

                if history.is_empty() {
                    return Ok(format!("No history for session {}", &session_id[..8]));
                }

                let mut output = format!("{} tool calls in session {}:\n", history.len(), &session_id[..8]);
                for entry in history {
                    let status = if entry.success { "✓" } else { "✗" };
                    let preview = entry.result_summary
                        .as_ref()
                        .map(|s| if s.len() > 60 { format!("{}...", &s[..60]) } else { s.clone() })
                        .unwrap_or_default();
                    output.push_str(&format!(
                        "  {} {} [{}] {}\n",
                        status, entry.tool_name, entry.created_at, preview
                    ));
                }
                Ok(output)
            }
            _ => Err(format!("Unknown action: {}. Use: list_sessions, get_history, current", req.action)),
        }
    }

    #[tool(description = "Send a response back to Mira during collaboration. Use this when Mira asks you a question via discuss().")]
    async fn reply_to_mira(
        &self,
        Parameters(req): Parameters<ReplyToMiraRequest>,
    ) -> Result<String, String> {
        let complete = req.complete.unwrap_or(true);

        // Try to find and fulfill the pending response
        let sender = {
            let mut pending = self.pending_responses.write().await;
            pending.remove(&req.in_reply_to)
        };

        match sender {
            Some(tx) => {
                // Send response through the channel
                if tx.send(req.content.clone()).is_err() {
                    return Err("Response channel was closed".to_string());
                }

                // Broadcast AgentResponse event for frontend
                self.broadcast(WsEvent::AgentResponse {
                    in_reply_to: req.in_reply_to.clone(),
                    from: AgentRole::Claude,
                    content: req.content,
                    complete,
                });

                Ok("Response sent to Mira".to_string())
            }
            None => {
                // No pending request found - might be stale or wrong ID
                Err(format!("No pending request found for message_id: {}. It may have timed out or been answered already.", req.in_reply_to))
            }
        }
    }

    // Expert consultation tools - delegate to DeepSeek Reasoner

    #[tool(description = "Consult the Architect expert for system design, patterns, tradeoffs, and architectural decisions. Provides deep analysis using extended reasoning.")]
    async fn consult_architect(
        &self,
        Parameters(req): Parameters<ConsultArchitectRequest>,
    ) -> Result<String, String> {
        tools::consult_architect(self, req.context, req.question).await
    }

    #[tool(description = "Consult the Plan Reviewer expert to validate implementation plans before coding. Identifies risks, gaps, and blockers.")]
    async fn consult_plan_reviewer(
        &self,
        Parameters(req): Parameters<ConsultPlanReviewerRequest>,
    ) -> Result<String, String> {
        tools::consult_plan_reviewer(self, req.context, req.question).await
    }

    #[tool(description = "Consult the Scope Analyst expert to find missing requirements, unstated assumptions, and edge cases. Surfaces unknowns early.")]
    async fn consult_scope_analyst(
        &self,
        Parameters(req): Parameters<ConsultScopeAnalystRequest>,
    ) -> Result<String, String> {
        tools::consult_scope_analyst(self, req.context, req.question).await
    }

    #[tool(description = "Consult the Code Reviewer expert to find bugs, quality issues, and improvements. Reviews code for correctness and maintainability.")]
    async fn consult_code_reviewer(
        &self,
        Parameters(req): Parameters<ConsultCodeReviewerRequest>,
    ) -> Result<String, String> {
        tools::consult_code_reviewer(self, req.context, req.question).await
    }

    #[tool(description = "Consult the Security Analyst expert to identify vulnerabilities, attack vectors, and hardening opportunities. Reviews for security best practices.")]
    async fn consult_security(
        &self,
        Parameters(req): Parameters<ConsultSecurityRequest>,
    ) -> Result<String, String> {
        tools::consult_security(self, req.context, req.question).await
    }

    #[tool(description = "Configure expert system prompts. Actions: set (customize prompt), get (view current), delete (revert to default), list (show all custom prompts).")]
    async fn configure_expert(
        &self,
        Parameters(req): Parameters<ConfigureExpertRequest>,
    ) -> Result<String, String> {
        tools::configure_expert(self, req.action, req.role, req.prompt).await
    }
}

impl ServerHandler for MiraServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: Default::default(),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: rmcp::model::Implementation {
                name: "mira".into(),
                title: Some("Mira - Memory and Intelligence Layer for Claude Code".into()),
                version: env!("CARGO_PKG_VERSION").into(),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Mira provides semantic memory, code intelligence, and persistent context for Claude Code.".into(),
            ),
        }
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult {
            tools: self.tool_router.list_all(),
            next_cursor: None,
            meta: None,
        }))
    }

    #[allow(clippy::manual_async_fn)]
    fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, ErrorData>> + Send + '_ {
        async move {
            let tool_name = request.name.to_string();
            let call_id = uuid::Uuid::new_v4().to_string();
            let start = std::time::Instant::now();

            // Get or create session for persistence
            let session_id = self.get_or_create_session().await;

            // Serialize arguments for storage
            let args_json = request.arguments.as_ref()
                .map(|a| serde_json::to_string(a).unwrap_or_default())
                .unwrap_or_default();

            // Broadcast tool start (direct, no HTTP)
            self.broadcast(mira_types::WsEvent::ToolStart {
                tool_name: tool_name.clone(),
                arguments: serde_json::Value::Object(request.arguments.clone().unwrap_or_default()),
                call_id: call_id.clone(),
            });

            let ctx = ToolCallContext::new(self, request, context);
            let result = self.tool_router.call(ctx).await;

            // Broadcast tool result
            let duration_ms = start.elapsed().as_millis() as u64;
            let (success, result_text) = match &result {
                Ok(r) => {
                    let text = r.content.first()
                        .and_then(|c| c.as_text())
                        .map(|t| t.text.to_string())
                        .unwrap_or_default();
                    (true, text)
                }
                Err(e) => (false, e.message.to_string()),
            };

            self.broadcast(mira_types::WsEvent::ToolResult {
                tool_name: tool_name.clone(),
                result: result_text.clone(),
                success,
                call_id,
                duration_ms,
            });

            // Persist to tool_history (summary for quick display, full result for recall)
            let summary = if result_text.len() > 2000 {
                format!("{}...", &result_text[..2000])
            } else {
                result_text.clone()
            };
            let full_result = if result_text.len() > 100 { Some(result_text.as_str()) } else { None };
            if let Err(e) = self.db.log_tool_call(&session_id, &tool_name, &args_json, &summary, full_result, success) {
                eprintln!("[HISTORY] Failed to log tool call: {}", e);
            }

            // Extract meaningful outcomes from tool results (async, non-blocking)
            if success {
                let project_id = self.project.read().await.as_ref().map(|p| p.id);
                extraction::spawn_tool_extraction(
                    self.db.clone(),
                    self.embeddings.clone(),
                    self.deepseek.clone(),
                    project_id,
                    tool_name,
                    args_json,
                    result_text,
                );
            }

            result
        }
    }
}
