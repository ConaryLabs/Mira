// crates/mira-server/src/main.rs
// Mira - Memory and Intelligence Layer for AI Agents

mod cli;

use anyhow::Result;
use clap::Parser;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

use cli::{Cli, Commands, HookAction, ProxyAction, BackendAction};

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env files (global first, then project - project overrides)
    if let Some(home) = dirs::home_dir() {
        if let Err(e) = dotenvy::from_path(home.join(".mira/.env")) {
            tracing::debug!("Failed to load global .env file: {}", e);
        }
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
        Some(Commands::Index { .. }) => Level::INFO,
        Some(Commands::DebugCarto { .. }) => Level::DEBUG,
        Some(Commands::DebugSession { .. }) => Level::DEBUG,
        Some(Commands::Proxy { .. }) => Level::INFO,
        Some(Commands::Backend { .. }) => Level::INFO,
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
        Some(Commands::Index { path, no_embed }) => {
            cli::run_index(path, no_embed).await?;
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
            HookAction::UserPrompt => {
                mira::hooks::user_prompt::run().await?;
            }
            HookAction::PostTool => {
                mira::hooks::post_tool::run().await?;
            }
            HookAction::Stop => {
                mira::hooks::stop::run().await?;
            }
            HookAction::Posttool | HookAction::Pretool => {
                // Legacy no-op hooks for compatibility
            }
        },
        Some(Commands::DebugCarto { path }) => {
            cli::run_debug_carto(path).await?;
        }
        Some(Commands::DebugSession { path }) => {
            cli::run_debug_session(path).await?;
        }
        Some(Commands::Proxy { action }) => match action {
            ProxyAction::Start { config, host, port, daemon } => {
                cli::run_proxy_start(config, host, port, daemon).await?;
            }
            ProxyAction::Stop => {
                cli::run_proxy_stop()?;
            }
            ProxyAction::Status => {
                cli::run_proxy_status()?;
            }
        }
        Some(Commands::Backend { action }) => match action {
            BackendAction::List => {
                cli::run_backend_list()?;
            }
            BackendAction::Use { name } => {
                cli::run_backend_use(&name).await?;
            }
            BackendAction::Test { name } => {
                cli::run_backend_test(&name).await?;
            }
            BackendAction::Env { name } => {
                cli::run_backend_env(name.as_deref())?;
            }
            BackendAction::Usage { backend, days } => {
                cli::run_backend_usage(backend.as_deref(), days).await?;
            }
        }
    }

    Ok(())
}
