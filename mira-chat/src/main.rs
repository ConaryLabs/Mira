//! Mira Chat - Power-armored coding assistant with GPT-5.2
//!
//! A standalone coding assistant that uses:
//! - GPT-5.2 Thinking with Responses API
//! - Variable reasoning effort (none/low/medium/high/xhigh)
//! - Persistent memory via SQLite + Qdrant
//! - Mira context injection (corrections, goals, memories)

use anyhow::Result;
use clap::Parser;
use sqlx::sqlite::SqlitePoolOptions;
use std::sync::Arc;
use tracing_subscriber::{fmt, EnvFilter};

mod config;
mod context;
mod reasoning;
mod repl;
mod responses;
mod semantic;
mod server;
mod session;
mod tools;

#[derive(Parser)]
#[command(name = "mira-chat")]
#[command(about = "Power-armored coding assistant with GPT-5.2")]
struct Args {
    /// Run as HTTP server instead of REPL (for Studio integration)
    #[arg(long)]
    serve: bool,

    /// HTTP server port (default: 3000)
    #[arg(long, default_value = "3000")]
    port: u16,

    /// Database path (sqlite URL)
    #[arg(long, env = "DATABASE_URL")]
    database_url: Option<String>,

    /// Qdrant URL
    #[arg(long, env = "QDRANT_URL")]
    qdrant_url: Option<String>,

    /// OpenAI API key
    #[arg(long, env = "OPENAI_API_KEY")]
    openai_api_key: Option<String>,

    /// Gemini API key for embeddings
    #[arg(long, env = "GEMINI_API_KEY")]
    gemini_api_key: Option<String>,

    /// Default reasoning effort
    #[arg(long)]
    reasoning_effort: Option<String>,

    /// Project path (defaults to current directory)
    #[arg(long, short = 'p')]
    project: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    // Load config file (~/.mira/config.toml)
    let config = config::Config::load();

    // Resolve values: CLI args > env vars (handled by clap) > config file > defaults
    let api_key = args.openai_api_key
        .or(config.openai_api_key)
        .expect("OPENAI_API_KEY required (set via --openai-api-key, env var, or ~/.mira/config.toml)");

    let database_url = args.database_url
        .or(config.database_url)
        .unwrap_or_else(|| "sqlite://data/mira.db".to_string());

    let qdrant_url = args.qdrant_url
        .or(config.qdrant_url)
        .unwrap_or_else(|| "http://localhost:6334".to_string());

    let reasoning_effort = args.reasoning_effort
        .or(config.reasoning_effort)
        .unwrap_or_else(|| "medium".to_string());

    let gemini_key = args.gemini_api_key
        .or(config.gemini_api_key);

    // Determine project path - resolve to absolute path for database lookup
    let project_path = args.project
        .or(config.project)
        .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().to_string()))
        .unwrap_or_else(|| ".".to_string());

    // Canonicalize to absolute path (required for project lookup in database)
    let project_path = std::path::Path::new(&project_path)
        .canonicalize()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or(project_path);

    use repl::colors::ansi::*;

    // Pretty startup banner
    println!();
    println!("{}{}  Mira Chat {}{}", BOLD, MAGENTA, env!("CARGO_PKG_VERSION"), RESET);
    println!("{}", repl::colors::separator(50));
    println!("{}Model{}       GPT-5.2 Thinking", DIM, RESET);
    println!("{}Reasoning{}   {}", DIM, RESET, reasoning_effort);
    println!("{}Project{}     {}", DIM, RESET, project_path);

    // Connect to database
    let db_url = if database_url.starts_with("sqlite:") {
        database_url.clone()
    } else {
        format!("sqlite:{}", database_url)
    };

    let db = match SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
    {
        Ok(pool) => {
            println!("{}Database{}    {}connected{}", DIM, RESET, GREEN, RESET);
            Some(pool)
        }
        Err(e) => {
            println!("{}Database{}    {}unavailable{} ({})", DIM, RESET, YELLOW, RESET, e);
            None
        }
    };

    // Initialize semantic search (Qdrant + Gemini embeddings)
    let semantic = Arc::new(
        semantic::SemanticSearch::new(Some(&qdrant_url), gemini_key).await
    );

    if semantic.is_available() {
        println!("{}Semantic{}    {}enabled{}", DIM, RESET, GREEN, RESET);
        // Ensure collection exists
        if let Err(e) = semantic.ensure_collection(semantic::COLLECTION_MEMORY).await {
            println!("{}Semantic{}    {}init failed{} ({})", DIM, RESET, RED, RESET, e);
        }
    } else {
        println!("{}Semantic{}    {}disabled{}", DIM, RESET, YELLOW, RESET);
    }

    // Load context from Mira
    let context = if let Some(ref pool) = db {
        match context::MiraContext::load(pool, &project_path).await {
            Ok(ctx) => {
                // Show persona status
                if ctx.persona.is_some() {
                    println!("{}Persona{}     {}loaded{}", DIM, RESET, GREEN, RESET);
                } else {
                    println!("{}Persona{}     {}fallback{}", DIM, RESET, YELLOW, RESET);
                }

                let n_corrections = ctx.corrections.len();
                let n_goals = ctx.goals.len();
                let n_memories = ctx.memories.len();
                if n_corrections > 0 || n_goals > 0 || n_memories > 0 {
                    println!("{}Context{}     {} corrections, {} goals, {} memories",
                        DIM, RESET, n_corrections, n_goals, n_memories);
                } else {
                    println!("{}Context{}     {}empty{}", DIM, RESET, DIM, RESET);
                }
                ctx
            }
            Err(e) => {
                println!("{}Context{}     {}failed{} ({})", DIM, RESET, RED, RESET, e);
                let mut ctx = context::MiraContext::default();
                ctx.project_path = Some(project_path.clone());
                ctx
            }
        }
    } else {
        let mut ctx = context::MiraContext::default();
        ctx.project_path = Some(project_path.clone());
        ctx
    };

    // Initialize session manager for invisible persistence
    let session = if let Some(ref pool) = db {
        match session::SessionManager::new(pool.clone(), Arc::clone(&semantic), project_path.clone()).await {
            Ok(sm) => {
                let stats = sm.stats().await.unwrap_or(session::SessionStats {
                    total_messages: 0,
                    summary_count: 0,
                    has_active_conversation: false,
                    has_code_compaction: false,
                });
                if stats.has_active_conversation {
                    println!("{}Session{}     {}resuming{} ({} messages, {} summaries)",
                        DIM, RESET, CYAN, RESET, stats.total_messages, stats.summary_count);
                } else {
                    println!("{}Session{}     {}new{}", DIM, RESET, GREEN, RESET);
                }
                Some(Arc::new(sm))
            }
            Err(e) => {
                println!("{}Session{}     {}unavailable{} ({})", DIM, RESET, YELLOW, RESET, e);
                None
            }
        }
    } else {
        println!("{}Session{}     {}disabled{}", DIM, RESET, YELLOW, RESET);
        None
    };

    println!("{}", repl::colors::separator(50));
    println!();

    // Run server or REPL based on --serve flag
    if args.serve {
        server::run(args.port, api_key, db, semantic, reasoning_effort).await
    } else {
        repl::run_with_context(api_key, context, db, semantic, session).await
    }
}
