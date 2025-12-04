// backend/src/agents/types.rs
// Core types for the agent system

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

/// Type of agent execution
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentType {
    /// Built-in agent runs in-process via tokio
    Builtin,
    /// Custom agent runs as subprocess (like MCP)
    Subprocess,
}

/// Agent scope - where it was loaded from
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentScope {
    /// Built-in agent (hardcoded)
    Builtin,
    /// User-global from ~/.mira/agents/
    User,
    /// Project-specific from .mira/agents/
    Project,
}

/// Tool access level for agents
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolAccess {
    /// Read-only tools (file read, search, git log, etc.)
    ReadOnly,
    /// All tools including file writes and command execution
    Full,
    /// Custom set of allowed tools
    Custom(HashSet<String>),
}

/// Read-only tools that don't modify state
pub const READ_ONLY_TOOLS: &[&str] = &[
    // File reading
    "read_project_file",
    "list_project_files",
    "search_codebase",
    "get_file_summary",
    "get_file_structure",
    // Git analysis (read-only)
    "git_history",
    "git_blame",
    "git_diff",
    "git_file_history",
    "git_branches",
    "git_show_commit",
    "git_file_at_commit",
    "git_recent_changes",
    "git_contributors",
    "git_status",
    // Code intelligence
    "find_function",
    "find_class_or_struct",
    "search_code_semantic",
    "find_imports",
    "analyze_dependencies",
    "get_complexity_hotspots",
    "get_quality_issues",
    "get_file_symbols",
    "find_tests_for_code",
    "get_codebase_stats",
    "find_callers",
    "get_element_definition",
    // External (read-only)
    "web_search",
    "fetch_url",
];

impl ToolAccess {
    /// Check if a tool is allowed
    pub fn is_allowed(&self, tool_name: &str) -> bool {
        match self {
            ToolAccess::Full => true,
            ToolAccess::ReadOnly => READ_ONLY_TOOLS.contains(&tool_name),
            ToolAccess::Custom(tools) => tools.contains(tool_name),
        }
    }

    /// Get the list of allowed tools for display
    pub fn allowed_tools(&self) -> Vec<&str> {
        match self {
            ToolAccess::Full => vec!["*"],
            ToolAccess::ReadOnly => READ_ONLY_TOOLS.to_vec(),
            ToolAccess::Custom(tools) => tools.iter().map(|s| s.as_str()).collect(),
        }
    }
}

impl Default for ToolAccess {
    fn default() -> Self {
        ToolAccess::Full
    }
}

/// Thinking level preference for the agent
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingLevelPreference {
    /// Low thinking - faster, cheaper
    Low,
    /// High thinking - deeper reasoning
    #[default]
    High,
    /// Let the executor decide based on task
    Adaptive,
}

/// Definition of an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    /// Unique identifier for the agent
    pub id: String,

    /// Human-readable name
    pub name: String,

    /// Description for LLM matching (critical for auto-delegation)
    pub description: String,

    /// Execution type
    pub agent_type: AgentType,

    /// Where it was loaded from
    pub scope: AgentScope,

    /// Tool access level
    pub tool_access: ToolAccess,

    /// System prompt injected for this agent
    pub system_prompt: String,

    /// For subprocess agents: command to spawn
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// For subprocess agents: command arguments
    #[serde(default)]
    pub args: Vec<String>,

    /// Environment variables to set
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,

    /// Timeout in milliseconds (default: 300000 = 5 minutes)
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,

    /// Maximum number of tool call iterations
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,

    /// Whether this agent can spawn sub-agents (only general can)
    #[serde(default)]
    pub can_spawn_agents: bool,

    /// Thinking level preference for Gemini
    #[serde(default)]
    pub thinking_level: ThinkingLevelPreference,
}

fn default_timeout() -> u64 {
    300000
} // 5 minutes
fn default_max_iterations() -> u32 {
    25
}

/// Configuration for spawning an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// The agent to spawn
    pub agent_id: String,

    /// Task description
    pub task: String,

    /// Additional context to include
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,

    /// Files to include in context
    #[serde(default)]
    pub context_files: Vec<String>,

    /// Parent operation ID for tracking
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_operation_id: Option<String>,

    /// Session ID for tool context
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// Project ID for tool context
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,

    /// Parent thought signature for reasoning continuity
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

/// Result from agent execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    /// Agent that was executed
    pub agent_id: String,

    /// Whether execution succeeded
    pub success: bool,

    /// Final response content
    pub response: String,

    /// Thought signature from the agent (for continuity)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,

    /// Artifacts produced
    #[serde(default)]
    pub artifacts: Vec<AgentArtifact>,

    /// Tools called during execution
    #[serde(default)]
    pub tool_calls: Vec<AgentToolCall>,

    /// Token usage - input
    pub tokens_input: i64,

    /// Token usage - output
    pub tokens_output: i64,

    /// Execution time in ms
    pub execution_time_ms: u64,

    /// Error message if failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Artifact produced by an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentArtifact {
    pub path: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub action: ArtifactAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactAction {
    Create,
    Modify,
    Delete,
}

/// Record of a tool call made by the agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolCall {
    pub tool_name: String,
    pub arguments: Value,
    pub result: Value,
    pub success: bool,
    pub duration_ms: u64,
}

/// Configuration file format for custom agents
/// Loaded from ~/.mira/agents.json or .mira/agents.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentsConfig {
    #[serde(default)]
    pub agents: Vec<CustomAgentConfig>,
}

/// Configuration for a custom subprocess agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomAgentConfig {
    pub id: String,
    pub name: String,
    pub description: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
    #[serde(default)]
    pub tool_access: ToolAccessConfig,
    #[serde(default)]
    pub thinking_level: ThinkingLevelPreference,
}

/// Tool access configuration for custom agents
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolAccessConfig {
    ReadOnly,
    #[default]
    Full,
    Custom {
        tools: Vec<String>,
    },
}

impl From<ToolAccessConfig> for ToolAccess {
    fn from(config: ToolAccessConfig) -> Self {
        match config {
            ToolAccessConfig::ReadOnly => ToolAccess::ReadOnly,
            ToolAccessConfig::Full => ToolAccess::Full,
            ToolAccessConfig::Custom { tools } => ToolAccess::Custom(tools.into_iter().collect()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_access_read_only() {
        let access = ToolAccess::ReadOnly;
        assert!(access.is_allowed("read_project_file"));
        assert!(access.is_allowed("git_history"));
        assert!(!access.is_allowed("write_project_file"));
        assert!(!access.is_allowed("execute_command"));
    }

    #[test]
    fn test_tool_access_full() {
        let access = ToolAccess::Full;
        assert!(access.is_allowed("read_project_file"));
        assert!(access.is_allowed("write_project_file"));
        assert!(access.is_allowed("execute_command"));
        assert!(access.is_allowed("any_tool"));
    }

    #[test]
    fn test_tool_access_custom() {
        let mut tools = HashSet::new();
        tools.insert("read_project_file".to_string());
        tools.insert("custom_tool".to_string());

        let access = ToolAccess::Custom(tools);
        assert!(access.is_allowed("read_project_file"));
        assert!(access.is_allowed("custom_tool"));
        assert!(!access.is_allowed("write_project_file"));
    }

    #[test]
    fn test_agents_config_parse() {
        let json = r#"{
            "agents": [
                {
                    "id": "test-agent",
                    "name": "Test Agent",
                    "description": "A test agent",
                    "command": "python",
                    "args": ["-m", "test_agent"],
                    "tool_access": "read_only"
                }
            ]
        }"#;

        let config: AgentsConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.agents.len(), 1);
        assert_eq!(config.agents[0].id, "test-agent");
    }
}
