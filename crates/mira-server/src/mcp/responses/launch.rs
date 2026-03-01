// crates/mira-server/src/mcp/responses/launch.rs
// Response types for the launch (context-aware team launcher) MCP tool.

use schemars::JsonSchema;
use serde::Serialize;

use super::ToolOutput;

pub type LaunchOutput = ToolOutput<LaunchData>;

#[derive(Debug, Serialize, JsonSchema)]
pub struct LaunchData {
    /// Team name from frontmatter (e.g. "expert-review-team")
    pub team_name: String,
    /// Team description from frontmatter
    pub team_description: String,
    /// Parsed and enriched agent definitions, ready to spawn
    pub agents: Vec<AgentSpec>,
    /// Shared project context block (project type, goals, code bundle)
    pub project_context: String,
    /// Suggested team name for TeamCreate (timestamped)
    pub suggested_team_id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AgentSpec {
    /// Agent name (lowercase first name, e.g. "nadia")
    pub name: String,
    /// Role title (e.g. "Systems Architect")
    pub role: String,
    /// Whether this agent is read-only (no file edits)
    pub read_only: bool,
    /// Suggested model ("sonnet" for read-only, empty for default)
    #[serde(skip_serializing_if = "String::is_empty")]
    pub model: String,
    /// Pre-assembled prompt: persona + focus + project context
    pub prompt: String,
    /// Suggested task subject for TaskCreate
    pub task_subject: String,
    /// Suggested task description for TaskCreate
    pub task_description: String,
}
