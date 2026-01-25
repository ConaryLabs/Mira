// crates/mira-server/src/cli/serve.rs
// MCP server initialization and main loop

use super::clients::{get_deepseek, get_deepseek_chat, get_embeddings_with_pool};
use super::get_db_path;
use anyhow::Result;
use mira::background;
use mira::db::pool::DatabasePool;
use mira::http::create_shared_client;
use mira::mcp::MiraServer;
use mira::tools::core::ToolContext;
use mira_types::ProjectContext;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::info;

/// Setup server context with database, embeddings, and restored project/session state
pub async fn setup_server_context() -> Result<MiraServer> {
    // Create shared HTTP client for all network operations
    let http_client = create_shared_client();

    // Open database pool
    let db_path = get_db_path();
    let pool = Arc::new(DatabasePool::open(&db_path).await?);
    let embeddings = get_embeddings_with_pool(Some(pool.clone()), http_client.clone());

    // Create server context
    let server = MiraServer::new(pool.clone(), embeddings);

    // Restore context (Project & Session)
    let restored_project = pool.interact(|conn| {
        // Try to get last active project
        if let Ok(Some(path)) = mira::db::get_last_active_project_sync(conn) {
            if let Ok((id, name)) = mira::db::get_or_create_project_sync(conn, &path, None) {
                return Ok(Some(ProjectContext { id, path, name }));
            }
        }
        // Fallback: Check if CWD is a project
        if let Ok(cwd) = std::env::current_dir() {
            let path_str = cwd.to_string_lossy().to_string();
            if let Ok((id, name)) = mira::db::get_or_create_project_sync(conn, &path_str, None) {
                return Ok(Some(ProjectContext { id, path: path_str, name }));
            }
        }
        Ok(None)
    }).await?;

    if let Some(project) = restored_project {
        server.set_project(project).await;
    }

    // Restore session ID
    if let Ok(Some(sid)) = pool.interact(|conn| {
        mira::db::get_server_state_sync(conn, "active_session_id")
            .map_err(|e| anyhow::anyhow!(e))
    }).await {
        server.set_session_id(sid).await;
    }

    Ok(server)
}

/// Run the MCP server with stdio transport
pub async fn run_mcp_server() -> Result<()> {
    // Create shared HTTP client for all network operations
    let http_client = create_shared_client();

    // Open database pool
    let db_path = get_db_path();
    let pool = Arc::new(DatabasePool::open(&db_path).await?);

    // Initialize embeddings if API key available (with usage tracking)
    let embeddings = get_embeddings_with_pool(Some(pool.clone()), http_client.clone());

    if embeddings.is_some() {
        info!("Semantic search enabled (Google embeddings)");
    } else {
        info!("Semantic search disabled (no GEMINI_API_KEY)");
    }

    // Initialize DeepSeek client if API key available
    let deepseek = get_deepseek(http_client.clone());
    let deepseek_chat = get_deepseek_chat(http_client.clone());

    if deepseek.is_some() {
        info!("DeepSeek enabled (for experts and module summaries)");
    } else {
        info!("DeepSeek disabled (no DEEPSEEK_API_KEY)");
    }

    // Spawn background worker for batch processing
    let bg_pool = pool.clone();
    let bg_embeddings = embeddings.clone();
    let bg_deepseek = deepseek.clone();
    let bg_deepseek_chat = deepseek_chat.clone();
    let _shutdown_tx = background::spawn(bg_pool, bg_embeddings, bg_deepseek, bg_deepseek_chat);
    info!("Background worker started");

    // Spawn file watcher for incremental indexing
    let (_watcher_shutdown_tx, watcher_shutdown_rx) = watch::channel(false);
    let watcher_handle = background::watcher::spawn(pool.clone(), watcher_shutdown_rx);
    info!("File watcher started");

    // Create MCP server with watcher
    let server = MiraServer::with_watcher(pool.clone(), embeddings, watcher_handle);

    // Restore context (Project & Session)
    let restore_pool = pool.clone();
    let restored = restore_pool.interact(|conn| {
        // Try to get last active project
        if let Ok(Some(path)) = mira::db::get_last_active_project_sync(conn) {
            if let Ok((id, name)) = mira::db::get_or_create_project_sync(conn, &path, None) {
                return Ok(Some((ProjectContext { id, path, name }, true)));
            }
        }
        // Fallback: Check if CWD is a project
        if let Ok(cwd) = std::env::current_dir() {
            let path_str = cwd.to_string_lossy().to_string();
            if let Ok((id, name)) = mira::db::get_or_create_project_sync(conn, &path_str, None) {
                return Ok(Some((ProjectContext { id, path: path_str, name }, false)));
            }
        }
        Ok(None)
    }).await?;

    if let Some((project, from_stored)) = restored {
        if from_stored {
            info!("Restoring project: {} (id: {})", project.path, project.id);
        } else {
            info!("Restoring project from CWD: {} (id: {})", project.path, project.id);
        }
        let project_path = project.path.clone();
        let project_id = project.id;
        server.set_project(project).await;

        // Register with watcher if available
        if let Some(watcher) = server.watcher() {
            watcher.watch(project_id, std::path::PathBuf::from(project_path)).await;
        }
    }

    // Restore session ID
    if let Ok(Some(sid)) = pool.interact(|conn| {
        mira::db::get_server_state_sync(conn, "active_session_id")
            .map_err(|e| anyhow::anyhow!(e))
    }).await {
        info!("Restoring session: {}", sid);
        server.set_session_id(sid).await;
    }

    // Run with stdio transport
    let transport = rmcp::transport::io::stdio();
    let service = rmcp::serve_server(server, transport).await?;
    service.waiting().await?;

    Ok(())
}
