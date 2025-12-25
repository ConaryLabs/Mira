//! Tool definitions for DeepSeek function calling

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

/// Get all tool definitions for GPT-5.2
pub fn get_tools() -> Vec<Tool> {
    let mut tools = Vec::new();

    tools.extend(tool_defs::file_ops_tools());
    tools.extend(tool_defs::web_tools());
    tools.extend(tool_defs::memory_tools());
    tools.extend(tool_defs::mira_tools());
    tools.extend(tool_defs::git_tools());
    tools.extend(tool_defs::testing_tools());
    tools.extend(tool_defs::council_tools());
    tools.extend(tool_defs::intel_tools());

    tools
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_get_tools() {
        let tools = get_tools();
        // 6 file_ops + 2 web + 2 memory + 5 mira + 9 git + 3 testing + 4 council + 9 intel = 40
        assert_eq!(tools.len(), 40);

        // Collect tool names
        let names: HashSet<&str> = tools.iter().map(|t| t.name.as_str()).collect();

        // Verify key tools exist from each group
        assert!(names.contains("read_file"));
        assert!(names.contains("edit_file"));
        assert!(names.contains("web_search"));
        assert!(names.contains("remember"));
        assert!(names.contains("recall"));
        assert!(names.contains("task"));
        assert!(names.contains("goal"));
        assert!(names.contains("correction"));
        assert!(names.contains("git_status"));
        assert!(names.contains("git_diff"));
        assert!(names.contains("run_tests"));
        assert!(names.contains("council"));
        assert!(names.contains("ask_gpt"));
        assert!(names.contains("get_symbols"));
        assert!(names.contains("get_call_graph"));
        assert!(names.contains("get_recent_commits"));
        assert!(names.contains("build"));
        assert!(names.contains("document"));
        assert!(names.contains("index"));
        assert!(names.contains("get_proactive_context"));
    }
}
