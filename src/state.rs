// src/state.rs
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use sqlx::SqlitePool;
use anyhow::Result;
use tracing::info;

use crate::config::CONFIG;
use crate::llm::provider::{
    OpenAiEmbeddings,
    gpt5::Gpt5Provider,
    deepseek::DeepSeekProvider,
};
use crate::memory::storage::sqlite::store::SqliteMemoryStore;
use crate::memory::storage::qdrant::multi_store::QdrantMultiStore;
use crate::memory::service::MemoryService;
use crate::memory::features::code_intelligence::CodeIntelligenceService;
use crate::project::store::ProjectStore;
use crate::git::store::GitStore;
use crate::git::client::GitClient;
use crate::operations::OperationEngine;
use crate::api::ws::chat::routing::MessageRouter;

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
    pub gpt5_provider: Arc<Gpt5Provider>,
    pub deepseek_provider: Arc<DeepSeekProvider>,
    pub embedding_client: Arc<OpenAiEmbeddings>,
    pub memory_service: Arc<MemoryService>,
    pub code_intelligence: Arc<CodeIntelligenceService>,
    pub upload_sessions: Arc<RwLock<HashMap<String, UploadSession>>>,
    pub operation_engine: Arc<OperationEngine>,
    pub message_router: Arc<MessageRouter>,
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
            (*code_intelligence).clone(),
        );
        
        // Validate config
        CONFIG.validate()?;
        
        // Initialize GPT-5 provider
        info!("Initializing GPT-5 provider: {}", CONFIG.gpt5_model);
        let gpt5_provider = Arc::new(Gpt5Provider::new(
            CONFIG.gpt5_api_key.clone(),
            CONFIG.gpt5_model.clone(),
            CONFIG.gpt5_max_tokens,
            CONFIG.gpt5_verbosity.clone(),
            CONFIG.gpt5_reasoning.clone(),
        ));
        
        // Initialize DeepSeek provider
        info!("Initializing DeepSeek provider for code generation");
        let deepseek_provider = Arc::new(DeepSeekProvider::new(
            CONFIG.deepseek_api_key.clone(),
        ));
        
        // Initialize OpenAI embeddings client
        let embedding_client = Arc::new(OpenAiEmbeddings::new(
            CONFIG.openai_api_key.clone(),
            CONFIG.openai_embedding_model.clone(),
        ));
        
        // Initialize Qdrant multi-store
        let multi_store = Arc::new(QdrantMultiStore::new(
            &CONFIG.qdrant_url,
            "mira",
        ).await?);
        
        // Memory service uses GPT-5 directly
        let memory_service = Arc::new(MemoryService::new(
            sqlite_store.clone(),
            multi_store.clone(),
            gpt5_provider.clone(),
            embedding_client.clone(),
        ));
        
        // PHASE 8: Initialize OperationEngine WITH MemoryService
        info!("Initializing OperationEngine with memory integration");
        let operation_engine = Arc::new(OperationEngine::new(
            Arc::new(pool.clone()),
            (*gpt5_provider).clone(),
            (*deepseek_provider).clone(),
            memory_service.clone(), // ADDED: Pass memory service
        ));
        
        // Initialize MessageRouter
        let message_router = Arc::new(MessageRouter::new((*gpt5_provider).clone()));
        
        info!("Application state initialized successfully");
        
        Ok(Self {
            sqlite_store,
            sqlite_pool: pool,
            project_store,
            git_store,
            git_client,
            gpt5_provider,
            deepseek_provider,
            embedding_client,
            memory_service,
            code_intelligence,
            upload_sessions: Arc::new(RwLock::new(HashMap::new())),
            operation_engine,
            message_router,
        })
    }
}
