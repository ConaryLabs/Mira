//! Orchestration tool definitions
//!
//! Tools for Studio to orchestrate Claude Code:
//! - View Claude Code's recent activity
//! - Send instructions for Claude Code to execute
//! - Track instruction status

use serde_json::json;
use super::super::definitions::Tool;

pub fn orchestration_tools() -> Vec<Tool> {
    vec![
        Tool {
            tool_type: "function".into(),
            name: "view_claude_activity".into(),
            description: Some("View recent MCP tool calls made by Claude Code. Use this to see what Claude Code has been doing, check progress on instructions, or understand the current state of work.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "tool_name": {
                        "type": "string",
                        "description": "Filter by tool name (e.g., 'remember', 'recall', 'Edit')"
                    },
                    "query": {
                        "type": "string",
                        "description": "Search query to filter by result summaries or arguments"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 20, max: 100)"
                    }
                },
                "required": []
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "send_instruction".into(),
            description: Some("Queue an instruction for Claude Code to pick up and execute. Use this to delegate implementation work, file changes, tests, or any task that requires writing code or running commands.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "instruction": {
                        "type": "string",
                        "description": "The instruction for Claude Code. Be specific and actionable."
                    },
                    "context": {
                        "type": "string",
                        "description": "Additional context, reasoning, or background information"
                    },
                    "priority": {
                        "type": "string",
                        "enum": ["low", "normal", "high", "urgent"],
                        "description": "Priority level (default: normal)"
                    }
                },
                "required": ["instruction"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "list_instructions".into(),
            description: Some("List instructions in the queue. Shows pending, in-progress, and recently completed instructions.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["pending", "delivered", "in_progress", "completed", "failed", "cancelled", "all"],
                        "description": "Filter by status (default: all active)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 20)"
                    }
                },
                "required": []
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "cancel_instruction".into(),
            description: Some("Cancel a pending instruction that hasn't been picked up yet.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "instruction_id": {
                        "type": "string",
                        "description": "ID of the instruction to cancel"
                    }
                },
                "required": ["instruction_id"]
            }),
        },
    ]
}
