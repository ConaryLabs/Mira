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

mod conductor;
mod config;
mod context;
mod provider;
mod reasoning;
mod repl;
mod responses;
mod server;
mod session;
mod tools;

// Re-export from mira-core for use by submodules
pub use mira_core::artifacts;
pub use mira_core::semantic;

// COLLECTION_MEMORY alias for backwards compatibility
pub use mira_core::COLLECTION_CONVERSATION;
pub const COLLECTION_MEMORY: &str = mira_core::COLLECTION_CONVERSATION;

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

    /// DeepSeek API key
    #[arg(long, env = "DEEPSEEK_API_KEY")]
    deepseek_api_key: Option<String>,

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
    // Load .env file (from ~/.mira/.env or current dir)
    let env_path = dirs::home_dir()
        .map(|h| h.join(".mira").join(".env"))
        .filter(|p| p.exists());
    if let Some(path) = env_path {
        let _ = dotenvy::from_path(&path);
    } else {
        let _ = dotenvy::dotenv(); // fallback to current dir
    }

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

    let deepseek_key = args.deepseek_api_key
        .or(config.deepseek_api_key);

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
    println!("{}DeepSeek{}    {}", DIM, RESET,
        if deepseek_key.is_some() { format!("{}available{}", GREEN, RESET) }
        else { format!("{}not configured{}", YELLOW, RESET) });
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
        if let Err(e) = semantic.ensure_collection(COLLECTION_CONVERSATION).await {
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

    // Artifact maintenance: cleanup expired + enforce size cap
    if let Some(ref pool) = db {
        let store = artifacts::ArtifactStore::new(pool.clone(), project_path.clone());
        match store.maintenance().await {
            Ok((expired, capped)) => {
                if expired > 0 || capped > 0 {
                    println!("{}Artifacts{}   cleaned {} expired, {} over cap",
                        DIM, RESET, expired, capped);
                }
            }
            Err(e) => {
                tracing::warn!("Artifact maintenance failed: {}", e);
            }
        }

        // Spawn background maintenance task (every hour)
        let maint_pool = pool.clone();
        let maint_path = project_path.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
            interval.tick().await; // Skip immediate tick
            loop {
                interval.tick().await;
                let store = artifacts::ArtifactStore::new(maint_pool.clone(), maint_path.clone());
                if let Ok((expired, capped)) = store.maintenance().await {
                    if expired > 0 || capped > 0 {
                        tracing::info!("Artifact maintenance: {} expired, {} capped", expired, capped);
                    }
                }
            }
        });
    }

    println!("{}", repl::colors::separator(50));
    println!();

    // Run server or REPL based on --serve flag
    if args.serve {
        // Get sync token from env (optional auth for /api/chat/sync)
        let sync_token = std::env::var("MIRA_SYNC_TOKEN").ok();
        server::run(args.port, api_key, db, semantic, reasoning_effort, sync_token).await
    } else {
        repl::run_with_context(api_key, context, db, semantic, session).await
    }
}
