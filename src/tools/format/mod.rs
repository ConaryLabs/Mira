// src/tools/format/mod.rs
// Human-readable formatters for MCP tool responses
// Makes output clean and concise like native Claude Code tools

mod memory;
mod entities;
mod code;
mod admin;
mod sessions;
mod proactive;

#[cfg(test)]
mod tests;

// Re-export all formatters for easy access
pub use memory::{remember, recall_results, forgotten};
pub use entities::{
    task_list, task_action,
    goal_list, goal_action,
    correction_recorded, correction_list,
    permission_saved, permission_list, permission_deleted,
};
pub use code::{
    index_status, code_search_results, symbols_list,
    commit_list, related_files, call_graph,
};
pub use admin::{project_set, table_list, query_results, build_errors, guidelines};
pub use sessions::{session_stored, session_results, session_start, session_context};
pub use proactive::{proactive_context, work_context};
