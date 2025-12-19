// src/main.rs
// Mira Power Suit - MCP Server for Claude Code
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
    ServiceExt,
    transport::{StreamableHttpService, StreamableHttpServerConfig},
    transport::streamable_http_server::session::local::LocalSessionManager,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::trace::TraceLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::cors::{CorsLayer, Any};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
use std::time::Duration;

mod chat;
mod tools;
mod indexer;
mod hooks;
mod server;
mod daemon;

use server::{MiraServer, create_optimized_pool};
use tools::SemanticSearch;
use chat::tools::WebSearchConfig;

// === CLI Definition ===

#[derive(Parser)]
#[command(name = "mira")]
#[command(about = "Memory and Intelligence Layer for Claude Code")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the MCP server over stdio (default)
    Serve,
    /// Run the full Mira server (MCP + Chat + Indexer)
    ServeHttp {
        /// Port to listen on
        #[arg(short, long, default_value = "3000")]
        port: u16,
        /// Auth token (required for MCP connections)
        #[arg(short, long, env = "MIRA_AUTH_TOKEN")]
        auth_token: Option<String>,
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


// === Middleware ===

/// Auth middleware that checks for Bearer token
async fn auth_middleware(
    req: Request<Body>,
    next: Next,
    expected_token: String,
) -> Result<Response, StatusCode> {
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

// === Main Entry Point ===

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Stdio mode: quiet (WARN only) to avoid polluting Claude Code output
    // HTTP/daemon mode: INFO for visibility
    let log_level = match &cli.command {
        None | Some(Commands::Serve) => Level::WARN,
        _ => Level::INFO,
    };

    let subscriber = FmtSubscriber::builder()
        .with_max_level(log_level)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    match cli.command {
        None | Some(Commands::Serve) => {
            // Default: run MCP server over stdio (quiet mode)
            let database_url = std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://data/mira.db".to_string());
            let qdrant_url = std::env::var("QDRANT_URL").ok();
            let gemini_key = std::env::var("GEMINI_API_KEY")
                .or_else(|_| std::env::var("GOOGLE_API_KEY"))
                .ok();

            let server = MiraServer::new(&database_url, qdrant_url.as_deref(), gemini_key).await?;
            let service = server.serve(rmcp::transport::stdio()).await?;
            service.waiting().await?;
        }
        Some(Commands::ServeHttp { port, auth_token }) => {
            // Run consolidated MCP + Chat + Indexer server over HTTP
            info!("Starting Mira Server on port {}...", port);

            let database_url = std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://data/mira.db".to_string());
            let qdrant_url = std::env::var("QDRANT_URL").ok();
            let gemini_key = std::env::var("GEMINI_API_KEY")
                .or_else(|_| std::env::var("GOOGLE_API_KEY"))
                .ok();

            // DeepSeek API key for chat
            let deepseek_key = std::env::var("DEEPSEEK_API_KEY")
                .ok()
                .filter(|s| !s.is_empty());

            // Google Custom Search config for web search tool
            let web_search_config = WebSearchConfig {
                google_api_key: std::env::var("GOOGLE_API_KEY").ok(),
                google_cx: std::env::var("GOOGLE_CX").ok(),
            };

            // Sync token for chat sync endpoint
            let sync_token = std::env::var("MIRA_SYNC_TOKEN").ok();

            // Create shared state that will be cloned for each session
            let db = Arc::new(create_optimized_pool(&database_url).await?);
            let semantic = Arc::new(SemanticSearch::new(qdrant_url.as_deref(), gemini_key).await);
            info!("Database connected");

            // Optional auth token validation
            let expected_token = auth_token.clone();

            // Create the MCP service with StreamableHttpService
            let mcp_service = StreamableHttpService::new(
                {
                    let db = db.clone();
                    let semantic = semantic.clone();
                    move || {
                        Ok(MiraServer {
                            db: db.clone(),
                            semantic: semantic.clone(),
                            tool_router: MiraServer::get_tool_router(),
                            active_project: Arc::new(RwLock::new(None)),
                        })
                    }
                },
                Arc::new(LocalSessionManager::default()),
                StreamableHttpServerConfig::default(),
            );

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
                    web_search_config,
                };
                Some(chat::create_router(chat_state))
            } else {
                info!("Chat endpoints disabled (no DEEPSEEK_API_KEY)");
                None
            };

            // Start background indexer (watches current directory)
            let project_path = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            info!("Indexer watching: {}", project_path.display());
            let daemon = daemon::Daemon::with_shared(
                vec![project_path],
                (*db).clone(),
                semantic.clone(),
            );
            let _daemon_tasks = match daemon.spawn_background_tasks().await {
                Ok(tasks) => Some(tasks),
                Err(e) => {
                    tracing::warn!("Failed to start indexer: {}", e);
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

            // CORS configuration for browser-based clients (Claude.ai web)
            let cors = CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any)
                .expose_headers([
                    "mcp-session-id".parse().unwrap(),
                    "content-type".parse().unwrap(),
                ]);

            // Build router with optional auth middleware
            // Health endpoint and chat endpoints are public, MCP endpoint requires auth
            let mut base_router = axum::Router::new()
                .route("/health", axum::routing::get(health_handler));

            // Add chat routes (already has its own CORS)
            if let Some(chat) = chat_router {
                base_router = base_router.merge(chat);
            }

            let app = if let Some(token) = expected_token {
                info!("Auth token required for MCP connections");
                let mcp_router = axum::Router::new()
                    .nest_service("/mcp", mcp_service)
                    .layer(axum::middleware::from_fn(move |req, next| {
                        let token = token.clone();
                        auth_middleware(req, next, token)
                    }));
                base_router
                    .merge(mcp_router)
                    .layer(cors)
                    .layer(TimeoutLayer::with_status_code(StatusCode::GATEWAY_TIMEOUT, Duration::from_secs(60)))
                    .layer(TraceLayer::new_for_http())
            } else {
                info!("Warning: No auth token set, server is open");
                base_router
                    .nest_service("/mcp", mcp_service)
                    .layer(cors)
                    .layer(TimeoutLayer::with_status_code(StatusCode::GATEWAY_TIMEOUT, Duration::from_secs(60)))
                    .layer(TraceLayer::new_for_http())
            };

            let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
            info!("Listening on http://0.0.0.0:{}", port);
            info!("  MCP:  /mcp");
            info!("  Chat: /api/chat/stream, /api/chat/sync");

            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal())
                .await?;
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
