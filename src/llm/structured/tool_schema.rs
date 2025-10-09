// src/llm/structured/tool_schema.rs
// Unified tool schemas for DeepSeek 3.2 + GPT-5
// OpenAI function calling format (compatible with both providers)

use serde_json::json;

/// Tool schema for structured chat responses
/// MANDATORY for all responses
pub fn get_response_tool_schema() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "respond_to_user",
            "description": "🚨 CRITICAL RESPONSE REQUIREMENT 🚨\n\nThis tool is MANDATORY for EVERY user message - no exceptions.\n\nYou MUST call this tool to communicate with the user. They cannot see your thinking or tool results unless you call this tool.\n\n⚠️ WHEN TO CALL THIS:\n- After gathering context with other tools\n- Even if just acknowledging a message\n- Even if you're unsure or need clarification\n- ALWAYS as the final step in your response\n\n⚠️ OTHER TOOLS ARE FOR GATHERING:\n- read_file, search_code, list_files: Information gathering\n- create_artifact, provide_code_fix: Code generation\n- These tools DO NOT communicate with the user\n\n⚠️ WORKFLOW:\n1. Use other tools to gather information (if needed)\n2. Call respond_to_user to send your message\n3. The conversation ends when you call respond_to_user\n\nThe user is waiting for your response. You must call this tool to communicate with them.",
            "parameters": {
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
                            }
                        },
                        "required": ["salience", "topics", "contains_code", "routed_to_heads", "language"]
                    }
                },
                "required": ["output", "analysis"]
            }
        }
    })
}

/// Tool schema for creating artifacts
pub fn get_create_artifact_tool_schema() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "create_artifact",
            "description": "Create a code artifact that the user can view, edit, and apply to their project. Use this when generating complete code files, large code snippets, or any code the user will want to save/use.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Title for the artifact. Use filename if this is a file (e.g., 'main.rs', 'App.tsx'), or descriptive title for code snippets (e.g., 'Binary Search Implementation')"
                    },
                    "content": {
                        "type": "string",
                        "description": "The COMPLETE code content from start to finish. Include ALL imports, ALL functions, ALL closing braces. Never truncate or use placeholders."
                    },
                    "language": {
                        "type": "string",
                        "description": "Programming language for syntax highlighting. Use one of: 'rust', 'typescript', 'javascript', 'python', 'go', 'java', 'cpp', 'c', 'html', 'css', 'json', 'yaml', 'sql', 'bash', 'markdown'",
                        "enum": ["rust", "typescript", "javascript", "python", "go", "java", "cpp", "c", "html", "css", "json", "yaml", "sql", "bash", "markdown", "text"]
                    },
                    "path": {
                        "type": "string",
                        "description": "Optional: File path if this represents a file in the project (e.g., 'src/main.rs'). Leave null for generic code snippets."
                    }
                },
                "required": ["title", "content", "language"]
            }
        }
    })
}

/// Tool schema for code fix responses
pub fn get_code_fix_tool_schema() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "provide_code_fix",
            "description": "Provide a complete fixed version of the file(s) with the error resolved",
            "parameters": {
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
                            "topics": { 
                                "type": "array", 
                                "items": { "type": "string" } 
                            },
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
        }
    })
}

/// Tool schema for reading a single file
pub fn get_read_file_tool_schema() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "read_file",
            "description": "Read the contents of a single file from the project",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to project root (e.g., 'src/main.rs')"
                    }
                },
                "required": ["path"]
            }
        }
    })
}

/// Tool schema for listing files
pub fn get_list_files_tool_schema() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "list_files",
            "description": "List files in a directory with optional filtering",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path relative to project root (default: '.')"
                    },
                    "pattern": {
                        "type": "string",
                        "description": "Optional glob pattern to filter files (e.g., '*.rs', 'src/**/*.ts')"
                    },
                    "recursive": {
                        "type": "boolean",
                        "description": "Whether to list files recursively (default: false)"
                    }
                }
            }
        }
    })
}

/// Tool schema for code search
pub fn get_code_search_tool_schema() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "search_code",
            "description": "Search for code elements (functions, structs, imports) in the project",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Search pattern (function name, struct name, etc.)"
                    },
                    "element_type": {
                        "type": "string",
                        "enum": ["function", "struct", "enum", "trait", "impl", "import", "any"],
                        "description": "Type of code element to search for (default: 'any')"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 10)"
                    }
                },
                "required": ["pattern"]
            }
        }
    })
}

/// Tool schema for image generation
pub fn get_image_generation_tool_schema() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "generate_image",
            "description": "Generate an image using DALL-E",
            "parameters": {
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "Detailed description of the image to generate"
                    },
                    "size": {
                        "type": "string",
                        "enum": ["1024x1024", "1792x1024", "1024x1792"],
                        "description": "Image size (default: '1024x1024')"
                    },
                    "quality": {
                        "type": "string",
                        "enum": ["standard", "hd"],
                        "description": "Image quality (default: 'standard')"
                    }
                },
                "required": ["prompt"]
            }
        }
    })
}

/// Tool schema for project context
pub fn get_project_context_tool_schema() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "get_project_context",
            "description": "Get comprehensive project context including file tree, git status, and key files",
            "parameters": {
                "type": "object",
                "properties": {
                    "include_tree": {
                        "type": "boolean",
                        "description": "Include file tree (default: true)"
                    },
                    "include_git": {
                        "type": "boolean",
                        "description": "Include git status (default: true)"
                    },
                    "max_depth": {
                        "type": "integer",
                        "description": "Maximum directory depth for file tree (default: 3)"
                    }
                }
            }
        }
    })
}

/// Tool schema for reading multiple files
pub fn get_read_files_tool_schema() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "read_files",
            "description": "Read multiple files in a single batch operation. More efficient than multiple read_file calls.",
            "parameters": {
                "type": "object",
                "properties": {
                    "paths": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Array of file paths to read. Each path should be relative to the project root."
                    }
                },
                "required": ["paths"]
            }
        }
    })
}

/// Tool schema for writing multiple files
pub fn get_write_files_tool_schema() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "write_files",
            "description": "Write MULTIPLE files in a single batch operation. Use this when you need to update several files at once (e.g., fixing imports across multiple files).",
            "parameters": {
                "type": "object",
                "properties": {
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
                                    "description": "Complete file content to write"
                                }
                            },
                            "required": ["path", "content"]
                        },
                        "description": "Array of files to write. Each file must have a path and content."
                    }
                },
                "required": ["files"]
            }
        }
    })
}

// ============================================================================
// TOOL COLLECTION FUNCTIONS
// ============================================================================

/// Get all available tools for regular chat
pub fn get_all_chat_tools() -> Vec<serde_json::Value> {
    vec![
        get_response_tool_schema(),
        get_create_artifact_tool_schema(),
        get_read_file_tool_schema(),
        get_list_files_tool_schema(),
        get_code_search_tool_schema(),
        get_image_generation_tool_schema(),
        get_project_context_tool_schema(),
        get_read_files_tool_schema(),
        get_write_files_tool_schema(),
    ]
}

/// Get tools for code fix operations
pub fn get_code_fix_tools() -> Vec<serde_json::Value> {
    vec![
        get_code_fix_tool_schema(),
        get_read_file_tool_schema(),
        get_code_search_tool_schema(),
    ]
}

/// Get minimal tools for simple queries
pub fn get_minimal_tools() -> Vec<serde_json::Value> {
    vec![
        get_response_tool_schema(),
        get_create_artifact_tool_schema(),
    ]
}
