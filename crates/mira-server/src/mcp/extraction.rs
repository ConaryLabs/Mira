// crates/mira-server/src/mcp/extraction.rs
// Extract meaningful outcomes from tool calls and store as project memories
//
// NOTE: This module previously used LLM for extraction. Since the LLM dependency
// has been removed, spawn_tool_extraction is now a no-op. The extraction logic
// is preserved in case a local computation replacement is added later.

use std::sync::Arc;
use tracing::debug;

use crate::db::pool::DatabasePool;

/// Tools that produce outcomes worth remembering
const EXTRACTABLE_TOOLS: &[&str] = &[
    "task", // Task completions and updates
    "goal", // Goal progress and milestones
    "code", // Code discoveries and call graph insights
    "diff", // Diff analysis insights
];

/// Spawn background extraction for a tool call.
/// Currently a no-op since extraction requires an LLM client which has been removed.
pub fn spawn_tool_extraction(
    _pool: Arc<DatabasePool>,
    _embeddings: Option<Arc<crate::embeddings::EmbeddingClient>>,
    _project_id: Option<i64>,
    tool_name: String,
    _args: String,
    _result: String,
) {
    // Only log for extractable tools to avoid noise
    if EXTRACTABLE_TOOLS.contains(&tool_name.as_str()) {
        debug!(
            "Tool extraction: skipping {} (no LLM provider -- extraction disabled)",
            tool_name
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // EXTRACTABLE_TOOLS tests
    // ============================================================================

    #[test]
    fn test_extractable_tools_contains_task() {
        assert!(EXTRACTABLE_TOOLS.contains(&"task"));
    }

    #[test]
    fn test_extractable_tools_contains_goal() {
        assert!(EXTRACTABLE_TOOLS.contains(&"goal"));
    }

    #[test]
    fn test_extractable_tools_contains_code() {
        assert!(EXTRACTABLE_TOOLS.contains(&"code"));
    }

    #[test]
    fn test_extractable_tools_contains_diff() {
        assert!(EXTRACTABLE_TOOLS.contains(&"diff"));
    }

    #[test]
    fn test_extractable_tools_excludes_common() {
        // These tools should NOT be extractable
        assert!(!EXTRACTABLE_TOOLS.contains(&"memory"));
        assert!(!EXTRACTABLE_TOOLS.contains(&"index"));
        assert!(!EXTRACTABLE_TOOLS.contains(&"session"));
    }
}
