// backend/src/bin/mira.rs
// Mira CLI - Claude Code-style command line interface

use anyhow::Result;
use clap::Parser;
use mira_backend::cli::{CliArgs, Repl};

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = CliArgs::parse();

    // Create and run REPL
    let mut repl = Repl::new(args).await?;
    repl.run().await?;

    Ok(())
}
