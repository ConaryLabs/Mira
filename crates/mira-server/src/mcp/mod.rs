// crates/mira-server/src/mcp/mod.rs
// MCP Server implementation

mod extraction;
pub mod requests;

use crate::mcp_client::McpClientManager;
use crate::tools::core as tools;
use crate::tools::core::ToolContext;

use std::collections::HashMap;
use tokio::sync::oneshot;

use crate::background::watcher::WatcherHandle;
use crate::config::ApiKeys;
use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use crate::fuzzy::FuzzyCache;
use crate::hooks::session::{read_claude_cwd, read_claude_session_id};
use crate::llm::ProviderFactory;
use mira_types::ProjectContext;
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::ToolCallContext, wrapper::Parameters},
    model::{
        CallToolRequestParam, CallToolResult, ListToolsResult, PaginatedRequestParam,
        ServerCapabilities, ServerInfo,
    },
    service::{RequestContext, RoleServer},
    tool, tool_router,
};
use std::sync::Arc;
use tokio::sync::RwLock;

use requests::*;

/// MCP Server state
#[derive(Clone)]
pub struct MiraServer {
    /// Async connection pool for main database operations (memory, sessions, goals, etc.)
    pub pool: Arc<DatabasePool>,
    /// Async connection pool for code index database (code_symbols, vec_code, etc.)
    pub code_pool: Arc<DatabasePool>,
    pub embeddings: Option<Arc<EmbeddingClient>>,
    pub llm_factory: Arc<ProviderFactory>,
    pub project: Arc<RwLock<Option<ProjectContext>>>,
    /// Current session ID (generated on first tool call or session_start)
    pub session_id: Arc<RwLock<Option<String>>>,
    /// Current git branch (detected from project path)
    pub branch: Arc<RwLock<Option<String>>>,
    /// WebSocket broadcaster (unused in MCP-only mode)
    pub ws_tx: Option<tokio::sync::broadcast::Sender<mira_types::WsEvent>>,
    /// File watcher handle for registering projects
    pub watcher: Option<WatcherHandle>,
    /// Pending responses for agent collaboration (message_id -> response sender)
    pub pending_responses: Arc<RwLock<HashMap<String, oneshot::Sender<String>>>>,
    /// MCP client manager for connecting to external MCP servers (expert tool access)
    pub mcp_client_manager: Option<Arc<McpClientManager>>,
    /// Fuzzy fallback cache for non-embedding searches
    pub fuzzy_cache: Arc<FuzzyCache>,
    /// Whether fuzzy fallback is enabled
    pub fuzzy_enabled: bool,
    tool_router: ToolRouter<Self>,
}

impl MiraServer {
    /// Create a new server from pre-loaded API keys (avoids duplicate env reads)
    pub fn from_api_keys(
        pool: Arc<DatabasePool>,
        code_pool: Arc<DatabasePool>,
        embeddings: Option<Arc<EmbeddingClient>>,
        api_keys: &ApiKeys,
        fuzzy_enabled: bool,
    ) -> Self {
        // Create provider factory from pre-loaded keys
        let llm_factory = Arc::new(ProviderFactory::from_api_keys(api_keys.clone()));

        Self {
            pool,
            code_pool,
            embeddings,
            llm_factory,
            project: Arc::new(RwLock::new(None)),
            session_id: Arc::new(RwLock::new(None)),
            branch: Arc::new(RwLock::new(None)),
            ws_tx: None,
            watcher: None,
            pending_responses: Arc::new(RwLock::new(HashMap::new())),
            mcp_client_manager: None,
            fuzzy_cache: Arc::new(FuzzyCache::new()),
            fuzzy_enabled,
            tool_router: Self::tool_router(),
        }
    }

    pub fn new(
        pool: Arc<DatabasePool>,
        code_pool: Arc<DatabasePool>,
        embeddings: Option<Arc<EmbeddingClient>>,
    ) -> Self {
        Self::from_api_keys(pool, code_pool, embeddings, &ApiKeys::from_env(), true)
    }

    /// Auto-initialize project from Claude's cwd if not already set or mismatched
    async fn maybe_auto_init_project(&self) {
        // Read the cwd that Claude's SessionStart hook captured
        let Some(cwd) = read_claude_cwd() else {
            return; // No cwd captured yet, skip auto-init
        };

        // Check if we already have a project set
        let current_project = self.project.read().await;
        if let Some(ref proj) = *current_project {
            // Already have a project - check if path matches
            if proj.path == cwd {
                return; // Already initialized to the correct project
            }
            // Path mismatch - we'll re-initialize below
            tracing::info!(
                "[mira] Project path mismatch: {} vs {}, auto-switching",
                proj.path,
                cwd
            );
        }
        drop(current_project); // Release the read lock

        // Auto-initialize the project
        tracing::info!("[mira] Auto-initializing project from cwd: {}", cwd);

        // Get session_id from hook (if available)
        let session_id = read_claude_session_id();

        // Call the project initialization (action=Start, with the cwd as path)
        // We use a minimal init here - just set up the project context
        // The full session_start output will be shown if user explicitly calls project(action="start")
        match tools::project(
            self,
            requests::ProjectAction::Set, // Use Set for silent init, not Start
            Some(cwd.clone()),
            None, // Auto-detect name
            session_id,
        )
        .await
        {
            Ok(_) => {
                tracing::info!("[mira] Auto-initialized project: {}", cwd);
            }
            Err(e) => {
                tracing::warn!("[mira] Failed to auto-initialize project: {}", e);
            }
        }
    }

    /// Broadcast an event (no-op in MCP-only mode)
    pub fn broadcast(&self, event: mira_types::WsEvent) {
        if let Some(tx) = &self.ws_tx {
            let receiver_count = tx.receiver_count();
            tracing::debug!(
                "[BROADCAST] Sending {:?} to {} receivers",
                event,
                receiver_count
            );
            match tx.send(event) {
                Ok(n) => tracing::debug!("[BROADCAST] Sent to {} receivers", n),
                Err(e) => tracing::warn!("[BROADCAST] Error: {:?}", e),
            }
        } else {
            tracing::debug!("[BROADCAST] No ws_tx configured!");
        }
    }
}

#[tool_router]
impl MiraServer {
    #[tool(
        description = "Manage project context. Actions: start (initialize session with codebase map), set (change project), get (show current)."
    )]
    async fn project(&self, Parameters(req): Parameters<ProjectRequest>) -> Result<String, String> {
        // For start action, use provided session ID or fall back to Claude's hook-generated ID
        let session_id = req.session_id.or_else(read_claude_session_id);
        tools::project(self, req.action, req.project_path, req.name, session_id).await
    }

    #[tool(
        description = "Store a fact for future recall. Scope controls visibility: personal (only you), project (default), team."
    )]
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
            req.scope,
        )
        .await
    }

    #[tool(description = "Search memories using semantic similarity.")]
    async fn recall(&self, Parameters(req): Parameters<RecallRequest>) -> Result<String, String> {
        tools::recall(self, req.query, req.limit, req.category, req.fact_type).await
    }

    #[tool(description = "Delete a memory by ID.")]
    async fn forget(&self, Parameters(req): Parameters<ForgetRequest>) -> Result<String, String> {
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

    #[tool(
        description = "Manage goals and milestones (create, list, update, delete). Supports bulk operations."
    )]
    async fn goal(&self, Parameters(req): Parameters<GoalRequest>) -> Result<String, String> {
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
            req.milestone_title,
            req.milestone_id,
            req.weight,
        )
        .await
    }

    #[tool(
        description = "Manage cross-project intelligence sharing (enable/disable sharing, view stats, sync patterns)."
    )]
    async fn cross_project(
        &self,
        Parameters(req): Parameters<CrossProjectRequest>,
    ) -> Result<String, String> {
        tools::cross_project(
            self,
            req.action,
            req.export,
            req.import,
            req.min_confidence,
            req.epsilon,
        )
        .await
    }

    #[tool(description = "Index code and git history. Actions: project/file/status")]
    async fn index(&self, Parameters(req): Parameters<IndexRequest>) -> Result<String, String> {
        tools::index(self, req.action, req.path, req.skip_embed.unwrap_or(false)).await
    }

    #[tool(description = "Generate LLM-powered summaries for codebase modules.")]
    async fn summarize_codebase(&self) -> Result<String, String> {
        tools::summarize_codebase(self).await
    }

    #[tool(description = "Get session recap (preferences, recent context, goals).")]
    async fn get_session_recap(&self) -> Result<String, String> {
        tools::get_session_recap(self).await
    }

    #[tool(description = "Query session history (list_sessions, get_history, current).")]
    async fn session_history(
        &self,
        Parameters(req): Parameters<SessionHistoryRequest>,
    ) -> Result<String, String> {
        tools::session_history(self, req.action, req.session_id, req.limit).await
    }

    #[tool(description = "Send a response back to Mira during collaboration.")]
    async fn reply_to_mira(
        &self,
        Parameters(req): Parameters<ReplyToMiraRequest>,
    ) -> Result<String, String> {
        tools::reply_to_mira(
            self,
            req.in_reply_to,
            req.content,
            req.complete.unwrap_or(true),
        )
        .await
    }

    #[tool(
        description = "Consult one or more experts in parallel. Roles: architect, plan_reviewer, scope_analyst, code_reviewer, security."
    )]
    async fn consult_experts(
        &self,
        Parameters(req): Parameters<ConsultExpertsRequest>,
    ) -> Result<String, String> {
        tools::consult_experts(self, req.roles, req.context, req.question, req.mode).await
    }

    #[tool(description = "Configure expert system prompts (set, get, delete, list, providers).")]
    async fn configure_expert(
        &self,
        Parameters(req): Parameters<ConfigureExpertRequest>,
    ) -> Result<String, String> {
        tools::configure_expert(
            self,
            req.action,
            req.role,
            req.prompt,
            req.provider,
            req.model,
        )
        .await
    }

    #[tool(
        description = "Export Mira memories to CLAUDE.local.md for persistence across Claude Code sessions."
    )]
    async fn export_claude_local(&self) -> Result<String, String> {
        tools::export_claude_local(self).await
    }

    #[tool(
        description = "Manage documentation tasks. Actions: list (show needed docs), get (full task details for Claude to write), complete (mark done after writing), skip (mark not needed), inventory (show all docs), scan (trigger scan)."
    )]
    async fn documentation(
        &self,
        Parameters(req): Parameters<DocumentationRequest>,
    ) -> Result<String, String> {
        tools::documentation(
            self,
            req.action,
            req.task_id,
            req.reason,
            req.doc_type,
            req.priority,
            req.status,
        )
        .await
    }

    #[tool(description = "Manage teams for shared memory (create, invite, remove, list, members).")]
    async fn team(&self, Parameters(req): Parameters<TeamRequest>) -> Result<String, String> {
        tools::team(
            self,
            req.action,
            req.team_id,
            req.name,
            req.description,
            req.user_identity,
            req.role,
        )
        .await
    }

    #[tool(
        description = "Manage code review findings. Actions: list, get, review (single or bulk with finding_ids), stats, patterns, extract."
    )]
    async fn finding(&self, Parameters(req): Parameters<FindingRequest>) -> Result<String, String> {
        tools::finding(
            self,
            req.action,
            req.finding_id,
            req.finding_ids,
            req.status,
            req.feedback,
            req.file_path,
            req.expert_role,
            req.correction_type,
            req.limit,
        )
        .await
    }

    // Semantic diff analysis tool

    #[tool(
        description = "Analyze git diff semantically. Identifies change types, impact, and risks."
    )]
    async fn analyze_diff(
        &self,
        Parameters(req): Parameters<AnalyzeDiffRequest>,
    ) -> Result<String, String> {
        tools::analyze_diff_tool(self, req.from_ref, req.to_ref, req.include_impact).await
    }

    #[tool(
        description = "Query LLM usage and cost analytics. Actions: summary (totals), stats (grouped by role/provider/model), list (recent)."
    )]
    async fn usage(&self, Parameters(req): Parameters<UsageRequest>) -> Result<String, String> {
        tools::usage(self, req.action, req.group_by, req.since_days, req.limit).await
    }
}

impl MiraServer {
    /// Returns a list of all MCP tool names.
    /// Used for verifying CLI dispatcher has parity with MCP router.
    pub fn list_tool_names(&self) -> Vec<String> {
        self.tool_router
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect()
    }

    /// Extract result text and success status from a tool call result
    fn extract_result_text(result: &Result<CallToolResult, ErrorData>) -> (bool, String) {
        match result {
            Ok(r) => {
                let text = r
                    .content
                    .first()
                    .and_then(|c| c.as_text())
                    .map(|t| t.text.to_string())
                    .unwrap_or_default();
                (true, text)
            }
            Err(e) => (false, e.message.to_string()),
        }
    }

    /// Persist a tool call to the database for history tracking
    async fn log_tool_call(
        &self,
        session_id: &str,
        tool_name: &str,
        args_json: &str,
        result_text: &str,
        success: bool,
    ) {
        let summary = if result_text.len() > 2000 {
            format!("{}...", &result_text[..2000])
        } else {
            result_text.to_string()
        };
        let full_result_str = if result_text.len() > 100 {
            Some(result_text.to_string())
        } else {
            None
        };
        let session_id = session_id.to_string();
        let tool_name = tool_name.to_string();
        let args_json = args_json.to_string();
        if let Err(e) = self
            .pool
            .interact(move |conn| {
                crate::db::log_tool_call_sync(
                    conn,
                    &session_id,
                    &tool_name,
                    &args_json,
                    &summary,
                    full_result_str.as_deref(),
                    success,
                )
                .map_err(|e| anyhow::anyhow!(e))
            })
            .await
        {
            tracing::warn!("[HISTORY] Failed to log tool call: {}", e);
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

            // Auto-initialize project from Claude's cwd if needed
            // Skip if the tool being called IS the project tool (avoid recursion)
            if tool_name != "project" {
                self.maybe_auto_init_project().await;
            }

            // Serialize arguments for storage
            let args_json = request
                .arguments
                .as_ref()
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

            // Extract result and broadcast
            let duration_ms = start.elapsed().as_millis() as u64;
            let (success, result_text) = Self::extract_result_text(&result);

            self.broadcast(mira_types::WsEvent::ToolResult {
                tool_name: tool_name.clone(),
                result: result_text.clone(),
                success,
                call_id,
                duration_ms,
            });

            // Persist to tool_history (fire-and-forget, never blocks tool response)
            {
                let server = self.clone();
                let sid = session_id.clone();
                let tn = tool_name.clone();
                let aj = args_json.clone();
                let rt = result_text.clone();
                tokio::spawn(async move {
                    server.log_tool_call(&sid, &tn, &aj, &rt, success).await;
                });
            }

            // Extract meaningful outcomes from tool results (async, non-blocking)
            if success {
                let project_id = self.project.read().await.as_ref().map(|p| p.id);
                extraction::spawn_tool_extraction(
                    self.pool.clone(),
                    self.embeddings.clone(),
                    self.llm_factory.client_for_background(),
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
