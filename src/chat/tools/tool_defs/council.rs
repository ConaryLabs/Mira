//! Council tool definitions
//!
//! Council members: GPT 5.2, Opus 4.5, Gemini 3 Pro, DeepSeek Reasoner

use serde_json::json;
use super::super::definitions::Tool;

pub fn council_tools() -> Vec<Tool> {
    vec![
        Tool {
            tool_type: "function".into(),
            name: "council".into(),
            description: Some("Consult the council - calls GPT 5.2, Opus 4.5, Gemini 3 Pro, and DeepSeek Reasoner in parallel. Use for important decisions, architecture review, or when you want diverse perspectives.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The question or topic to consult the council about"
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional context to include (code, previous discussion, etc.)"
                    }
                },
                "required": ["message"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "ask_gpt".into(),
            description: Some("Ask GPT 5.2 directly. Good for reasoning, analysis, and complex problem-solving.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The question or request for GPT 5.2"
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional context to include"
                    }
                },
                "required": ["message"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "ask_opus".into(),
            description: Some("Ask Claude Opus 4.5 directly. Good for nuanced analysis, creative tasks, and detailed explanations.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The question or request for Opus 4.5"
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional context to include"
                    }
                },
                "required": ["message"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "ask_gemini".into(),
            description: Some("Ask Gemini 3 Pro directly. Good for research, factual questions, and multi-modal tasks.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The question or request for Gemini 3 Pro"
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional context to include"
                    }
                },
                "required": ["message"]
            }),
        },
        Tool {
            tool_type: "function".into(),
            name: "ask_deepseek".into(),
            description: Some("Ask DeepSeek Reasoner directly. Excellent for deep reasoning, step-by-step analysis, and complex problem decomposition.".into()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The question or request for DeepSeek Reasoner"
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional context to include"
                    }
                },
                "required": ["message"]
            }),
        },
    ]
}
