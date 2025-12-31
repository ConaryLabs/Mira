// src/main.rs
// Mira - Memory and Intelligence Layer for Claude Code

use anyhow::Result;
use clap::{Parser, Subcommand};
use mira::{db::Database, embeddings::Embeddings, mcp::MiraServer};
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

    // Create MCP server
    let server = MiraServer::new(db, embeddings);

    // Run with stdio transport
    let transport = rmcp::transport::io::stdio();
    let service = rmcp::serve_server(server, transport).await?;
    service.waiting().await?;

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
    let project_id = db.get_or_create_project(
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
    let cli = Cli::parse();

    // Set up logging based on command
    let log_level = match &cli.command {
        Some(Commands::Serve) | None => Level::WARN, // Quiet for MCP stdio
        Some(Commands::Hook { .. }) => Level::WARN,
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
            run_mcp_server().await?;
        }
        Some(Commands::Index { path }) => {
            run_index(path).await?;
        }
        Some(Commands::Hook { action }) => match action {
            HookAction::Permission => {
                mira::hooks::permission::run().await?;
            }
        },
    }

    Ok(())
}
