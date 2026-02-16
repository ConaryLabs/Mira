// crates/mira-server/src/mcp/router.rs
// MCP tool router — #[tool] annotated methods and tool call lifecycle

use crate::mcp::responses::{HasMessage, Json};
use crate::tools::core as tools;

use rmcp::{
    ErrorData,
    handler::server::{router::tool::ToolRouter, tool::IntoCallToolResult, wrapper::Parameters},
    model::{CallToolRequestParams, CallToolResult, Content},
    service::{RequestContext, RoleServer},
    task_manager::{self, OperationDescriptor, OperationMessage, ToolCallTaskResult},
    tool, tool_router,
};
use schemars::JsonSchema;
use serde::Serialize;

use super::MiraServer;
use super::requests::*;
use super::responses;
use crate::hooks::session::read_claude_session_id;
use crate::utils::truncate;

fn tool_result<T>(result: Result<Json<T>, String>) -> Result<CallToolResult, ErrorData>
where
    T: Serialize + JsonSchema + HasMessage + 'static,
{
    match result {
        Ok(json) => json.into_call_tool_result(),
        Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
    }
}

#[allow(clippy::expect_used)] // schema_for_output on derived JsonSchema types is infallible
#[tool_router]
impl MiraServer {
    #[tool(
        description = "Manage project context and workspace initialization. Actions: start (initialize session with codebase map), get (show current).",
        output_schema = rmcp::handler::server::tool::schema_for_output::<responses::ProjectOutput>()
            .expect("ProjectOutput schema")
    )]
    async fn project(
        &self,
        Parameters(req): Parameters<McpProjectRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let action: ProjectAction = req.action.into();
        let session_id = req.session_id.or_else(read_claude_session_id);
        tool_result(tools::project(self, action, req.project_path, req.name, session_id).await)
    }

    #[tool(
        description = "Persistent cross-session knowledge base. Actions: remember (store a fact), recall (search by similarity), forget (delete by ID), archive (exclude from auto-export, keep for history). Scope: personal, project (default), team.",
        output_schema = rmcp::handler::server::tool::schema_for_output::<responses::MemoryOutput>()
            .expect("MemoryOutput schema")
    )]
    async fn memory(
        &self,
        Parameters(req): Parameters<McpMemoryRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        tool_result(tools::handle_memory(self, req.into()).await)
    }

    #[tool(
        description = "Code intelligence: semantic search and call graph analysis. Actions: search (find code by meaning), symbols (list definitions in file), callers/callees (trace call graph).",
        output_schema = rmcp::handler::server::tool::schema_for_output::<responses::CodeOutput>()
            .expect("CodeOutput schema")
    )]
    async fn code(
        &self,
        Parameters(req): Parameters<McpCodeRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        tool_result(tools::handle_code(self, req.into()).await)
    }

    #[tool(
        description = "Analyze git changes semantically with change classification, impact assessment, and risk analysis. Compares refs, staged changes, or working tree.",
        output_schema = rmcp::handler::server::tool::schema_for_output::<responses::DiffOutput>()
            .expect("DiffOutput schema")
    )]
    async fn diff(
        &self,
        Parameters(req): Parameters<McpDiffRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        tool_result(
            tools::analyze_diff_tool(self, req.from_ref, req.to_ref, req.include_impact).await,
        )
    }

    #[tool(
        description = "Track cross-session objectives, progress, and milestones. Actions: create, bulk_create, list, get, update, delete, add_milestone, complete_milestone, delete_milestone, progress, sessions. Use for multi-session work planning and progress tracking.",
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
        description = "Index codebase for semantic search and analysis. Actions: project (full reindex), file (single file), status (index stats). Builds embeddings and symbol tables.",
        output_schema = rmcp::handler::server::tool::schema_for_output::<responses::IndexOutput>()
            .expect("IndexOutput schema")
    )]
    async fn index(
        &self,
        Parameters(req): Parameters<McpIndexRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let action: IndexAction = req.action.into();
        tool_result(tools::index(self, action, req.path, req.skip_embed.unwrap_or(false)).await)
    }

    #[tool(
        description = "Session management and insights. Actions: recap (preferences + context + goals), insights (background analysis digest), dismiss_insight (remove resolved insight; insight_source required: 'pondering' or 'doc_gap'), current_session (show current).",
        output_schema = rmcp::handler::server::tool::schema_for_output::<responses::SessionOutput>()
            .expect("SessionOutput schema")
    )]
    async fn session(
        &self,
        Parameters(req): Parameters<McpSessionRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        tool_result(tools::handle_session(self, req.into()).await)
    }

    #[tool(
        description = "Get agentic team recipes for expert review, full-cycle, QA hardening, and refactoring workflows. Actions: list (available recipes), get (full recipe with members, tasks, coordination).",
        output_schema = rmcp::handler::server::tool::schema_for_output::<responses::RecipeOutput>()
            .expect("RecipeOutput schema")
    )]
    async fn recipe(
        &self,
        Parameters(req): Parameters<RecipeRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        tool_result(tools::handle_recipe(req).await)
    }

    // documentation, team tools removed from MCP surface.
    // Still accessible via `mira tool <name> '<json>'` CLI.
}

impl MiraServer {
    /// Expose the macro-generated tool_router() to the parent module constructor.
    pub(super) fn create_tool_router() -> ToolRouter<Self> {
        Self::tool_router()
    }

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
    pub(crate) fn extract_result_text(
        result: &Result<CallToolResult, ErrorData>,
    ) -> (bool, String) {
        match result {
            Ok(r) => {
                if let Some(structured) = r.structured_content.as_ref()
                    && let Some(message) = structured.get("message").and_then(|v| v.as_str())
                {
                    return (true, message.to_string());
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
    pub(crate) async fn log_tool_call(
        &self,
        session_id: &str,
        tool_name: &str,
        args_json: &str,
        result_text: &str,
        success: bool,
    ) {
        let summary = truncate(result_text, 2000);
        let full_result_str = if result_text.len() > 100 {
            Some(result_text.to_string())
        } else {
            None
        };
        let session_id = session_id.to_string();
        let tool_name = tool_name.to_string();
        let args_json = args_json.to_string();
        self.pool
            .try_interact("log tool call", move |conn| {
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
            .await;
    }
}

/// Result of submitting a task to the operation processor.
pub(crate) struct EnqueuedTask {
    pub task_id: String,
    pub tool_name: String,
    pub created_at: String,
    pub ttl: u64,
}

impl MiraServer {
    /// Shared logic for submitting a tool call as an async task.
    /// Generates a task ID, builds the execution future, and submits to the processor.
    /// Used by both `auto_enqueue_task` (call_tool path) and `enqueue_task` (native tasks).
    pub(crate) async fn submit_tool_task(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
        tool_name: &str,
        ttl: u64,
    ) -> Result<EnqueuedTask, ErrorData> {
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

        Ok(EnqueuedTask {
            task_id,
            tool_name: tn,
            created_at: now,
            ttl,
        })
    }

    /// Auto-enqueue a task-eligible tool call via the OperationProcessor.
    /// Returns a CallToolResult immediately with the task ID so the client can poll.
    pub(crate) async fn auto_enqueue_task(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
        tool_name: &str,
        ttl: u64,
    ) -> Result<CallToolResult, ErrorData> {
        let enqueued = self
            .submit_tool_task(request, context, tool_name, ttl)
            .await?;

        tracing::info!(
            task_id = %enqueued.task_id,
            tool = %enqueued.tool_name,
            ttl_secs = enqueued.ttl,
            "Auto-enqueued async task (client used call_tool)"
        );

        Ok(CallToolResult {
            content: vec![Content::text(format!(
                "Task {} started (running {} asynchronously). \
                 Poll with MCP tasks/get, retrieve with tasks/result.",
                enqueued.task_id, enqueued.tool_name
            ))],
            structured_content: Some(serde_json::json!({
                "task_id": enqueued.task_id,
                "status": "working",
                "message": format!("Running {} asynchronously", enqueued.tool_name),
                "poll": "MCP tasks/get and tasks/result",
                "created_at": enqueued.created_at,
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
        use crate::tools::core::ToolContext;
        use rmcp::handler::server::tool::ToolCallContext;

        let tool_name = request.name.to_string();

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

        let ctx = ToolCallContext::new(self, request, context);
        let result = self.tool_router.call(ctx).await;

        let (success, result_text) = Self::extract_result_text(&result);

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
            super::extraction::spawn_tool_extraction(
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

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{CallToolResult, Content};

    // ═══════════════════════════════════════
    // extract_result_text
    // ═══════════════════════════════════════

    #[test]
    fn extract_result_text_with_text_content() {
        let result = Ok(CallToolResult {
            content: vec![Content::text("hello world")],
            structured_content: None,
            is_error: Some(false),
            meta: None,
        });
        let (success, text) = MiraServer::extract_result_text(&result);
        assert!(success);
        assert_eq!(text, "hello world");
    }

    #[test]
    fn extract_result_text_empty_content() {
        let result = Ok(CallToolResult {
            content: vec![],
            structured_content: None,
            is_error: Some(false),
            meta: None,
        });
        let (success, text) = MiraServer::extract_result_text(&result);
        assert!(success);
        assert_eq!(text, "");
    }

    #[test]
    fn extract_result_text_structured_with_message() {
        let result = Ok(CallToolResult {
            content: vec![Content::text("fallback text")],
            structured_content: Some(serde_json::json!({
                "message": "structured message",
                "data": 42
            })),
            is_error: Some(false),
            meta: None,
        });
        let (success, text) = MiraServer::extract_result_text(&result);
        assert!(success);
        // structured_content.message takes priority over content text
        assert_eq!(text, "structured message");
    }

    #[test]
    fn extract_result_text_structured_without_message() {
        let result = Ok(CallToolResult {
            content: vec![Content::text("fallback text")],
            structured_content: Some(serde_json::json!({
                "data": 42
            })),
            is_error: Some(false),
            meta: None,
        });
        let (success, text) = MiraServer::extract_result_text(&result);
        assert!(success);
        // No "message" field in structured, falls back to content
        assert_eq!(text, "fallback text");
    }

    #[test]
    fn extract_result_text_error_result() {
        let result: Result<CallToolResult, ErrorData> = Err(ErrorData::internal_error(
            "something broke".to_string(),
            None,
        ));
        let (success, text) = MiraServer::extract_result_text(&result);
        assert!(!success);
        assert_eq!(text, "something broke");
    }

    // ═══════════════════════════════════════
    // tool_result
    // ═══════════════════════════════════════

    #[test]
    fn tool_result_err_produces_error_content() {
        use crate::mcp::responses::MemoryOutput;
        let result: Result<CallToolResult, ErrorData> =
            tool_result::<MemoryOutput>(Err("bad request".to_string()));
        // Should be Ok (not protocol error), but with error content
        let call_result = result.expect("tool_result Err should produce Ok(CallToolResult)");
        let text = call_result
            .content
            .first()
            .and_then(|c| c.as_text())
            .map(|t| t.text.to_string())
            .unwrap_or_default();
        assert!(
            text.contains("bad request"),
            "expected 'bad request' in: {text}"
        );
    }
}
