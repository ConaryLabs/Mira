// backend/src/main.rs
// Mira Power Suit - MCP Server for Claude Code

use anyhow::Result;
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::*,
    tool, tool_router, tool_handler,
};
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

mod tools;
use tools::*;

// === Mira MCP Server ===

#[derive(Clone)]
pub struct MiraServer {
    db: Arc<SqlitePool>,
    semantic: Arc<SemanticSearch>,
    tool_router: ToolRouter<Self>,
}

impl MiraServer {
    pub async fn new(database_url: &str, qdrant_url: Option<&str>, openai_key: Option<String>) -> Result<Self> {
        info!("Connecting to database: {}", database_url);
        let db = SqlitePool::connect(database_url).await?;
        info!("Database connected successfully");

        let semantic = SemanticSearch::new(qdrant_url, openai_key).await;
        if semantic.is_available() {
            info!("Semantic search enabled (Qdrant + OpenAI)");
        } else {
            info!("Semantic search disabled (using text-based fallback)");
        }

        Ok(Self {
            db: Arc::new(db),
            semantic: Arc::new(semantic),
            tool_router: Self::tool_router(),
        })
    }
}

#[tool_router]
impl MiraServer {
    // === Analytics ===

    #[tool(description = "List all tables in the Mira database with row counts.")]
    async fn list_tables(&self) -> Result<CallToolResult, McpError> {
        let result = analytics::list_tables(self.db.as_ref()).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Execute a read-only SQL query against the Mira database. Only SELECT statements are allowed.")]
    async fn query(&self, Parameters(req): Parameters<QueryRequest>) -> Result<CallToolResult, McpError> {
        match analytics::query(self.db.as_ref(), req).await {
            Ok(result) => Ok(json_response(result)),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e.to_string())])),
        }
    }

    // === Memory (semantic search) ===

    #[tool(description = "Remember a fact, decision, preference, or context for future sessions. Stores in both SQLite and vector database for semantic recall.")]
    async fn remember(&self, Parameters(req): Parameters<RememberRequest>) -> Result<CallToolResult, McpError> {
        let result = memory::remember(self.db.as_ref(), self.semantic.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Search through stored memories using semantic similarity. Find memories by meaning, not just exact text match.")]
    async fn recall(&self, Parameters(req): Parameters<RecallRequest>) -> Result<CallToolResult, McpError> {
        let query = req.query.clone();
        let result = memory::recall(self.db.as_ref(), self.semantic.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No memories found matching '{}'", query)))
    }

    #[tool(description = "Forget (delete) a stored memory by its ID.")]
    async fn forget(&self, Parameters(req): Parameters<ForgetRequest>) -> Result<CallToolResult, McpError> {
        let result = memory::forget(self.db.as_ref(), self.semantic.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    // === Cross-Session Memory ===

    #[tool(description = "Store a summary of the current session for cross-session recall. Call at the end of significant sessions.")]
    async fn store_session(&self, Parameters(req): Parameters<StoreSessionRequest>) -> Result<CallToolResult, McpError> {
        let result = sessions::store_session(self.db.as_ref(), self.semantic.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Search across past sessions using semantic similarity. Find what was discussed, decided, or worked on in previous sessions.")]
    async fn search_sessions(&self, Parameters(req): Parameters<SearchSessionsRequest>) -> Result<CallToolResult, McpError> {
        let query = req.query.clone();
        let result = sessions::search_sessions(self.db.as_ref(), self.semantic.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No past sessions found matching '{}'", query)))
    }

    #[tool(description = "Store an important decision or context for future reference. Decisions are keyed for updates.")]
    async fn store_decision(&self, Parameters(req): Parameters<StoreDecisionRequest>) -> Result<CallToolResult, McpError> {
        let result = sessions::store_decision(self.db.as_ref(), self.semantic.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    // === Code Intelligence ===

    #[tool(description = "Get all symbols (functions, classes, structs, etc.) defined in a file.")]
    async fn get_symbols(&self, Parameters(req): Parameters<GetSymbolsRequest>) -> Result<CallToolResult, McpError> {
        let file_path = req.file_path.clone();
        let result = code_intel::get_symbols(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No symbols found in '{}'", file_path)))
    }

    #[tool(description = "Get the call graph showing what functions call and are called by a given function.")]
    async fn get_call_graph(&self, Parameters(req): Parameters<GetCallGraphRequest>) -> Result<CallToolResult, McpError> {
        let result = code_intel::get_call_graph(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Find files that are related to a given file through imports or co-change patterns.")]
    async fn get_related_files(&self, Parameters(req): Parameters<GetRelatedFilesRequest>) -> Result<CallToolResult, McpError> {
        let result = code_intel::get_related_files(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Search code using natural language. Find 'authentication logic' or 'error handling' by meaning, not just keywords.")]
    async fn semantic_code_search(&self, Parameters(req): Parameters<SemanticCodeSearchRequest>) -> Result<CallToolResult, McpError> {
        let query = req.query.clone();
        let result = code_intel::semantic_code_search(self.db.as_ref(), self.semantic.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No code found matching '{}'", query)))
    }

    // === Git Intelligence ===

    #[tool(description = "Get recent git commits, optionally filtered by file or author.")]
    async fn get_recent_commits(&self, Parameters(req): Parameters<GetRecentCommitsRequest>) -> Result<CallToolResult, McpError> {
        let result = git_intel::get_recent_commits(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, "No commits found"))
    }

    #[tool(description = "Search git commits by message content.")]
    async fn search_commits(&self, Parameters(req): Parameters<SearchCommitsRequest>) -> Result<CallToolResult, McpError> {
        let query = req.query.clone();
        let result = git_intel::search_commits(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No commits found matching '{}'", query)))
    }

    #[tool(description = "Find files that frequently change together. Useful for understanding implicit dependencies.")]
    async fn find_cochange_patterns(&self, Parameters(req): Parameters<FindCochangeRequest>) -> Result<CallToolResult, McpError> {
        let file_path = req.file_path.clone();
        let result = git_intel::find_cochange_patterns(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No co-change patterns found for '{}'", file_path)))
    }

    #[tool(description = "Search for similar errors that were fixed before. Uses semantic search to find 'this error feels like...' matches.")]
    async fn find_similar_fixes(&self, Parameters(req): Parameters<FindSimilarFixesRequest>) -> Result<CallToolResult, McpError> {
        let error = req.error.clone();
        let result = git_intel::find_similar_fixes(self.db.as_ref(), self.semantic.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No similar fixes found for: {}", error)))
    }

    #[tool(description = "Record an error fix for future learning. When you fix an error, record it so similar fixes can be found later.")]
    async fn record_error_fix(&self, Parameters(req): Parameters<RecordErrorFixRequest>) -> Result<CallToolResult, McpError> {
        let result = git_intel::record_error_fix(self.db.as_ref(), self.semantic.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    // === Build Intelligence ===

    #[tool(description = "Get recent build errors, optionally filtered by file or category.")]
    async fn get_build_errors(&self, Parameters(req): Parameters<GetBuildErrorsRequest>) -> Result<CallToolResult, McpError> {
        let result = build_intel::get_build_errors(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, "No build errors found"))
    }

    #[tool(description = "Record a build run result for tracking.")]
    async fn record_build(&self, Parameters(req): Parameters<RecordBuildRequest>) -> Result<CallToolResult, McpError> {
        let result = build_intel::record_build(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Record a build error for tracking and later analysis.")]
    async fn record_build_error(&self, Parameters(req): Parameters<RecordBuildErrorRequest>) -> Result<CallToolResult, McpError> {
        let result = build_intel::record_build_error(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Mark a build error as resolved.")]
    async fn resolve_error(&self, Parameters(req): Parameters<ResolveErrorRequest>) -> Result<CallToolResult, McpError> {
        let result = build_intel::resolve_error(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    // === Workspace Context ===

    #[tool(description = "Record file activity (read, write, error, test) for tracking what's being worked on.")]
    async fn record_activity(&self, Parameters(req): Parameters<RecordActivityRequest>) -> Result<CallToolResult, McpError> {
        let result = workspace::record_activity(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Get recent file activity to see what has been worked on.")]
    async fn get_recent_activity(&self, Parameters(req): Parameters<GetRecentActivityRequest>) -> Result<CallToolResult, McpError> {
        let result = workspace::get_recent_activity(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, "No recent activity found"))
    }

    #[tool(description = "Set work context for tracking current focus (active task, recent error, current file).")]
    async fn set_context(&self, Parameters(req): Parameters<SetContextRequest>) -> Result<CallToolResult, McpError> {
        let result = workspace::set_context(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "Get current work context to understand what's being focused on.")]
    async fn get_context(&self, Parameters(req): Parameters<GetContextRequest>) -> Result<CallToolResult, McpError> {
        let result = workspace::get_context(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, "No active context"))
    }

    // === Project Context ===

    #[tool(description = "Get coding guidelines and conventions for a project.")]
    async fn get_guidelines(&self, Parameters(req): Parameters<GetGuidelinesRequest>) -> Result<CallToolResult, McpError> {
        let result = project::get_guidelines(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, "No guidelines found. Use 'add_guideline' to add project conventions."))
    }

    #[tool(description = "Add a coding guideline or convention for a project.")]
    async fn add_guideline(&self, Parameters(req): Parameters<AddGuidelineRequest>) -> Result<CallToolResult, McpError> {
        let result = project::add_guideline(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    // === Task Management ===

    #[tool(description = "Create a new task or todo item. Tasks persist across sessions.")]
    async fn create_task(&self, Parameters(req): Parameters<CreateTaskRequest>) -> Result<CallToolResult, McpError> {
        let result = tasks::create_task(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(json_response(result))
    }

    #[tool(description = "List tasks/todos with optional filters. Returns pending and in-progress tasks by default.")]
    async fn list_tasks(&self, Parameters(req): Parameters<ListTasksRequest>) -> Result<CallToolResult, McpError> {
        let result = tasks::list_tasks(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, "No tasks found. Use 'create_task' to add a new task."))
    }

    #[tool(description = "Get detailed information about a specific task.")]
    async fn get_task(&self, Parameters(req): Parameters<GetTaskRequest>) -> Result<CallToolResult, McpError> {
        let task_id = req.task_id.clone();
        let result = tasks::get_task(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(option_response(result, format!("Task {} not found", task_id)))
    }

    #[tool(description = "Update a task's title, description, status, or priority.")]
    async fn update_task(&self, Parameters(req): Parameters<UpdateTaskRequest>) -> Result<CallToolResult, McpError> {
        let task_id = req.task_id.clone();
        let result = tasks::update_task(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(option_response(result, format!("Task {} not found", task_id)))
    }

    #[tool(description = "Mark a task as completed with optional notes.")]
    async fn complete_task(&self, Parameters(req): Parameters<CompleteTaskRequest>) -> Result<CallToolResult, McpError> {
        let task_id = req.task_id.clone();
        let result = tasks::complete_task(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(option_response(result, format!("Task {} not found", task_id)))
    }

    #[tool(description = "Delete a task and all its subtasks.")]
    async fn delete_task(&self, Parameters(req): Parameters<DeleteTaskRequest>) -> Result<CallToolResult, McpError> {
        let task_id = req.task_id.clone();
        match tasks::delete_task(self.db.as_ref(), req).await.map_err(to_mcp_err)? {
            Some(title) => Ok(json_response(serde_json::json!({
                "status": "deleted",
                "task_id": task_id,
                "title": title,
            }))),
            None => Ok(text_response(format!("Task {} not found", task_id))),
        }
    }

    // === Document Search (semantic) ===

    #[tool(description = "List documents that have been uploaded and processed.")]
    async fn list_documents(&self, Parameters(req): Parameters<ListDocumentsRequest>) -> Result<CallToolResult, McpError> {
        let result = documents::list_documents(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, "No documents found."))
    }

    #[tool(description = "Search through documents using semantic similarity. Find relevant content by meaning.")]
    async fn search_documents(&self, Parameters(req): Parameters<SearchDocumentsRequest>) -> Result<CallToolResult, McpError> {
        let query = req.query.clone();
        let result = documents::search_documents(self.db.as_ref(), self.semantic.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(vec_response(result, format!("No document content found matching '{}'", query)))
    }

    #[tool(description = "Get detailed information about a specific document, optionally including full content.")]
    async fn get_document(&self, Parameters(req): Parameters<GetDocumentRequest>) -> Result<CallToolResult, McpError> {
        let doc_id = req.document_id.clone();
        let result = documents::get_document(self.db.as_ref(), req).await.map_err(to_mcp_err)?;
        Ok(option_response(result, format!("Document '{}' not found", doc_id)))
    }
}

#[tool_handler]
impl ServerHandler for MiraServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some("Mira Power Suit - Memory and Intelligence Layer for Claude Code. \
                Features: semantic memory (remember/recall), cross-session context, persistent tasks, \
                code intelligence, git intelligence, and document search. All search tools use \
                semantic similarity when Qdrant/OpenAI are configured.".to_string()),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting Mira MCP Server...");

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://data/mira.db".to_string());
    let qdrant_url = std::env::var("QDRANT_URL").ok();
    let openai_key = std::env::var("OPENAI_API_KEY").ok();

    let server = MiraServer::new(&database_url, qdrant_url.as_deref(), openai_key).await?;
    info!("Server initialized");

    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;

    Ok(())
}
