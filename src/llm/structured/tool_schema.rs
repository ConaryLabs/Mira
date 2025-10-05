// src/llm/structured/tool_schema.rs
// Response schemas and custom function tool definitions

use serde_json::json;

/// Tool schema for structured chat responses
/// FIXED: Crystal clear that this tool is MANDATORY for all responses
pub fn get_response_tool_schema() -> serde_json::Value {
    json!({
        "name": "respond_to_user",
        "description": "MANDATORY: Use this tool for EVERY response to the user. This is your ONLY way to communicate with them. Other tools (read_file, search_code, list_files) are for gathering information BEFORE calling this tool. After using other tools to gather context, you MUST call this tool to respond. Every user message requires a response via this tool.",
        "input_schema": {
            "type": "object",
            "properties": {
                "output": {
                    "type": "string",
                    "description": "Your actual response to the user - the message they will see"
                },
                "analysis": {
                    "type": "object",
                    "properties": {
                        "salience": {
                            "type": "number",
                            "description": "Importance score 0.0-1.0. How important is this to remember long-term? 0.0=trivial, 0.5=normal, 1.0=critical. Default to 0.5 if unsure."
                        },
                        "topics": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "List of topics discussed. Use ['general'] if no specific topics."
                        },
                        "contains_code": {
                            "type": "boolean",
                            "description": "Does this message contain actual code (code blocks, snippets)? NOT just technical terms."
                        },
                        "programming_lang": {
                            "type": "string",
                            "description": "REQUIRED if contains_code=true. Must be one of: 'rust', 'typescript', 'javascript', 'python', 'go', 'java'. Set to null if contains_code=false or language unknown."
                        },
                        "contains_error": {
                            "type": "boolean",
                            "description": "Does this message contain an actual error that needs fixing (compiler error, runtime error, stack trace, build failure)? NOT just discussing errors in general."
                        },
                        "error_type": {
                            "type": "string",
                            "description": "REQUIRED if contains_error=true. One of: 'compiler', 'runtime', 'test_failure', 'build_failure', 'linter', 'type_error'. Set to null if contains_error=false."
                        },
                        "error_file": {
                            "type": "string",
                            "description": "If contains_error=true and a file path is mentioned in the error, extract it. Otherwise null."
                        },
                        "error_severity": {
                            "type": "string",
                            "description": "If contains_error=true, rate as 'critical' (blocking), 'warning' (should fix), or 'info' (minor). Otherwise null."
                        },
                        "routed_to_heads": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Which memory heads should process this (valid: 'semantic', 'code', 'summary', 'documents'). Use 'code' if contains_code=true or contains_error=true. Use ['semantic'] as default."
                        },
                        "language": {
                            "type": "string",
                            "description": "Natural language code (e.g., 'en', 'es', 'fr'). Default to 'en'."
                        },
                        "mood": {
                            "type": "string",
                            "description": "Optional mood assessment"
                        },
                        "intensity": {
                            "type": "number",
                            "description": "Optional intensity score 0.0-1.0 (0.0=low, 1.0=high)"
                        },
                        "intent": {
                            "type": "string",
                            "description": "User's intent (e.g., 'question', 'command', 'chat', 'debugging')"
                        },
                        "summary": {
                            "type": "string",
                            "description": "Brief summary of the exchange"
                        },
                        "relationship_impact": {
                            "type": "string",
                            "description": "Optional relationship impact assessment"
                        }
                    },
                    "required": ["salience", "topics", "contains_code", "contains_error", "routed_to_heads", "language"]
                }
            },
            "required": ["output", "analysis"]
        }
    })
}

/// Tool schema for code fix responses
pub fn get_code_fix_tool_schema() -> serde_json::Value {
    json!({
        "name": "provide_code_fix",
        "description": "Provide a complete fixed version of the file(s) with the error resolved",
        "input_schema": {
            "type": "object",
            "properties": {
                "output": {
                    "type": "string",
                    "description": "Explanation of the fix for the user"
                },
                "analysis": {
                    "type": "object",
                    "properties": {
                        "salience": { 
                            "type": "number",
                            "description": "Importance score 0.0-1.0"
                        },
                        "topics": { "type": "array", "items": { "type": "string" } },
                        "contains_code": { 
                            "type": "boolean",
                            "description": "Always true for code fixes"
                        },
                        "programming_lang": { 
                            "type": "string",
                            "description": "REQUIRED. Must be one of: 'rust', 'typescript', 'javascript', 'python', 'go', 'java'"
                        },
                        "contains_error": {
                            "type": "boolean",
                            "description": "Always true for error fixes"
                        },
                        "error_type": {
                            "type": "string",
                            "description": "Type of error being fixed"
                        },
                        "routed_to_heads": { 
                            "type": "array", 
                            "items": { "type": "string" },
                            "description": "Valid values: 'semantic', 'code', 'summary', 'documents'. Should include 'code'."
                        },
                        "language": { 
                            "type": "string",
                            "description": "Natural language (e.g., 'en')"
                        }
                    },
                    "required": ["salience", "topics", "contains_code", "programming_lang", "contains_error", "error_type", "routed_to_heads", "language"]
                },
                "reasoning": {
                    "type": "string",
                    "description": "Detailed reasoning about the fix"
                },
                "fix_type": {
                    "type": "string",
                    "description": "Type of fix (e.g., 'compiler_error', 'runtime_error')"
                },
                "files": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "File path relative to project root"
                            },
                            "content": {
                                "type": "string",
                                "description": "COMPLETE file content from line 1 to last line"
                            },
                            "change_type": {
                                "type": "string",
                                "enum": ["primary", "import", "type", "cascade"],
                                "description": "Type of change"
                            }
                        },
                        "required": ["path", "content", "change_type"]
                    }
                },
                "confidence": {
                    "type": "number",
                    "description": "Confidence score 0.0-1.0"
                }
            },
            "required": ["output", "analysis", "fix_type", "files", "confidence"]
        }
    })
}

/// Tool schema for code search
pub fn get_code_search_tool_schema() -> serde_json::Value {
    json!({
        "name": "search_code",
        "description": "Search the codebase for functions, types, or symbols by name",
        "input_schema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search term (function name, type name, etc.)"
                },
                "element_type": {
                    "type": "string",
                    "description": "Optional: filter by element type (function, struct, trait, etc.)"
                }
            },
            "required": ["query"]
        }
    })
}

/// Tool schema for file reading
pub fn get_read_file_tool_schema() -> serde_json::Value {
    json!({
        "name": "read_file",
        "description": "Read the complete contents of a FILE (not a directory). For directories, use list_files instead.",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path to the FILE within the project (e.g., 'src/main.rs', not 'src')"
                }
            },
            "required": ["path"]
        }
    })
}

/// Tool schema for listing files
pub fn get_list_files_tool_schema() -> serde_json::Value {
    json!({
        "name": "list_files",
        "description": "List files in a directory, optionally filtered by pattern",
        "input_schema": {
            "type": "object",
            "properties": {
                "directory": {
                    "type": "string",
                    "description": "Directory path to list (relative to project root)"
                },
                "pattern": {
                    "type": "string",
                    "description": "Optional glob pattern to filter files (e.g., '*.rs')"
                },
                "recursive": {
                    "type": "boolean",
                    "description": "Whether to recursively list subdirectories"
                }
            },
            "required": ["directory"]
        }
    })
}

/// Tool schema for image generation via OpenAI gpt-image-1
pub fn get_image_generation_tool_schema() -> serde_json::Value {
    json!({
        "name": "generate_image",
        "description": "Generate an image using OpenAI's gpt-image-1 model",
        "input_schema": {
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Detailed description of the image to generate"
                },
                "size": {
                    "type": "string",
                    "enum": ["1024x1024", "1024x1536", "1536x1024"],
                    "description": "Image dimensions (default: 1024x1024)"
                },
                "quality": {
                    "type": "string",
                    "enum": ["low", "medium", "high"],
                    "description": "Image quality (default: high)"
                }
            },
            "required": ["prompt"]
        }
    })
}
