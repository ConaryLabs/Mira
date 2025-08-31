// src/state.rs
// PHASE 5: Enhanced with multi-head memory service and context service integration
// PHASE 3 UPDATE: Added ImageGenerationManager and FileSearchService to AppState

use crate::{
    config::CONFIG,
    git::{GitClient, GitStore},
    llm::{
        client::OpenAIClient,
        responses::{ImageGenerationManager, ResponsesManager, ThreadManager, VectorStoreManager},
    },
    memory::{
        qdrant::{multi_store::QdrantMultiStore, store::QdrantMemoryStore},
        sqlite::store::SqliteMemoryStore,
    },
    persona::PersonaOverlay,
    project::store::ProjectStore,
    services::{
        chat::ChatConfig, summarization::SummarizationService, ChatService, ContextService,
        DocumentService, FileSearchService, MemoryService,
    },
};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    // -------- Storage --------
    pub sqlite_store: Arc<SqliteMemoryStore>,
    pub qdrant_store: Arc<QdrantMemoryStore>,
    pub qdrant_multi_store: Arc<QdrantMultiStore>,
    pub project_store: Arc<ProjectStore>,
    pub git_store: GitStore,
    pub git_client: GitClient,

    // -------- LLM Core --------
    pub llm_client: Arc<OpenAIClient>,
    pub responses_manager: Arc<ResponsesManager>,
    pub vector_store_manager: Arc<VectorStoreManager>,
    pub thread_manager: Arc<ThreadManager>,
    pub image_generation_manager: Arc<ImageGenerationManager>,

    // -------- Services --------
    pub chat_service: Arc<ChatService>,
    pub memory_service: Arc<MemoryService>,
    pub context_service: Arc<ContextService>,
    pub document_service: Arc<DocumentService>,
    pub file_search_service: Arc<FileSearchService>,
}

impl AppState {
    /// ── Phase 5: Enhanced ChatService assembly with multi-head support ──
    pub fn assemble_chat_service(
        llm_client: Arc<OpenAIClient>,
        thread_manager: Arc<ThreadManager>,
        vector_store_manager: Arc<VectorStoreManager>,
        memory_service: Arc<MemoryService>,
        sqlite_store: Arc<SqliteMemoryStore>,
        qdrant_store: Arc<QdrantMemoryStore>,
        persona: PersonaOverlay,
        config: Option<ChatConfig>,
    ) -> Arc<ChatService> {
        let chat_config = config.unwrap_or_default();

        let summarization_service = Arc::new(SummarizationService::new_with_stores(
            llm_client.clone(),
            Arc::new(chat_config.clone()),
            sqlite_store.clone(),
            memory_service.clone(),
        ));

        Arc::new(ChatService::new(
            llm_client,
            thread_manager,
            vector_store_manager,
            persona,
            memory_service,
            sqlite_store,
            qdrant_store,
            summarization_service,
            Some(chat_config),
        ))
    }

    /// ── Phase 5: Enhanced MemoryService assembly with multi-collection support ──
    pub fn assemble_memory_service_with_multi_store(
        sqlite_store: Arc<SqliteMemoryStore>,
        qdrant_store: Arc<QdrantMemoryStore>,
        qdrant_multi_store: Arc<QdrantMultiStore>,
        llm_client: Arc<OpenAIClient>,
    ) -> Arc<MemoryService> {
        Arc::new(MemoryService::new_with_multi_store(
            sqlite_store,
            qdrant_store,
            qdrant_multi_store,
            llm_client,
        ))
    }

    /// ── Phase 5: Enhanced ContextService assembly with MemoryService integration ──
    pub fn assemble_context_service_with_memory_service(
        llm_client: Arc<OpenAIClient>,
        sqlite_store: Arc<SqliteMemoryStore>,
        qdrant_store: Arc<QdrantMemoryStore>,
        multi_store: Option<Arc<QdrantMultiStore>>,
        memory_service: Arc<MemoryService>,
    ) -> Arc<ContextService> {
        Arc::new(ContextService::new_with_memory_service(
            llm_client,
            sqlite_store,
            qdrant_store,
            multi_store,
            memory_service,
        ))
    }

    /// Helper to create FileSearchService with required dependencies
    pub fn assemble_file_search_service(
        vector_store_manager: Arc<VectorStoreManager>,
        git_client: GitClient,
    ) -> Arc<FileSearchService> {
        Arc::new(FileSearchService::new(vector_store_manager, git_client))
    }

    /// ── Phase 5: Enhanced method to wire chat service with multi-collection memory support ──
    pub fn wire_chat_service(&mut self, persona: PersonaOverlay, config: Option<ChatConfig>) {
        let chat = Self::assemble_chat_service(
            self.llm_client.clone(),
            self.thread_manager.clone(),
            self.vector_store_manager.clone(),
            self.memory_service.clone(),
            self.sqlite_store.clone(),
            self.qdrant_store.clone(),
            persona,
            config,
        );
        self.chat_service = chat;
    }

    /// Helper to access ImageGenerationManager for tool integration
    pub fn image_generation_manager(&self) -> &Arc<ImageGenerationManager> {
        &self.image_generation_manager
    }

    /// Helper to access FileSearchService for tool integration
    pub fn file_search_service(&self) -> &Arc<FileSearchService> {
        &self.file_search_service
    }

    /// ── Phase 5: Helper to access QdrantMultiStore ──
    pub fn qdrant_multi_store(&self) -> &Arc<QdrantMultiStore> {
        &self.qdrant_multi_store
    }

    /// ── Phase 5: Check if enhanced features are available ──
    pub fn is_enhanced_mode(&self) -> bool {
        CONFIG.is_robust_memory_enabled() && self.memory_service.is_multi_head_enabled()
    }

    /// ── Phase 5: Get system capabilities for monitoring ──
    pub fn get_system_capabilities(&self) -> AppStateCapabilities {
        AppStateCapabilities {
            robust_memory_enabled: CONFIG.is_robust_memory_enabled(),
            multi_head_available: self.memory_service.is_multi_head_enabled(),
            rolling_summaries_enabled: CONFIG.summary_rolling_10 || CONFIG.summary_rolling_100,
            parallel_recall_available: true, // Always available with current setup
            vector_search_enabled: CONFIG.enable_vector_search,
            file_search_enabled: true, // FileSearchService always available
            image_generation_enabled: CONFIG.enable_image_generation,
            collections_available: self.qdrant_multi_store.get_enabled_heads().len(),
        }
    }
}

/// ── Phase 5: System capabilities for monitoring and debugging ──
#[derive(Debug, Clone)]
pub struct AppStateCapabilities {
    pub robust_memory_enabled: bool,
    pub multi_head_available: bool,
    pub rolling_summaries_enabled: bool,
    pub parallel_recall_available: bool,
    pub vector_search_enabled: bool,
    pub file_search_enabled: bool,
    pub image_generation_enabled: bool,
    pub collections_available: usize,
}

// ── Phase 5: Factory functions for creating enhanced AppState ──

/// ── Phase 5: Create AppState with properly initialized multi-collection Qdrant support ──
pub async fn create_app_state_with_multi_qdrant(
    sqlite_store: Arc<SqliteMemoryStore>,
    qdrant_url: &str,
    llm_client: Arc<OpenAIClient>,
    project_store: Arc<ProjectStore>,
    git_store: GitStore,
    git_client: GitClient,
) -> anyhow::Result<AppState> {
    use tracing::{info, warn};

    info!("🚀 Creating AppState with Phase 5 enhancements");

    // Initialize single Qdrant store for backward compatibility
    let qdrant_store =
        Arc::new(QdrantMemoryStore::new(qdrant_url, &CONFIG.qdrant_collection).await?);

    // ── Phase 5: Initialize multi-collection Qdrant store ──
    let qdrant_multi_store = if CONFIG.is_robust_memory_enabled() {
        info!("🏗️  Robust memory enabled - initializing multi-collection Qdrant store");
        Arc::new(QdrantMultiStore::new(qdrant_url, &CONFIG.qdrant_collection).await?)
    } else {
        info!("📦 Robust memory disabled - using compatibility multi-store wrapper");
        Arc::new(QdrantMultiStore::from_single_store(qdrant_store.clone()))
    };

    // Initialize LLM response managers
    let responses_manager = Arc::new(ResponsesManager::new(llm_client.clone()));
    let vector_store_manager = Arc::new(VectorStoreManager::new(llm_client.clone()));
    let thread_manager = Arc::new(ThreadManager::new(
        CONFIG.history_message_cap,
        CONFIG.history_token_limit,
    ));
    let image_generation_manager = Arc::new(ImageGenerationManager::new(llm_client.clone()));

    // ── Phase 5: Create enhanced memory service with multi-store support ──
    let memory_service = AppState::assemble_memory_service_with_multi_store(
        sqlite_store.clone(),
        qdrant_store.clone(),
        qdrant_multi_store.clone(),
        llm_client.clone(),
    );

    // ── Phase 5: Create enhanced context service with memory service integration ──
    let context_service = AppState::assemble_context_service_with_memory_service(
        llm_client.clone(),
        sqlite_store.clone(),
        qdrant_store.clone(),
        Some(qdrant_multi_store.clone()),
        memory_service.clone(),
    );

    // Initialize document service
    let document_service =
        Arc::new(DocumentService::new(memory_service.clone(), vector_store_manager.clone()));

    // Initialize file search service
    let file_search_service = AppState::assemble_file_search_service(
        vector_store_manager.clone(),
        git_client.clone(),
    );

    // Create default persona for chat service initialization
    let default_persona = PersonaOverlay::mira();

    // Create enhanced chat service
    let chat_service = AppState::assemble_chat_service(
        llm_client.clone(),
        thread_manager.clone(),
        vector_store_manager.clone(),
        memory_service.clone(),
        sqlite_store.clone(),
        qdrant_store.clone(),
        default_persona,
        None, // Use default ChatConfig
    );

    // Log initialization results
    info!(
        "✅ AppState initialized with {} Qdrant collections",
        if CONFIG.is_robust_memory_enabled() { "multiple" } else { "single" }
    );

    if CONFIG.is_robust_memory_enabled() {
        let collection_info = qdrant_multi_store.get_collection_info();
        info!("📊 Multi-collection setup:");
        for (head, collection_name) in collection_info {
            info!("   - {}: {}", head.as_str(), collection_name);
        }
    } else {
        warn!("⚠️  Robust memory disabled - using single collection mode");
    }

    // ── Phase 5: Log enhanced capabilities ──
    let capabilities = AppStateCapabilities {
        robust_memory_enabled: CONFIG.is_robust_memory_enabled(),
        multi_head_available: memory_service.is_multi_head_enabled(),
        rolling_summaries_enabled: CONFIG.summary_rolling_10 || CONFIG.summary_rolling_100,
        parallel_recall_available: true,
        vector_search_enabled: CONFIG.enable_vector_search,
        file_search_enabled: true,
        image_generation_enabled: CONFIG.enable_image_generation,
        collections_available: qdrant_multi_store.get_enabled_heads().len(),
    };

    info!("🎯 Phase 5 capabilities summary:");
    info!("   - Multi-head retrieval: {}", capabilities.multi_head_available);
    info!("   - Rolling summaries: {}", capabilities.rolling_summaries_enabled);
    info!("   - Parallel recall: {}", capabilities.parallel_recall_available);
    info!("   - Collections: {}", capabilities.collections_available);

    Ok(AppState {
        sqlite_store,
        qdrant_store,
        qdrant_multi_store,
        project_store,
        git_store,
        git_client,
        llm_client,
        responses_manager,
        vector_store_manager,
        thread_manager,
        image_generation_manager,
        chat_service,
        memory_service,
        context_service,
        document_service,
        file_search_service,
    })
}

/// ── Phase 5: Create AppState with legacy mode (for testing/compatibility) ──
pub async fn create_app_state_legacy_mode(
    sqlite_store: Arc<SqliteMemoryStore>,
    qdrant_url: &str,
    llm_client: Arc<OpenAIClient>,
    project_store: Arc<ProjectStore>,
    git_store: GitStore,
    git_client: GitClient,
) -> anyhow::Result<AppState> {
    use tracing::info;

    info!("🚀 Creating AppState in legacy mode (Phase 5 features disabled)");

    // Initialize single Qdrant store
    let qdrant_store =
        Arc::new(QdrantMemoryStore::new(qdrant_url, &CONFIG.qdrant_collection).await?);

    // Use compatibility wrapper for multi-store
    let qdrant_multi_store = Arc::new(QdrantMultiStore::from_single_store(qdrant_store.clone()));

    // Initialize LLM response managers
    let responses_manager = Arc::new(ResponsesManager::new(llm_client.clone()));
    let vector_store_manager = Arc::new(VectorStoreManager::new(llm_client.clone()));
    let thread_manager = Arc::new(ThreadManager::new(
        CONFIG.history_message_cap,
        CONFIG.history_token_limit,
    ));
    let image_generation_manager = Arc::new(ImageGenerationManager::new(llm_client.clone()));

    // Create legacy memory service (single-head only)
    let memory_service = Arc::new(MemoryService::new(
        sqlite_store.clone(),
        qdrant_store.clone(),
        llm_client.clone(),
    ));

    // Create legacy context service (no memory service integration)
    let context_service = Arc::new(ContextService::new(
        llm_client.clone(),
        sqlite_store.clone(),
        qdrant_store.clone(),
    ));

    // Initialize remaining services
    let document_service =
        Arc::new(DocumentService::new(memory_service.clone(), vector_store_manager.clone()));
    let file_search_service = AppState::assemble_file_search_service(
        vector_store_manager.clone(),
        git_client.clone(),
    );

    let default_persona = PersonaOverlay::mira();
    let chat_service = AppState::assemble_chat_service(
        llm_client.clone(),
        thread_manager.clone(),
        vector_store_manager.clone(),
        memory_service.clone(),
        sqlite_store.clone(),
        qdrant_store.clone(),
        default_persona,
        None,
    );

    info!("✅ AppState initialized in legacy mode");

    Ok(AppState {
        sqlite_store,
        qdrant_store,
        qdrant_multi_store,
        project_store,
        git_store,
        git_client,
        llm_client,
        responses_manager,
        vector_store_manager,
        thread_manager,
        image_generation_manager,
        chat_service,
        memory_service,
        context_service,
        document_service,
        file_search_service,
    })
}

/// ── Phase 5: Health check function for monitoring ──
pub async fn check_app_state_health(app_state: &AppState) -> anyhow::Result<AppStateHealth> {
    use tracing::debug;

    debug!("Performing Phase 5 health check");

    let capabilities = app_state.get_system_capabilities();
    let context_health = app_state.context_service.health_check().await?;
    let _memory_stats = app_state.memory_service.get_service_stats("health_check").await?;

    Ok(AppStateHealth {
        capabilities: capabilities.clone(),
        context_service_health: context_health,
        memory_service_available: true,
        total_collections: capabilities.collections_available,
        enhanced_mode_active: app_state.is_enhanced_mode(),
        services_operational: true, // Could be expanded with more detailed checks
    })
}

/// ── Phase 5: AppState health status ──
#[derive(Debug, Clone)]
pub struct AppStateHealth {
    pub capabilities: AppStateCapabilities,
    pub context_service_health: crate::services::context::ContextServiceHealth,
    pub memory_service_available: bool,
    pub total_collections: usize,
    pub enhanced_mode_active: bool,
    pub services_operational: bool,
}
