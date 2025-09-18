// src/state.rs
use crate::{
    config::CONFIG,
    git::{GitClient, GitStore},
    llm::{
        client::OpenAIClient,
        responses::{ImageGenerationManager, ResponsesManager},
    },
    memory::{
        storage::qdrant::multi_store::QdrantMultiStore,
        storage::sqlite::store::SqliteMemoryStore,
        cache::recent::RecentCache,  // NEW
        MemoryService,
    },
    project::store::ProjectStore,
};
use crate::tools::file_search::FileSearchService;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::info;
use sqlx::SqlitePool;  // NEW - needed for passing pool around

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
    pub sqlite_pool: SqlitePool,  // NEW - needed for some operations
    pub project_store: Arc<ProjectStore>,
    pub git_store: GitStore,
    pub git_client: GitClient,
    
    pub llm_client: Arc<OpenAIClient>,
    pub responses_manager: Arc<ResponsesManager>,
    pub image_generation_manager: Arc<ImageGenerationManager>,
    
    pub memory_service: Arc<MemoryService>,
    pub multi_store: Arc<QdrantMultiStore>,
    pub recent_cache: Option<Arc<RecentCache>>,  // NEW
    pub file_search_service: Arc<FileSearchService>,
    
    pub upload_sessions: Arc<RwLock<HashMap<String, UploadSession>>>,
}

pub async fn create_app_state(
    sqlite_store: Arc<SqliteMemoryStore>,
    sqlite_pool: SqlitePool,  // NEW parameter - need the pool
    qdrant_url: &str,
    llm_client: Arc<OpenAIClient>,
    project_store: Arc<ProjectStore>,
    git_store: GitStore,
    git_client: GitClient,
) -> anyhow::Result<AppState> {
    info!("Creating AppState with robust memory features including Recent Cache");
    
    let multi_store = Arc::new(QdrantMultiStore::new(qdrant_url, &CONFIG.qdrant_collection).await?);
    
    let responses_manager = Arc::new(ResponsesManager::new(llm_client.clone()));
    let image_generation_manager = Arc::new(ImageGenerationManager::new(llm_client.clone()));
    
    // Create recent cache if enabled (NEW)
    let recent_cache = if CONFIG.is_recent_cache_enabled() {
        let cache_config = CONFIG.get_recent_cache_config();
        info!(
            "Initializing Recent Cache: capacity={}, ttl={}s, max_per_session={}", 
            cache_config.capacity, 
            cache_config.ttl_seconds,
            cache_config.max_per_session
        );
        
        let cache = Arc::new(RecentCache::new(
            cache_config.capacity,
            cache_config.ttl_seconds,
        ));
        
        // Warm up cache with active sessions if enabled
        if cache_config.warmup {
            info!("Warming up cache with active sessions from last 24 hours...");
            
            match sqlite_store.get_active_sessions(24).await {
                Ok(active_sessions) if !active_sessions.is_empty() => {
                    info!("Found {} active sessions to warm up", active_sessions.len());
                    if let Err(e) = cache.warmup_active_sessions(active_sessions, &sqlite_store).await {
                        tracing::warn!("Failed to warm up cache: {}", e);
                    } else {
                        let stats = cache.get_stats().await;
                        info!("Cache warmup complete: {} sessions loaded", stats.total_entries);
                    }
                }
                Ok(_) => {
                    info!("No active sessions found for cache warmup");
                }
                Err(e) => {
                    tracing::warn!("Failed to get active sessions for warmup: {}", e);
                }
            }
        }
        
        Some(cache)
    } else {
        info!("Recent Cache is disabled via configuration");
        None
    };
    
    // Create memory service with cache integration
    let memory_service = if let Some(cache) = &recent_cache {
        Arc::new(MemoryService::new_with_cache(
            sqlite_store.clone(),
            multi_store.clone(),
            llm_client.clone(),
            Some(cache.clone()),  // Pass cache to memory service
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
    
    // Log final state
    if recent_cache.is_some() {
        info!("AppState initialized successfully WITH Recent Cache");
    } else {
        info!("AppState initialized successfully WITHOUT Recent Cache");
    }
    
    Ok(AppState {
        sqlite_store,
        sqlite_pool,
        project_store,
        git_store,
        git_client,
        llm_client,
        responses_manager,
        image_generation_manager,
        memory_service,
        multi_store,
        recent_cache,
        file_search_service,
        upload_sessions: Arc::new(RwLock::new(HashMap::new())),
    })
}
