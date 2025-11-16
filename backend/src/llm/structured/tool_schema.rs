// src/llm/structured/tool_schema.rs
// Tool schemas for GPT-5 function calling
// FIXED: Added required "function" wrapper to match OpenAI API spec

use serde_json::json;

/// Tool schema for artifact creation
pub fn get_create_artifact_tool_schema() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "create_artifact",
            "description": "Create a code artifact that the user can view, edit, and apply to their project. Use this when generating complete code files, large code snippets, or any code the user will want to save/use.\n\nCODING STYLE GUIDELINES:\n- Write complete, production-ready code with no placeholders or TODOs\n- Use descriptive variable names (e.g., user_session, file_content, not x, y)\n- Include comprehensive error handling\n- Add inline comments for complex logic\n- Follow language idioms (e.g., Rust: use Result<T>, match; TypeScript: use const, async/await)\n- Keep functions focused and under 50 lines when possible\n- Include all necessary imports and dependencies",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Title for the artifact. Use filename if this is a file (e.g., 'main.rs', 'App.tsx'), or descriptive title for code snippets (e.g., 'Binary Search Implementation')"
                    },
                    "content": {
                        "type": "string",
                        "description": "The COMPLETE code content from start to finish. Include ALL imports, ALL functions, ALL closing braces. Never truncate or use placeholders like '...', '// rest of code', or 'TODO'."
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
            "description": "List files and directories in a specific directory of the project. Use this to explore the project structure or find files.",
            "parameters": {
                "type": "object",
                "properties": {
                    "directory": {
                        "type": "string",
                        "description": "Directory path relative to project root (e.g., 'src' or 'src/api'). Use empty string '' for project root."
                    }
                },
                "required": ["directory"]
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
            "description": "Search for code elements (functions, structs, classes, components) in the project by name or pattern. Returns matching elements with their locations and signatures.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Search pattern or name (e.g., 'handle_request', 'User', 'Button')"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default: 50)",
                        "default": 50
                    }
                },
                "required": ["pattern"]
            }
        }
    })
}

/// Tool schema for web search (built-in GPT-5 Responses API tool)
pub fn get_web_search_tool_schema() -> serde_json::Value {
    json!({
        "type": "web_search"
    })
}

/// Tool schema for image generation
pub fn get_image_generation_tool_schema() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "generate_image",
            "description": "Generate an image using DALL-E based on a text prompt. Use this when the user asks you to create, draw, or generate an image.",
            "parameters": {
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "Detailed description of the image to generate"
                    },
                    "size": {
                        "type": "string",
                        "description": "Image size",
                        "enum": ["1024x1024", "1792x1024", "1024x1792"],
                        "default": "1024x1024"
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
            "description": "Get comprehensive information about the project including file tree, statistics, languages used, and overall structure. Use this to understand the project at a high level.",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": []
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
            "description": "Read MULTIPLE files in a single batch operation. More efficient than multiple read_file calls. Use this when you need to examine several related files at once.",
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
        get_web_search_tool_schema(), // ADDED: Step 2.3
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
        get_web_search_tool_schema(), // ADDED: Useful even without projects
    ]
}
