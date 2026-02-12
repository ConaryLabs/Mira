// crates/mira-server/src/main.rs
// Mira - Memory and Intelligence Layer for AI Agents

mod cli;

use anyhow::Result;
use clap::Parser;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

use cli::{Cli, Commands, ConfigAction, HookAction};

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env from ~/.mira/.env only (never from CWD — a malicious repo could override API keys)
    if let Some(home) = dirs::home_dir()
        && let Err(e) = dotenvy::from_path(home.join(".mira/.env"))
    {
        tracing::debug!("Failed to load global .env file: {}", e);
    }

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
        Some(Commands::Config { .. }) => Level::WARN,
        Some(Commands::Setup { .. }) => Level::WARN,
        Some(Commands::StatusLine) => Level::WARN,
    };

    let subscriber = FmtSubscriber::builder()
        .with_max_level(log_level)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);

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
        Some(Commands::Hook { action }) => {
            // Hooks must NEVER exit with a non-zero code -- Claude Code
            // treats any non-zero exit as a "hook error".  Catch all errors
            // AND panics, log them to stderr, and emit `{}` on stdout so the
            // hook is silently ignored rather than flagged as broken.
            use std::io::Write;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                tokio::runtime::Handle::current().block_on(async {
                    match action {
                        HookAction::Permission => mira::hooks::permission::run().await,
                        HookAction::SessionStart => mira::hooks::session::run().await,
                        HookAction::PreCompact => mira::hooks::precompact::run().await,
                        HookAction::PreTool => mira::hooks::pre_tool::run().await,
                        HookAction::UserPrompt => mira::hooks::user_prompt::run().await,
                        HookAction::PostTool => mira::hooks::post_tool::run().await,
                        HookAction::Stop => mira::hooks::stop::run().await,
                        HookAction::SessionEnd => mira::hooks::stop::run_session_end().await,
                        HookAction::SubagentStart => mira::hooks::subagent::run_start().await,
                        HookAction::SubagentStop => mira::hooks::subagent::run_stop().await,
                    }
                })
            }));
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    eprintln!("[mira] Hook error (non-fatal): {e:#}");
                    let _ = writeln!(std::io::stdout(), "{{}}");
                }
                Err(_panic) => {
                    eprintln!("[mira] Hook panic (non-fatal)");
                    let _ = writeln!(std::io::stdout(), "{{}}");
                }
            }
        }
        Some(Commands::DebugCarto { path }) => {
            cli::run_debug_carto(path).await?;
        }
        Some(Commands::DebugSession { path }) => {
            cli::run_debug_session(path).await?;
        }
        Some(Commands::Config { action }) => match action {
            ConfigAction::Show => cli::config::run_config_show()?,
            ConfigAction::Set { key, value } => cli::config::run_config_set(&key, &value)?,
        },
        Some(Commands::Setup { check, yes }) => {
            cli::setup::run(check, yes).await?;
        }
        Some(Commands::StatusLine) => {
            cli::statusline::run()?;
        }
    }

    Ok(())
}
