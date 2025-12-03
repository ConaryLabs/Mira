// src/operations/tools/mod.rs
// Modular tool definitions for LLM tool calling
//
// This module organizes tool schemas by domain:
// - code_generation: Generate, refactor, and debug code
// - code_intelligence: AST-powered code analysis
// - file_operations: Read, write, and analyze files
// - git_analysis: Git history and collaboration
// - external: Web search, URL fetching, command execution
// - skills: Specialized task handling
// - project_management: Task and guidelines tracking

pub mod common;
pub mod code_generation;
pub mod code_intelligence;
pub mod external;
pub mod file_operations;
pub mod git_analysis;
pub mod project_management;
pub mod skills;

use serde_json::Value;

/// Get all delegation tool schemas for LLM
/// Includes code generation tools and all analysis tools
pub fn get_delegation_tools() -> Vec<Value> {
    let mut tools = Vec::new();

    // Code generation delegation tools
    tools.extend(code_generation::get_tools());

    // File operation tools
    tools.extend(file_operations::get_tools());

    // External tools - web and command execution
    tools.extend(external::get_tools());

    // Git analysis tools
    tools.extend(git_analysis::get_tools());

    // Code intelligence tools
    tools.extend(code_intelligence::get_tools());

    // Skills system
    tools.extend(skills::get_tools());

    tools
}

/// Get tool schemas for LLM (executable tools for tool calling loop)
/// These are the actual tools the LLM can execute
pub fn get_llm_tools() -> Vec<Value> {
    let mut tools = Vec::new();

    // File operation tools (no code generation - those are for delegation)
    tools.extend(file_operations::get_tools());

    // External tools - web and command execution
    tools.extend(external::get_tools());

    // Git analysis tools
    tools.extend(git_analysis::get_tools());

    // Code intelligence tools
    tools.extend(code_intelligence::get_tools());

    // Skills system
    tools.extend(skills::get_tools());

    // Project management tools
    tools.extend(project_management::get_tools());

    tools
}

/// Get all code intelligence tools
pub fn get_code_tools() -> Vec<Value> {
    code_intelligence::get_tools()
}

/// Get all git analysis tools
pub fn get_git_tools() -> Vec<Value> {
    git_analysis::get_tools()
}

/// Get all file operation tools
pub fn get_file_operation_tools() -> Vec<Value> {
    file_operations::get_tools()
}

/// Get low-level file tools (used by meta-tools)
pub fn get_file_low_level_tools() -> Vec<Value> {
    file_operations::get_low_level_tools()
}

/// Get all external tools
pub fn get_external_tools() -> Vec<Value> {
    external::get_tools()
}
