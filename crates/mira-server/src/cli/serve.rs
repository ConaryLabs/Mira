// crates/mira-server/src/cli/serve.rs
// MCP server initialization and main loop

use super::clients::get_embeddings_from_config;
use super::get_db_path;
use anyhow::Result;
use mira::background;
use mira::config::EnvConfig;
use mira::db::pool::DatabasePool;
use mira::http::create_shared_client;
use mira::mcp::MiraServer;
use mira::mcp::client::McpClientManager;
use mira::tools::core::ToolContext;
use mira::utils::path_to_string;
use mira_types::ProjectContext;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{info, warn};

/// Migrate code tables from main DB to code DB on first run after sharding.
///
/// Checks if code_symbols still has data in the main DB. If so, it means
/// this is the first run with the sharded layout. We don't copy data
/// (re-indexing is cheap and safer) - we just drop the old tables from the
/// main DB to reclaim space.
async fn migrate_code_tables_if_needed(
    main_pool: &Arc<DatabasePool>,
    _code_pool: &Arc<DatabasePool>,
) {
    let has_old_code_tables = main_pool
        .interact(|conn| {
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM sqlite_master WHERE type='table' AND name='code_symbols'",
                    [],
                    |_| Ok(true),
                )
                .unwrap_or(false);
            Ok(exists)
        })
        .await
        .unwrap_or(false);

    if has_old_code_tables {
        info!(
            "Migrating: dropping code index tables from main DB (will re-index into mira-code.db)"
        );
        if let Err(e) = main_pool
            .interact(|conn| {
                // Drop in dependency order. Ignore errors for tables that may not exist.
                let tables = [
                    "DROP TABLE IF EXISTS code_fts",
                    "DROP TABLE IF EXISTS pending_embeddings",
                    "DROP TABLE IF EXISTS call_graph",
                    "DROP TABLE IF EXISTS imports",
                    "DROP TABLE IF EXISTS codebase_modules",
                    "DROP TABLE IF EXISTS vec_code",
                    "DROP TABLE IF EXISTS code_symbols",
                ];
                for sql in &tables {
                    if let Err(e) = conn.execute_batch(sql) {
                        tracing::warn!("Migration warning (non-fatal): {} - {}", sql, e);
                    }
                }
                // Reclaim space
                if let Err(e) = conn.execute_batch("VACUUM") {
                    tracing::warn!("VACUUM after migration failed (non-fatal): {}", e);
                }
                Ok(())
            })
            .await
        {
            warn!(
                "Code table migration failed (non-fatal, will retry next start): {}",
                e
            );
        }
    }
}

/// Shared server components produced by `init_server_context`.
struct ServerContext {
    server: MiraServer,
    pool: Arc<DatabasePool>,
    env_config: EnvConfig,
}

/// Initialize configuration, database pools, embeddings, MCP client manager,
/// and restore project + session state. Shared by `setup_server_context` and
/// `run_mcp_server` to avoid duplicating ~40 lines of setup code.
async fn init_server_context() -> Result<ServerContext> {
    // Load configuration once (single source of truth)
    let env_config = EnvConfig::load();

    // Validate and log warnings
    let validation = env_config.validate();
    for warning in &validation.warnings {
        warn!("{}", warning);
    }

    // Create shared HTTP client for all network operations
    let http_client = create_shared_client();

    // Open database pools (main + code index)
    let db_path = get_db_path();
    let pool = Arc::new(DatabasePool::open(&db_path).await?);
    let code_db_path = db_path.with_file_name("mira-code.db");
    let code_pool = Arc::new(DatabasePool::open_code_db(&code_db_path).await?);

    // Migrate code tables from main DB to code DB if needed
    migrate_code_tables_if_needed(&pool, &code_pool).await;

    // Create embeddings from centralized config
    let embeddings = get_embeddings_from_config(
        &env_config.api_keys,
        &env_config.embeddings,
        Some(pool.clone()),
        http_client.clone(),
    );

    // Create server context from centralized config
    let mut server = MiraServer::from_api_keys(
        pool.clone(),
        code_pool,
        embeddings,
        &env_config.api_keys,
        env_config.fuzzy_fallback,
        env_config.expert.clone(),
    );

    // Initialize MCP client manager for external MCP server access (expert tools)
    let cwd = std::env::current_dir().ok().map(|p| path_to_string(&p));
    let mut mcp_manager = McpClientManager::from_mcp_configs(cwd.as_deref());
    mcp_manager.set_mcp_tool_timeout(std::time::Duration::from_secs(
        env_config.expert.mcp_tool_timeout_secs,
    ));
    if mcp_manager.has_servers() {
        server.mcp_client_manager = Some(Arc::new(mcp_manager));
    }

    // Restore project context
    let restored_project = pool
        .interact(|conn| {
            // Try to get last active project
            if let Ok(Some(path)) = mira::db::get_last_active_project_sync(conn)
                && let Ok((id, name)) = mira::db::get_or_create_project_sync(conn, &path, None)
            {
                return Ok(Some(ProjectContext { id, path, name }));
            }
            // Fallback: Check if CWD is a project
            if let Ok(cwd) = std::env::current_dir() {
                let path_str = path_to_string(&cwd);
                if let Ok((id, name)) = mira::db::get_or_create_project_sync(conn, &path_str, None)
                {
                    return Ok(Some(ProjectContext {
                        id,
                        path: path_str,
                        name,
                    }));
                }
            }
            Ok(None)
        })
        .await?;

    if let Some(project) = restored_project {
        server.set_project(project).await;
    }

    // Restore session ID
    if let Ok(Some(sid)) = pool
        .interact(|conn| {
            mira::db::get_server_state_sync(conn, "active_session_id")
                .map_err(|e| anyhow::anyhow!(e))
        })
        .await
    {
        server.set_session_id(sid).await;
    }

    Ok(ServerContext {
        server,
        pool,
        env_config,
    })
}

/// Setup server context with database, embeddings, and restored project/session state
pub async fn setup_server_context() -> Result<MiraServer> {
    let ctx = init_server_context().await?;
    Ok(ctx.server)
}

/// Run the MCP server with stdio transport
pub async fn run_mcp_server() -> Result<()> {
    let ctx = init_server_context().await?;
    let mut server = ctx.server;
    let pool = ctx.pool;
    let env_config = ctx.env_config;

    if let Some(ref emb) = server.embeddings {
        info!("Semantic search enabled (OpenAI embeddings)");

        // Check for embedding provider change and invalidate stale vectors
        let provider_id = emb.provider_id().to_string();
        let check_pool = pool.clone();
        if let Err(e) = check_pool
            .interact(move |conn| {
                mira::db::check_embedding_provider_change(conn, &provider_id)
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
        {
            warn!("Failed to check embedding provider change: {}", e);
        }
    } else {
        info!("Semantic search disabled (no OPENAI_API_KEY)");
    }

    // Initialize LLM provider factory from centralized config
    let llm_factory = Arc::new(mira::llm::ProviderFactory::from_api_keys(
        env_config.api_keys.clone(),
    ));

    if llm_factory.has_providers() {
        let providers: Vec<_> = llm_factory
            .available_providers()
            .iter()
            .map(|p| p.to_string())
            .collect();
        info!("LLM providers available: {}", providers.join(", "));
    } else {
        info!("No LLM providers configured (set DEEPSEEK_API_KEY, ZHIPU_API_KEY, or OLLAMA_HOST)");
    }

    // Spawn background workers with separate pools
    let bg_embeddings = server.embeddings.clone();
    let (_shutdown_tx, fast_lane_notify) = background::spawn_with_pools(
        server.code_pool.clone(),
        pool.clone(),
        bg_embeddings,
        llm_factory.clone(),
    );
    info!("Background worker started");

    // Spawn file watcher for incremental indexing (uses code_pool)
    let (_watcher_shutdown_tx, watcher_shutdown_rx) = watch::channel(false);
    let watcher_handle = background::watcher::spawn(
        server.code_pool.clone(),
        Some(server.fuzzy_cache.clone()),
        watcher_shutdown_rx,
        Some(fast_lane_notify),
    );
    info!("File watcher started");
    server.watcher = Some(watcher_handle);

    // Log project restoration
    let restored_project = server
        .project
        .read()
        .await
        .as_ref()
        .map(|p| (p.id, p.path.clone()));
    if let Some((pid, path)) = restored_project {
        info!("Restoring project: {} (id: {})", path, pid);

        // Register with watcher if available
        if let Some(watcher) = server.watcher() {
            watcher.watch(pid, std::path::PathBuf::from(&path)).await;
        }
    }

    // Log session restoration
    if server.session_id.read().await.is_some() {
        info!(
            "Restoring session: {}",
            server.session_id.read().await.as_deref().unwrap_or("?")
        );
    }

    // Run with stdio transport
    let transport = rmcp::transport::io::stdio();
    let service = rmcp::serve_server(server, transport).await?;
    service.waiting().await?;

    Ok(())
}
