// src/operations/tools/skills.rs
// Specialized skill activation tool

use serde_json::Value;
use crate::operations::tool_builder::{ToolBuilder, properties};
use super::common;

/// Get skills tool schema
pub fn get_tools() -> Vec<Value> {
    vec![activate_skill_tool()]
}

/// Tool: activate_skill
/// Activates a specialized skill for complex tasks
pub fn activate_skill_tool() -> Value {
    ToolBuilder::new(
        "activate_skill",
        "Activate a specialized skill for complex tasks like refactoring, testing, debugging, or documentation. Skills provide expert guidance, best practices, and restrict available tools to what's relevant for the task."
    )
    .property("skill_name", common::skill_name_enum(), true)
    .property(
        "task_description",
        properties::description("Detailed description of what you want to accomplish with this skill"),
        true
    )
    .property(
        "context",
        properties::optional_string("Additional context about the code, project, or requirements that the skill should know about"),
        false
    )
    .build()
}
