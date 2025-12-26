//! Tool definitions for Gemini 3 Pro (Orchestrator mode)
//!
//! Orchestrator mode: Studio doesn't write code, Claude Code does.
//! Tools here are read-only + management/intelligence.

use serde::Serialize;

use super::tool_defs;

/// Tool definition for function calling
#[derive(Debug, Clone, Serialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: serde_json::Value,
}

/// Get all tool definitions for Gemini 3 Pro (Orchestrator)
///
/// REMOVED (Claude Code handles via MCP):
/// - write_file, edit_file, bash, run_tests, git_commit
///
/// REMOVED (replaced by Gemini built-in tools):
/// - web_search, web_fetch -> google_search, code_execution, url_context
pub fn get_tools() -> Vec<Tool> {
    let mut tools = Vec::new();

    tools.extend(tool_defs::file_ops_tools());      // read_file, glob, grep (3)
    tools.extend(tool_defs::memory_tools());        // remember, recall (2)
    tools.extend(tool_defs::mira_tools());          // task, goal, correction, etc (5)
    tools.extend(tool_defs::git_tools());           // git_status, git_diff, git_log + intel (8)
    tools.extend(tool_defs::artifact_tools());      // fetch_artifact, search_artifact (2)
    tools.extend(tool_defs::council_tools());       // council, ask_* (5)
    tools.extend(tool_defs::intel_tools());         // code intel + build/doc/index/proactive (9)
    tools.extend(tool_defs::orchestration_tools()); // view_claude_activity, send_instruction, list_instructions, cancel_instruction (4)

    tools
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_get_tools() {
        let tools = get_tools();
        // 3 file_ops + 2 memory + 5 mira + 8 git + 2 artifact + 5 council + 9 intel + 4 orchestration = 38
        // (web_search/web_fetch removed - using Gemini's built-in tools)
        assert_eq!(tools.len(), 38, "Expected 38 tools, got {}", tools.len());

        // Collect tool names
        let names: HashSet<&str> = tools.iter().map(|t| t.name.as_str()).collect();

        // Verify read-only file tools exist
        assert!(names.contains("read_file"));
        assert!(names.contains("glob"));
        assert!(names.contains("grep"));

        // Verify removed tools are NOT present
        assert!(!names.contains("write_file"), "write_file should be removed");
        assert!(!names.contains("edit_file"), "edit_file should be removed");
        assert!(!names.contains("bash"), "bash should be removed");
        assert!(!names.contains("run_tests"), "run_tests should be removed");
        assert!(!names.contains("git_commit"), "git_commit should be removed");
        assert!(!names.contains("web_search"), "web_search replaced by Gemini's google_search");
        assert!(!names.contains("web_fetch"), "web_fetch replaced by Gemini's url_context");

        // Verify key tools exist
        assert!(names.contains("remember"));
        assert!(names.contains("recall"));
        assert!(names.contains("task"));
        assert!(names.contains("goal"));
        assert!(names.contains("correction"));
        assert!(names.contains("git_status"));
        assert!(names.contains("git_diff"));
        assert!(names.contains("git_log"));
        assert!(names.contains("council"));
        assert!(names.contains("ask_gpt"));
        assert!(names.contains("ask_deepseek"));
        assert!(names.contains("get_symbols"));
        assert!(names.contains("get_call_graph"));
        assert!(names.contains("fetch_artifact"));
        assert!(names.contains("search_artifact"));

        // Verify orchestration tools exist
        assert!(names.contains("view_claude_activity"));
        assert!(names.contains("send_instruction"));
        assert!(names.contains("list_instructions"));
        assert!(names.contains("cancel_instruction"));
    }
}
