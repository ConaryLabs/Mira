// src/state.rs
use anyhow::Result;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use crate::auth::AuthService;
use crate::budget::BudgetTracker;
use crate::build::{BuildTracker, ErrorResolver};
use crate::cache::LlmCache;
use crate::config::CONFIG;
use crate::context_oracle::ContextOracle;
use crate::git::client::GitClient;
use crate::git::intelligence::{CochangeService, ExpertiseService, FixService};
use crate::git::store::GitStore;
use crate::llm::provider::{Gpt5Provider, OpenAiEmbeddings};
use crate::memory::features::code_intelligence::{CodeIntelligenceService, SemanticGraphService};
use crate::memory::service::MemoryService;
use crate::memory::storage::qdrant::multi_store::QdrantMultiStore;
use crate::memory::storage::sqlite::store::SqliteMemoryStore;
use crate::operations::{ContextLoader, OperationEngine};
use crate::patterns::{PatternMatcher, PatternStorage};
use crate::project::guidelines::ProjectGuidelinesService;
use crate::project::store::ProjectStore;
use crate::relationship::{FactsService, RelationshipService};
use crate::sudo::SudoPermissionService;
use crate::synthesis::storage::SynthesisStorage;
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
    pub guidelines_service: Arc<ProjectGuidelinesService>,
    pub git_store: GitStore,
    pub git_client: GitClient,
    pub gpt5_provider: Arc<Gpt5Provider>,
    pub embedding_client: Arc<OpenAiEmbeddings>,
    pub memory_service: Arc<MemoryService>,
    pub code_intelligence: Arc<CodeIntelligenceService>,
    pub semantic_graph: Arc<SemanticGraphService>,
    pub context_loader: Arc<ContextLoader>,
    pub upload_sessions: Arc<RwLock<HashMap<String, UploadSession>>>,
    pub operation_engine: Arc<OperationEngine>,
    pub relationship_service: Arc<RelationshipService>,
    pub facts_service: Arc<FactsService>,
    pub sudo_service: Arc<SudoPermissionService>,
    pub terminal_store: Arc<TerminalStore>,
    pub auth_service: Arc<AuthService>,
    // Git intelligence services
    pub cochange_service: Arc<CochangeService>,
    pub expertise_service: Arc<ExpertiseService>,
    pub fix_service: Arc<FixService>,
    // Build system
    pub build_tracker: Arc<BuildTracker>,
    pub error_resolver: Arc<ErrorResolver>,
    // Pattern services
    pub pattern_storage: Arc<PatternStorage>,
    pub pattern_matcher: Arc<PatternMatcher>,
    // Context Oracle - unified context gathering
    pub context_oracle: Arc<ContextOracle>,
    // Budget tracking
    pub budget_tracker: Arc<BudgetTracker>,
    // LLM response cache
    pub llm_cache: Arc<LlmCache>,
    // Tool synthesis
    pub synthesis_storage: Arc<SynthesisStorage>,
}

impl AppState {
    pub async fn new(pool: SqlitePool) -> Result<Self> {
        // Initialize SQLite store
        let sqlite_store = Arc::new(SqliteMemoryStore::new(pool.clone()));

        // Initialize project store
        let project_store = Arc::new(ProjectStore::new(pool.clone()));

        // Initialize guidelines service
        let guidelines_service = Arc::new(ProjectGuidelinesService::new(pool.clone()));

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

        // Initialize semantic graph service for concept-based code search
        info!("Initializing semantic graph service");
        let semantic_graph = Arc::new(code_intelligence.create_semantic_service(gpt5_provider.clone()));

        // Initialize git store and client with code intelligence
        let git_store = GitStore::new(pool.clone());
        let git_client = GitClient::with_code_intelligence(
            std::path::PathBuf::from("./repos"),
            git_store.clone(),
            (*code_intelligence).clone(),
        );

        // Initialize git intelligence services (needed for context oracle)
        info!("Initializing git intelligence services");
        let cochange_service = Arc::new(CochangeService::new(pool.clone()));
        let expertise_service = Arc::new(ExpertiseService::new(pool.clone()));
        let fix_service = Arc::new(FixService::new(pool.clone()));

        // Initialize build tracker (needed for context oracle)
        info!("Initializing build tracker");
        let build_tracker = Arc::new(BuildTracker::new(Arc::new(pool.clone())));

        // Initialize error resolver (needed for context oracle)
        info!("Initializing error resolver");
        let error_resolver = Arc::new(ErrorResolver::new(Arc::new(pool.clone()), build_tracker.clone()));

        // Initialize budget tracker
        info!("Initializing budget tracker");
        let daily_limit = std::env::var("BUDGET_DAILY_LIMIT_USD")
            .unwrap_or_else(|_| "5.0".to_string())
            .parse::<f64>()
            .unwrap_or(5.0);
        let monthly_limit = std::env::var("BUDGET_MONTHLY_LIMIT_USD")
            .unwrap_or_else(|_| "150.0".to_string())
            .parse::<f64>()
            .unwrap_or(150.0);
        let budget_tracker = Arc::new(BudgetTracker::new(pool.clone(), daily_limit, monthly_limit));

        // Initialize LLM cache
        info!("Initializing LLM cache");
        let cache_enabled = std::env::var("CACHE_ENABLED")
            .unwrap_or_else(|_| "true".to_string())
            .parse::<bool>()
            .unwrap_or(true);
        let cache_ttl = std::env::var("CACHE_TTL_SECONDS")
            .unwrap_or_else(|_| "86400".to_string())
            .parse::<i64>()
            .unwrap_or(86400);
        let llm_cache = Arc::new(LlmCache::new(pool.clone(), cache_enabled, cache_ttl));

        // Initialize pattern services (needed for context oracle)
        info!("Initializing pattern services");
        let pattern_storage = Arc::new(PatternStorage::new(Arc::new(pool.clone())));
        let pattern_matcher = Arc::new(PatternMatcher::new(pattern_storage.clone()));

        // Initialize synthesis storage
        info!("Initializing synthesis storage");
        let synthesis_storage = Arc::new(SynthesisStorage::new(Arc::new(pool.clone())));

        // Initialize Context Oracle with all intelligence services
        info!("Initializing Context Oracle");
        let context_oracle = Arc::new(
            ContextOracle::new(Arc::new(pool.clone()))
                .with_code_intelligence(code_intelligence.clone())
                .with_semantic_graph(semantic_graph.clone())
                .with_guidelines(guidelines_service.clone())
                .with_cochange(cochange_service.clone())
                .with_expertise(expertise_service.clone())
                .with_fix_service(fix_service.clone())
                .with_build_tracker(build_tracker.clone())
                .with_error_resolver(error_resolver.clone())
                .with_pattern_storage(pattern_storage.clone())
                .with_pattern_matcher(pattern_matcher.clone()),
        );

        // Memory service uses GPT 5.1 for analysis with Context Oracle for code intelligence
        let memory_service = Arc::new(MemoryService::with_oracle(
            sqlite_store.clone(),
            multi_store.clone(),
            gpt5_provider.clone(),
            embedding_client.clone(),
            Some(context_oracle.clone()),
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

        // OperationEngine with GPT 5.1 architecture and Context Oracle
        info!("Initializing OperationEngine with GPT 5.1 and Context Oracle");
        let operation_engine = Arc::new(OperationEngine::new(
            Arc::new(pool.clone()),
            (*gpt5_provider).clone(),
            memory_service.clone(),
            relationship_service.clone(),
            git_client.clone(),
            code_intelligence.clone(),
            Some(sudo_service.clone()), // Sudo permissions for system administration
            Some(context_oracle.clone()), // Context Oracle for unified intelligence
            Some(budget_tracker.clone()), // Budget tracking for cost control
            Some(llm_cache.clone()), // LLM response cache for cost optimization
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
            guidelines_service,
            git_store,
            git_client,
            gpt5_provider,
            embedding_client,
            memory_service,
            code_intelligence,
            semantic_graph,
            context_loader,
            upload_sessions: Arc::new(RwLock::new(HashMap::new())),
            operation_engine,
            relationship_service,
            facts_service,
            sudo_service,
            terminal_store,
            auth_service,
            cochange_service,
            expertise_service,
            fix_service,
            build_tracker,
            error_resolver,
            pattern_storage,
            pattern_matcher,
            context_oracle,
            budget_tracker,
            llm_cache,
            synthesis_storage,
        })
    }
}
