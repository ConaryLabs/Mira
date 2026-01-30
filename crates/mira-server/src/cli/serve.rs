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
use mira::mcp_client::McpClientManager;
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
        info!("Migrating: dropping code index tables from main DB (will re-index into mira-code.db)");
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
            warn!("Code table migration failed (non-fatal, will retry next start): {}", e);
        }
    }
}

/// Setup server context with database, embeddings, and restored project/session state
pub async fn setup_server_context() -> Result<MiraServer> {
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
    let mut server =
        MiraServer::from_api_keys(pool.clone(), code_pool, embeddings, &env_config.api_keys);

    // Initialize MCP client manager for external MCP server access (expert tools)
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| path_to_string(&p));
    let mcp_manager = McpClientManager::from_mcp_configs(cwd.as_deref());
    if mcp_manager.has_servers() {
        server.mcp_client_manager = Some(Arc::new(mcp_manager));
    }

    // Restore context (Project & Session)
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

    Ok(server)
}

/// Run the MCP server with stdio transport
pub async fn run_mcp_server() -> Result<()> {
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

    // Initialize embeddings from centralized config
    let embeddings = get_embeddings_from_config(
        &env_config.api_keys,
        &env_config.embeddings,
        Some(pool.clone()),
        http_client.clone(),
    );

    if embeddings.is_some() {
        info!("Semantic search enabled (Google embeddings)");
    } else {
        info!("Semantic search disabled (no GEMINI_API_KEY)");
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
        info!("No LLM providers configured (set DEEPSEEK_API_KEY or GEMINI_API_KEY)");
    }

    // Spawn background workers with separate pools
    let bg_embeddings = embeddings.clone();
    let (_shutdown_tx, fast_lane_notify) = background::spawn_with_pools(
        code_pool.clone(),
        pool.clone(),
        bg_embeddings,
        llm_factory.clone(),
    );
    info!("Background worker started");

    // Spawn file watcher for incremental indexing (uses code_pool)
    let (_watcher_shutdown_tx, watcher_shutdown_rx) = watch::channel(false);
    let watcher_handle = background::watcher::spawn(
        code_pool.clone(),
        watcher_shutdown_rx,
        Some(fast_lane_notify),
    );
    info!("File watcher started");

    // Create MCP server with watcher from centralized config
    let mut server = MiraServer::from_api_keys(
        pool.clone(),
        code_pool,
        embeddings,
        &env_config.api_keys,
    );
    server.watcher = Some(watcher_handle);

    // Initialize MCP client manager for external MCP server access (expert tools)
    // We initialize with CWD initially; project path will be used when available
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| path_to_string(&p));
    let mcp_manager = McpClientManager::from_mcp_configs(cwd.as_deref());
    if mcp_manager.has_servers() {
        info!("MCP client manager initialized for expert tool access");
        server.mcp_client_manager = Some(Arc::new(mcp_manager));
    }

    // Restore context (Project & Session)
    let restore_pool = pool.clone();
    let restored = restore_pool
        .interact(|conn| {
            // Try to get last active project
            if let Ok(Some(path)) = mira::db::get_last_active_project_sync(conn)
                && let Ok((id, name)) = mira::db::get_or_create_project_sync(conn, &path, None)
            {
                return Ok(Some((ProjectContext { id, path, name }, true)));
            }
            // Fallback: Check if CWD is a project
            if let Ok(cwd) = std::env::current_dir() {
                let path_str = path_to_string(&cwd);
                if let Ok((id, name)) = mira::db::get_or_create_project_sync(conn, &path_str, None)
                {
                    return Ok(Some((
                        ProjectContext {
                            id,
                            path: path_str,
                            name,
                        },
                        false,
                    )));
                }
            }
            Ok(None)
        })
        .await?;

    if let Some((project, from_stored)) = restored {
        if from_stored {
            info!("Restoring project: {} (id: {})", project.path, project.id);
        } else {
            info!(
                "Restoring project from CWD: {} (id: {})",
                project.path, project.id
            );
        }
        let project_path = project.path.clone();
        let project_id = project.id;
        server.set_project(project).await;

        // Register with watcher if available
        if let Some(watcher) = server.watcher() {
            watcher
                .watch(project_id, std::path::PathBuf::from(project_path))
                .await;
        }
    }

    // Restore session ID
    if let Ok(Some(sid)) = pool
        .interact(|conn| {
            mira::db::get_server_state_sync(conn, "active_session_id")
                .map_err(|e| anyhow::anyhow!(e))
        })
        .await
    {
        info!("Restoring session: {}", sid);
        server.set_session_id(sid).await;
    }

    // Run with stdio transport
    let transport = rmcp::transport::io::stdio();
    let service = rmcp::serve_server(server, transport).await?;
    service.waiting().await?;

    Ok(())
}
