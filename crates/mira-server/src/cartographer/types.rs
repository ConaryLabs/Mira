// crates/mira-server/src/cartographer/types.rs
// Data types for codebase mapping

/// A logical module in the codebase
#[derive(Debug, Clone)]
pub struct Module {
    /// Unique identifier (e.g., "mcp/tools")
    pub id: String,
    /// Human-readable name (e.g., "tools")
    pub name: String,
    /// Directory path relative to project root
    pub path: String,
    /// Purpose summary (LLM-generated or heuristic)
    pub purpose: Option<String>,
    /// Key public exports
    pub exports: Vec<String>,
    /// Module IDs this depends on
    pub depends_on: Vec<String>,
    /// Symbol count
    pub symbol_count: u32,
    /// Line count
    pub line_count: u32,
}

/// Complete codebase map
#[derive(Debug, Clone)]
pub struct CodebaseMap {
    /// Project name
    pub name: String,
    /// Project type (rust/node/python/go)
    pub project_type: String,
    /// Detected modules
    pub modules: Vec<Module>,
    /// Entry points (main.rs, lib.rs, etc.)
    pub entry_points: Vec<String>,
    /// External dependencies
    pub external_deps: Vec<String>,
    /// When the map was last updated
    pub updated_at: String,
}

/// Context for a module to be summarized by LLM
#[derive(Debug, Clone)]
pub struct ModuleSummaryContext {
    pub module_id: String,
    pub name: String,
    pub path: String,
    pub exports: Vec<String>,
    pub code_preview: String,
    pub line_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // Module tests
    // ============================================================================

    #[test]
    fn test_module_clone() {
        let module = Module {
            id: "mira/db".to_string(),
            name: "db".to_string(),
            path: "crates/mira-server/src/db".to_string(),
            purpose: Some("Database operations".to_string()),
            exports: vec!["Database".to_string(), "Pool".to_string()],
            depends_on: vec!["mira/config".to_string()],
            symbol_count: 50,
            line_count: 1000,
        };
        let cloned = module.clone();
        assert_eq!(module.id, cloned.id);
        assert_eq!(module.name, cloned.name);
        assert_eq!(module.exports, cloned.exports);
        assert_eq!(module.depends_on, cloned.depends_on);
    }

    #[test]
    fn test_module_empty_collections() {
        let module = Module {
            id: "simple".to_string(),
            name: "simple".to_string(),
            path: "src/simple".to_string(),
            purpose: None,
            exports: vec![],
            depends_on: vec![],
            symbol_count: 0,
            line_count: 0,
        };
        assert!(module.exports.is_empty());
        assert!(module.depends_on.is_empty());
        assert!(module.purpose.is_none());
    }

    #[test]
    fn test_module_debug() {
        let module = Module {
            id: "test".to_string(),
            name: "test".to_string(),
            path: "src/test".to_string(),
            purpose: None,
            exports: vec![],
            depends_on: vec![],
            symbol_count: 10,
            line_count: 100,
        };
        let debug = format!("{:?}", module);
        assert!(debug.contains("test"));
        assert!(debug.contains("Module"));
    }

    // ============================================================================
    // CodebaseMap tests
    // ============================================================================

    #[test]
    fn test_codebase_map_clone() {
        let map = CodebaseMap {
            name: "my-project".to_string(),
            project_type: "rust".to_string(),
            modules: vec![Module {
                id: "mod1".to_string(),
                name: "mod1".to_string(),
                path: "src/mod1".to_string(),
                purpose: None,
                exports: vec![],
                depends_on: vec![],
                symbol_count: 0,
                line_count: 0,
            }],
            entry_points: vec!["src/main.rs".to_string()],
            external_deps: vec!["tokio".to_string(), "serde".to_string()],
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let cloned = map.clone();
        assert_eq!(map.name, cloned.name);
        assert_eq!(map.project_type, cloned.project_type);
        assert_eq!(map.modules.len(), cloned.modules.len());
        assert_eq!(map.entry_points, cloned.entry_points);
        assert_eq!(map.external_deps, cloned.external_deps);
    }

    #[test]
    fn test_codebase_map_empty() {
        let map = CodebaseMap {
            name: "empty".to_string(),
            project_type: "unknown".to_string(),
            modules: vec![],
            entry_points: vec![],
            external_deps: vec![],
            updated_at: "".to_string(),
        };
        assert!(map.modules.is_empty());
        assert!(map.entry_points.is_empty());
        assert!(map.external_deps.is_empty());
    }

    #[test]
    fn test_codebase_map_multiple_modules() {
        let map = CodebaseMap {
            name: "multi".to_string(),
            project_type: "rust".to_string(),
            modules: vec![
                Module {
                    id: "a".to_string(),
                    name: "a".to_string(),
                    path: "src/a".to_string(),
                    purpose: Some("Module A".to_string()),
                    exports: vec!["A".to_string()],
                    depends_on: vec![],
                    symbol_count: 10,
                    line_count: 100,
                },
                Module {
                    id: "b".to_string(),
                    name: "b".to_string(),
                    path: "src/b".to_string(),
                    purpose: Some("Module B".to_string()),
                    exports: vec!["B".to_string()],
                    depends_on: vec!["a".to_string()],
                    symbol_count: 20,
                    line_count: 200,
                },
            ],
            entry_points: vec!["src/main.rs".to_string(), "src/lib.rs".to_string()],
            external_deps: vec!["dep1".to_string()],
            updated_at: "2024-01-01".to_string(),
        };
        assert_eq!(map.modules.len(), 2);
        assert_eq!(map.entry_points.len(), 2);
        assert_eq!(map.modules[1].depends_on, vec!["a".to_string()]);
    }

    // ============================================================================
    // ModuleSummaryContext tests
    // ============================================================================

    #[test]
    fn test_module_summary_context_clone() {
        let ctx = ModuleSummaryContext {
            module_id: "mira/tools".to_string(),
            name: "tools".to_string(),
            path: "src/tools".to_string(),
            exports: vec!["Tool1".to_string(), "Tool2".to_string()],
            code_preview: "pub fn tool1() {}".to_string(),
            line_count: 500,
        };
        let cloned = ctx.clone();
        assert_eq!(ctx.module_id, cloned.module_id);
        assert_eq!(ctx.exports, cloned.exports);
        assert_eq!(ctx.code_preview, cloned.code_preview);
        assert_eq!(ctx.line_count, cloned.line_count);
    }

    #[test]
    fn test_module_summary_context_empty_preview() {
        let ctx = ModuleSummaryContext {
            module_id: "empty".to_string(),
            name: "empty".to_string(),
            path: "src/empty".to_string(),
            exports: vec![],
            code_preview: "".to_string(),
            line_count: 0,
        };
        assert!(ctx.code_preview.is_empty());
        assert!(ctx.exports.is_empty());
    }
}
