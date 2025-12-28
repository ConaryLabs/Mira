// src/main.rs
// Mira Power Suit - Unified Daemon for Claude Code
// CLI entry point

#![allow(clippy::collapsible_if)] // Nested ifs often clearer than let-chains

use anyhow::Result;
use clap::{Parser, Subcommand};
use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};
use rmcp::{
    transport::{StreamableHttpService, StreamableHttpServerConfig},
    transport::streamable_http_server::session::local::LocalSessionManager,
};
use std::sync::Arc;
use std::path::PathBuf;
use tokio::sync::RwLock;
use tower_http::trace::TraceLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::cors::{CorsLayer, Any};
use tracing::{info, warn, error, Level};
use tracing_subscriber::FmtSubscriber;
use std::time::Duration;

mod batch;
mod chat;
mod context;
mod core;
mod tools;
mod indexer;
mod hooks;
mod orchestrator;
mod server;
mod spawner;
mod daemon;
mod connect;

use server::{MiraServer, create_optimized_pool, run_migrations};
use tools::SemanticSearch;

// === Constants ===

const DEFAULT_PORT: u16 = 3000;
const TOKEN_FILE: &str = ".mira/token";

// === CLI Definition ===

#[derive(Parser)]
#[command(name = "mira")]
#[command(about = "Memory and Intelligence Layer for Claude Code")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the daemon (default when no command given)
    Daemon {
        /// Port to listen on (default: 3000, env: MIRA_PORT)
        #[arg(short, long, env = "MIRA_PORT", default_value_t = DEFAULT_PORT)]
        port: u16,

        /// Bind address (default: 127.0.0.1 for security, use 0.0.0.0 to expose)
        #[arg(short, long, env = "MIRA_LISTEN", default_value = "127.0.0.1")]
        listen: String,
    },

    /// Connect stdio to running daemon (for Claude Code MCP)
    Connect {
        /// Daemon URL (default: http://localhost:3000)
        #[arg(short, long, env = "MIRA_URL")]
        url: Option<String>,
    },

    /// Check daemon status
    Status {
        /// Daemon URL (default: http://localhost:3000)
        #[arg(short, long, env = "MIRA_URL")]
        url: Option<String>,
    },

    /// Stop running daemon
    Stop {
        /// Daemon URL (default: http://localhost:3000)
        #[arg(short, long, env = "MIRA_URL")]
        url: Option<String>,
    },

    /// Claude Code hook handlers (for use in settings.json)
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
}

#[derive(Subcommand)]
enum HookAction {
    /// Handle PermissionRequest hooks - auto-approve based on saved rules
    Permission,
    /// Handle PreCompact hooks - save context before conversation compaction
    Precompact,
    /// Handle PostToolCall hooks - auto-remember significant actions
    Posttool,
    /// Handle PreToolUse hooks - provide code context before file operations
    Pretool,
    /// Handle SessionStart hooks - check for unfinished work and prompt to resume
    Sessionstart,
}

// === Token Management ===

/// Get or create auth token from ~/.mira/token
fn get_or_create_token() -> Result<String> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("No home directory"))?;
    let token_path = home.join(TOKEN_FILE);

    if token_path.exists() {
        let token = std::fs::read_to_string(&token_path)?;
        Ok(token.trim().to_string())
    } else {
        // Generate new token
        let token = uuid::Uuid::new_v4().to_string();

        // Create directory if needed
        if let Some(parent) = token_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&token_path, &token)?;
        info!("Generated auth token at {}", token_path.display());
        Ok(token)
    }
}

/// Get daemon URL with default fallback
fn get_daemon_url(url: Option<String>, port: u16) -> String {
    url.unwrap_or_else(|| format!("http://localhost:{}", port))
}

// === Middleware ===

/// Auth middleware that checks for Bearer token
/// When exposed (not localhost), requires auth for all endpoints except /health
async fn auth_middleware(
    req: Request<Body>,
    next: Next,
    expected_token: String,
    is_localhost: bool,
) -> Result<Response, StatusCode> {
    let path = req.uri().path();

    // Health check is always public
    if path == "/health" {
        return Ok(next.run(req).await);
    }

    // When bound to localhost only, /api/* can skip auth (trusted local access)
    // When exposed (0.0.0.0), require auth for everything
    if is_localhost && path.starts_with("/api/") {
        return Ok(next.run(req).await);
    }

    // Check Authorization header
    if let Some(auth_header) = req.headers().get("authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                if token == expected_token {
                    return Ok(next.run(req).await);
                }
            }
        }
    }

    // Also check X-Auth-Token header for simpler clients
    if let Some(token_header) = req.headers().get("x-auth-token") {
        if let Ok(token) = token_header.to_str() {
            if token == expected_token {
                return Ok(next.run(req).await);
            }
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}

/// Graceful shutdown signal handler
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to install CTRL+C signal handler");
    info!("Shutdown signal received, stopping server...");
}

// === Daemon Implementation ===

async fn run_daemon(port: u16, listen: &str) -> Result<()> {
    let is_localhost = listen == "127.0.0.1" || listen == "localhost" || listen == "::1";

    info!("Starting Mira Daemon on {}:{}...", listen, port);
    if !is_localhost {
        warn!("⚠️  Exposed mode: binding to {} - auth required for ALL endpoints", listen);
        warn!("   Use Bearer token from ~/.mira/token for /api/* access");
    }

    // Get or create auth token
    let auth_token = get_or_create_token()?;
    info!("Auth token loaded from ~/.mira/token");

    // Database setup
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| {
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
            format!("sqlite://{}", home.join(".mira/mira.db").display())
        });

    let qdrant_url = std::env::var("QDRANT_URL").ok();
    let gemini_key = std::env::var("GEMINI_API_KEY")
        .or_else(|_| std::env::var("GOOGLE_API_KEY"))
        .ok();

    // DeepSeek API key for chat
    let deepseek_key = std::env::var("DEEPSEEK_API_KEY")
        .ok()
        .filter(|s| !s.is_empty());

    // Sync token for chat sync endpoint
    let sync_token = std::env::var("MIRA_SYNC_TOKEN").ok();

    // Create shared state that will be cloned for each session
    let db = Arc::new(create_optimized_pool(&database_url).await?);
    info!("Database connected: {}", database_url);

    // Run pending migrations
    // Look for migrations in the standard location relative to the executable
    let migrations_path = std::env::var("MIRA_MIGRATIONS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // Try relative to executable first, then fall back to current dir
            if let Ok(exe) = std::env::current_exe() {
                if let Some(parent) = exe.parent() {
                    let path = parent.join("migrations");
                    if path.exists() {
                        return path;
                    }
                }
            }
            PathBuf::from("migrations")
        });

    if migrations_path.exists() {
        run_migrations(&db, &migrations_path).await?;
    }

    let semantic = Arc::new(SemanticSearch::new(qdrant_url.as_deref(), gemini_key.clone()).await);

    // Initialize orchestrator if Gemini key is available
    let orchestrator: Arc<RwLock<Option<orchestrator::GeminiOrchestrator>>> = if let Some(key) = gemini_key {
        match orchestrator::GeminiOrchestrator::new((*db).clone(), key).await {
            Ok(orch) => {
                info!("Gemini orchestrator enabled");
                Arc::new(RwLock::new(Some(orch)))
            }
            Err(e) => {
                warn!("Failed to initialize orchestrator: {}", e);
                Arc::new(RwLock::new(None))
            }
        }
    } else {
        info!("Gemini orchestrator disabled (no API key)");
        Arc::new(RwLock::new(None))
    };

    // Create the MCP service with StreamableHttpService
    let mcp_service = StreamableHttpService::new(
        {
            let db = db.clone();
            let semantic = semantic.clone();
            let orchestrator = orchestrator.clone();
            move || {
                Ok(MiraServer {
                    db: db.clone(),
                    semantic: semantic.clone(),
                    orchestrator: orchestrator.clone(),
                    tool_router: MiraServer::get_tool_router(),
                    active_project: Arc::new(RwLock::new(None)),
                    carousel: Arc::new(RwLock::new(None)),
                    mcp_session_id: Arc::new(RwLock::new(None)),
                    session_phase: Arc::new(RwLock::new(core::ops::mcp_session::SessionPhase::Early)),
                })
            }
        },
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );

    // Initialize Claude Code spawner
    let spawner_config = spawner::SpawnerConfig::from_env();
    let claude_spawner = Arc::new(spawner::ClaudeCodeSpawner::new((*db).clone(), spawner_config));
    claude_spawner.start_heartbeat(30); // Send heartbeat every 30 seconds
    info!("Claude Code spawner initialized");

    // Create chat router (if DeepSeek API key is available)
    let chat_router = if let Some(api_key) = deepseek_key {
        info!("Chat endpoints enabled (DeepSeek API key found)");
        let chat_state = chat::AppState {
            db: Some((*db).clone()),
            semantic: semantic.clone(),
            api_key,
            default_reasoning_effort: "medium".to_string(),
            sync_token: sync_token.clone(),
            sync_semaphore: Arc::new(tokio::sync::Semaphore::new(3)),
            project_locks: Arc::new(chat::server::ProjectLocks::new()),
            context_caches: Arc::new(chat::server::ContextCaches::new()),
            spawner: Some(claude_spawner.clone()),
        };
        Some(chat::create_router(chat_state))
    } else {
        // No chat, but still add spawner endpoints
        info!("Chat endpoints disabled (no DEEPSEEK_API_KEY)");
        let spawner_state = chat::server::SpawnerState {
            db: Some((*db).clone()),
            spawner: claude_spawner.clone(),
        };
        Some(chat::server::create_spawner_router(spawner_state))
    };

    // Start background indexer - watch registered projects from database
    let project_paths: Vec<PathBuf> = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT path FROM projects ORDER BY last_accessed DESC LIMIT 10"
    )
    .fetch_all(&*db)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(PathBuf::from)
    .filter(|p| p.exists())
    .collect();

    let project_paths = if project_paths.is_empty() {
        // Fallback to current directory if no projects registered
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        info!("No projects in database, watching current directory: {}", cwd.display());
        vec![cwd]
    } else {
        info!("Watching {} registered projects: {:?}", project_paths.len(), project_paths);
        project_paths
    };

    let daemon = daemon::Daemon::with_shared(
        project_paths,
        (*db).clone(),
        semantic.clone(),
    ).with_orchestrator(orchestrator.clone());
    let _daemon_tasks = match daemon.spawn_background_tasks().await {
        Ok(tasks) => Some(tasks),
        Err(e) => {
            warn!("Failed to start indexer: {}", e);
            None
        }
    };

    // Health check handler
    let health_db = db.clone();
    let health_semantic = semantic.clone();
    let health_handler = move || {
        let db = health_db.clone();
        let semantic = health_semantic.clone();
        async move {
            let mut status = serde_json::json!({
                "status": "ok",
                "version": env!("CARGO_PKG_VERSION"),
                "port": port,
            });

            // Check database
            let db_ok = sqlx::query("SELECT 1")
                .fetch_one(db.as_ref())
                .await
                .is_ok();
            status["database"] = serde_json::json!(if db_ok { "ok" } else { "error" });

            // Check Qdrant
            status["semantic_search"] = serde_json::json!(
                if semantic.is_available() { "ok" } else { "disabled" }
            );

            if !db_ok {
                status["status"] = serde_json::json!("degraded");
            }

            axum::Json(status)
        }
    };

    // CORS configuration
    // - Localhost: permissive (trusted local access)
    // - Exposed: restricted to configured origins or same-origin only
    let cors = if is_localhost {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
            .expose_headers([
                "mcp-session-id".parse().unwrap(),
                "content-type".parse().unwrap(),
            ])
    } else {
        // When exposed, only allow same-origin or explicitly configured origins
        // For now, be restrictive - users can configure MIRA_CORS_ORIGINS if needed
        let allowed_origins = std::env::var("MIRA_CORS_ORIGINS")
            .ok()
            .map(|s| s.split(',').map(|o| o.trim().to_string()).collect::<Vec<_>>());

        if let Some(origins) = allowed_origins {
            info!("CORS allowed origins: {:?}", origins);
            let origins: Vec<_> = origins
                .iter()
                .filter_map(|o| o.parse().ok())
                .collect();
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods([axum::http::Method::GET, axum::http::Method::POST, axum::http::Method::OPTIONS])
                .allow_headers([axum::http::header::CONTENT_TYPE, axum::http::header::AUTHORIZATION])
                .expose_headers([
                    "mcp-session-id".parse().unwrap(),
                    "content-type".parse().unwrap(),
                ])
        } else {
            // No origins configured - very restrictive (same-origin only effectively)
            warn!("No MIRA_CORS_ORIGINS set - cross-origin requests will be blocked");
            CorsLayer::new()
                .allow_methods([axum::http::Method::GET, axum::http::Method::POST, axum::http::Method::OPTIONS])
                .allow_headers([axum::http::header::CONTENT_TYPE, axum::http::header::AUTHORIZATION])
                .expose_headers([
                    "mcp-session-id".parse().unwrap(),
                    "content-type".parse().unwrap(),
                ])
        }
    };

    // Build router with auth middleware
    let mut base_router = axum::Router::new()
        .route("/health", axum::routing::get(health_handler));

    // Add chat routes
    if let Some(chat) = chat_router {
        base_router = base_router.merge(chat);
    }

    // Add MCP routes with auth
    let token = auth_token.clone();
    let app = base_router
        .nest_service("/mcp", mcp_service)
        .layer(axum::middleware::from_fn(move |req, next| {
            let token = token.clone();
            auth_middleware(req, next, token, is_localhost)
        }))
        .layer(cors)
        .layer(TimeoutLayer::with_status_code(StatusCode::GATEWAY_TIMEOUT, Duration::from_secs(60)))
        .layer(TraceLayer::new_for_http());

    let bind_addr = format!("{}:{}", listen, port);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    info!("Mira Daemon listening on http://{}", bind_addr);
    info!("  Health: /health");
    info!("  MCP:    /mcp (requires auth)");
    if is_localhost {
        info!("  Chat:   /api/chat/stream, /api/chat/sync (local access, no auth)");
    } else {
        info!("  Chat:   /api/chat/stream, /api/chat/sync (requires auth)");
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

// === Status Command ===

async fn run_status(url: String) -> Result<()> {
    let client = reqwest::Client::new();
    let health_url = format!("{}/health", url);

    match client.get(&health_url).timeout(Duration::from_secs(5)).send().await {
        Ok(resp) if resp.status().is_success() => {
            let status: serde_json::Value = resp.json().await?;
            println!("Mira Daemon Status:");
            println!("  URL:      {}", url);
            println!("  Status:   {}", status["status"].as_str().unwrap_or("unknown"));
            println!("  Version:  {}", status["version"].as_str().unwrap_or("unknown"));
            println!("  Database: {}", status["database"].as_str().unwrap_or("unknown"));
            println!("  Semantic: {}", status["semantic_search"].as_str().unwrap_or("unknown"));
            Ok(())
        }
        Ok(resp) => {
            error!("Daemon returned error: {}", resp.status());
            std::process::exit(1);
        }
        Err(e) => {
            error!("Cannot connect to daemon at {}: {}", url, e);
            error!("Is the daemon running? Start with: mira");
            std::process::exit(1);
        }
    }
}

// === Stop Command ===

async fn run_stop(url: String) -> Result<()> {
    // For now, just check if it's running and print instructions
    // TODO: Add proper shutdown endpoint
    let client = reqwest::Client::new();
    let health_url = format!("{}/health", url);

    match client.get(&health_url).timeout(Duration::from_secs(5)).send().await {
        Ok(resp) if resp.status().is_success() => {
            println!("Daemon is running at {}", url);
            println!("To stop, press Ctrl+C in the daemon terminal or:");
            println!("  pkill -f 'mira daemon'");
            println!("  # or if running as systemd service:");
            println!("  systemctl --user stop mira");
            Ok(())
        }
        _ => {
            println!("Daemon is not running at {}", url);
            Ok(())
        }
    }
}

// === Main Entry Point ===

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Determine log level based on command
    let log_level = match &cli.command {
        Some(Commands::Connect { .. }) => Level::WARN,  // Quiet for stdio
        Some(Commands::Hook { .. }) => Level::WARN,     // Quiet for hooks
        _ => Level::INFO,
    };

    let subscriber = FmtSubscriber::builder()
        .with_max_level(log_level)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    match cli.command {
        None => {
            // Default: run daemon on default port and localhost
            let port = std::env::var("MIRA_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(DEFAULT_PORT);
            let listen = std::env::var("MIRA_LISTEN")
                .unwrap_or_else(|_| "127.0.0.1".to_string());
            run_daemon(port, &listen).await?;
        }
        Some(Commands::Daemon { port, listen }) => {
            run_daemon(port, &listen).await?;
        }
        Some(Commands::Connect { url }) => {
            // Run stdio shim that connects to daemon
            let daemon_url = get_daemon_url(url, DEFAULT_PORT);
            connect::run(daemon_url).await?;
        }
        Some(Commands::Status { url }) => {
            let daemon_url = get_daemon_url(url, DEFAULT_PORT);
            run_status(daemon_url).await?;
        }
        Some(Commands::Stop { url }) => {
            let daemon_url = get_daemon_url(url, DEFAULT_PORT);
            run_stop(daemon_url).await?;
        }
        Some(Commands::Hook { action }) => {
            match action {
                HookAction::Permission => {
                    hooks::permission::run().await?;
                }
                HookAction::Precompact => {
                    hooks::precompact::run().await?;
                }
                HookAction::Posttool => {
                    hooks::posttool::run().await?;
                }
                HookAction::Pretool => {
                    hooks::pretool::run().await?;
                }
                HookAction::Sessionstart => {
                    hooks::sessionstart::run().await?;
                }
            }
        }
    }

    Ok(())
}
