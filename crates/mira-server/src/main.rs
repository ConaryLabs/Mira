// crates/mira-server/src/main.rs
// Mira - Memory and Intelligence Layer for AI Agents

mod cli;

use anyhow::Result;
use clap::Parser;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

use cli::{Cli, Commands, ConfigAction, HookAction, analyze};

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
    match &cli.command {
        Some(Commands::Hook { .. }) => {
            // Hooks: configurable via MIRA_HOOK_LOG_LEVEL (default: warn)
            // No timestamps or module targets — hooks are ephemeral processes
            let hook_level =
                std::env::var("MIRA_HOOK_LOG_LEVEL").unwrap_or_else(|_| "warn".to_string());
            let level = match hook_level.to_lowercase().as_str() {
                "off" => None,
                "error" => Some(Level::ERROR),
                "warn" => Some(Level::WARN),
                "info" => Some(Level::INFO),
                "debug" | "trace" => Some(Level::DEBUG),
                other => {
                    eprintln!("[mira] Unknown MIRA_HOOK_LOG_LEVEL={other:?}, using warn");
                    Some(Level::WARN)
                }
            };
            if let Some(level) = level {
                let subscriber = FmtSubscriber::builder()
                    .with_max_level(level)
                    .with_writer(std::io::stderr)
                    .with_ansi(false)
                    .without_time()
                    .with_target(false)
                    .finish();
                let _ = tracing::subscriber::set_global_default(subscriber);
            }
        }
        command => {
            let log_level = match command {
                Some(Commands::Serve) | None => Level::WARN, // Quiet for MCP stdio
                Some(Commands::Tool { .. }) => Level::WARN,
                Some(Commands::Index { quiet, .. }) if *quiet => Level::WARN,
                Some(Commands::Index { .. }) => Level::INFO,
                Some(Commands::DebugCarto { .. }) => Level::DEBUG,
                Some(Commands::DebugSession { .. }) => Level::DEBUG,
                Some(Commands::Config { .. }) => Level::WARN,
                Some(Commands::Setup { .. }) => Level::WARN,
                Some(Commands::Cleanup { .. }) => Level::INFO,
                Some(Commands::StatusLine) => Level::WARN,
                Some(Commands::AnalyzeSession { .. }) => Level::WARN,
                _ => Level::WARN,
            };
            let subscriber = FmtSubscriber::builder()
                .with_max_level(log_level)
                .with_writer(std::io::stderr)
                .with_ansi(false)
                .finish();
            let _ = tracing::subscriber::set_global_default(subscriber);
        }
    }

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
            //
            // We use tokio::task::spawn to run the async hook work on a
            // separate task. This avoids the "Cannot start a runtime from
            // within a runtime" panic that Handle::block_on causes inside
            // #[tokio::main], and JoinError captures any panics for us.
            use std::io::Write;
            let hook_name = action.to_string();
            let start = std::time::Instant::now();
            let result = tokio::task::spawn(async move {
                match action {
                    HookAction::SessionStart => mira::hooks::session::run().await,
                    HookAction::PreCompact => mira::hooks::precompact::run().await,
                    HookAction::PreTool => mira::hooks::pre_tool::run().await,
                    HookAction::UserPrompt => mira::hooks::user_prompt::run().await,
                    HookAction::PostTool => mira::hooks::post_tool::run().await,
                    HookAction::Stop => mira::hooks::stop::run().await,
                    HookAction::SessionEnd => mira::hooks::stop::run_session_end().await,
                    HookAction::SubagentStart => mira::hooks::subagent::run_start().await,
                    HookAction::SubagentStop => mira::hooks::subagent::run_stop().await,
                    HookAction::PostToolFailure => mira::hooks::post_tool_failure::run().await,
                    HookAction::TaskCompleted => mira::hooks::task_completed::run().await,
                    HookAction::TeammateIdle => mira::hooks::teammate_idle::run().await,
                }
            })
            .await;
            let latency_ms = start.elapsed().as_millis();
            match result {
                Ok(Ok(())) => {
                    mira::hooks::record_hook_outcome(&hook_name, true, latency_ms, None).await;
                }
                Ok(Err(e)) => {
                    let msg = format!("{e:#}");
                    mira::hooks::record_hook_outcome(&hook_name, false, latency_ms, Some(&msg))
                        .await;
                    eprintln!("[mira] Hook error (non-fatal): {msg}");
                    let _ = writeln!(std::io::stdout(), "{{}}");
                }
                Err(join_err) => {
                    let msg = format!("{join_err}");
                    mira::hooks::record_hook_outcome(&hook_name, false, latency_ms, Some(&msg))
                        .await;
                    eprintln!("[mira] Hook panic (non-fatal): {msg}");
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
        Some(Commands::Cleanup {
            execute,
            yes,
            category,
        }) => {
            cli::run_cleanup(!execute, yes, category).await?;
        }
        Some(Commands::StatusLine) => {
            cli::statusline::run()?;
        }
        Some(Commands::AnalyzeSession {
            session,
            turns,
            tools,
            correlate,
        }) => {
            analyze::run(session, turns, tools, correlate)?;
        }
    }

    Ok(())
}
