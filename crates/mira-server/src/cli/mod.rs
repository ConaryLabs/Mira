// crates/mira-server/src/cli/mod.rs
// CLI module for Mira commands

use clap::{Parser, Subcommand};
use std::path::PathBuf;

pub mod backend;
pub mod clients;
pub mod debug;
pub mod index;
pub mod proxy;
pub mod serve;
pub mod tool;

// Re-export command handlers
pub use backend::*;
pub use debug::*;
pub use index::run_index;
pub use proxy::*;
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

    /// LLM proxy server management
    Proxy {
        #[command(subcommand)]
        action: ProxyAction,
    },

    /// Manage LLM backends
    Backend {
        #[command(subcommand)]
        action: BackendAction,
    },
}

#[derive(Subcommand)]
pub enum ProxyAction {
    /// Start the proxy server
    Start {
        /// Config file path (default: ~/.config/mira/proxy.toml)
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// Host to bind to (overrides config)
        #[arg(long)]
        host: Option<String>,

        /// Port to listen on (overrides config)
        #[arg(short, long)]
        port: Option<u16>,

        /// Run in background (daemon mode)
        #[arg(short, long)]
        daemon: bool,
    },

    /// Stop the running proxy server
    Stop,

    /// Check proxy server status
    Status,
}

#[derive(Subcommand)]
pub enum HookAction {
    /// Handle PermissionRequest hooks
    Permission,
    /// Handle SessionStart hooks - captures Claude's session_id
    SessionStart,
    /// Handle PreCompact hooks - preserve context before summarization
    PreCompact,
    /// Handle UserPromptSubmit hooks - inject proactive context
    UserPrompt,
    /// Handle PostToolUse hooks - track file changes, provide hints
    PostTool,
    /// Handle Stop hooks - check goals, save session state
    Stop,
    /// Legacy hooks (no-op for compatibility)
    #[command(hide = true)]
    Posttool,
    #[command(hide = true)]
    Pretool,
}

#[derive(Subcommand)]
pub enum BackendAction {
    /// List configured backends
    List,

    /// Set the default backend
    Use {
        /// Backend name to set as default
        name: String,
    },

    /// Test connectivity to a backend
    Test {
        /// Backend name to test
        name: String,
    },

    /// Print environment variables for a backend (shell export format)
    Env {
        /// Backend name (uses default if not specified)
        name: Option<String>,
    },

    /// Show usage statistics
    Usage {
        /// Filter by backend name
        #[arg(short, long)]
        backend: Option<String>,

        /// Number of days to show (default: 7)
        #[arg(short, long, default_value = "7")]
        days: u32,
    },
}

/// Get the default database path
pub fn get_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/mira.db")
}
