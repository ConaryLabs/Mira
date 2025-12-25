//! Memory tool definitions

use serde_json::json;
use super::super::definitions::Tool;

pub fn memory_tools() -> Vec<Tool> {
    vec![
        Tool {
            tool_type: "function".into(),
            name: "remember".into(),
            description: Some("Store a fact, decision, or preference for future recall. Uses semantic search for intelligent retrieval.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "The fact or information to remember"
                    },
                    "fact_type": {
                        "type": "string",
                        "description": "Type of fact: preference, decision, context, general (default)"
                    },
                    "category": {
                        "type": "string",
                        "description": "Optional category for organization"
                    }
                },
                "required": ["content"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "recall".into(),
            description: Some("Search for previously stored memories using semantic similarity. Returns relevant facts, decisions, and preferences.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query - uses semantic similarity matching"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max number of results (default 5)"
                    },
                    "fact_type": {
                        "type": "string",
                        "description": "Filter by fact type: preference, decision, context, general"
                    }
                },
                "required": ["query"]
            }),
        },
    ]
}
