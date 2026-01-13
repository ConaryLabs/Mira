//! crates/mira-server/src/tools/mcp.rs
//! MCP adapter for unified tool core

use crate::mcp::MiraServer;
use crate::tools::core::ToolContext;
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

    fn embeddings(&self) -> Option<&Arc<crate::embeddings::Embeddings>> {
        self.embeddings.as_ref()
    }

    fn deepseek(&self) -> Option<&Arc<crate::llm::DeepSeekClient>> {
        self.deepseek.as_ref()
    }

    async fn get_project(&self) -> Option<mira_types::ProjectContext> {
        self.project.read().await.clone()
    }

    async fn set_project(&self, project: mira_types::ProjectContext) {
        *self.project.write().await = Some(project);
    }

    async fn get_session_id(&self) -> Option<String> {
        self.session_id.read().await.clone()
    }

    async fn set_session_id(&self, session_id: String) {
        *self.session_id.write().await = Some(session_id);
    }

    async fn get_or_create_session(&self) -> String {
        // For MCP, generate or return existing session ID
        let mut session_id = self.session_id.write().await;
        if session_id.is_none() {
            *session_id = Some(uuid::Uuid::new_v4().to_string());
        }
        session_id.clone().unwrap()
    }

    fn broadcast(&self, event: WsEvent) {
        if let Some(tx) = &self.ws_tx {
            let _ = tx.send(event);
        }
    }

    fn pending_responses(&self) -> Option<&Arc<RwLock<HashMap<String, oneshot::Sender<String>>>>> {
        Some(&self.pending_responses)
    }

    fn watcher(&self) -> Option<&crate::background::watcher::WatcherHandle> {
        self.watcher.as_ref()
    }
}
