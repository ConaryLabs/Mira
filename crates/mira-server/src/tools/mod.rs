//! crates/mira-server/src/tools/mod.rs
//! Unified tool core for Mira MCP server

pub mod core;
pub mod mcp;

// Re-export public API for CLI dispatcher, tests, and external callers
pub use core::{
    // Trait & types
    ProjectInfo,
    ToolContext,
    // Tool handlers (used by CLI tool dispatcher and integration tests)
    analyze_diff_tool,
    documentation,
    ensure_session,
    find_function_callees,
    find_function_callers,
    get_project,
    get_project_info,
    get_session_recap,
    get_symbols,
    goal,
    handle_code,
    handle_session,
    handle_team,
    index,
    project,
    search_code,
    session_start,
    set_project,
    summarize_codebase,
    usage_list,
    usage_stats,
    usage_summary,
};
// Sub-module access for tasks (used by CLI and router)
pub use core::tasks;
