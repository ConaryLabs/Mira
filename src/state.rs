// src/state.rs
use crate::{
    config::CONFIG,
    git::{GitClient, GitStore},
    llm::{
        client::OpenAIClient,
        responses::ImageGenerationManager,
    },
    memory::{
        storage::qdrant::multi_store::QdrantMultiStore,
        storage::sqlite::store::SqliteMemoryStore,
        cache::recent::RecentCache,
        features::code_intelligence::CodeIntelligenceService,
        MemoryService,
    },
    project::store::ProjectStore,
    tools::file_search::FileSearchService,
};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::info;
use sqlx::SqlitePool;

#[derive(Debug, Clone)]
pub struct UploadSession {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub chunks: Vec<Vec<u8>>,
    pub total_size: usize,
    pub received_size: usize,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone)]
pub struct AppState {
    pub sqlite_store: Arc<SqliteMemoryStore>,
    pub sqlite_pool: SqlitePool,
    pub project_store: Arc<ProjectStore>,
    pub git_store: GitStore,
    pub git_client: GitClient,
    
    pub llm_client: Arc<OpenAIClient>,
    pub image_generation_manager: Arc<ImageGenerationManager>,
    
    pub memory_service: Arc<MemoryService>,
    pub multi_store: Arc<QdrantMultiStore>,
    pub recent_cache: Option<Arc<RecentCache>>,
    pub file_search_service: Arc<FileSearchService>,
    
    pub code_intelligence: Arc<CodeIntelligenceService>,
    
    pub upload_sessions: Arc<RwLock<HashMap<String, UploadSession>>>,
}

pub async fn create_app_state(
    sqlite_store: Arc<SqliteMemoryStore>,
    sqlite_pool: SqlitePool,
    qdrant_url: &str,
    llm_client: Arc<OpenAIClient>,
    project_store: Arc<ProjectStore>,
    git_store: GitStore,
) -> anyhow::Result<AppState> {
    info!("Initializing AppState with code intelligence");
    
    let multi_store = Arc::new(QdrantMultiStore::new(qdrant_url, &CONFIG.qdrant_collection).await?);
    let image_generation_manager = Arc::new(ImageGenerationManager::new(llm_client.clone()));
    
    let code_intelligence = Arc::new(CodeIntelligenceService::new(sqlite_pool.clone()));
    let git_client = GitClient::with_code_intelligence(
        "./repos",
        git_store.clone(),
        code_intelligence.as_ref().clone(),
    );
    
    let recent_cache = if CONFIG.is_recent_cache_enabled() {
        let cache_config = CONFIG.get_recent_cache_config();
        let cache = Arc::new(RecentCache::new(
            cache_config.capacity,
            cache_config.ttl_seconds,
        ));
        
        if cache_config.warmup {
            // FIXED: Get active sessions directly from the database instead of missing method
            match get_active_sessions_from_db(&sqlite_pool).await {
                Ok(active_sessions) if !active_sessions.is_empty() => {
                    if let Err(e) = cache.warmup_active_sessions(active_sessions, &sqlite_store).await {
                        tracing::warn!("Failed to warm up cache: {}", e);
                    }
                }
                Err(e) => tracing::warn!("Failed to get active sessions for warmup: {}", e),
                _ => {}
            }
        }
        
        Some(cache)
    } else {
        None
    };
    
    let memory_service = if let Some(cache) = &recent_cache {
        Arc::new(MemoryService::new_with_cache(
            sqlite_store.clone(),
            multi_store.clone(),
            llm_client.clone(),
            Some(cache.clone()),
        ))
    } else {
        Arc::new(MemoryService::new(
            sqlite_store.clone(),
            multi_store.clone(),
            llm_client.clone(),
        ))
    };
    
    let file_search_service = Arc::new(FileSearchService::new(
        multi_store.clone(),
        llm_client.clone(),
    ));
    
    info!("AppState initialized successfully");
    
    Ok(AppState {
        sqlite_store,
        sqlite_pool,
        project_store,
        git_store,
        git_client,
        llm_client,
        image_generation_manager,
        memory_service,
        multi_store,
        recent_cache,
        file_search_service,
        code_intelligence,
        upload_sessions: Arc::new(RwLock::new(HashMap::new())),
    })
}

// Helper function to get active sessions directly from the database
async fn get_active_sessions_from_db(pool: &SqlitePool) -> anyhow::Result<Vec<String>> {
    let sessions: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT DISTINCT session_id
        FROM memory_entries
        WHERE timestamp > datetime('now', '-24 hours')
        ORDER BY timestamp DESC
        LIMIT 100
        "#
    )
    .fetch_all(pool)
    .await?;
    
    Ok(sessions)
}
