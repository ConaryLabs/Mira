// crates/mira-server/src/main.rs
// Mira - Memory and Intelligence Layer for Claude Code

use anyhow::Result;
use clap::{Parser, Subcommand};
use mira::{background, db::Database, embeddings::Embeddings, llm::DeepSeekClient, mcp::MiraServer};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

/// Get embeddings client if API key is available (filters empty keys)
fn get_embeddings() -> Option<Arc<Embeddings>> {
    std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .map(|key| Arc::new(Embeddings::new(key)))
}

/// Get DeepSeek client if API key is available
fn get_deepseek() -> Option<Arc<DeepSeekClient>> {
    std::env::var("DEEPSEEK_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .map(|key| Arc::new(DeepSeekClient::new(key)))
}

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

        /// Skip embeddings (faster, no semantic search)
        #[arg(long)]
        no_embed: bool,
    },

    /// Claude Code hook handlers
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },

    /// Debug cartographer module detection
    DebugCarto {
        /// Project path to analyze
        #[arg(short, long)]
        path: Option<PathBuf>,
    },

    /// Debug session_start output
    DebugSession {
        /// Project path
        #[arg(short, long)]
        path: Option<PathBuf>,
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
    let embeddings = get_embeddings();

    if embeddings.is_some() {
        info!("Semantic search enabled (OpenAI embeddings)");
    } else {
        info!("Semantic search disabled (no OPENAI_API_KEY)");
    }

    // Initialize DeepSeek client if API key available
    let deepseek = get_deepseek();

    if deepseek.is_some() {
        info!("DeepSeek enabled (for experts and module summaries)");
    } else {
        info!("DeepSeek disabled (no DEEPSEEK_API_KEY)");
    }

    // Spawn background worker for batch processing
    let bg_db = db.clone();
    let bg_embeddings = embeddings.clone();
    let bg_deepseek = deepseek.clone();
    let _shutdown_tx = background::spawn(bg_db, bg_embeddings, bg_deepseek);
    info!("Background worker started");

    // Create MCP server
    let server = MiraServer::new(db, embeddings);

    // Run with stdio transport
    let transport = rmcp::transport::io::stdio();
    let service = rmcp::serve_server(server, transport).await?;
    service.waiting().await?;

    Ok(())
}

async fn run_index(path: Option<PathBuf>, no_embed: bool) -> Result<()> {
    let path = path.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    info!("Indexing project at {}", path.display());

    let db_path = get_db_path();
    let db = Arc::new(Database::open(&db_path)?);

    let embeddings = if no_embed { None } else { get_embeddings() };

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
        Some(Commands::Index { .. }) => Level::INFO,
        Some(Commands::DebugCarto { .. }) => Level::DEBUG,
        Some(Commands::DebugSession { .. }) => Level::DEBUG,
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
        Some(Commands::Index { path, no_embed }) => {
            run_index(path, no_embed).await?;
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
        Some(Commands::DebugCarto { path }) => {
            run_debug_carto(path)?;
        }
        Some(Commands::DebugSession { path }) => {
            run_debug_session(path).await?;
        }
    }

    Ok(())
}

/// Debug session_start output
async fn run_debug_session(path: Option<PathBuf>) -> Result<()> {
    let project_path = path.unwrap_or_else(|| std::env::current_dir().unwrap());
    println!("=== Debug Session Start ===\n");
    println!("Project: {:?}\n", project_path);

    let db_path = get_db_path();
    let db = Arc::new(Database::open(&db_path)?);

    // Create a minimal MCP server context
    let server = mira::mcp::MiraServer::new(db.clone(), None);

    // Call session_start
    let result = mira::tools::session_start(
        &server,
        project_path.to_string_lossy().to_string(),
        None,
        None,
    ).await;

    match result {
        Ok(output) => {
            println!("--- Session Start Output ({} chars) ---\n", output.len());
            println!("{}", output);
        }
        Err(e) => {
            println!("ERROR: {}", e);
        }
    }

    Ok(())
}

/// Debug cartographer module detection
fn run_debug_carto(path: Option<PathBuf>) -> Result<()> {
    let project_path = path.unwrap_or_else(|| std::env::current_dir().unwrap());
    println!("=== Cartographer Debug ===\n");
    println!("Project path: {:?}\n", project_path);

    // Test module detection
    let modules = mira::cartographer::detect_rust_modules(&project_path);
    println!("Detected {} modules:\n", modules.len());

    for m in &modules {
        println!("  {} ({})", m.id, m.path);
        if let Some(ref purpose) = m.purpose {
            println!("    Purpose: {}", purpose);
        }
    }

    // Try full map generation with database
    println!("\n--- Database Integration ---\n");
    let db_path = get_db_path();
    let db = Database::open(&db_path)?;
    let (project_id, name) = db.get_or_create_project(
        project_path.to_str().unwrap(),
        None,
    )?;
    println!("Project ID: {}, Name: {:?}", project_id, name);

    match mira::cartographer::get_or_generate_map(
        &db,
        project_id,
        project_path.to_str().unwrap(),
        name.as_deref().unwrap_or("unknown"),
        "rust",
    ) {
        Ok(map) => {
            println!("\nCodebase map generated with {} modules", map.modules.len());
            println!("\n{}", mira::cartographer::format_compact(&map));
        }
        Err(e) => {
            println!("\nError generating map: {}", e);
        }
    }

    Ok(())
}
