// src/context_oracle_stubs.rs
// Context oracle stubs for power suit mode
// Claude Code handles context gathering; these are minimal for compatibility

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Context configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextConfig {
    pub max_tokens: usize,
    pub include_memory: bool,
    pub include_code: bool,
    pub include_git: bool,
}

impl ContextConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_memory(mut self, include: bool) -> Self {
        self.include_memory = include;
        self
    }

    pub fn with_code(mut self, include: bool) -> Self {
        self.include_code = include;
        self
    }

    pub fn with_git(mut self, include: bool) -> Self {
        self.include_git = include;
        self
    }
}

/// Gathered context from various sources
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GatheredContext {
    pub memories: Vec<String>,
    pub code_context: Vec<String>,
    pub git_context: Vec<String>,
    pub metadata: HashMap<String, String>,
    pub sources_used: Vec<String>,
    pub estimated_tokens: usize,
}

impl GatheredContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.memories.is_empty() && self.code_context.is_empty() && self.git_context.is_empty()
    }

    pub fn with_memories(mut self, memories: Vec<String>) -> Self {
        self.memories = memories;
        self
    }

    pub fn as_context_string(&self) -> String {
        let mut parts = Vec::new();
        if !self.memories.is_empty() {
            parts.push(format!("Memories:\n{}", self.memories.join("\n")));
        }
        if !self.code_context.is_empty() {
            parts.push(format!("Code:\n{}", self.code_context.join("\n")));
        }
        if !self.git_context.is_empty() {
            parts.push(format!("Git:\n{}", self.git_context.join("\n")));
        }
        parts.join("\n\n")
    }
}

/// Context request parameters
#[derive(Debug, Clone, Default)]
pub struct ContextRequest {
    pub query: String,
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    pub file_path: Option<String>,
    pub error_context: Option<String>,
    pub config: ContextConfig,
}

impl ContextRequest {
    pub fn new(query: String, session_id: String) -> Self {
        Self {
            query,
            session_id: Some(session_id),
            project_id: None,
            file_path: None,
            error_context: None,
            config: ContextConfig::default(),
        }
    }

    pub fn with_config(mut self, config: ContextConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_query(mut self, query: String) -> Self {
        self.query = query;
        self
    }

    pub fn with_project(mut self, project_id: &str) -> Self {
        self.project_id = Some(project_id.to_string());
        self
    }

    pub fn with_file(mut self, file_path: &str) -> Self {
        self.file_path = Some(file_path.to_string());
        self
    }

    pub fn with_error(mut self, error: &str, _code: Option<&str>) -> Self {
        self.error_context = Some(error.to_string());
        self
    }
}

/// Context oracle - gathers context from multiple sources
/// In power suit mode, this is a minimal implementation
pub struct ContextOracle;

impl ContextOracle {
    pub fn new() -> Self {
        Self
    }

    /// Gather context (stub - returns empty context in power suit mode)
    pub async fn gather(&self, _request: &ContextRequest) -> anyhow::Result<GatheredContext> {
        Ok(GatheredContext::default())
    }
}

impl Default for ContextOracle {
    fn default() -> Self {
        Self::new()
    }
}
