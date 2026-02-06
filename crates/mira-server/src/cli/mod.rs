// crates/mira-server/src/cli/mod.rs
// CLI module for Mira commands

use clap::{Parser, Subcommand};
use std::path::PathBuf;

pub mod clients;
pub mod debug;
pub mod index;
pub mod serve;
pub mod setup;
pub mod tool;

// Re-export command handlers
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
        /// Tool name (e.g. search_code, remember)
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

    /// Interactive setup wizard for API keys and providers
    Setup {
        /// Read-only validation mode: show current config status without modifying
        #[arg(long)]
        check: bool,
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
}

/// Get the default database path
pub fn get_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/mira.db")
}
