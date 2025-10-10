// src/llm/structured/tool_schema.rs
// Tool schemas for GPT-5 function calling

use serde_json::json;

/// Tool schema for artifact creation
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

/// Tool schema for reading a file
pub fn get_read_file_tool_schema() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "read_file",
            "description": "Read the contents of a single file from the project. Call this when you need to examine a specific file's contents to answer the user's question or understand the codebase.",
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
            "description": "List files in a directory with optional filtering. Call this when you need to explore the project structure or find files matching a pattern.",
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
            "description": "Search for code elements (functions, structs, classes, types) in the project. Returns matching code elements with file paths and line numbers.",
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
            "description": "Generate an image using DALL-E. Call this when the user explicitly requests image generation or visual content creation.",
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
            "description": "Get comprehensive project context including file tree, languages, recent files, and code statistics.",
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
                        "description": "Maximum directory depth for file tree (default: 100, effectively unlimited)"
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
            "description": "Read multiple files in a single batch operation. More efficient than multiple read_file calls. Use this when you need to examine several related files at once.",
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
            "description": "Write MULTIPLE files in a single batch operation. Use this when you need to update several files at once (e.g., fixing imports across multiple files, creating new modules with multiple files).",
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

/// Get all available tools for chat with project context
pub fn get_all_chat_tools() -> Vec<serde_json::Value> {
    vec![
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

/// Get minimal tools for simple queries
pub fn get_minimal_tools() -> Vec<serde_json::Value> {
    vec![
        get_create_artifact_tool_schema(),
    ]
}
