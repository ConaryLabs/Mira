//! crates/mira-server/src/tools/mod.rs
//! Unified tool core for Mira MCP server

pub mod core;
pub mod mcp;

// Re-export public API for CLI dispatcher, tests, and external callers
pub use core::{
    // Trait & types
    PendingResponseMap, ProjectInfo, ToolContext, get_project_info,
    // Tool handlers (used by CLI tool dispatcher and integration tests)
    analyze_diff_tool, configure_expert, documentation, ensure_session, finding,
    find_function_callees, find_function_callers, forget, get_project, get_session_recap,
    get_symbols, goal, handle_code, handle_expert, handle_memory, handle_session, index,
    project, recall, remember, reply_to_mira, search_code, session_history, session_start,
    set_project, summarize_codebase, usage,
};
// Sub-module access for tasks (used by CLI and router)
pub use core::tasks;
