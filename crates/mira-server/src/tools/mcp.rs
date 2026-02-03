//! crates/mira-server/src/tools/mcp.rs
//! MCP adapter for unified tool core

use crate::mcp::MiraServer;
use crate::tools::core::ToolContext;
use crate::tools::core::ensure_session;
use async_trait::async_trait;
use mira_types::WsEvent;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, oneshot};
use uuid::Uuid;

#[async_trait]
impl ToolContext for MiraServer {
    fn pool(&self) -> &Arc<crate::db::pool::DatabasePool> {
        &self.pool
    }

    fn code_pool(&self) -> &Arc<crate::db::pool::DatabasePool> {
        &self.code_pool
    }

    fn embeddings(&self) -> Option<&Arc<crate::embeddings::EmbeddingClient>> {
        self.embeddings.as_ref()
    }

    fn fuzzy_cache(&self) -> Option<&Arc<crate::fuzzy::FuzzyCache>> {
        if self.fuzzy_enabled {
            Some(&self.fuzzy_cache)
        } else {
            None
        }
    }

    fn llm_factory(&self) -> &crate::llm::ProviderFactory {
        &self.llm_factory
    }

    async fn get_project(&self) -> Option<mira_types::ProjectContext> {
        self.project.read().await.clone()
    }

    async fn set_project(&self, project: mira_types::ProjectContext) {
        // Update embeddings client with project ID for usage tracking
        if let Some(ref emb) = self.embeddings {
            emb.set_project_id(Some(project.id)).await;
        }
        *self.project.write().await = Some(project);
    }

    async fn get_session_id(&self) -> Option<String> {
        self.session_id.read().await.clone()
    }

    async fn set_session_id(&self, session_id: String) {
        *self.session_id.write().await = Some(session_id);
    }

    async fn get_branch(&self) -> Option<String> {
        // First check cached value
        let cached = self.branch.read().await.clone();
        if cached.is_some() {
            return cached;
        }

        // If not cached, try to detect from project path
        if let Some(project) = self.get_project().await {
            let branch = crate::git::get_git_branch(&project.path);
            if branch.is_some() {
                *self.branch.write().await = branch.clone();
            }
            return branch;
        }

        None
    }

    async fn set_branch(&self, branch: Option<String>) {
        *self.branch.write().await = branch;
    }

    async fn get_or_create_session(&self) -> String {
        match ensure_session(self).await {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!("[SESSION] Failed to create session in database: {}", e);
                // Fallback to local session ID generation
                let mut session_id = self.session_id.write().await;
                match &*session_id {
                    Some(id) => id.clone(),
                    None => {
                        let new_id = Uuid::new_v4().to_string();
                        *session_id = Some(new_id.clone());
                        new_id
                    }
                }
            }
        }
    }

    fn broadcast(&self, event: WsEvent) {
        if let Some(tx) = &self.ws_tx {
            if let Err(e) = tx.send(event) {
                tracing::debug!("WebSocket channel closed, ignoring broadcast: {}", e);
            }
        }
    }

    fn is_collaborative(&self) -> bool {
        self.ws_tx.is_some()
    }

    fn pending_responses(&self) -> Option<&Arc<RwLock<HashMap<String, oneshot::Sender<String>>>>> {
        Some(&self.pending_responses)
    }

    fn watcher(&self) -> Option<&crate::background::watcher::WatcherHandle> {
        self.watcher.as_ref()
    }

    async fn list_mcp_tools(&self) -> Vec<(String, Vec<crate::tools::core::McpToolInfo>)> {
        if let Some(ref manager) = self.mcp_client_manager {
            manager.list_tools().await
        } else {
            Vec::new()
        }
    }

    async fn mcp_call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<String, String> {
        if let Some(ref manager) = self.mcp_client_manager {
            manager.call_tool(server_name, tool_name, args).await
        } else {
            Err("MCP client manager not configured".to_string())
        }
    }

    async fn mcp_expert_tools(&self) -> Vec<crate::llm::Tool> {
        if let Some(ref manager) = self.mcp_client_manager {
            manager.get_expert_tools().await
        } else {
            Vec::new()
        }
    }

    fn has_sampling(&self) -> bool {
        // Check if peer is set and client advertised sampling capability.
        // We can't do async here, so use try_read for a non-blocking check.
        self.peer
            .try_read()
            .ok()
            .and_then(|guard| {
                guard.as_ref().and_then(|p| {
                    p.peer_info()
                        .map(|info| info.capabilities.sampling.is_some())
                })
            })
            .unwrap_or(false)
    }

    fn has_elicitation(&self) -> bool {
        self.peer
            .try_read()
            .ok()
            .and_then(|guard| {
                guard.as_ref().and_then(|p| {
                    p.peer_info()
                        .map(|info| info.capabilities.elicitation.is_some())
                })
            })
            .unwrap_or(false)
    }

    fn elicitation_client(&self) -> Option<crate::elicitation::ElicitationClient> {
        Some(crate::elicitation::ElicitationClient::new(self.peer.clone()))
    }
}
