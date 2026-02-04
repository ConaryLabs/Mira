// crates/mira-server/src/mcp/mod.rs
// MCP Server implementation

mod extraction;
pub mod requests;
pub mod responses;
mod tasks;

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
use crate::mcp::responses::{HasMessage, Json};
use mira_types::ProjectContext;
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{
        router::tool::ToolRouter, tool::IntoCallToolResult, tool::ToolCallContext,
        wrapper::Parameters,
    },
    model::{
        CallToolRequestParams, CallToolResult, CancelTaskParams, Content, CreateTaskResult,
        GetTaskInfoParams, GetTaskInfoResult, GetTaskResultParams, ListTasksResult,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo, Task,
        TaskResult as ModelTaskResult, TaskStatus, TasksCapability,
    },
    service::{RequestContext, RoleServer},
    task_manager::{
        self, OperationDescriptor, OperationMessage, OperationProcessor, ToolCallTaskResult,
    },
    tool, tool_router,
};
use schemars::JsonSchema;
use serde::Serialize;
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
    /// MCP peer for sampling/createMessage fallback (captured on first tool call)
    pub peer: Arc<RwLock<Option<rmcp::service::Peer<RoleServer>>>>,
    /// Task processor for async long-running operations (SEP-1686)
    pub processor: Arc<tokio::sync::Mutex<OperationProcessor>>,
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
        let mut factory = ProviderFactory::from_api_keys(api_keys.clone());

        // Shared peer slot — populated on first call_tool, used by SamplingClient
        let peer = Arc::new(RwLock::new(None));
        factory.set_sampling_peer(peer.clone());

        let llm_factory = Arc::new(factory);

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
            peer,
            processor: Arc::new(tokio::sync::Mutex::new(OperationProcessor::new())),
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
        let mut cwd = read_claude_cwd();

        // Codex-friendly fallback: allow explicit project path via env var
        if cwd.is_none() {
            cwd = std::env::var("MIRA_PROJECT_PATH")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }

        let Some(cwd) = cwd else {
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

fn tool_result<T>(result: Result<Json<T>, String>) -> Result<CallToolResult, ErrorData>
where
    T: Serialize + JsonSchema + HasMessage + 'static,
{
    match result {
        Ok(json) => json.into_call_tool_result(),
        Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
    }
}

#[tool_router]
impl MiraServer {
    #[tool(
        description = "Manage project context. Actions: start (initialize session with codebase map), set (change project), get (show current).",
        output_schema = rmcp::handler::server::tool::schema_for_output::<responses::ProjectOutput>()
            .expect("ProjectOutput schema")
    )]
    async fn project(
        &self,
        Parameters(req): Parameters<ProjectRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        // For start action, use provided session ID or fall back to Claude's hook-generated ID
        let session_id = req.session_id.or_else(read_claude_session_id);
        tool_result(tools::project(self, req.action, req.project_path, req.name, session_id).await)
    }

    #[tool(
        description = "Manage memories. Actions: remember (store a fact), recall (search by similarity), forget (delete by ID). Scope controls visibility: personal, project (default), team.",
        output_schema = rmcp::handler::server::tool::schema_for_output::<responses::MemoryOutput>()
            .expect("MemoryOutput schema")
    )]
    async fn memory(
        &self,
        Parameters(req): Parameters<MemoryRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        tool_result(tools::handle_memory(self, req).await)
    }

    #[tool(
        description = "Code intelligence. Actions: search (by meaning), symbols (from file), callers (of function), callees (by function), dependencies (module graph + circular deps), patterns (architectural pattern detection), tech_debt (per-module debt scores).",
        output_schema = rmcp::handler::server::tool::schema_for_output::<responses::CodeOutput>()
            .expect("CodeOutput schema")
    )]
    async fn code(
        &self,
        Parameters(req): Parameters<CodeRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        tool_result(tools::handle_code(self, req).await)
    }

    #[tool(
        description = "Manage goals and milestones (create, list, update, delete). Supports bulk operations.",
        output_schema = rmcp::handler::server::tool::schema_for_output::<responses::GoalOutput>()
            .expect("GoalOutput schema")
    )]
    async fn goal(
        &self,
        Parameters(req): Parameters<GoalRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        tool_result(tools::goal(self, req).await)
    }

    #[tool(
        description = "Index code and git history. Actions: project/file/status/compact/summarize/health",
        output_schema = rmcp::handler::server::tool::schema_for_output::<responses::IndexOutput>()
            .expect("IndexOutput schema")
    )]
    async fn index(
        &self,
        Parameters(req): Parameters<IndexRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        tool_result(tools::index(self, req.action, req.path, req.skip_embed.unwrap_or(false)).await)
    }

    #[tool(
        description = "Session management. Actions: history (list_sessions/get_history/current via history_action), recap (preferences + context + goals), usage (summary/stats/list via usage_action), insights (unified digest of pondering/proactive/doc_gap insights).",
        output_schema = rmcp::handler::server::tool::schema_for_output::<responses::SessionOutput>()
            .expect("SessionOutput schema")
    )]
    async fn session(
        &self,
        Parameters(req): Parameters<SessionRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        tool_result(tools::handle_session(self, req).await)
    }

    #[tool(
        description = "Send a response back to Mira during collaboration.",
        output_schema = rmcp::handler::server::tool::schema_for_output::<responses::ReplyOutput>()
            .expect("ReplyOutput schema")
    )]
    async fn reply_to_mira(
        &self,
        Parameters(req): Parameters<ReplyToMiraRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        tool_result(
            tools::reply_to_mira(
                self,
                req.in_reply_to,
                req.content,
                req.complete.unwrap_or(true),
            )
            .await,
        )
    }

    #[tool(
        description = "Consult experts or configure them. Actions: consult (get expert opinions), configure (set/get/delete/list/providers for expert prompts).",
        output_schema = rmcp::handler::server::tool::schema_for_output::<responses::ExpertOutput>()
            .expect("ExpertOutput schema")
    )]
    async fn expert(
        &self,
        Parameters(req): Parameters<ExpertRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        tool_result(tools::handle_expert(self, req).await)
    }

    #[tool(
        description = "Manage documentation tasks. Actions: list (show needed docs), get (full task details for Claude to write), complete (mark done after writing), skip (mark not needed), inventory (show all docs), scan (trigger scan), export_claude_local (export memories to CLAUDE.local.md).",
        output_schema = rmcp::handler::server::tool::schema_for_output::<responses::DocOutput>()
            .expect("DocOutput schema")
    )]
    async fn documentation(
        &self,
        Parameters(req): Parameters<DocumentationRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        tool_result(
            tools::documentation(
                self,
                req.action,
                req.task_id,
                req.reason,
                req.doc_type,
                req.priority,
                req.status,
            )
            .await,
        )
    }

    #[tool(
        description = "Manage code review findings. Actions: list, get, review (single or bulk with finding_ids), stats, patterns, extract.",
        output_schema = rmcp::handler::server::tool::schema_for_output::<responses::FindingOutput>()
            .expect("FindingOutput schema")
    )]
    async fn finding(
        &self,
        Parameters(req): Parameters<FindingRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        tool_result(
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
            .await,
        )
    }

    // Semantic diff analysis tool

    #[tool(
        description = "Analyze git diff semantically. Identifies change types, impact, and risks.",
        output_schema = rmcp::handler::server::tool::schema_for_output::<responses::DiffOutput>()
            .expect("DiffOutput schema")
    )]
    async fn analyze_diff(
        &self,
        Parameters(req): Parameters<AnalyzeDiffRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        tool_result(
            tools::analyze_diff_tool(self, req.from_ref, req.to_ref, req.include_impact).await,
        )
    }

    #[tool(
        description = "Manage async tasks. Actions: list, get (by task_id), cancel (by task_id).",
        output_schema = rmcp::handler::server::tool::schema_for_output::<responses::TasksOutput>()
            .expect("TasksOutput schema")
    )]
    async fn tasks(
        &self,
        Parameters(req): Parameters<TasksRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        tool_result(tools::tasks::handle_tasks(self, req).await)
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
                if let Some(structured) = r.structured_content.as_ref() {
                    if let Some(message) = structured.get("message").and_then(|v| v.as_str()) {
                        return (true, message.to_string());
                    }
                }
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

impl MiraServer {
    /// Auto-enqueue a task-eligible tool call via the OperationProcessor.
    /// Returns a CallToolResult immediately with the task ID so the client can poll.
    async fn auto_enqueue_task(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
        tool_name: &str,
        ttl: u64,
    ) -> Result<CallToolResult, ErrorData> {
        let task_id = uuid::Uuid::new_v4().to_string();
        let now = task_manager::current_timestamp();

        // Strip task field to prevent re-enqueue loops
        let mut clean_request = request;
        clean_request.task = None;

        // Build the async future that calls run_tool_call
        let server = self.clone();
        let ctx = context.clone();
        let tid = task_id.clone();
        let future: task_manager::OperationFuture = Box::pin(async move {
            let result = server.run_tool_call(clean_request, ctx).await;
            let transport = ToolCallTaskResult::new(tid, result);
            Ok(Box::new(transport) as Box<dyn task_manager::OperationResultTransport>)
        });

        // Build descriptor and submit
        let tn = tool_name.to_string();
        let descriptor = OperationDescriptor::new(task_id.clone(), tn.clone()).with_ttl(ttl);
        let message = OperationMessage::new(descriptor, future);

        let mut proc = self.processor.lock().await;
        proc.submit_operation(message).map_err(|e| {
            ErrorData::internal_error(format!("Failed to enqueue task: {}", e), None)
        })?;

        tracing::info!(
            task_id = %task_id,
            tool = %tn,
            ttl_secs = ttl,
            "Auto-enqueued async task (client used call_tool)"
        );

        let poll_hint = format!("tasks(action=\"get\", task_id=\"{}\")", task_id);
        Ok(CallToolResult {
            content: vec![Content::text(format!(
                "Task {} started. Check status with: {}",
                task_id, poll_hint
            ))],
            structured_content: Some(serde_json::json!({
                "task_id": task_id,
                "status": "working",
                "message": format!("Running {} asynchronously", tn),
                "poll_with": poll_hint,
                "created_at": now,
            })),
            is_error: Some(false),
            meta: None,
        })
    }

    /// Execute a tool call with full lifecycle (session init, broadcast, logging, extraction).
    /// Called from both synchronous `call_tool` and async task futures (`enqueue_task`).
    pub(crate) async fn run_tool_call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let tool_name = request.name.to_string();
        let call_id = uuid::Uuid::new_v4().to_string();
        let start = std::time::Instant::now();

        // Capture peer on first tool call (for MCP sampling fallback)
        if self.peer.read().await.is_none() {
            let peer_clone = context.peer.clone();
            if let Some(info) = peer_clone.peer_info() {
                if info.capabilities.sampling.is_some() {
                    tracing::info!("[mira] Client supports MCP sampling");
                }
                if info.capabilities.elicitation.is_some() {
                    tracing::info!("[mira] Client supports MCP elicitation");
                }
            }
            *self.peer.write().await = Some(peer_clone);
        }

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

impl ServerHandler for MiraServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: Default::default(),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_tasks_with(TasksCapability::server_default())
                .build(),
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
        _request: Option<PaginatedRequestParams>,
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
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, ErrorData>> + Send + '_ {
        async move {
            // Auto-enqueue task-eligible tools that arrive via synchronous call_tool
            // (i.e. not already going through the native task protocol).
            // Extract tool name + action up front to avoid borrow conflicts with the move.
            let maybe_enqueue = if request.task.is_none() {
                let action = request
                    .arguments
                    .as_ref()
                    .and_then(|a| a.get("action"))
                    .and_then(|v| v.as_str())
                    .map(String::from);
                tasks::task_ttl(&request.name, action.as_deref())
            } else {
                None
            };

            if let Some(ttl) = maybe_enqueue {
                let tool_name = request.name.to_string();
                return self
                    .auto_enqueue_task(request, context, &tool_name, ttl)
                    .await;
            }

            self.run_tool_call(request, context).await
        }
    }

    #[allow(clippy::manual_async_fn)]
    fn enqueue_task(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CreateTaskResult, ErrorData>> + Send + '_ {
        async move {
            let tool_name = request.name.to_string();

            // Extract action from arguments for eligibility check
            let action = request
                .arguments
                .as_ref()
                .and_then(|a| a.get("action"))
                .and_then(|v| v.as_str())
                .map(String::from);

            // Check eligibility by tool name + action
            let ttl = match tasks::task_ttl(&tool_name, action.as_deref()) {
                Some(ttl) => ttl,
                None => {
                    return Err(ErrorData::internal_error(
                        format!(
                            "Tool '{}' (action: {:?}) does not support async tasks",
                            tool_name, action
                        ),
                        None,
                    ));
                }
            };

            // Generate task ID
            let task_id = uuid::Uuid::new_v4().to_string();
            let now = task_manager::current_timestamp();

            // Strip the `task` field to prevent re-enqueue loops
            let mut clean_request = request;
            clean_request.task = None;

            // Build the async future that calls run_tool_call
            let server = self.clone();
            let ctx = context.clone();
            let tid = task_id.clone();
            let future: task_manager::OperationFuture = Box::pin(async move {
                let result = server.run_tool_call(clean_request, ctx).await;
                let transport = ToolCallTaskResult::new(tid, result);
                Ok(Box::new(transport) as Box<dyn task_manager::OperationResultTransport>)
            });

            // Build descriptor and submit
            let descriptor =
                OperationDescriptor::new(task_id.clone(), tool_name.clone()).with_ttl(ttl);
            let message = OperationMessage::new(descriptor, future);

            let mut proc = self.processor.lock().await;
            proc.submit_operation(message).map_err(|e| {
                ErrorData::internal_error(format!("Failed to enqueue task: {}", e), None)
            })?;

            tracing::info!(
                task_id = %task_id,
                tool = %tool_name,
                ttl_secs = ttl,
                "Enqueued async task"
            );

            Ok(CreateTaskResult {
                task: Task {
                    task_id,
                    status: TaskStatus::Working,
                    status_message: Some(format!("Running {} asynchronously", tool_name)),
                    created_at: now,
                    last_updated_at: None,
                    ttl: Some(ttl * 1000), // Protocol uses milliseconds
                    poll_interval: Some(2000),
                },
            })
        }
    }

    #[allow(clippy::manual_async_fn)]
    fn list_tasks(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListTasksResult, ErrorData>> + Send + '_ {
        async move {
            let mut proc = self.processor.lock().await;
            proc.check_timeouts();

            let running_ids = proc.list_running();
            let mut all_tasks: Vec<Task> = running_ids
                .iter()
                .filter_map(|id| {
                    proc.task_descriptor(id).map(|desc| Task {
                        task_id: id.clone(),
                        status: TaskStatus::Working,
                        status_message: Some(format!("Running {}", desc.name)),
                        created_at: String::new(), // Not tracked in descriptor
                        last_updated_at: None,
                        ttl: desc.ttl.map(|t| t * 1000),
                        poll_interval: Some(2000),
                    })
                })
                .collect();

            // Include completed results that haven't been collected yet
            for result in proc.peek_completed() {
                let status = match &result.result {
                    Ok(_) => TaskStatus::Completed,
                    Err(e) if e.to_string().contains("cancelled") => TaskStatus::Cancelled,
                    Err(_) => TaskStatus::Failed,
                };
                all_tasks.push(Task {
                    task_id: result.descriptor.operation_id.clone(),
                    status,
                    status_message: Some(result.descriptor.name.clone()),
                    created_at: String::new(),
                    last_updated_at: None,
                    ttl: None,
                    poll_interval: None,
                });
            }

            Ok(ListTasksResult {
                tasks: all_tasks,
                next_cursor: None,
                total: None,
            })
        }
    }

    #[allow(clippy::manual_async_fn)]
    fn get_task_info(
        &self,
        request: GetTaskInfoParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<GetTaskInfoResult, ErrorData>> + Send + '_ {
        async move {
            let proc = self.processor.lock().await;

            // Check running tasks first
            if let Some(desc) = proc.task_descriptor(&request.task_id) {
                return Ok(GetTaskInfoResult {
                    task: Some(Task {
                        task_id: request.task_id,
                        status: TaskStatus::Working,
                        status_message: Some(format!("Running {}", desc.name)),
                        created_at: String::new(),
                        last_updated_at: None,
                        ttl: desc.ttl.map(|t| t * 1000),
                        poll_interval: Some(2000),
                    }),
                });
            }

            // Check completed results
            for result in proc.peek_completed() {
                if result.descriptor.operation_id == request.task_id {
                    let status = match &result.result {
                        Ok(_) => TaskStatus::Completed,
                        Err(e) if e.to_string().contains("cancelled") => TaskStatus::Cancelled,
                        Err(_) => TaskStatus::Failed,
                    };
                    return Ok(GetTaskInfoResult {
                        task: Some(Task {
                            task_id: request.task_id,
                            status,
                            status_message: Some(result.descriptor.name.clone()),
                            created_at: String::new(),
                            last_updated_at: None,
                            ttl: None,
                            poll_interval: None,
                        }),
                    });
                }
            }

            // Not found
            Ok(GetTaskInfoResult { task: None })
        }
    }

    #[allow(clippy::manual_async_fn)]
    fn get_task_result(
        &self,
        request: GetTaskResultParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ModelTaskResult, ErrorData>> + Send + '_ {
        async move {
            let mut proc = self.processor.lock().await;
            // Collect any newly completed results
            proc.collect_completed_results();

            match proc.take_completed_result(&request.task_id) {
                Some(task_result) => match task_result.result {
                    Ok(boxed) => {
                        // Downcast to ToolCallTaskResult
                        if let Some(tcr) = boxed.as_any().downcast_ref::<ToolCallTaskResult>() {
                            let value = match &tcr.result {
                                Ok(call_result) => {
                                    serde_json::to_value(call_result).unwrap_or_default()
                                }
                                Err(e) => serde_json::json!({ "error": e.message }),
                            };
                            let summary = match &tcr.result {
                                Ok(r) => r.content.first().and_then(|c| c.as_text()).map(|t| {
                                    if t.text.len() > 200 {
                                        format!("{}...", &t.text[..200])
                                    } else {
                                        t.text.to_string()
                                    }
                                }),
                                Err(e) => Some(e.message.to_string()),
                            };
                            Ok(ModelTaskResult {
                                content_type: "application/json".to_string(),
                                value,
                                summary,
                            })
                        } else {
                            Err(ErrorData::internal_error(
                                "Task result has unexpected type".to_string(),
                                None,
                            ))
                        }
                    }
                    Err(e) => Err(ErrorData::internal_error(
                        format!("Task failed: {}", e),
                        None,
                    )),
                },
                None => {
                    // Check if still running
                    if proc.task_descriptor(&request.task_id).is_some() {
                        Err(ErrorData::internal_error(
                            "Task is still running".to_string(),
                            None,
                        ))
                    } else {
                        Err(ErrorData::internal_error(
                            format!("Task '{}' not found", request.task_id),
                            None,
                        ))
                    }
                }
            }
        }
    }

    #[allow(clippy::manual_async_fn)]
    fn cancel_task(
        &self,
        request: CancelTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<(), ErrorData>> + Send + '_ {
        async move {
            let mut proc = self.processor.lock().await;
            if proc.cancel_task(&request.task_id) {
                tracing::info!(task_id = %request.task_id, "Task cancelled");
                Ok(())
            } else {
                Err(ErrorData::internal_error(
                    format!("Task '{}' not found or already completed", request.task_id),
                    None,
                ))
            }
        }
    }
}
