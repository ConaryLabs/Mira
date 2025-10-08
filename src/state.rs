// src/state.rs
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use sqlx::SqlitePool;
use anyhow::{anyhow, Result};
use tracing::info;

use crate::config::CONFIG;
use crate::llm::client::{OpenAIClient, config::ClientConfig};
use crate::llm::provider::{
    LlmProvider,
    claude::ClaudeProvider,
    openai::OpenAiProvider,
    deepseek::DeepSeekProvider,
};
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
    pub llm: Arc<dyn LlmProvider>,  // Multi-provider LLM for chat/analysis
    pub embedding_client: Arc<OpenAIClient>,  // OpenAI for embeddings only
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
            (*code_intelligence).clone(),
        );
        
        // Get OpenAI key for embeddings (try both env vars)
        let openai_key = CONFIG.get_openai_key()
            .ok_or_else(|| anyhow!("OPENAI_API_KEY or OPENAI_EMBEDDING_API_KEY required for embeddings"))?;
        
        // Initialize LLM provider based on config
        let llm: Arc<dyn LlmProvider> = match CONFIG.llm_provider.as_str() {
            "claude" => {
                info!("🤖 Initializing LLM provider: {}", CONFIG.anthropic_model);
                Arc::new(ClaudeProvider::new(
                    CONFIG.anthropic_api_key.clone(),
                    CONFIG.anthropic_model.clone(),
                    CONFIG.anthropic_max_tokens,
                ))
            },
            
            "gpt5" => {
                info!("🤖 Initializing GPT-5 provider: {}", CONFIG.openai_chat_model);
                Arc::new(OpenAiProvider::new(
                    openai_key.clone(),
                    CONFIG.openai_chat_model.clone(),
                    CONFIG.openai_max_tokens,
                    CONFIG.openai_reasoning_effort.clone(),
                    CONFIG.openai_verbosity.clone(),
                ))
            },
            
            "deepseek" => {
                let deepseek_key = CONFIG.deepseek_api_key.clone()
                    .ok_or_else(|| anyhow!("DEEPSEEK_API_KEY required when LLM_PROVIDER=deepseek"))?;
                info!("🤖 Initializing DeepSeek provider: {}", CONFIG.deepseek_model);
                Arc::new(DeepSeekProvider::new(
                    deepseek_key,
                    CONFIG.deepseek_model.clone(),
                    CONFIG.deepseek_max_tokens,
                ))
            },
            
            unknown => {
                return Err(anyhow!(
                    "Unknown LLM_PROVIDER: '{}'. Valid options: claude, gpt5, deepseek",
                    unknown
                ));
            }
        };
        
        // Keep OpenAI client for embeddings only
        let embedding_config = ClientConfig {
            api_key: openai_key,
            base_url: "https://api.openai.com".to_string(),
            model: CONFIG.openai_embedding_model.clone(),
            max_output_tokens: 8192,  // Not used for embeddings, but required
        };
        let embedding_client = Arc::new(OpenAIClient::new(embedding_config)?);
        
        // Initialize Qdrant multi-store
        let multi_store = Arc::new(QdrantMultiStore::new(
            &CONFIG.qdrant_url,
            "mira",
        ).await?);
        
        // Initialize memory service with both LLM provider and embedding client
        let memory_service = Arc::new(MemoryService::new(
            sqlite_store.clone(),
            multi_store.clone(),
            llm.clone(),                // For chat/analysis
            embedding_client.clone(),   // For embeddings
        ));
        
        Ok(Self {
            sqlite_store,
            sqlite_pool: pool,
            project_store,
            git_store,
            git_client,
            llm,  // Multi-provider LLM
            embedding_client,  // OpenAI embeddings
            memory_service,
            code_intelligence,
            upload_sessions: Arc::new(RwLock::new(HashMap::new())),
        })
    }
}
