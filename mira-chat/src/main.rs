//! Mira Chat - Power-armored coding assistant with GPT-5.2
//!
//! A standalone coding assistant that uses:
//! - GPT-5.2 Thinking with Responses API
//! - Variable reasoning effort (none/low/medium/high/xhigh)
//! - Persistent memory via SQLite + Qdrant
//! - Mira context injection (corrections, goals, memories)

use anyhow::Result;
use clap::Parser;
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
    /// Database path
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
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    // Verify API key is set
    if args.openai_api_key.is_none() && std::env::var("OPENAI_API_KEY").is_err() {
        eprintln!("Error: OPENAI_API_KEY environment variable or --openai-api-key required");
        std::process::exit(1);
    }

    println!("Mira Chat v{}", env!("CARGO_PKG_VERSION"));
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("GPT-5.2 Thinking | Reasoning: {}", args.reasoning_effort);
    println!();

    // TODO: Initialize database connection
    // TODO: Initialize Qdrant connection
    // TODO: Start REPL

    repl::run().await
}
