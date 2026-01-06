//! Web chat adapter for unified tool core

use crate::tools::core::ToolContext;
use crate::web::state::AppState;
use async_trait::async_trait;
use mira_types::WsEvent;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, oneshot, RwLock};

#[async_trait]
impl ToolContext for AppState {
    fn db(&self) -> &Arc<crate::db::Database> {
        &self.db
    }

    fn embeddings(&self) -> Option<&Arc<crate::embeddings::Embeddings>> {
        self.embeddings.as_ref()
    }

    fn deepseek(&self) -> Option<&Arc<crate::web::deepseek::DeepSeekClient>> {
        self.deepseek.as_ref()
    }

    async fn get_project(&self) -> Option<mira_types::ProjectContext> {
        self.get_project().await
    }

    async fn set_project(&self, project: mira_types::ProjectContext) {
        *self.project.write().await = Some(project);
    }

    async fn get_session_id(&self) -> Option<String> {
        self.session_id.read().await.clone()
    }

    async fn get_or_create_session(&self) -> String {
        // For web chat, sessions are managed by the chat endpoint
        // Return current session ID or empty string
        self.session_id.read().await.clone().unwrap_or_default()
    }

    fn broadcast(&self, event: WsEvent) {
        let _ = self.ws_tx.send(event);
    }

    fn google_search(&self) -> Option<&Arc<crate::web::search::GoogleSearchClient>> {
        self.google_search.as_ref()
    }

    fn web_fetcher(&self) -> Option<&Arc<crate::web::search::WebFetcher>> {
        Some(&self.web_fetcher)
    }

    fn claude_manager(&self) -> Option<&Arc<crate::web::claude::ClaudeManager>> {
        Some(&self.claude_manager)
    }

    fn pending_responses(&self) -> Option<&Arc<RwLock<HashMap<String, oneshot::Sender<String>>>>> {
        Some(&self.pending_responses)
    }

    fn ws_tx(&self) -> Option<&broadcast::Sender<WsEvent>> {
        Some(&self.ws_tx)
    }

    fn watcher(&self) -> Option<&crate::background::watcher::WatcherHandle> {
        self.watcher.as_ref()
    }
}

// Web-specific tool wrappers that convert from web chat format to unified core
// These will replace the current execute_* functions in web/chat/tools.rs
