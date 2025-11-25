// src/state.rs
use anyhow::Result;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use crate::auth::AuthService;
use crate::config::CONFIG;
use crate::git::client::GitClient;
use crate::git::store::GitStore;
use crate::llm::provider::{OpenAiEmbeddings, Gpt5Provider};
use crate::memory::features::code_intelligence::CodeIntelligenceService;
use crate::memory::service::MemoryService;
use crate::memory::storage::qdrant::multi_store::QdrantMultiStore;
use crate::memory::storage::sqlite::store::SqliteMemoryStore;
use crate::operations::{ContextLoader, OperationEngine};
use crate::project::store::ProjectStore;
use crate::relationship::{FactsService, RelationshipService};
use crate::sudo::SudoPermissionService;
use crate::terminal::TerminalStore;

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
    pub embedding_client: Arc<OpenAiEmbeddings>,
    pub memory_service: Arc<MemoryService>,
    pub code_intelligence: Arc<CodeIntelligenceService>,
    pub context_loader: Arc<ContextLoader>,
    pub upload_sessions: Arc<RwLock<HashMap<String, UploadSession>>>,
    pub operation_engine: Arc<OperationEngine>,
    pub relationship_service: Arc<RelationshipService>,
    pub facts_service: Arc<FactsService>,
    pub sudo_service: Arc<SudoPermissionService>,
    pub terminal_store: Arc<TerminalStore>,
    pub auth_service: Arc<AuthService>,
}

impl AppState {
    pub async fn new(pool: SqlitePool) -> Result<Self> {
        // Initialize SQLite store
        let sqlite_store = Arc::new(SqliteMemoryStore::new(pool.clone()));

        // Initialize project store
        let project_store = Arc::new(ProjectStore::new(pool.clone()));

        // Validate config
        CONFIG.validate()?;

        // Initialize GPT 5.1 provider (primary LLM)
        info!("Initializing GPT 5.1 provider as primary LLM");
        let gpt5_provider = Arc::new(Gpt5Provider::new(
            CONFIG.openai_api_key.clone(),
            CONFIG.gpt5_model.clone(),
            CONFIG.gpt5_reasoning.clone(),
        ).expect("Failed to create GPT 5.1 provider"));

        // Initialize OpenAI embeddings client
        let embedding_client = Arc::new(OpenAiEmbeddings::new(
            CONFIG.openai_api_key.clone(),
            CONFIG.openai_embedding_model.clone(),
        ));

        // Initialize Qdrant multi-store
        let multi_store = Arc::new(QdrantMultiStore::new(&CONFIG.qdrant_url, "mira").await?);

        // Initialize code intelligence service with embedding support
        let code_intelligence = Arc::new(CodeIntelligenceService::new(
            pool.clone(),
            multi_store.clone(),
            embedding_client.clone(),
        ));

        // Initialize git store and client with code intelligence
        let git_store = GitStore::new(pool.clone());
        let git_client = GitClient::with_code_intelligence(
            std::path::PathBuf::from("./repos"),
            git_store.clone(),
            (*code_intelligence).clone(),
        );

        // Memory service uses GPT 5.1 for analysis
        let memory_service = Arc::new(MemoryService::new(
            sqlite_store.clone(),
            multi_store.clone(),
            gpt5_provider.clone(),
            embedding_client.clone(),
        ));

        // Initialize FactsService
        info!("Initializing FactsService");
        let facts_service = Arc::new(FactsService::new(pool.clone()));

        // Initialize RelationshipService with FactsService
        info!("Initializing RelationshipService with FactsService");
        let relationship_service = Arc::new(RelationshipService::new(
            Arc::new(pool.clone()),
            facts_service.clone(),
        ));

        // Initialize ContextLoader (shared for loading file tree + code intelligence)
        info!("Initializing ContextLoader");
        let context_loader = Arc::new(ContextLoader::new(
            git_client.clone(),
            code_intelligence.clone(),
        ));

        // Initialize sudo permission service
        info!("Initializing sudo permission service");
        let sudo_service = Arc::new(SudoPermissionService::new(Arc::new(pool.clone())));

        // OperationEngine with GPT 5.1 architecture
        info!("Initializing OperationEngine with GPT 5.1");
        let operation_engine = Arc::new(OperationEngine::new(
            Arc::new(pool.clone()),
            (*gpt5_provider).clone(),
            memory_service.clone(),
            relationship_service.clone(),
            git_client.clone(),
            code_intelligence.clone(),
            Some(sudo_service.clone()), // Sudo permissions for system administration
        ));

        // Initialize terminal services
        info!("Initializing terminal services");
        let terminal_store = Arc::new(TerminalStore::new(Arc::new(pool.clone())));

        // Initialize authentication service
        info!("Initializing authentication service");
        let auth_service = Arc::new(AuthService::new(pool.clone()));

        info!("Application state initialized successfully");

        Ok(Self {
            sqlite_store,
            sqlite_pool: pool,
            project_store,
            git_store,
            git_client,
            gpt5_provider,
            embedding_client,
            memory_service,
            code_intelligence,
            context_loader,
            upload_sessions: Arc::new(RwLock::new(HashMap::new())),
            operation_engine,
            relationship_service,
            facts_service,
            sudo_service,
            terminal_store,
            auth_service,
        })
    }
}
