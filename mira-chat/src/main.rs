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
use tracing_subscriber::{fmt, EnvFilter};

mod context;
mod reasoning;
mod repl;
mod responses;
mod tools;

#[derive(Parser)]
#[command(name = "mira-chat")]
#[command(about = "Power-armored coding assistant with GPT-5.2")]
struct Args {
    /// Database path (sqlite URL)
    #[arg(long, env = "DATABASE_URL", default_value = "sqlite://data/mira.db")]
    database_url: String,

    /// Qdrant URL
    #[arg(long, env = "QDRANT_URL", default_value = "http://localhost:6334")]
    qdrant_url: String,

    /// OpenAI API key
    #[arg(long, env = "OPENAI_API_KEY")]
    openai_api_key: Option<String>,

    /// Default reasoning effort
    #[arg(long, default_value = "medium")]
    reasoning_effort: String,

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

    // Verify API key is set
    let api_key = args.openai_api_key
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .expect("OPENAI_API_KEY environment variable or --openai-api-key required");

    // Determine project path
    let project_path = args.project
        .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().to_string()))
        .unwrap_or_else(|| ".".to_string());

    println!("Mira Chat v{}", env!("CARGO_PKG_VERSION"));
    println!("{}", "=".repeat(50));
    println!("GPT-5.2 Thinking | Reasoning: {}", args.reasoning_effort);
    println!("Project: {}", project_path);

    // Connect to database
    let db_url = if args.database_url.starts_with("sqlite:") {
        args.database_url.clone()
    } else {
        format!("sqlite:{}", args.database_url)
    };

    let db = match SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
    {
        Ok(pool) => {
            println!("Database: connected");
            Some(pool)
        }
        Err(e) => {
            println!("Database: not available ({})", e);
            None
        }
    };

    // Load context from Mira
    let context = if let Some(ref pool) = db {
        match context::MiraContext::load(pool, &project_path).await {
            Ok(ctx) => {
                let n_corrections = ctx.corrections.len();
                let n_goals = ctx.goals.len();
                let n_memories = ctx.memories.len();
                if n_corrections > 0 || n_goals > 0 || n_memories > 0 {
                    println!("Context: {} corrections, {} goals, {} memories",
                        n_corrections, n_goals, n_memories);
                } else {
                    println!("Context: (empty)");
                }
                ctx
            }
            Err(e) => {
                println!("Context: failed to load ({})", e);
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

    println!("{}", "=".repeat(50));
    println!();

    // Run REPL
    repl::run_with_context(api_key, context).await
}
