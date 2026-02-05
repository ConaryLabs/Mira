// crates/mira-server/src/main.rs
// Mira - Memory and Intelligence Layer for AI Agents
// Mira supports Ralph for autonomous development loops

mod cli;

use anyhow::Result;
use clap::Parser;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

use cli::{Cli, Commands, HookAction};

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env files (global first, then project - project overrides)
    if let Some(home) = dirs::home_dir()
        && let Err(e) = dotenvy::from_path(home.join(".mira/.env"))
    {
        tracing::debug!("Failed to load global .env file: {}", e);
    }
    if let Err(e) = dotenvy::dotenv() {
        tracing::debug!("Failed to load local .env file: {}", e);
    } // Load .env from current directory

    let cli = Cli::parse();

    // Set up logging based on command
    let log_level = match &cli.command {
        Some(Commands::Serve) | None => Level::WARN, // Quiet for MCP stdio
        Some(Commands::Tool { .. }) => Level::WARN,
        Some(Commands::Hook { .. }) => Level::WARN,
        Some(Commands::Index { quiet, .. }) if *quiet => Level::WARN,
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
            cli::run_mcp_server().await?;
        }
        Some(Commands::Tool { name, args }) => {
            cli::run_tool(name, args).await?;
        }
        Some(Commands::Index {
            path,
            no_embed,
            quiet,
        }) => {
            cli::run_index(path, no_embed, quiet).await?;
        }
        Some(Commands::Hook { action }) => match action {
            HookAction::Permission => {
                mira::hooks::permission::run().await?;
            }
            HookAction::SessionStart => {
                mira::hooks::session::run()?;
            }
            HookAction::PreCompact => {
                mira::hooks::precompact::run().await?;
            }
            HookAction::PreTool => {
                mira::hooks::pre_tool::run().await?;
            }
            HookAction::UserPrompt => {
                mira::hooks::user_prompt::run().await?;
            }
            HookAction::PostTool => {
                mira::hooks::post_tool::run().await?;
            }
            HookAction::Stop => {
                mira::hooks::stop::run().await?;
            }
            HookAction::SessionEnd => {
                mira::hooks::stop::run_session_end().await?;
            }
            HookAction::SubagentStart => {
                mira::hooks::subagent::run_start().await?;
            }
            HookAction::SubagentStop => {
                mira::hooks::subagent::run_stop().await?;
            }
        },
        Some(Commands::DebugCarto { path }) => {
            cli::run_debug_carto(path).await?;
        }
        Some(Commands::DebugSession { path }) => {
            cli::run_debug_session(path).await?;
        }
    }

    Ok(())
}
