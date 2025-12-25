//! Web tool definitions

use serde_json::json;
use super::super::definitions::Tool;

pub fn web_tools() -> Vec<Tool> {
    vec![
        Tool {
            tool_type: "function".into(),
            name: "web_search".into(),
            description: Some("Search the web using DuckDuckGo".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max number of results (default 5)"
                    }
                },
                "required": ["query"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "web_fetch".into(),
            description: Some("Fetch content from a URL and convert to text. Automatically falls back to Google Cache on 403 errors.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "URL to fetch"
                    },
                    "max_length": {
                        "type": "integer",
                        "description": "Max content length (default 10000)"
                    },
                    "use_cache": {
                        "type": "boolean",
                        "description": "Force fetch from Google Cache instead of direct URL (useful for sites that block bots)"
                    }
                },
                "required": ["url"]
            }),
        },
    ]
}
