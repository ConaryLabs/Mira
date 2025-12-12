// src/main.rs
// Mira Power Suit - MCP Server for Claude Code
// CLI entry point

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
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
use std::time::Duration;

mod tools;
mod indexer;
mod hooks;
mod server;
mod daemon;

use server::{MiraServer, create_optimized_pool};
use tools::SemanticSearch;

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
    /// Run the MCP server over HTTP/SSE
    ServeHttp {
        /// Port to listen on
        #[arg(short, long, default_value = "3000")]
        port: u16,
        /// Auth token (required for connections)
        #[arg(short, long, env = "MIRA_AUTH_TOKEN")]
        auth_token: Option<String>,
    },
    /// Daemon management commands
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
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
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the background watcher daemon
    Start {
        /// Project path to watch (defaults to current directory)
        #[arg(short, long)]
        path: Option<String>,
    },
    /// Stop the daemon
    Stop {
        /// Project path (defaults to current directory)
        #[arg(short, long)]
        path: Option<String>,
    },
    /// Check daemon status
    Status {
        /// Project path (defaults to current directory)
        #[arg(short, long)]
        path: Option<String>,
    },
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
            if auth_str.starts_with("Bearer ") {
                let token = &auth_str[7..];
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
            // Run MCP server over HTTP/SSE
            info!("Starting Mira MCP Server (HTTP/SSE) on port {}...", port);

            let database_url = std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://data/mira.db".to_string());
            let qdrant_url = std::env::var("QDRANT_URL").ok();
            let gemini_key = std::env::var("GEMINI_API_KEY")
                .or_else(|_| std::env::var("GOOGLE_API_KEY"))
                .ok();

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

            // Build router with optional auth middleware
            // Health endpoint is public, MCP endpoint requires auth
            let app = if let Some(token) = expected_token {
                info!("Auth token required for connections");
                let mcp_router = axum::Router::new()
                    .nest_service("/mcp", mcp_service)
                    .layer(axum::middleware::from_fn(move |req, next| {
                        let token = token.clone();
                        auth_middleware(req, next, token)
                    }));
                axum::Router::new()
                    .route("/health", axum::routing::get(health_handler))
                    .merge(mcp_router)
                    .layer(TimeoutLayer::with_status_code(StatusCode::GATEWAY_TIMEOUT, Duration::from_secs(60)))
                    .layer(TraceLayer::new_for_http())
            } else {
                info!("Warning: No auth token set, server is open");
                axum::Router::new()
                    .route("/health", axum::routing::get(health_handler))
                    .nest_service("/mcp", mcp_service)
                    .layer(TimeoutLayer::with_status_code(StatusCode::GATEWAY_TIMEOUT, Duration::from_secs(60)))
                    .layer(TraceLayer::new_for_http())
            };

            let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
            info!("Listening on http://0.0.0.0:{}/mcp", port);

            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal())
                .await?;
        }
        Some(Commands::Daemon { action }) => {
            let database_url = std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://data/mira.db".to_string());
            let qdrant_url = std::env::var("QDRANT_URL").ok();
            let gemini_key = std::env::var("GEMINI_API_KEY")
                .or_else(|_| std::env::var("GOOGLE_API_KEY"))
                .ok();

            match action {
                DaemonAction::Start { path } => {
                    let project_path = path
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|| std::env::current_dir().unwrap());

                    // Check if already running
                    if let Some(pid) = daemon::is_running(&project_path) {
                        println!("Daemon already running with PID {}", pid);
                        return Ok(());
                    }

                    info!("Starting Mira daemon for {}", project_path.display());
                    let d = daemon::Daemon::new(
                        &project_path,
                        &database_url,
                        qdrant_url.as_deref(),
                        gemini_key,
                    ).await?;
                    d.run().await?;
                }
                DaemonAction::Stop { path } => {
                    let project_path = path
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|| std::env::current_dir().unwrap());

                    if daemon::stop(&project_path)? {
                        println!("Daemon stopped");
                    } else {
                        println!("No daemon running");
                    }
                }
                DaemonAction::Status { path } => {
                    let project_path = path
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|| std::env::current_dir().unwrap());

                    if let Some(pid) = daemon::is_running(&project_path) {
                        println!("Daemon running with PID {}", pid);
                    } else {
                        println!("Daemon not running");
                    }
                }
            }
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
            }
        }
    }

    Ok(())
}
