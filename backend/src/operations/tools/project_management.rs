// src/operations/tools/project_management.rs
// Project task and guidelines management tools

use serde_json::Value;
use crate::operations::tool_builder::{ToolBuilder, properties};
use super::common;

/// Get project management tool schemas
pub fn get_tools() -> Vec<Value> {
    vec![
        manage_project_task_tool(),
        manage_project_guidelines_tool(),
    ]
}

/// Tool: manage_project_task
/// Create, update, or complete persistent project tasks
pub fn manage_project_task_tool() -> Value {
    ToolBuilder::new(
        "manage_project_task",
        "Create, update, or complete persistent project tasks. Tasks persist across sessions and track your progress on work items. Use this when:
        - User requests new work (feature, fix, improvement) -> create a task
        - You're continuing work on an existing task -> update with progress notes
        - Work is finished -> complete the task with a summary

        Tasks automatically link to artifacts and commits you produce."
    )
    .property("action", common::task_action_enum(), true)
    .property(
        "task_id",
        serde_json::json!({
            "type": "integer",
            "description": "Task ID (required for update/complete actions)"
        }),
        false
    )
    .property(
        "title",
        properties::description("Task title - short description of what needs to be done (required for create)"),
        false
    )
    .property(
        "description",
        properties::optional_string("Detailed description of the task or progress update"),
        false
    )
    .property("priority", common::priority_enum(), false)
    .property(
        "progress_notes",
        properties::optional_string("Progress update or completion summary (for update/complete)"),
        false
    )
    .property(
        "tags",
        properties::string_array("Tags for categorization (e.g., ['feature', 'frontend'])"),
        false
    )
    .build()
}

/// Tool: manage_project_guidelines
/// Create, view, or update project guidelines that persist across sessions
pub fn manage_project_guidelines_tool() -> Value {
    ToolBuilder::new(
        "manage_project_guidelines",
        "Create or update project guidelines that persist across sessions. Guidelines are automatically included in every conversation about this project. Use this when:
        - User asks to initialize or setup project context (like 'claude init')
        - User wants to document coding standards, preferences, or architecture
        - User asks to view or update existing guidelines

        Guidelines help maintain consistency across conversations and provide context about the project."
    )
    .property("action", common::guidelines_action_enum(), true)
    .property(
        "content",
        serde_json::json!({
            "type": "string",
            "description": "Guidelines content in markdown format (required for set/append)"
        }),
        false
    )
    .property(
        "section",
        serde_json::json!({
            "type": "string",
            "description": "Section heading to add (for append action, creates a new ## section)"
        }),
        false
    )
    .build()
}
