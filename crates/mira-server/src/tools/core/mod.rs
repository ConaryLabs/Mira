//! Core tool implementations for MCP.
//!
//! All tools are implemented as async functions that accept `&impl ToolContext`
//! and return `Result<String, String>` for consistent error handling.

use async_trait::async_trait;
use mira_types::{ProjectContext, WsEvent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, oneshot};

use crate::background::watcher::WatcherHandle;
use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use crate::fuzzy::FuzzyCache;
use crate::llm::ProviderFactory;

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

    /// LLM provider factory for multi-provider support
    fn llm_factory(&self) -> &ProviderFactory;

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

    // === Event Broadcasting ===

    /// Broadcast a WebSocket event to connected clients (no-op if no broadcaster)
    fn broadcast(&self, event: WsEvent);

    // === Optional Services ===

    /// Check if the context supports real-time collaboration (WebSocket active)
    /// Returns true for MCP server with connected frontend, false for CLI
    fn is_collaborative(&self) -> bool {
        false
    }

    /// Pending responses for agent collaboration
    fn pending_responses(&self) -> Option<&Arc<RwLock<HashMap<String, oneshot::Sender<String>>>>> {
        None
    }

    /// Watcher handle for file system monitoring (optional)
    fn watcher(&self) -> Option<&WatcherHandle> {
        None
    }

    /// List available MCP tools (optional, for expert context)
    async fn list_mcp_tools(&self) -> Vec<(String, Vec<McpToolInfo>)> {
        Vec::new()
    }

    /// Call an MCP tool on a specific server (optional, for expert tool execution)
    async fn mcp_call_tool(
        &self,
        _server_name: &str,
        _tool_name: &str,
        _args: serde_json::Value,
    ) -> Result<String, String> {
        Err("MCP tool calling not available".to_string())
    }

    /// Get MCP tools as expert Tool definitions with full schemas (optional)
    /// Returns tools with prefixed names: mcp__{server}__{tool_name}
    async fn mcp_expert_tools(&self) -> Vec<crate::llm::Tool> {
        Vec::new()
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
    fn elicitation_client(&self) -> Option<crate::elicitation::ElicitationClient> {
        None
    }
}

// Sub-modules with tool implementations
pub mod claude_local;
pub mod code;
pub mod cross_project;
pub mod dev;
pub mod diff;
pub mod documentation;
pub mod experts;
pub mod goals;
pub mod memory;
pub mod project;
pub mod reviews;
pub mod session;
pub mod session_notes;
pub mod tasks;
pub mod teams;
pub mod usage;

// Re-export commonly used functions
pub use claude_local::export_claude_local;
pub use code::*;
pub use cross_project::*;
pub use dev::*;
pub use diff::*;
pub use documentation::*;
pub use experts::*;
pub use goals::*;
pub use memory::*;
pub use project::*;
pub use reviews::*;
pub use session::*;
pub use teams::*;
pub use usage::*;
