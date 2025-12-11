// src/operations/engine/tool_router/registry.rs
// Tool Registry - Maps tool names to handlers using a table-driven approach

use std::collections::HashMap;

/// Handler type for routing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlerType {
    Git,
    Code,
    External,
    Mcp,
}

/// Route configuration for a tool
#[derive(Debug, Clone)]
pub struct ToolRoute {
    pub handler_type: HandlerType,
    pub internal_name: String,
}

/// Registry of tool routes
pub struct ToolRegistry {
    routes: HashMap<String, ToolRoute>,
}

impl ToolRegistry {
    /// Create a new tool registry with all routes configured
    pub fn new() -> Self {
        let mut registry = Self {
            routes: HashMap::new(),
        };

        // Register all tool routes
        registry.register_git_tools();
        registry.register_code_tools();
        registry.register_external_tools();

        registry
    }

    /// Get the route for a tool
    pub fn get_route(&self, tool_name: &str) -> Option<&ToolRoute> {
        self.routes.get(tool_name)
    }

    /// Register a tool route
    fn register(&mut self, tool_name: &str, handler_type: HandlerType, internal_name: &str) {
        self.routes.insert(
            tool_name.to_string(),
            ToolRoute {
                handler_type,
                internal_name: internal_name.to_string(),
            },
        );
    }

    /// Register git tools (10 tools)
    fn register_git_tools(&mut self) {
        let git_tools = [
            ("git_history", "git_history_internal"),
            ("git_blame", "git_blame_internal"),
            ("git_diff", "git_diff_internal"),
            ("git_file_history", "git_file_history_internal"),
            ("git_branches", "git_branches_internal"),
            ("git_show_commit", "git_show_commit_internal"),
            ("git_file_at_commit", "git_file_at_commit_internal"),
            ("git_recent_changes", "git_recent_changes_internal"),
            ("git_contributors", "git_contributors_internal"),
            ("git_status", "git_status_internal"),
        ];

        for (tool_name, internal_name) in git_tools {
            self.register(tool_name, HandlerType::Git, internal_name);
        }
    }

    /// Register code intelligence tools (12 tools)
    fn register_code_tools(&mut self) {
        let code_tools = [
            ("find_function", "find_function_internal"),
            ("find_class_or_struct", "find_class_or_struct_internal"),
            ("search_code_semantic", "search_code_semantic_internal"),
            ("find_imports", "find_imports_internal"),
            ("analyze_dependencies", "analyze_dependencies_internal"),
            ("get_complexity_hotspots", "get_complexity_hotspots_internal"),
            ("get_quality_issues", "get_quality_issues_internal"),
            ("get_file_symbols", "get_file_symbols_internal"),
            ("find_tests_for_code", "find_tests_for_code_internal"),
            ("get_codebase_stats", "get_codebase_stats_internal"),
            ("find_callers", "find_callers_internal"),
            ("get_element_definition", "get_element_definition_internal"),
        ];

        for (tool_name, internal_name) in code_tools {
            self.register(tool_name, HandlerType::Code, internal_name);
        }
    }

    /// Register external tools (4 tools)
    fn register_external_tools(&mut self) {
        let external_tools = [
            ("web_search", "web_search_internal"),
            ("fetch_url", "fetch_url_internal"),
            ("execute_command", "execute_command_internal"),
            ("run_tests", "run_tests_internal"),
        ];

        for (tool_name, internal_name) in external_tools {
            self.register(tool_name, HandlerType::External, internal_name);
        }
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
