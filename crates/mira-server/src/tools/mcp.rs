//! crates/mira-server/src/tools/mcp.rs
//! MCP adapter for unified tool core

use crate::mcp::MiraServer;
use crate::tools::core::ToolContext;
use crate::tools::core::ensure_session;
use uuid::Uuid;
use async_trait::async_trait;
use mira_types::WsEvent;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{oneshot, RwLock};

#[async_trait]
impl ToolContext for MiraServer {
    fn db(&self) -> &Arc<crate::db::Database> {
        &self.db
    }

    fn pool(&self) -> &Arc<crate::db::pool::DatabasePool> {
        &self.pool
    }

    fn embeddings(&self) -> Option<&Arc<crate::embeddings::Embeddings>> {
        self.embeddings.as_ref()
    }

    fn deepseek(&self) -> Option<&Arc<crate::llm::DeepSeekClient>> {
        self.deepseek.as_ref()
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

    async fn get_or_create_session(&self) -> String {
        match ensure_session(self).await {
            Ok(id) => id,
            Err(e) => {
                eprintln!("[SESSION] Failed to create session in database: {}", e);
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

    fn pending_responses(&self) -> Option<&Arc<RwLock<HashMap<String, oneshot::Sender<String>>>>> {
        Some(&self.pending_responses)
    }

    fn watcher(&self) -> Option<&crate::background::watcher::WatcherHandle> {
        self.watcher.as_ref()
    }
}
