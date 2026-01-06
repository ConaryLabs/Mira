//! Core tool implementations shared between web chat and MCP.
//!
//! All tools are implemented as async functions that accept `&impl ToolContext`
//! and return `Result<String, String>` for consistent error handling.

use async_trait::async_trait;
use mira_types::{ProjectContext, WsEvent};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, oneshot, RwLock};

use crate::db::Database;
use crate::embeddings::Embeddings;
use crate::web::claude::ClaudeManager;
use crate::web::deepseek::DeepSeekClient;

/// Common context required by all tools.
/// Implemented by both AppState (web chat) and MiraServer (MCP).
#[async_trait]
pub trait ToolContext: Send + Sync {
    // === Core Resources (always available) ===
    
    /// Database connection with semantic search capabilities
    fn db(&self) -> &Arc<Database>;
    
    /// Embeddings client for semantic search (optional)
    fn embeddings(&self) -> Option<&Arc<Embeddings>>;
    
    /// DeepSeek client for chat/completion (optional)
    fn deepseek(&self) -> Option<&Arc<DeepSeekClient>>;
    
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
    
    /// Get or create a session ID for the current project
    async fn get_or_create_session(&self) -> String;
    
    // === Event Broadcasting ===
    
    /// Broadcast a WebSocket event to connected clients
    fn broadcast(&self, event: WsEvent);
    
    // === Web-Only Services (default to None for MCP) ===
    
    /// Google Custom Search client (web-only)
    fn google_search(&self) -> Option<&Arc<crate::web::search::GoogleSearchClient>> {
        None
    }
    
    /// Web page fetcher (web-only)
    fn web_fetcher(&self) -> Option<&Arc<crate::web::search::WebFetcher>> {
        None
    }
    
    /// Claude Code instance manager (web-only)
    fn claude_manager(&self) -> Option<&Arc<ClaudeManager>> {
        None
    }
    
    /// Pending responses for agent collaboration (web-only)
    fn pending_responses(&self) -> Option<&Arc<RwLock<HashMap<String, oneshot::Sender<String>>>>> {
        None
    }
    
    /// WebSocket broadcast sender (web-only)
    fn ws_tx(&self) -> Option<&broadcast::Sender<WsEvent>> {
        None
    }
    
    /// Watcher handle for file system monitoring (optional)
    fn watcher(&self) -> Option<&crate::background::watcher::WatcherHandle> {
        None
    }
}

// Sub-modules with tool implementations
pub mod memory;
pub mod code;
pub mod project;
pub mod tasks_goals;
pub mod web;
pub mod claude;
pub mod bash;

// Re-export commonly used functions
pub use memory::*;
pub use code::*;
pub use project::*;
pub use tasks_goals::*;
pub use web::*;
pub use claude::*;
pub use bash::*;
