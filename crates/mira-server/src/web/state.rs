// src/web/state.rs
// Web server state management

use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use crate::db::Database;
use crate::embeddings::Embeddings;
use mira_types::{ProjectContext, WsEvent};

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    /// Database connection (holds sqlite-vec for semantic search)
    pub db: Arc<Database>,

    /// Embeddings client (Gemini API)
    pub embeddings: Option<Arc<Embeddings>>,

    /// WebSocket event broadcaster
    pub ws_tx: broadcast::Sender<WsEvent>,

    /// Currently active project
    pub project: Arc<RwLock<Option<ProjectContext>>>,
}

impl AppState {
    /// Create new application state
    pub fn new(db: Arc<Database>, embeddings: Option<Arc<Embeddings>>) -> Self {
        let (ws_tx, _) = broadcast::channel(256);

        Self {
            db,
            embeddings,
            ws_tx,
            project: Arc::new(RwLock::new(None)),
        }
    }

    /// Broadcast a WebSocket event to all connected clients
    pub fn broadcast(&self, event: WsEvent) {
        // Ignore send errors (no subscribers is fine)
        let _ = self.ws_tx.send(event);
    }

    /// Set the active project
    pub async fn set_project(&self, project: ProjectContext) {
        let mut guard = self.project.write().await;
        *guard = Some(project);
    }

    /// Get the active project
    pub async fn get_project(&self) -> Option<ProjectContext> {
        self.project.read().await.clone()
    }

    /// Get the active project ID
    pub async fn project_id(&self) -> Option<i64> {
        self.project.read().await.as_ref().map(|p| p.id)
    }
}
