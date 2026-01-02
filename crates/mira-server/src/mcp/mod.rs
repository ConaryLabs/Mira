// crates/mira-server/src/mcp/mod.rs
// MCP Server implementation

pub mod tools;

use crate::db::Database;
use crate::embeddings::Embeddings;
use crate::web::deepseek::DeepSeekClient;
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

/// Active project context
#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub id: i64,
    pub path: String,
    pub name: Option<String>,
}

/// MCP Server state
#[derive(Clone)]
pub struct MiraServer {
    pub db: Arc<Database>,
    pub embeddings: Option<Arc<Embeddings>>,
    pub deepseek: Option<Arc<DeepSeekClient>>,
    pub project: Arc<RwLock<Option<ProjectContext>>>,
    /// Current session ID (generated on first tool call or session_start)
    pub session_id: Arc<RwLock<Option<String>>>,
    /// WebSocket broadcaster (shared with web server)
    pub ws_tx: Option<tokio::sync::broadcast::Sender<mira_types::WsEvent>>,
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
            tool_router: Self::tool_router(),
        }
    }

    /// Create with a broadcast channel (for embedded web server mode)
    pub fn with_broadcaster(
        db: Arc<Database>,
        embeddings: Option<Arc<Embeddings>>,
        deepseek: Option<Arc<DeepSeekClient>>,
        ws_tx: tokio::sync::broadcast::Sender<mira_types::WsEvent>,
        session_id: Arc<RwLock<Option<String>>>,
    ) -> Self {
        Self {
            db,
            embeddings,
            deepseek,
            project: Arc::new(RwLock::new(None)),
            session_id,
            ws_tx: Some(ws_tx),
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

    /// Broadcast an event to connected WebSocket clients
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
pub struct TaskRequest {
    #[schemars(description = "Action: create/list/get/update/complete/delete")]
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
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GoalRequest {
    #[schemars(description = "Action: create/list/get/update/delete/add_milestone/complete_milestone/progress")]
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
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct IndexRequest {
    #[schemars(description = "Action: project/file/status/cleanup")]
    pub action: String,
    #[schemars(description = "Path")]
    pub path: Option<String>,
    #[schemars(description = "Include git")]
    pub include_git: Option<bool>,
    #[schemars(description = "Commit limit")]
    pub commit_limit: Option<i64>,
    #[schemars(description = "Parallel")]
    pub parallel: Option<bool>,
    #[schemars(description = "Max workers")]
    pub max_workers: Option<i64>,
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

#[tool_router]
impl MiraServer {
    #[tool(description = "Initialize session: sets project, loads persona, context, corrections, goals. Call once at session start.")]
    async fn session_start(
        &self,
        Parameters(req): Parameters<SessionStartRequest>,
    ) -> Result<String, String> {
        tools::project::session_start(self, req.project_path, req.name, req.session_id).await
    }

    #[tool(description = "Set active project.")]
    async fn set_project(
        &self,
        Parameters(req): Parameters<SetProjectRequest>,
    ) -> Result<String, String> {
        tools::project::set_project(self, req.project_path, req.name).await
    }

    #[tool(description = "Get currently active project.")]
    async fn get_project(&self) -> Result<String, String> {
        tools::project::get_project(self).await
    }

    #[tool(description = "Store a fact/decision/preference for future recall. Scoped to active project.")]
    async fn remember(
        &self,
        Parameters(req): Parameters<RememberRequest>,
    ) -> Result<String, String> {
        tools::memory::remember(
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
        tools::memory::recall(self, req.query, req.limit, req.category, req.fact_type).await
    }

    #[tool(description = "Delete a memory by ID.")]
    async fn forget(
        &self,
        Parameters(req): Parameters<ForgetRequest>,
    ) -> Result<String, String> {
        tools::memory::forget(self, req.id).await
    }

    #[tool(description = "Get symbols from a file.")]
    async fn get_symbols(
        &self,
        Parameters(req): Parameters<GetSymbolsRequest>,
    ) -> Result<String, String> {
        tools::code::get_symbols(self, req.file_path, req.symbol_type).await
    }

    #[tool(description = "Search code by meaning.")]
    async fn semantic_code_search(
        &self,
        Parameters(req): Parameters<SemanticCodeSearchRequest>,
    ) -> Result<String, String> {
        tools::code::semantic_code_search(self, req.query, req.language, req.limit).await
    }

    #[tool(description = "Manage tasks. Actions: create/list/get/update/complete/delete")]
    async fn task(
        &self,
        Parameters(req): Parameters<TaskRequest>,
    ) -> Result<String, String> {
        tools::tasks::task(
            self,
            req.action,
            req.task_id,
            req.title,
            req.description,
            req.status,
            req.priority,
            req.include_completed,
            req.limit,
        )
        .await
    }

    #[tool(description = "Manage goals/milestones. Actions: create/list/get/update/delete/add_milestone/complete_milestone/progress")]
    async fn goal(
        &self,
        Parameters(req): Parameters<GoalRequest>,
    ) -> Result<String, String> {
        tools::tasks::goal(
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
        )
        .await
    }

    #[tool(description = "Index code and git history. Actions: project/file/status/cleanup")]
    async fn index(
        &self,
        Parameters(req): Parameters<IndexRequest>,
    ) -> Result<String, String> {
        tools::code::index(self, req.action, req.path).await
    }

    #[tool(description = "Generate LLM-powered summaries for codebase modules. Uses DeepSeek to analyze code and create descriptions.")]
    async fn summarize_codebase(&self) -> Result<String, String> {
        tools::code::summarize_codebase(self).await
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
                let session_id = req.session_id
                    .or_else(|| {
                        // Use current session if not specified
                        futures::executor::block_on(self.session_id.read()).clone()
                    })
                    .ok_or("No session_id provided and no active session")?;

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

            // Persist to tool_history
            let summary = if result_text.len() > 500 {
                format!("{}...", &result_text[..500])
            } else {
                result_text
            };
            if let Err(e) = self.db.log_tool_call(&session_id, &tool_name, &args_json, &summary, success) {
                eprintln!("[HISTORY] Failed to log tool call: {}", e);
            }

            result
        }
    }
}
