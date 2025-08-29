// src/main.rs
// PHASE 1: Multi-Collection Qdrant Support for GPT-5 Robust Memory
// PHASE 3: File search and image generation integration

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, warn, error};
use sqlx::SqlitePool;
use axum::{
    routing::{get, post},
    Router,
    http::Method,
};
use tower::ServiceBuilder;
use tower_http::cors::{CorsLayer, Any};

use mira_backend::config::CONFIG;
use mira_backend::memory::sqlite::store::SqliteMemoryStore;
use mira_backend::memory::qdrant::store::QdrantMemoryStore;
use mira_backend::project::store::ProjectStore;
use mira_backend::git::{GitStore, GitClient};
use mira_backend::llm::client::OpenAIClient;
use mira_backend::state::{AppState, create_app_state_with_multi_qdrant};  // PHASE 1: New import
use mira_backend::api::http::chat::{rest_chat_handler, get_chat_history};
use mira_backend::api::ws::chat::websocket_chat_handler;
use mira_backend::api::http::handlers::{health_handler, project_details_handler};
use mira_backend::project::{
    create_project_handler,
    list_projects_handler,
    get_project_handler,
    update_project_handler,
    delete_project_handler,
    create_artifact_handler,
    get_artifact_handler,
    list_project_artifacts_handler,
    update_artifact_handler,
    delete_artifact_handler,
};
use mira_backend::api::http::git::{
    attach_repo_handler,
    list_attached_repos_handler,
    sync_repo_handler,
    get_file_tree_handler,
    get_file_content_handler,
    update_file_content_handler,
    list_branches,
    switch_branch,
    get_commit_history,
    get_commit_diff,
    get_file_at_commit,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("🚀 Starting Mira Backend (Phase 1: Multi-Collection Support)");
    info!("Config loaded from environment and .env file");
    
    // PHASE 1: Log robust memory configuration status
    if CONFIG.is_robust_memory_enabled() {
        info!("🧠 Robust Memory: ENABLED");
        info!("  - Embedding heads: {}", CONFIG.embed_heads);
        info!("  - Rolling summaries (10): {}", CONFIG.summary_rolling_10);
        info!("  - Rolling summaries (100): {}", CONFIG.summary_rolling_100);
    } else {
        info!("🧠 Robust Memory: DISABLED (using single-collection mode)");
    }

    // Initialize database pool
    info!("Initializing database connection");
    let database_url = &CONFIG.database_url;
    info!("Database URL: {}", database_url);

    let pool = SqlitePool::connect(database_url).await?;
    
    // Run database migrations
    info!("Running database migrations");
    sqlx::migrate!("./migrations").run(&pool).await?;
    info!("✅ Database migrations completed successfully");

    // Initialize stores
    info!("Initializing memory stores");
    let sqlite_store = Arc::new(SqliteMemoryStore::new(pool.clone()));
    info!("  - SQLite store: {}", database_url);

    // PHASE 1: Note about Qdrant initialization change
    info!("Initializing Qdrant vector stores");
    info!("  - Qdrant URL: {}", CONFIG.qdrant_url);
    if CONFIG.is_robust_memory_enabled() {
        info!("  - Multi-collection mode: {} heads", CONFIG.get_embedding_heads().len());
    } else {
        info!("  - Single collection: {}", CONFIG.qdrant_collection);
    }

    // Initialize OpenAI client
    info!("Initializing LLM clients");
    let openai_client = Arc::new(OpenAIClient::new()?);
    info!("  - Base URL: {}", CONFIG.openai_base_url);
    info!("  - Model: {} for conversation", CONFIG.model);
    info!("  - Image: gpt-image-1 for image generation");
    info!("  - Embeddings: text-embedding-3-large");

    // Initialize project store
    info!("Initializing project store");
    let project_store = Arc::new(ProjectStore::new(pool.clone()));

    // Initialize Git client and store
    info!("Initializing Git client and store");
    let git_store = GitStore::new(pool.clone());
    let git_client = GitClient::new(&CONFIG.git_repos_dir, git_store.clone());

    // PHASE 1: Use new multi-collection AppState initialization
    info!("🏗️  Initializing AppState with multi-collection support");
    let app_state = Arc::new(
        create_app_state_with_multi_qdrant(
            sqlite_store,
            &CONFIG.qdrant_url,
            openai_client,
            project_store,
            git_store,
            git_client,
        ).await?
    );

    info!("✅ Application state assembled successfully");

    // Build CORS layer
    let cors = CorsLayer::new()
        .allow_origin(CONFIG.cors_origin.parse::<tower_http::cors::Any>().unwrap_or(Any))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers(Any);

    // Build the router
    let app = Router::new()
        // Health check
        .route("/health", get(health_handler))
        
        // Chat endpoints
        .route("/chat", post(rest_chat_handler))
        .route("/chat/history", get(get_chat_history))
        .route("/ws/chat", get(websocket_chat_handler))
        
        // Project management
        .route("/projects", post(create_project_handler))
        .route("/projects", get(list_projects_handler))
        .route("/projects/:id", get(get_project_handler))
        .route("/projects/:id", post(update_project_handler))
        .route("/projects/:id", delete(delete_project_handler))
        .route("/project/:project_id", get(project_details_handler))
        
        // Artifact management
        .route("/projects/:project_id/artifacts", post(create_artifact_handler))
        .route("/projects/:project_id/artifacts", get(list_project_artifacts_handler))
        .route("/artifacts/:id", get(get_artifact_handler))
        .route("/artifacts/:id", post(update_artifact_handler))
        .route("/artifacts/:id", delete(delete_artifact_handler))
        
        // Git integration
        .route("/projects/:project_id/git/attach", post(attach_repo_handler))
        .route("/projects/:project_id/git/repos", get(list_attached_repos_handler))
        .route("/projects/:project_id/git/sync/:attachment_id", post(sync_repo_handler))
        
        // File operations
        .route("/projects/:project_id/git/files/:attachment_id/tree", get(get_file_tree_handler))
        .route("/projects/:project_id/git/files/:attachment_id/content/*path", get(get_file_content_handler))
        .route("/projects/:project_id/git/files/:attachment_id/content/*path", post(update_file_content_handler))
        
        // Branch operations
        .route("/projects/:project_id/git/branches/:attachment_id", get(list_branches))
        .route("/projects/:project_id/git/branch/:attachment_id", post(switch_branch))
        
        // Commit operations
        .route("/projects/:project_id/git/commits/:attachment_id", get(get_commit_history))
        .route("/projects/:project_id/git/diff/:attachment_id/:commit_sha", get(get_commit_diff))
        .route("/projects/:project_id/git/file-at-commit/:attachment_id/:commit_sha/*path", get(get_file_at_commit))
        
        .layer(ServiceBuilder::new().layer(cors))
        .with_state(app_state);

    // Server configuration
    let bind_address = CONFIG.bind_address();
    info!("🌐 Starting HTTP server on {}", bind_address);
    
    // PHASE 1: Log startup summary with multi-collection info
    info!("🎯 Mira Backend Ready!");
    info!("  - HTTP API: http://{}", bind_address);
    info!("  - WebSocket: ws://{}/ws/chat", bind_address);
    if CONFIG.is_robust_memory_enabled() {
        info!("  - Memory: Multi-collection Qdrant + SQLite");
        info!("  - Collections: {}", CONFIG.get_embedding_heads().join(", "));
    } else {
        info!("  - Memory: Single-collection Qdrant + SQLite");
    }
    info!("  - Session ID: {}", CONFIG.session_id);
    
    // Start the server
    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
