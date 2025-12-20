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
    ServiceExt,
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

mod chat;
mod core;
mod tools;
mod indexer;
mod hooks;
mod server;
mod daemon;
mod connect;

use server::{MiraServer, create_optimized_pool};
use tools::SemanticSearch;
use chat::tools::WebSearchConfig;

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
async fn auth_middleware(
    req: Request<Body>,
    next: Next,
    expected_token: String,
) -> Result<Response, StatusCode> {
    let path = req.uri().path();

    // Skip auth for public endpoints
    if path == "/health" || path.starts_with("/api/") {
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

async fn run_daemon(port: u16) -> Result<()> {
    info!("Starting Mira Daemon on port {}...", port);

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
    info!("Database connected: {}", database_url);

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
    let project_path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    info!("Indexer watching: {}", project_path.display());
    let daemon = daemon::Daemon::with_shared(
        vec![project_path],
        (*db).clone(),
        semantic.clone(),
    );
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

    // CORS configuration for browser-based clients (Claude.ai web)
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .expose_headers([
            "mcp-session-id".parse().unwrap(),
            "content-type".parse().unwrap(),
        ]);

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
            auth_middleware(req, next, token)
        }))
        .layer(cors)
        .layer(TimeoutLayer::with_status_code(StatusCode::GATEWAY_TIMEOUT, Duration::from_secs(60)))
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    info!("Mira Daemon listening on http://0.0.0.0:{}", port);
    info!("  Health: /health");
    info!("  MCP:    /mcp (requires auth)");
    info!("  Chat:   /api/chat/stream, /api/chat/sync");

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
            // Default: run daemon on default port
            let port = std::env::var("MIRA_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(DEFAULT_PORT);
            run_daemon(port).await?;
        }
        Some(Commands::Daemon { port }) => {
            run_daemon(port).await?;
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
