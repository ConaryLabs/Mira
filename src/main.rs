// src/main.rs
// PHASE 1: Multi-Collection Qdrant Support for GPT-5 Robust Memory
// PHASE 3: File search and image generation integration

use anyhow::Result;
use axum::{
    http::{HeaderValue, Method},
    routing::{get, post},
    Router,
};
use mira_backend::{
    api::{
        http::{
            chat::{get_chat_history, rest_chat_handler},
            git::{
                attach_repo_handler, get_commit_diff, get_commit_history, get_file_at_commit,
                get_file_content_handler, get_file_tree_handler, list_attached_repos_handler,
                list_branches, switch_branch, sync_repo_handler, update_file_content_handler,
            },
            handlers::{health_handler, project_details_handler},
            memory::{import_memories, pin_memory, unpin_memory}, // ⬅️ add memory endpoints
        },
        ws::chat::ws_chat_handler,
    },
    config::CONFIG,
    git::{GitClient, GitStore},
    llm::client::OpenAIClient,
    memory::{
        decay_scheduler::spawn_decay_scheduler, // hourly decay task
        sqlite::{migration::run_migrations, store::SqliteMemoryStore},
    },
    project::{
        create_artifact_handler, create_project_handler, delete_artifact_handler,
        delete_project_handler, get_artifact_handler, get_project_handler,
        list_project_artifacts_handler, list_projects_handler, update_artifact_handler,
        update_project_handler,
    },
    state::{AppState, create_app_state_with_multi_qdrant}, // ⬅️ import AppState for explicit Arc type
};
use sqlx::SqlitePool;
use std::{env, sync::Arc, time::Duration};
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tracing::info;
use tracing_subscriber::fmt;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    fmt().with_max_level(tracing::Level::INFO).init();

    info!("🚀 Starting Mira Backend (Phase 1: Multi-Collection Support)");
    info!("Config loaded from environment and .env file");

    if CONFIG.is_robust_memory_enabled() {
        info!("🧠 Robust Memory: ENABLED");
        info!("  - Embedding heads: {}", CONFIG.embed_heads);
    } else {
        info!("🧠 Robust Memory: DISABLED (using single-collection mode)");
    }

    // -- Database -----------------------------------------------------------------
    let pool = SqlitePool::connect(&CONFIG.database_url).await?;

    // Run idempotent, code-based migrations (includes Phase-4 columns + indexes)
    run_migrations(&pool).await?;
    info!("📚 SQLite migrations complete");

    // -- Stores & Clients ----------------------------------------------------------
    let sqlite_store = Arc::new(SqliteMemoryStore::new(pool.clone()));
    let openai_client = OpenAIClient::new()?;
    let project_store = Arc::new(mira_backend::project::store::ProjectStore::new(pool.clone()));
    let git_store = GitStore::new(pool);
    let git_client = GitClient::new(&CONFIG.git_repos_dir, git_store.clone());

    // Explicit type so the compiler knows Arc<T>
    let app_state: Arc<AppState> = Arc::new(
        create_app_state_with_multi_qdrant(
            sqlite_store,
            &CONFIG.qdrant_url,
            openai_client,
            project_store,
            git_store,
            git_client,
        )
        .await?,
    );

    // -- Background tasks ----------------------------------------------------------
    // Configurable subject-aware decay interval (seconds). Default: 3600 (1h).
    let decay_interval_secs = env::var("DECAY_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(3600);

    let _decay_task =
        spawn_decay_scheduler(app_state.clone(), Duration::from_secs(decay_interval_secs));
    info!("⏳ Decay scheduler started (every {}s)", decay_interval_secs);

    // -- CORS ----------------------------------------------------------------------
    let cors = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(Any);

    let cors = if CONFIG.cors_origin == "*" {
        cors.allow_origin(Any)
    } else {
        let origin = CONFIG.cors_origin.parse::<HeaderValue>()?;
        cors.allow_origin(AllowOrigin::exact(origin))
    };

    // -- Router -------------------------------------------------------------------
    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/chat", post(rest_chat_handler))
        .route("/chat/history", get(get_chat_history))
        .route("/ws/chat", get(ws_chat_handler))
        .route("/projects", get(list_projects_handler).post(create_project_handler))
        .route(
            "/projects/:id",
            get(get_project_handler)
                .put(update_project_handler)
                .delete(delete_project_handler),
        )
        .route("/project/:project_id", get(project_details_handler))
        .route(
            "/projects/:project_id/artifacts",
            get(list_project_artifacts_handler).post(create_artifact_handler),
        )
        .route(
            "/artifacts/:id",
            get(get_artifact_handler)
                .put(update_artifact_handler)
                .delete(delete_artifact_handler),
        )
        .route("/projects/:project_id/git/attach", post(attach_repo_handler))
        .route("/projects/:project_id/git/repos", get(list_attached_repos_handler))
        .route("/projects/:project_id/git/sync/:attachment_id", post(sync_repo_handler))
        .route(
            "/projects/:project_id/git/files/:attachment_id/tree",
            get(get_file_tree_handler),
        )
        .route(
            "/projects/:project_id/git/files/:attachment_id/content/*path",
            get(get_file_content_handler).post(update_file_content_handler),
        )
        .route("/projects/:project_id/git/branches/:attachment_id", get(list_branches))
        .route("/projects/:project_id/git/branch/:attachment_id", post(switch_branch))
        .route(
            "/projects/:project_id/git/commits/:attachment_id",
            get(get_commit_history),
        )
        .route(
            "/projects/:project_id/git/diff/:attachment_id/:commit_sha",
            get(get_commit_diff),
        )
        .route(
            "/projects/:project_id/git/file-at-commit/:attachment_id/:commit_sha/*path",
            get(get_file_at_commit),
        )
        // Memory maintenance (Phase 4)
        .route("/memory/:id/pin", post(pin_memory))
        .route("/memory/:id/unpin", post(unpin_memory))
        .route("/memory/import", post(import_memories))
        .layer(cors)
        .with_state(app_state);

    // -- Server -------------------------------------------------------------------
    let bind_address = CONFIG.bind_address();
    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    info!("🌐 Starting HTTP server on {}", bind_address);

    axum::serve(listener, app).await?;
    Ok(())
}
