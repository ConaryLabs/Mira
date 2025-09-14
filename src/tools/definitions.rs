// src/tools/definitions.rs

use serde_json::json;
use tracing::debug;

use crate::llm::responses::types::{Tool, FunctionDefinition, CodeInterpreterConfig, ContainerConfig};
use crate::config::CONFIG;

pub fn get_enabled_tools() -> Vec<Tool> {
    let mut tools = Vec::new();
    
    if CONFIG.enable_web_search {
        tools.push(Tool {
            tool_type: "web_search".to_string(),
            function: Some(FunctionDefinition {
                name: "web_search".to_string(),
                description: "Search the web for current information, news, or real-time data. Use when the user asks about recent events, current data, or information that might not be in training data.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query to find relevant web content"
                        },
                        "search_type": {
                            "type": "string",
                            "enum": ["general", "news", "academic", "images"],
                            "description": "Type of search to perform (optional, default: general)"
                        }
                    },
                    "required": ["query"]
                }),
            }),
            web_search: None,
            code_interpreter: None,
        });
    }

    if CONFIG.enable_code_interpreter {
        tools.push(Tool {
            tool_type: "code_interpreter".to_string(),
            function: None,
            web_search: None,
            code_interpreter: Some(CodeInterpreterConfig {
                container: ContainerConfig {
                    container_type: "python".to_string(),
                },
            }),
        });
    }

    if CONFIG.enable_file_search {
        tools.push(Tool {
            tool_type: "file_search".to_string(),
            function: Some(FunctionDefinition {
                name: "file_search".to_string(),
                description: "Search through project files for specific content, functions, or patterns. Useful for finding code, documentation, or specific text within the project repository.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query to find in files (content, function names, patterns, keywords)"
                        },
                        "file_extensions": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "File extensions to limit search scope (optional, e.g. ['rs', 'js', 'py'])"
                        },
                        "max_files": {
                            "type": "integer",
                            "description": "Maximum number of files to return in results (optional, default from config)"
                        },
                        "case_sensitive": {
                            "type": "boolean", 
                            "description": "Whether search should be case sensitive (optional, default false)"
                        }
                    },
                    "required": ["query"]
                }),
            }),
            web_search: None,
            code_interpreter: None,
        });
    }
    
    if CONFIG.enable_image_generation {
        tools.push(Tool {
            tool_type: "image_generation".to_string(),
            function: Some(FunctionDefinition {
                name: "image_generation".to_string(),
                description: "Generate images from text descriptions using AI. Creates visual content, diagrams, illustrations, or artistic images based on detailed prompts.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "prompt": {
                            "type": "string",
                            "description": "Detailed description of the image to generate. Be specific about style, composition, colors, and content."
                        },
                        "style": {
                            "type": "string",
                            "enum": ["vivid", "natural"],
                            "description": "Image style preference (optional, default from config)"
                        },
                        "quality": {
                            "type": "string", 
                            "enum": ["standard", "hd"],
                            "description": "Image quality level (optional, default from config)"
                        },
                        "size": {
                            "type": "string",
                            "enum": ["1024x1024", "1792x1024", "1024x1792"],
                            "description": "Image dimensions (optional, default from config)"
                        }
                    },
                    "required": ["prompt"]
                }),
            }),
            web_search: None,
            code_interpreter: None,
        });
    }
    
    debug!("Enabled {} tools: {:?}", 
        tools.len(), 
        tools.iter().map(|t| &t.tool_type).collect::<Vec<_>>()
    );
    
    tools
}
