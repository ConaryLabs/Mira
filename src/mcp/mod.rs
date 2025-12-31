// src/mcp/mod.rs
// MCP Server implementation

pub mod tools;

use crate::db::Database;
use crate::embeddings::Embeddings;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_router, ServerHandler,
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
    pub project: Arc<RwLock<Option<ProjectContext>>>,
    tool_router: ToolRouter<Self>,
}

impl MiraServer {
    pub fn new(db: Arc<Database>, embeddings: Option<Arc<Embeddings>>) -> Self {
        Self {
            db,
            embeddings,
            project: Arc::new(RwLock::new(None)),
            tool_router: Self::tool_router(),
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

#[tool_router]
impl MiraServer {
    #[tool(description = "Initialize session: sets project, loads persona, context, corrections, goals. Call once at session start.")]
    async fn session_start(
        &self,
        Parameters(req): Parameters<SessionStartRequest>,
    ) -> Result<String, String> {
        tools::project::session_start(self, req.project_path, req.name).await
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
            instructions: Some("Mira provides semantic memory, code intelligence, and persistent context for Claude Code.".into()),
        }
    }
}
