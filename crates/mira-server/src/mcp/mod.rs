// crates/mira-server/src/mcp/mod.rs
// MCP Server — struct definition, construction, and shared helpers

pub mod client;
pub mod elicitation;
mod extraction;
mod handler;
pub mod requests;
pub mod responses;
mod router;
mod tasks;

use crate::tools::core as tools;
use client::McpClientManager;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::background::watcher::WatcherHandle;
use crate::config::{ApiKeys, ExpertGuardrails};
use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use crate::fuzzy::FuzzyCache;
use crate::hooks::session::{read_claude_cwd, read_claude_session_id};
use crate::llm::ProviderFactory;
use mira_types::ProjectContext;
use rmcp::{
    handler::server::router::tool::ToolRouter, service::RoleServer,
    task_manager::OperationProcessor,
};

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
    pub pending_responses: tools::PendingResponseMap,
    /// MCP client manager for connecting to external MCP servers (expert tool access)
    pub mcp_client_manager: Option<Arc<McpClientManager>>,
    /// Fuzzy fallback cache for non-embedding searches
    pub fuzzy_cache: Arc<FuzzyCache>,
    /// Whether fuzzy fallback is enabled
    pub fuzzy_enabled: bool,
    /// Expert agentic loop guardrails
    pub expert_guardrails: ExpertGuardrails,
    /// Cached team membership (per-process, avoids global file race)
    pub team_membership: Arc<RwLock<Option<crate::hooks::session::TeamMembership>>>,
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
        expert_guardrails: ExpertGuardrails,
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
            expert_guardrails,
            team_membership: Arc::new(RwLock::new(None)),
            peer,
            processor: Arc::new(tokio::sync::Mutex::new(OperationProcessor::new())),
            tool_router: Self::create_tool_router(),
        }
    }

    pub fn new(
        pool: Arc<DatabasePool>,
        code_pool: Arc<DatabasePool>,
        embeddings: Option<Arc<EmbeddingClient>>,
    ) -> Self {
        Self::from_api_keys(pool, code_pool, embeddings, &ApiKeys::from_env(), true, ExpertGuardrails::default())
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
                drop(current_project);
                // Project matches but team cache may have been invalidated
                // (e.g. after set_session_id clears it). Repopulate if needed.
                self.maybe_repopulate_team_cache().await;
                return;
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
                self.maybe_repopulate_team_cache().await;
            }
            Err(e) => {
                tracing::warn!("[mira] Failed to auto-initialize project: {}", e);
            }
        }
    }

    /// Repopulate team membership cache from DB if it was invalidated
    /// (e.g. after set_session_id clears it).
    async fn maybe_repopulate_team_cache(&self) {
        if self.team_membership.read().await.is_some() {
            return; // Already populated
        }
        // Prefer in-memory session ID (set via project(..., session_id=...)),
        // fall back to filesystem hook file for standard Claude Code sessions.
        let sid = self
            .session_id
            .read()
            .await
            .clone()
            .or_else(read_claude_session_id);
        if let Some(sid) = sid {
            let pool_clone = self.pool.clone();
            let sid_clone = sid.clone();
            if let Ok(Some(membership)) = pool_clone
                .interact(move |conn| {
                    Ok::<_, anyhow::Error>(
                        crate::db::get_team_membership_for_session_sync(conn, &sid_clone),
                    )
                })
                .await
            {
                *self.team_membership.write().await = Some(membership);
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
