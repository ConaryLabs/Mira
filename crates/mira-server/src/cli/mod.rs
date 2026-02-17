// crates/mira-server/src/cli/mod.rs
// CLI module for Mira commands

use clap::{Parser, Subcommand};
use std::path::PathBuf;

pub mod cleanup;
pub mod clients;
pub mod config;
pub mod debug;
pub mod index;
pub mod serve;
pub mod setup;
pub mod statusline;
pub mod tool;

// Re-export command handlers
pub use cleanup::run_cleanup;
pub use debug::*;
pub use index::run_index;
pub use serve::run_mcp_server;
pub use tool::run_tool;

#[derive(Parser)]
#[command(name = "mira")]
#[command(about = "Memory and Intelligence Layer for AI Agents")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run as MCP server (default)
    Serve,

    /// Execute a tool directly
    Tool {
        /// Tool name (e.g. memory, code, goal, session)
        #[arg(index = 1)]
        name: String,

        /// JSON arguments (e.g. '{"query": "foo"}')
        #[arg(index = 2)]
        args: String,
    },

    /// Index a project
    Index {
        /// Project path (default: current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Skip embeddings (faster, no semantic search)
        #[arg(long)]
        no_embed: bool,

        /// Suppress verbose output (show only summary)
        #[arg(short, long)]
        quiet: bool,
    },

    /// Client hook handlers
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

    /// View or update provider configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Interactive setup wizard for API keys and providers
    Setup {
        /// Read-only validation mode: show current config status without modifying
        #[arg(long)]
        check: bool,
        /// Non-interactive mode: auto-detect Ollama, skip API key prompts, use defaults
        #[arg(long, alias = "non-interactive")]
        yes: bool,
    },

    /// Run data cleanup and retention (dry-run by default)
    Cleanup {
        /// Actually execute the cleanup (default is dry-run preview)
        #[arg(long)]
        execute: bool,

        /// Skip confirmation prompt
        #[arg(long, short)]
        yes: bool,

        /// Filter by category: sessions, analytics, chat, behavior, or all (default)
        #[arg(long)]
        category: Option<String>,
    },

    /// Output status line for Claude Code (reads stdin, prints stats to stdout)
    #[command(name = "statusline")]
    StatusLine,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Show current provider configuration
    Show,
    /// Set a config value (e.g. `mira config set background_provider deepseek`)
    Set {
        /// Config key (background_provider, default_provider)
        #[arg(index = 1)]
        key: String,
        /// Value to set (deepseek, ollama)
        #[arg(index = 2)]
        value: String,
    },
}

#[derive(Subcommand)]
pub enum HookAction {
    /// Handle PermissionRequest hooks
    Permission,
    /// Handle SessionStart hooks - captures Claude's session_id
    SessionStart,
    /// Handle PreCompact hooks - preserve context before summarization
    PreCompact,
    /// Handle PreToolUse hooks - inject context before Grep/Glob searches
    PreTool,
    /// Handle UserPromptSubmit hooks - inject proactive context
    UserPrompt,
    /// Handle PostToolUse hooks - track file changes, provide hints
    PostTool,
    /// Handle Stop hooks - check goals, save session state
    Stop,
    /// Handle SessionEnd hooks - snapshot tasks on user interrupt
    SessionEnd,
    /// Handle SubagentStart hooks - inject context when subagents spawn
    SubagentStart,
    /// Handle SubagentStop hooks - capture discoveries from subagent work
    SubagentStop,
    /// Handle PostToolUseFailure hooks - track and learn from tool failures
    PostToolFailure,
    /// Handle TaskCompleted hooks - auto-link tasks to goals
    TaskCompleted,
    /// Handle TeammateIdle hooks - check teammate status
    TeammateIdle,
}

/// Get the default database path
pub fn get_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/mira.db")
}
