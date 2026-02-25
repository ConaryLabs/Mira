//! Core tool implementations for MCP.
//!
//! All tools are implemented as async functions that accept `&impl ToolContext`
//! and return `Result<String, String>` for consistent error handling.

use async_trait::async_trait;

/// Standard error message when no project is active.
pub const NO_ACTIVE_PROJECT_ERROR: &str = "No active project. Auto-detection failed — call project(action=\"start\", project_path=\"/your/path\") to set one explicitly.";
use mira_types::ProjectContext;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::background::watcher::WatcherHandle;
use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use crate::fuzzy::FuzzyCache;

/// Information about an MCP tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
}

/// Common context required by all tools.
/// Implemented by MiraServer (MCP).
#[async_trait]
pub trait ToolContext: Send + Sync {
    // === Core Resources (always available) ===

    /// Async connection pool for main database operations (memory, sessions, goals, etc.)
    fn pool(&self) -> &Arc<DatabasePool>;

    /// Async connection pool for code index database operations
    /// (code_symbols, call_graph, imports, codebase_modules, vec_code, code_fts, pending_embeddings)
    fn code_pool(&self) -> &Arc<DatabasePool>;

    /// Embeddings client for semantic search (optional)
    fn embeddings(&self) -> Option<&Arc<EmbeddingClient>>;

    /// Fuzzy fallback cache for non-embedding searches (optional)
    fn fuzzy_cache(&self) -> Option<&Arc<FuzzyCache>> {
        None
    }

    // === Project/Session State ===

    /// Get current project context (if any)
    async fn get_project(&self) -> Option<ProjectContext>;

    /// Set current project context
    async fn set_project(&self, project: ProjectContext);

    /// Get current project ID (convenience method)
    async fn project_id(&self) -> Option<i64> {
        self.get_project().await.map(|p| p.id)
    }

    /// Get current session ID (if any)
    async fn get_session_id(&self) -> Option<String>;

    /// Set the session ID
    async fn set_session_id(&self, session_id: String);

    /// Get or create a session ID for the current project
    async fn get_or_create_session(&self) -> String;

    // === Branch Context ===

    /// Get the current git branch (cached, refreshes every ~5 seconds)
    /// Returns None if not in a git repository or branch cannot be determined
    async fn get_branch(&self) -> Option<String>;

    /// Set the current branch (typically called during session_start)
    async fn set_branch(&self, branch: Option<String>);

    // === User Identity ===

    /// Get the current user's identity (for multi-user memory scoping)
    /// Returns None if identity cannot be determined
    fn get_user_identity(&self) -> Option<String> {
        crate::identity::get_current_user_identity()
    }

    // === Team Context ===

    /// Get team membership if in a team.
    /// Default uses filesystem (CLI/hook contexts). MCP server overrides
    /// with in-memory cache backed by DB lookups for session isolation.
    fn get_team_membership(&self) -> Option<crate::hooks::session::TeamMembership> {
        crate::hooks::session::read_team_membership()
    }

    // === Optional Services ===

    /// Watcher handle for file system monitoring (optional)
    fn watcher(&self) -> Option<&WatcherHandle> {
        None
    }

    /// Whether the MCP client supports sampling/createMessage
    fn has_sampling(&self) -> bool {
        false
    }

    /// Whether the MCP client supports elicitation (interactive user input)
    fn has_elicitation(&self) -> bool {
        false
    }

    /// Get an elicitation client for requesting user input during tool execution
    fn elicitation_client(&self) -> Option<crate::mcp::elicitation::ElicitationClient> {
        None
    }
}

/// Bundled project context extracted from a ToolContext.
/// Avoids the repeated 3-4 line pattern of project_id + get_project + path + header.
pub struct ProjectInfo {
    pub id: Option<i64>,
    pub context: Option<ProjectContext>,
    pub path: Option<String>,
    pub header: String,
}

/// Extract all commonly-needed project info in one call.
pub async fn get_project_info<C: ToolContext + ?Sized>(ctx: &C) -> ProjectInfo {
    let context = ctx.get_project().await;
    ProjectInfo {
        id: context.as_ref().map(|p| p.id),
        path: context.as_ref().map(|p| p.path.clone()),
        header: crate::search::format_project_header(context.as_ref()),
        context,
    }
}

// Shared test utilities (only compiled during tests)
#[cfg(test)]
pub(crate) mod test_utils;

// Sub-modules with tool implementations
pub mod claude_local;
pub mod code;
pub mod diff;
pub mod documentation;
pub mod goals;
pub mod insights;
pub mod memory;
pub mod project;
pub mod recipe;
pub mod session;
pub mod session_notes;
pub mod tasks;
pub mod team;
pub mod usage;

// Re-export handler functions used by MCP router, CLI, and tests
pub use claude_local::export_claude_local;
pub use code::{
    find_function_callees, find_function_callers, get_symbols, handle_code, index, query_callees,
    query_callers, query_search_code, search_code, summarize_codebase,
};
pub use diff::{analyze_diff_tool, list_diff_analyses};
pub use documentation::documentation;
pub use goals::goal;
pub use insights::{dismiss_insight, query_insights};
pub use memory::{archive, forget, handle_memory, recall, remember};
pub use project::{get_project, project, session_start, set_project};
pub use recipe::handle_recipe;
pub use session::{ensure_session, get_session_recap, handle_session};
pub use team::handle_team;
pub use usage::{usage_list, usage_stats, usage_summary};
