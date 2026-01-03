// src/web/state.rs
// Web server state management

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, oneshot, RwLock};

use crate::background::watcher::WatcherHandle;
use crate::db::Database;
use crate::embeddings::Embeddings;
use crate::web::claude::ClaudeManager;
use crate::web::deepseek::DeepSeekClient;
use crate::web::search::{GoogleSearchClient, WebFetcher};
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

    /// Current MCP session ID (shared with MCP server)
    pub session_id: Arc<RwLock<Option<String>>>,

    /// Session-level persona overlay (ephemeral, clears on session end)
    pub session_persona: Arc<RwLock<Option<String>>>,

    /// DeepSeek client for chat (Reasoner)
    pub deepseek: Option<Arc<DeepSeekClient>>,

    /// Claude Code instance manager
    pub claude_manager: Arc<ClaudeManager>,

    /// File watcher handle for registering projects
    pub watcher: Option<WatcherHandle>,

    /// Pending responses for agent collaboration (shared with MCP server)
    pub pending_responses: Arc<RwLock<HashMap<String, oneshot::Sender<String>>>>,

    /// Current collaborator Claude instance ID
    pub collaborator_id: Arc<RwLock<Option<String>>>,

    /// Google Custom Search client
    pub google_search: Option<Arc<GoogleSearchClient>>,

    /// Web page fetcher
    pub web_fetcher: Arc<WebFetcher>,
}

impl AppState {
    /// Create new application state
    pub fn new(db: Arc<Database>, embeddings: Option<Arc<Embeddings>>) -> Self {
        let (ws_tx, _) = broadcast::channel(256);

        // Initialize DeepSeek client if API key is available
        let deepseek = std::env::var("DEEPSEEK_API_KEY")
            .ok()
            .map(|key| Arc::new(DeepSeekClient::new(key)));

        let claude_manager = Arc::new(ClaudeManager::new(ws_tx.clone()));

        // Initialize Google Search client if credentials available
        let google_search = GoogleSearchClient::from_env().map(Arc::new);

        Self {
            db,
            embeddings,
            ws_tx,
            project: Arc::new(RwLock::new(None)),
            session_id: Arc::new(RwLock::new(None)),
            session_persona: Arc::new(RwLock::new(None)),
            deepseek,
            claude_manager,
            watcher: None,
            pending_responses: Arc::new(RwLock::new(HashMap::new())),
            collaborator_id: Arc::new(RwLock::new(None)),
            google_search,
            web_fetcher: Arc::new(WebFetcher::new()),
        }
    }

    /// Create with file watcher for automatic indexing
    pub fn with_watcher(
        db: Arc<Database>,
        embeddings: Option<Arc<Embeddings>>,
        watcher: WatcherHandle,
    ) -> Self {
        let (ws_tx, _) = broadcast::channel(256);

        // Initialize DeepSeek client if API key is available
        let deepseek = std::env::var("DEEPSEEK_API_KEY")
            .ok()
            .map(|key| Arc::new(DeepSeekClient::new(key)));

        let claude_manager = Arc::new(ClaudeManager::new(ws_tx.clone()));

        // Initialize Google Search client if credentials available
        let google_search = GoogleSearchClient::from_env().map(Arc::new);

        Self {
            db,
            embeddings,
            ws_tx,
            project: Arc::new(RwLock::new(None)),
            session_id: Arc::new(RwLock::new(None)),
            session_persona: Arc::new(RwLock::new(None)),
            deepseek,
            claude_manager,
            watcher: Some(watcher),
            pending_responses: Arc::new(RwLock::new(HashMap::new())),
            collaborator_id: Arc::new(RwLock::new(None)),
            google_search,
            web_fetcher: Arc::new(WebFetcher::new()),
        }
    }

    /// Create with an existing broadcast channel (for shared mode with MCP server)
    pub fn with_broadcaster(
        db: Arc<Database>,
        embeddings: Option<Arc<Embeddings>>,
        ws_tx: broadcast::Sender<WsEvent>,
        session_id: Arc<RwLock<Option<String>>>,
        project: Arc<RwLock<Option<ProjectContext>>>,
        pending_responses: Arc<RwLock<HashMap<String, oneshot::Sender<String>>>>,
    ) -> Self {
        // Initialize DeepSeek client if API key is available
        let deepseek = std::env::var("DEEPSEEK_API_KEY")
            .ok()
            .map(|key| Arc::new(DeepSeekClient::new(key)));

        let claude_manager = Arc::new(ClaudeManager::new(ws_tx.clone()));

        // Initialize Google Search client if credentials available
        let google_search = GoogleSearchClient::from_env().map(Arc::new);

        Self {
            db,
            embeddings,
            ws_tx,
            project,
            session_id,
            session_persona: Arc::new(RwLock::new(None)),
            deepseek,
            claude_manager,
            watcher: None,
            pending_responses,
            collaborator_id: Arc::new(RwLock::new(None)),
            google_search,
            web_fetcher: Arc::new(WebFetcher::new()),
        }
    }

    /// Broadcast a WebSocket event to all connected clients
    pub fn broadcast(&self, event: WsEvent) {
        // Ignore send errors (no subscribers is fine)
        let _ = self.ws_tx.send(event);
    }

    /// Set the active project and register for file watching
    pub async fn set_project(&self, project: ProjectContext) {
        // Register project with file watcher if available
        if let Some(ref watcher) = self.watcher {
            watcher.watch(project.id, std::path::PathBuf::from(&project.path)).await;
        }

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

    /// Get the session persona overlay
    pub async fn get_session_persona(&self) -> Option<String> {
        self.session_persona.read().await.clone()
    }

    /// Set the session persona overlay (None to clear)
    pub async fn set_session_persona(&self, persona: Option<String>) {
        let mut guard = self.session_persona.write().await;
        *guard = persona;
    }
}
