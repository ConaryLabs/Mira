// src/main.rs
// Mira - Memory and Intelligence Layer for Claude Code

use anyhow::Result;
use clap::{Parser, Subcommand};
use mira::{db::Database, embeddings::Embeddings, mcp::MiraServer, web};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

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
    /// Run as MCP server (default, for Claude Code)
    Serve,

    /// Run web UI server (Mira Studio)
    Web {
        /// Port to listen on
        #[arg(short, long, default_value = "3000")]
        port: u16,
    },

    /// Index a project
    Index {
        /// Project path (default: current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,
    },

    /// Claude Code hook handlers
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
}

#[derive(Subcommand)]
enum HookAction {
    /// Handle PermissionRequest hooks
    Permission,
    /// Handle SessionStart hooks - captures Claude's session_id
    SessionStart,
    /// Legacy PostToolUse hook (no-op for compatibility)
    Posttool,
    /// Legacy PreToolUse hook (no-op for compatibility)
    Pretool,
}

fn get_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/mira.db")
}

async fn run_mcp_server() -> Result<()> {
    // Open database
    let db_path = get_db_path();
    let db = Arc::new(Database::open(&db_path)?);

    // Initialize embeddings if API key available
    let embeddings = std::env::var("GEMINI_API_KEY")
        .ok()
        .or_else(|| std::env::var("GOOGLE_API_KEY").ok())
        .map(|key| Arc::new(Embeddings::new(key)));

    if embeddings.is_some() {
        info!("Semantic search enabled (Gemini API key found)");
    } else {
        info!("Semantic search disabled (no GEMINI_API_KEY)");
    }

    // Create shared broadcast channel for MCP <-> Web communication
    let (ws_tx, _) = tokio::sync::broadcast::channel::<mira_types::WsEvent>(256);

    // Shared session ID between MCP server and web server
    let session_id: Arc<tokio::sync::RwLock<Option<String>>> = Arc::new(tokio::sync::RwLock::new(None));

    // Spawn embedded web server in background
    let web_port: u16 = std::env::var("MIRA_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001);

    let web_db = db.clone();
    let web_embeddings = embeddings.clone();
    let web_ws_tx = ws_tx.clone();
    let web_session_id = session_id.clone();

    tokio::spawn(async move {
        let state = web::state::AppState::with_broadcaster(web_db, web_embeddings, web_ws_tx, web_session_id);
        let app = web::create_router(state);
        let addr = format!("0.0.0.0:{}", web_port);

        if let Ok(listener) = tokio::net::TcpListener::bind(&addr).await {
            eprintln!("Mira Studio running on http://localhost:{}", web_port);
            let _ = axum::serve(listener, app).await;
        }
    });

    // Create MCP server with broadcaster and shared session ID
    let server = MiraServer::with_broadcaster(db, embeddings, ws_tx, session_id);

    // Run with stdio transport
    let transport = rmcp::transport::io::stdio();
    let service = rmcp::serve_server(server, transport).await?;
    service.waiting().await?;

    Ok(())
}

async fn run_web_server(port: u16) -> Result<()> {
    // Open database
    let db_path = get_db_path();
    let db = Arc::new(Database::open(&db_path)?);

    // Initialize embeddings if API key available
    let embeddings = std::env::var("GEMINI_API_KEY")
        .ok()
        .or_else(|| std::env::var("GOOGLE_API_KEY").ok())
        .map(|key| Arc::new(Embeddings::new(key)));

    if embeddings.is_some() {
        info!("Semantic search enabled (Gemini API key found)");
    } else {
        info!("Semantic search disabled (no GEMINI_API_KEY)");
    }

    // Create app state
    let state = web::state::AppState::new(db, embeddings);

    // Create router
    let app = web::create_router(state);

    // Start server
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!("Mira Studio running on http://localhost:{}", port);
    println!("Mira Studio running on http://localhost:{}", port);

    axum::serve(listener, app).await?;

    Ok(())
}

async fn run_index(path: Option<PathBuf>) -> Result<()> {
    let path = path.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    info!("Indexing project at {}", path.display());

    let db_path = get_db_path();
    let db = Arc::new(Database::open(&db_path)?);

    let embeddings = std::env::var("GEMINI_API_KEY")
        .ok()
        .or_else(|| std::env::var("GOOGLE_API_KEY").ok())
        .map(|key| Arc::new(Embeddings::new(key)));

    // Get or create project
    let (project_id, _project_name) = db.get_or_create_project(
        path.to_string_lossy().as_ref(),
        path.file_name().and_then(|n| n.to_str()),
    )?;

    let stats = mira::indexer::index_project(&path, db, embeddings, Some(project_id)).await?;

    println!(
        "Indexed {} files, {} symbols, {} code chunks",
        stats.files, stats.symbols, stats.chunks
    );

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env files (global first, then project - project overrides)
    if let Some(home) = dirs::home_dir() {
        let _ = dotenvy::from_path(home.join(".mira/.env"));
    }
    let _ = dotenvy::dotenv(); // Load .env from current directory

    let cli = Cli::parse();

    // Set up logging based on command
    let log_level = match &cli.command {
        Some(Commands::Serve) | None => Level::WARN, // Quiet for MCP stdio
        Some(Commands::Hook { .. }) => Level::WARN,
        Some(Commands::Web { .. }) => Level::INFO,   // Verbose for web server
        Some(Commands::Index { .. }) => Level::INFO,
    };

    let subscriber = FmtSubscriber::builder()
        .with_max_level(log_level)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    match cli.command {
        None | Some(Commands::Serve) => {
            run_mcp_server().await?;
        }
        Some(Commands::Web { port }) => {
            run_web_server(port).await?;
        }
        Some(Commands::Index { path }) => {
            run_index(path).await?;
        }
        Some(Commands::Hook { action }) => match action {
            HookAction::Permission => {
                mira::hooks::permission::run().await?;
            }
            HookAction::SessionStart => {
                mira::hooks::session::run()?;
            }
            HookAction::Posttool | HookAction::Pretool => {
                // Legacy no-op hooks for compatibility
            }
        },
    }

    Ok(())
}
