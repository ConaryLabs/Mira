//! Mira Power Armor tool definitions

use serde_json::json;
use super::super::definitions::Tool;

pub fn mira_tools() -> Vec<Tool> {
    vec![
        Tool {
            tool_type: "function".into(),
            name: "task".into(),
            description: Some("Manage persistent tasks. Actions: create, list, update, complete, delete.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "Action: create/list/update/complete/delete"
                    },
                    "task_id": {
                        "type": "string",
                        "description": "Task ID (for update/complete/delete). Supports short prefixes."
                    },
                    "title": {
                        "type": "string",
                        "description": "Task title (for create/update)"
                    },
                    "description": {
                        "type": "string",
                        "description": "Task description"
                    },
                    "priority": {
                        "type": "string",
                        "description": "Priority: low/medium/high/urgent"
                    },
                    "status": {
                        "type": "string",
                        "description": "Status: pending/in_progress/completed/blocked (for update)"
                    },
                    "parent_id": {
                        "type": "string",
                        "description": "Parent task ID for subtasks"
                    },
                    "notes": {
                        "type": "string",
                        "description": "Completion notes (for complete)"
                    },
                    "include_completed": {
                        "type": "boolean",
                        "description": "Include completed tasks in list (default false)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results for list (default 20)"
                    }
                },
                "required": ["action"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "goal".into(),
            description: Some("Manage high-level goals with milestones. Actions: create, list, update, add_milestone, complete_milestone.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "Action: create/list/update/add_milestone/complete_milestone"
                    },
                    "goal_id": {
                        "type": "string",
                        "description": "Goal ID (for update/add_milestone)"
                    },
                    "milestone_id": {
                        "type": "string",
                        "description": "Milestone ID (for complete_milestone)"
                    },
                    "title": {
                        "type": "string",
                        "description": "Title (for create/update/add_milestone)"
                    },
                    "description": {
                        "type": "string",
                        "description": "Description"
                    },
                    "success_criteria": {
                        "type": "string",
                        "description": "Success criteria (for create)"
                    },
                    "priority": {
                        "type": "string",
                        "description": "Priority: low/medium/high/critical"
                    },
                    "status": {
                        "type": "string",
                        "description": "Status: planning/in_progress/blocked/completed/abandoned"
                    },
                    "progress_percent": {
                        "type": "integer",
                        "description": "Progress 0-100 (for update)"
                    },
                    "weight": {
                        "type": "integer",
                        "description": "Milestone weight for progress calculation"
                    },
                    "include_finished": {
                        "type": "boolean",
                        "description": "Include finished goals in list"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results for list"
                    }
                },
                "required": ["action"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "correction".into(),
            description: Some("Record and manage corrections. When the user corrects your approach, record it to avoid the same mistake. Actions: record, list, validate.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "Action: record/list/validate"
                    },
                    "correction_id": {
                        "type": "string",
                        "description": "Correction ID (for validate)"
                    },
                    "correction_type": {
                        "type": "string",
                        "description": "Type: style/approach/pattern/preference/anti_pattern"
                    },
                    "what_was_wrong": {
                        "type": "string",
                        "description": "What you did wrong (for record)"
                    },
                    "what_is_right": {
                        "type": "string",
                        "description": "What you should do instead (for record)"
                    },
                    "rationale": {
                        "type": "string",
                        "description": "Why this is the right approach"
                    },
                    "scope": {
                        "type": "string",
                        "description": "Scope: global/project/file/topic"
                    },
                    "keywords": {
                        "type": "string",
                        "description": "Comma-separated keywords for matching"
                    },
                    "outcome": {
                        "type": "string",
                        "description": "Outcome for validate: validated/deprecated"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results for list"
                    }
                },
                "required": ["action"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "store_decision".into(),
            description: Some("Store an important architectural or design decision with context. Decisions are recalled semantically when relevant.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Unique key for this decision (e.g., 'auth-method', 'database-choice')"
                    },
                    "decision": {
                        "type": "string",
                        "description": "The decision that was made"
                    },
                    "category": {
                        "type": "string",
                        "description": "Category: architecture/design/tech-stack/workflow"
                    },
                    "context": {
                        "type": "string",
                        "description": "Context and rationale for the decision"
                    }
                },
                "required": ["key", "decision"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "record_rejected_approach".into(),
            description: Some("Record an approach that was tried and rejected. This prevents re-suggesting failed approaches in similar contexts.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "problem_context": {
                        "type": "string",
                        "description": "What problem you were trying to solve"
                    },
                    "approach": {
                        "type": "string",
                        "description": "The approach that was tried"
                    },
                    "rejection_reason": {
                        "type": "string",
                        "description": "Why this approach was rejected"
                    },
                    "related_files": {
                        "type": "string",
                        "description": "Comma-separated related file paths"
                    },
                    "related_topics": {
                        "type": "string",
                        "description": "Comma-separated related topics"
                    }
                },
                "required": ["problem_context", "approach", "rejection_reason"]
            }),
        },
    ]
}
