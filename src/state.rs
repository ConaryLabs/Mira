// src/state.rs
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use sqlx::SqlitePool;
use anyhow::Result;

use crate::config::CONFIG;
use crate::llm::client::{OpenAIClient, config::ClientConfig};
use crate::memory::storage::sqlite::store::SqliteMemoryStore;
use crate::memory::storage::qdrant::multi_store::QdrantMultiStore;
use crate::memory::service::MemoryService;
use crate::memory::features::code_intelligence::CodeIntelligenceService;
use crate::project::store::ProjectStore;
use crate::git::store::GitStore;
use crate::git::client::GitClient;

/// Session data for file uploads
#[derive(Clone)]
pub struct UploadSession {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub chunks: Vec<Vec<u8>>,
    pub total_size: usize,
    pub received_size: usize,
}

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub sqlite_store: Arc<SqliteMemoryStore>,
    pub sqlite_pool: SqlitePool,
    pub project_store: Arc<ProjectStore>,
    pub git_store: GitStore,
    pub git_client: GitClient,
    pub llm_client: Arc<OpenAIClient>,
    pub memory_service: Arc<MemoryService>,
    pub code_intelligence: Arc<CodeIntelligenceService>,
    pub upload_sessions: Arc<RwLock<HashMap<String, UploadSession>>>,
}

impl AppState {
    pub async fn new(pool: SqlitePool) -> Result<Self> {
        // Initialize SQLite store
        let sqlite_store = Arc::new(SqliteMemoryStore::new(pool.clone()));
        
        // Initialize project store
        let project_store = Arc::new(ProjectStore::new(pool.clone()));
        
        // Initialize code intelligence service FIRST (needed for git client)
        let code_intelligence = Arc::new(CodeIntelligenceService::new(pool.clone()));
        
        // Initialize git store and client WITH code intelligence
        let git_store = GitStore::new(pool.clone());
        let git_client = GitClient::with_code_intelligence(
            std::path::PathBuf::from("./repos"),
            git_store.clone(),
            (*code_intelligence).clone(),  // Clone the service, not the Arc
        );
        
        // FIXED: Use Claude API key for chat, not OpenAI embedding key
        let client_config = ClientConfig {
            api_key: CONFIG.anthropic_api_key.clone(),
            base_url: CONFIG.anthropic_base_url.clone(),
            model: CONFIG.anthropic_model.clone(),
            max_output_tokens: CONFIG.anthropic_max_tokens,
        };
        let llm_client = Arc::new(OpenAIClient::new(client_config)?);
        
        // Initialize Qdrant multi-store
        let multi_store = Arc::new(QdrantMultiStore::new(
            &CONFIG.qdrant_url,
            "mira", // collection base name
        ).await?);
        
        // Initialize memory service with all components
        let memory_service = Arc::new(MemoryService::new(
            sqlite_store.clone(),
            multi_store.clone(),
            llm_client.clone(),
        ));
        
        Ok(Self {
            sqlite_store,
            sqlite_pool: pool,
            project_store,
            git_store,
            git_client,
            llm_client,
            memory_service,
            code_intelligence,
            upload_sessions: Arc::new(RwLock::new(HashMap::new())),
        })
    }
}
