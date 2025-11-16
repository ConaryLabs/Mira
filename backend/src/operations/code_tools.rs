// src/operations/code_tools.rs
// Code intelligence tool schemas for exposing AST analysis to DeepSeek

use serde_json::{json, Value};

/// Get all code intelligence tool schemas
pub fn get_code_tools() -> Vec<Value> {
    vec![
        find_function_tool(),
        find_class_or_struct_tool(),
        search_code_semantic_tool(),
        find_imports_tool(),
        analyze_dependencies_tool(),
        get_complexity_hotspots_tool(),
        get_quality_issues_tool(),
        get_file_symbols_tool(),
        find_tests_for_code_tool(),
        get_codebase_stats_tool(),
        find_callers_tool(),
        get_element_definition_tool(),
    ]
}

/// Find function/method definitions
fn find_function_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "find_function_internal",
            "description": "Find function or method definitions by name or pattern. Supports wildcards for flexible matching.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Function name or pattern (use % as wildcard, e.g. 'handle%' finds handleClick, handleSubmit)"
                    },
                    "project_id": {
                        "type": "string",
                        "description": "Project ID to search within"
                    },
                    "include_tests": {
                        "type": "string",
                        "description": "Include test functions in results (default: false)"
                    },
                    "min_complexity": {
                        "type": "string",
                        "description": "Filter by minimum complexity score"
                    },
                    "limit": {
                        "type": "string",
                        "description": "Maximum results to return (default: 20)"
                    }
                },
                "required": ["name", "project_id"]
            }
        }
    })
}

/// Find class/struct/enum definitions
fn find_class_or_struct_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "find_class_or_struct_internal",
            "description": "Find class, struct, or enum definitions by name. Returns type definitions with their methods and documentation.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Type name or pattern (supports % wildcard)"
                    },
                    "project_id": {
                        "type": "string",
                        "description": "Project ID to search within"
                    },
                    "include_private": {
                        "type": "string",
                        "description": "Include private/internal types (default: false)"
                    },
                    "limit": {
                        "type": "string",
                        "description": "Maximum results to return (default: 20)"
                    }
                },
                "required": ["name", "project_id"]
            }
        }
    })
}

/// Semantic code search
fn search_code_semantic_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "search_code_semantic_internal",
            "description": "Semantic search across codebase using natural language. Uses vector embeddings to find relevant code based on meaning, not just keywords.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language description of what to find (e.g., 'authentication middleware', 'error handling utilities')"
                    },
                    "project_id": {
                        "type": "string",
                        "description": "Project ID to search within"
                    },
                    "limit": {
                        "type": "string",
                        "description": "Maximum results to return (default: 10)"
                    }
                },
                "required": ["query", "project_id"]
            }
        }
    })
}

/// Find imports/usage of a symbol
fn find_imports_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "find_imports_internal",
            "description": "Find where a symbol is imported or used across the codebase. Shows all files that import a specific function, class, or module.",
            "parameters": {
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "Symbol to find (e.g., 'useState', 'HashMap', 'express')"
                    },
                    "project_id": {
                        "type": "string",
                        "description": "Project ID to search within"
                    },
                    "limit": {
                        "type": "string",
                        "description": "Maximum results to return (default: 50)"
                    }
                },
                "required": ["symbol", "project_id"]
            }
        }
    })
}

/// Analyze dependencies
fn analyze_dependencies_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "analyze_dependencies_internal",
            "description": "Analyze external dependencies for a file or entire project. Shows npm packages, local imports, and standard library usage.",
            "parameters": {
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Specific file to analyze (optional, omit for project-wide analysis)"
                    },
                    "project_id": {
                        "type": "string",
                        "description": "Project ID to analyze"
                    },
                    "group_by": {
                        "type": "string",
                        "description": "How to group results: 'type' (npm/local/std) or 'frequency' (most used first). Default: 'type'"
                    }
                },
                "required": ["project_id"]
            }
        }
    })
}

/// Get complexity hotspots
fn get_complexity_hotspots_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "get_complexity_hotspots_internal",
            "description": "Find the most complex functions in the codebase. High complexity indicates code that may be hard to maintain and test.",
            "parameters": {
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "Project ID to analyze"
                    },
                    "min_complexity": {
                        "type": "string",
                        "description": "Minimum complexity score to include (default: 10)"
                    },
                    "limit": {
                        "type": "string",
                        "description": "Maximum results to return (default: 10)"
                    }
                },
                "required": ["project_id"]
            }
        }
    })
}

/// Get quality issues
fn get_quality_issues_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "get_quality_issues_internal",
            "description": "Get code quality issues for a file or project. Includes complexity problems, missing documentation, and auto-fix suggestions.",
            "parameters": {
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Specific file to analyze (optional, omit for project-wide)"
                    },
                    "project_id": {
                        "type": "string",
                        "description": "Project ID to analyze"
                    },
                    "severity": {
                        "type": "string",
                        "description": "Filter by severity: 'critical', 'high', 'medium', 'low', 'info'"
                    },
                    "issue_type": {
                        "type": "string",
                        "description": "Filter by type: 'complexity', 'documentation', 'security'"
                    },
                    "limit": {
                        "type": "string",
                        "description": "Maximum results to return (default: 50)"
                    }
                },
                "required": ["project_id"]
            }
        }
    })
}

/// Get file symbols
fn get_file_symbols_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "get_file_symbols_internal",
            "description": "Get all symbols (functions, classes, types) in a specific file. Returns structured overview of file contents.",
            "parameters": {
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to file to analyze"
                    },
                    "project_id": {
                        "type": "string",
                        "description": "Project ID"
                    },
                    "include_private": {
                        "type": "string",
                        "description": "Include private/internal symbols (default: true)"
                    },
                    "include_content": {
                        "type": "string",
                        "description": "Include full source code of elements (default: false, only signatures)"
                    }
                },
                "required": ["file_path", "project_id"]
            }
        }
    })
}

/// Find tests for code
fn find_tests_for_code_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "find_tests_for_code_internal",
            "description": "Find test files and test functions related to a code element. Helps verify test coverage.",
            "parameters": {
                "type": "object",
                "properties": {
                    "element_name": {
                        "type": "string",
                        "description": "Function or class name to find tests for"
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Source file path (optional, helps narrow search)"
                    },
                    "project_id": {
                        "type": "string",
                        "description": "Project ID"
                    }
                },
                "required": ["element_name", "project_id"]
            }
        }
    })
}

/// Get codebase statistics
fn get_codebase_stats_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "get_codebase_stats_internal",
            "description": "Get comprehensive statistics about the codebase. Includes file counts, complexity metrics, test coverage, and quality summary.",
            "parameters": {
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "Project ID to analyze"
                    },
                    "breakdown_by": {
                        "type": "string",
                        "description": "How to break down stats: 'language', 'file_type', or 'complexity'. Default: 'language'"
                    }
                },
                "required": ["project_id"]
            }
        }
    })
}

/// Find callers of a function
fn find_callers_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "find_callers_internal",
            "description": "Find all places where a function is called. Useful for impact analysis before refactoring.",
            "parameters": {
                "type": "object",
                "properties": {
                    "function_name": {
                        "type": "string",
                        "description": "Function name to find callers for"
                    },
                    "project_id": {
                        "type": "string",
                        "description": "Project ID"
                    },
                    "limit": {
                        "type": "string",
                        "description": "Maximum results to return (default: 50)"
                    }
                },
                "required": ["function_name", "project_id"]
            }
        }
    })
}

/// Get element definition
fn get_element_definition_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "get_element_definition_internal",
            "description": "Get the full definition of a code element (function, class, type) including signature, documentation, and metadata.",
            "parameters": {
                "type": "object",
                "properties": {
                    "element_name": {
                        "type": "string",
                        "description": "Name of element to get definition for"
                    },
                    "file_path": {
                        "type": "string",
                        "description": "File path to narrow search (optional)"
                    },
                    "project_id": {
                        "type": "string",
                        "description": "Project ID"
                    }
                },
                "required": ["element_name", "project_id"]
            }
        }
    })
}
