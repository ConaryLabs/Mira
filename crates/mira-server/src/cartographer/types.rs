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
    /// Detected architectural patterns (JSON, from code health scan)
    pub detected_patterns: Option<String>,
}

impl Module {
    /// Create a new Module with default fields (empty exports, deps, zero counts).
    pub fn new(id: impl Into<String>, name: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            path: path.into(),
            purpose: None,
            exports: vec![],
            depends_on: vec![],
            symbol_count: 0,
            line_count: 0,
            detected_patterns: None,
        }
    }
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
