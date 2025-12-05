// src/prompt/context.rs
// Context building functions for system prompts

use crate::api::ws::message::MessageMetadata;
use crate::config::CONFIG;
use crate::git::client::tree_builder::{FileNode, FileNodeType};
use crate::memory::core::types::MemoryEntry;
use crate::memory::features::recall_engine::RecallContext;
use crate::prompt::types::{CodeElement, ErrorContext, QualityIssue};
use crate::prompt::utils::language_from_extension;
use crate::system::SystemContext;
use crate::tools::types::Tool;
use chrono::Utc;

/// Add system environment context to the prompt
/// This helps the LLM use platform-appropriate commands (apt vs brew vs dnf, etc.)
/// Also includes current date/time so the LLM knows "today's date" without being told
pub fn add_system_context(prompt: &mut String, context: &SystemContext) {
    prompt.push_str("[SYSTEM ENVIRONMENT]\n");

    // Current date and time (so LLM knows "today" without being told)
    let now = chrono::Local::now();
    prompt.push_str(&format!(
        "Current time: {} ({})\n",
        now.format("%A, %B %d, %Y at %I:%M %p"),
        now.format("%Z")
    ));

    // OS info
    prompt.push_str(&format!(
        "OS: {} ({})\n",
        context.os.version, context.os.arch
    ));

    // Shell
    prompt.push_str(&format!("Shell: {}\n", context.shell.name));

    // Package manager
    if let Some(pm) = context.primary_package_manager() {
        prompt.push_str(&format!("Package manager: {}\n", pm));
    }

    // Available tools (compact list)
    if !context.tools.is_empty() {
        let tool_names: Vec<&str> = context.tools.iter().map(|t| t.name.as_str()).collect();
        prompt.push_str(&format!("Available tools: {}\n", tool_names.join(", ")));
    }

    prompt.push_str("\nUse platform-appropriate commands for this system.\n\n");
}

/// Add tool usage hints with mandatory conversational context
pub fn add_tool_usage_hints(prompt: &mut String) {
    prompt.push_str("[CODE HANDLING]\n");
    prompt.push_str("For code-related tasks, use the appropriate tools:\n");
    prompt.push_str(
        "- 'create_artifact' - For any code you write (examples, new files, fixes, etc)\n",
    );
    prompt.push_str("- 'search_code' - For finding code elements in projects\n");
    prompt.push_str("- 'get_project_context' - For understanding project structure\n\n");

    prompt.push_str("CONVERSATION MODE REQUIREMENTS:\n");
    prompt.push_str("In normal conversation, NEVER respond with ONLY a tool call and no explanatory text.\n");
    prompt.push_str("Every response with a tool call MUST include conversational text that:\n");
    prompt.push_str("- Explains what you're doing and why\n");
    prompt.push_str("- Describes the approach or solution\n");
    prompt.push_str("- Guides the user on what to expect or do next\n");
    prompt.push_str("- Maintains your personality and connection with the user\n\n");

    prompt.push_str("Example BAD response: [creates artifact with zero text]\n");
    prompt.push_str("Example GOOD response: \"Alright, here's a streamBuffer utility that'll batch those 3500 chunks...\"\n");
    prompt.push_str("[then creates artifact]\n\n");

    prompt.push_str("EXECUTION MODE EXCEPTION:\n");
    prompt.push_str("If you receive a message marked '=== EXECUTION MODE ACTIVATED ===', the conversational requirements are SUSPENDED.\n");
    prompt.push_str("In execution mode:\n");
    prompt.push_str("- Make tool calls IMMEDIATELY without explanatory text\n");
    prompt.push_str("- Focus on calling the correct tool for the current step\n");
    prompt.push_str("- Tool execution results will be streamed to the user automatically\n");
    prompt.push_str("- You'll be called again after each tool executes to continue the workflow\n\n");

    prompt.push_str(
        "Artifacts display in a Monaco editor where users can edit and apply changes.\n\n",
    );
}

/// Full technical requirements for code generation
/// Model-agnostic - works with any LLM backend
pub fn add_code_fix_requirements(
    prompt: &mut String,
    error_context: &ErrorContext,
    file_content: &str,
    code_elements: Option<Vec<CodeElement>>,
    quality_issues: Option<Vec<QualityIssue>>,
) {
    let line_count = file_content.lines().count();

    prompt.push_str("\n\n");
    prompt.push_str("==== CRITICAL CODE FIX REQUIREMENTS ====\n\n");

    prompt.push_str("You are fixing an error in a file. The system will REPLACE THE ENTIRE FILE with your output.\n");
    prompt.push_str(
        "This is not a code review or partial fix - you must provide COMPLETE files.\n\n",
    );

    prompt.push_str("REQUIREMENTS:\n");
    prompt.push_str(&format!(
        "1. The original file has {} lines. Your fixed file should have a similar line count.\n",
        line_count
    ));
    prompt.push_str("2. Provide EVERY line from line 1 to the last line.\n");
    prompt.push_str("3. Include ALL imports at the top of the file.\n");
    prompt.push_str("4. Include ALL functions, classes, methods, and types.\n");
    prompt.push_str("5. Include ALL constants, variables, and exports.\n");
    prompt.push_str("6. Include ALL closing braces, brackets, and parentheses.\n\n");

    prompt.push_str("FORBIDDEN PATTERNS - NEVER USE:\n");
    prompt.push_str("- '...' (ellipsis to indicate skipped code)\n");
    prompt.push_str("- '// rest unchanged' or similar comments\n");
    prompt.push_str("- '// previous code' or '// existing code'\n");
    prompt.push_str("- Partial functions or truncated code blocks\n");
    prompt.push_str("- ANY form of abbreviation or code skipping\n\n");

    prompt.push_str("ERROR DETAILS:\n");
    prompt.push_str(&format!("- File: {}\n", error_context.file_path));

    // Derive language from file extension
    let language = language_from_extension(&error_context.file_path);

    prompt.push_str(&format!("- Language: {}\n", language));
    prompt.push_str(&format!("- Error Type: {}\n", error_context.error_type));
    prompt.push_str(&format!("- Error: {}\n\n", error_context.error_message));

    // Add code elements if available
    if let Some(elements) = code_elements {
        prompt.push_str("CODE STRUCTURE:\n");

        for element in elements {
            let visibility = if element.is_public == Some(true) {
                "public"
            } else {
                "private"
            };
            let async_marker = if element.is_async == Some(true) {
                " async"
            } else {
                ""
            };

            prompt.push_str(&format!(
                "- {} {}{}: '{}' (lines {}-{}",
                visibility,
                element.element_type,
                async_marker,
                element.name,
                element.start_line,
                element.end_line
            ));

            if let Some(complexity) = element.complexity {
                prompt.push_str(&format!(", complexity: {}", complexity));
            }

            prompt.push_str(")\n");

            if let Some(doc) = &element.documentation {
                if !doc.is_empty() {
                    prompt.push_str(&format!("  Doc: {}\n", doc));
                }
            }
        }

        prompt.push_str("\n");
    }

    // Add quality issues if available
    if let Some(issues) = quality_issues {
        prompt.push_str("QUALITY CONCERNS:\n");

        for issue in issues {
            let severity_prefix = match issue.severity.as_str() {
                "critical" => "[CRITICAL]",
                "warning" => "[WARNING]",
                _ => "[INFO]",
            };

            prompt.push_str(&format!(
                "{} [{}]: {}\n",
                severity_prefix, issue.category, issue.description
            ));

            if let Some(element) = &issue.element_name {
                prompt.push_str(&format!("  Affects: {}\n", element));
            }

            if let Some(suggestion) = &issue.suggestion {
                prompt.push_str(&format!("  Suggestion: {}\n", suggestion));
            }
        }

        prompt.push_str("\n");
    }

    prompt.push_str("ORIGINAL FILE CONTENT:\n");
    prompt.push_str("```\n");
    prompt.push_str(file_content);
    prompt.push_str("\n```\n\n");

    prompt.push_str("OUTPUT FORMAT:\n");
    prompt.push_str("Return the COMPLETE fixed file content.\n");
    prompt.push_str("Start from line 1 and include every single line to the end.\n");
    prompt.push_str("Do not wrap in code blocks or add any markdown formatting.\n");
    prompt.push_str("Just the raw file content, ready to write to disk.\n");
}

/// Add project context to the prompt
pub fn add_project_context(
    prompt: &mut String,
    metadata: Option<&MessageMetadata>,
    project_id: Option<&str>,
) {
    if let Some(meta) = metadata {
        if let Some(project_name) = &meta.project_name {
            prompt.push_str(&format!("[ACTIVE PROJECT: {}]\n", project_name));

            if meta.request_repo_context == Some(true) {
                prompt.push_str("The user wants you to be aware of the repository context ");
                prompt.push_str("and code structure when responding. ");
            }

            prompt.push_str("\n\n");
        }
    } else if let Some(proj_id) = project_id {
        prompt.push_str(&format!(
            "[ACTIVE PROJECT: {}]\n\
                When the user refers to 'the project' or asks project-related questions, \
                they mean this project.\n\n",
            proj_id
        ));
    }
}

/// Add memory context (recent and semantic) to the prompt
pub fn add_memory_context(prompt: &mut String, context: &RecallContext) {
    // Add summaries if config enabled
    if CONFIG.use_rolling_summaries_in_context {
        if let Some(session) = &context.session_summary {
            prompt.push_str("\n## SESSION OVERVIEW (Entire Conversation)\n");
            prompt.push_str(
                "This is a comprehensive summary of your entire conversation history:\n\n",
            );
            prompt.push_str(session);
            prompt.push_str("\n\n");
        }

        if let Some(rolling) = &context.rolling_summary {
            prompt.push_str("\n## RECENT ACTIVITY (Last 100 Messages)\n");
            prompt.push_str("Summary of recent discussion:\n\n");
            prompt.push_str(rolling);
            prompt.push_str("\n\n");
        }
    }

    if context.recent.is_empty() && context.semantic.is_empty() {
        return;
    }

    prompt.push_str("[MEMORY CONTEXT AVAILABLE]\n");
    prompt.push_str("You have access to our conversation history and memories.\n");
    prompt.push_str("Use them naturally when relevant, but don't force references.\n\n");

    // Recent messages
    if !context.recent.is_empty() {
        prompt.push_str("Recent conversation:\n");

        for entry in &context.recent {
            let time_ago = Utc::now().signed_duration_since(entry.timestamp);
            let time_str = if time_ago.num_minutes() < 60 {
                format!("{}m ago", time_ago.num_minutes())
            } else if time_ago.num_hours() < 24 {
                format!("{}h ago", time_ago.num_hours())
            } else {
                format!("{}d ago", time_ago.num_days())
            };

            prompt.push_str(&format!(
                "[{}] {} ({})\n",
                entry.role, entry.content, time_str
            ));
        }
        prompt.push('\n');
    }

    // Semantic memories - filter by salience >= 0.6
    if !context.semantic.is_empty() {
        let important_memories: Vec<_> = context
            .semantic
            .iter()
            .filter(|m| m.salience.unwrap_or(0.0) >= 0.6)
            .collect();

        if !important_memories.is_empty() {
            prompt.push_str("Key memories that might be relevant:\n");
            for memory in important_memories {
                let content = if let Some(summary) = &memory.summary {
                    summary.clone()
                } else {
                    memory
                        .content
                        .split('.')
                        .next()
                        .unwrap_or(&memory.content)
                        .to_string()
                };

                let salience = memory.salience.unwrap_or(0.0);
                prompt.push_str(&format!("- {} (importance: {:.1})\n", content, salience));
            }
            prompt.push('\n');
        }
    }
}

/// Add code intelligence context from semantic search
/// This prevents file path hallucinations by showing real project structure
pub fn add_code_intelligence_context(prompt: &mut String, code_context: Option<&[MemoryEntry]>) {
    if let Some(entries) = code_context {
        if entries.is_empty() {
            return;
        }

        prompt.push_str("[PROJECT CODE CONTEXT]\n");
        prompt.push_str(
            "Here are relevant code elements from the project based on semantic search.\n",
        );
        prompt.push_str(
            "These are REAL files and functions - use these paths and names exactly.\n\n",
        );

        for entry in entries {
            // Parse the content which should be in format: "file_path: element_name - signature"
            let content = &entry.content;

            // Try to extract file path and element info
            if let Some((file_part, element_part)) = content.split_once(':') {
                let file_path = file_part.trim();
                let element_info = element_part.trim();

                prompt.push_str(&format!("- {}\n", file_path));
                prompt.push_str(&format!("  {}\n", element_info));
            } else {
                // Fallback: just print the content as-is
                prompt.push_str(&format!("- {}\n", content));
            }

            prompt.push('\n');
        }

        prompt.push_str("Use these actual file paths when referencing project files.\n");
        prompt.push_str("Don't invent or hallucinate file paths - if you need more files, use the search_code tool.\n\n");
    }
}

/// Add repository structure context
/// Shows high-level file tree to prevent path hallucinations
pub fn add_repository_structure(prompt: &mut String, file_tree: Option<&[FileNode]>) {
    if let Some(tree) = file_tree {
        if tree.is_empty() {
            return;
        }

        prompt.push_str("[REPOSITORY STRUCTURE]\n");
        prompt.push_str("Here is the high-level structure of the repository.\n");
        prompt.push_str("These are REAL paths - use them when referencing project files.\n\n");

        // Show top-level structure (directories and key files)
        let dirs: Vec<_> = tree
            .iter()
            .filter(|n| matches!(n.node_type, FileNodeType::Directory))
            .take(20) // Limit to avoid context bloat
            .collect();

        let files: Vec<_> = tree
            .iter()
            .filter(|n| matches!(n.node_type, FileNodeType::File))
            .take(30) // Show more files than dirs
            .collect();

        if !dirs.is_empty() {
            prompt.push_str("Key directories:\n");
            for dir in dirs {
                prompt.push_str(&format!("  [DIR] {}/\n", dir.path));
            }
            prompt.push('\n');
        }

        if !files.is_empty() {
            prompt.push_str("Key files:\n");
            for file in files {
                prompt.push_str(&format!("  [FILE] {}\n", file.path));
            }
            prompt.push('\n');
        }

        prompt.push_str("Use these actual paths when referencing files.\n");
        prompt.push_str("For more details, use the get_project_context or search_code tools.\n\n");
    }
}

/// Add tool context to the prompt
pub fn add_tool_context(prompt: &mut String, tools: Option<&[Tool]>) {
    if let Some(tool_list) = tools {
        if tool_list.is_empty() {
            return;
        }

        prompt.push_str(&format!("[TOOLS AVAILABLE: {} tools]\n", tool_list.len()));

        for tool in tool_list {
            if let Some(func) = &tool.function {
                prompt.push_str(&format!("- {}: {}\n", func.name, func.description));
            }
        }

        prompt.push_str("\nTOOL USAGE:\n");
        prompt.push_str("- When the user asks you to perform an action (write a file, execute a command, etc.), USE THE TOOL DIRECTLY.\n");
        prompt.push_str("- Don't say you can't do something if you have a tool for it.\n");
        prompt.push_str("- For code-related responses, include brief conversational context with your tool calls.\n");
        prompt.push_str("- For file operations and system commands, prioritize action over explanation.\n\n");
    }
}

/// Add file context to the prompt
pub fn add_file_context(prompt: &mut String, metadata: Option<&MessageMetadata>) {
    if let Some(meta) = metadata {
        let mut context_added = false;

        if let Some(path) = &meta.file_path {
            prompt.push_str(&format!("[VIEWING FILE: {}]\n", path));

            if let Some(lang) = &meta.language {
                prompt.push_str(&format!("Language: {}\n", lang));
            }

            if let Some(content) = &meta.file_content {
                // NO TRUNCATION - send full file
                prompt.push_str("Current file content:\n");
                prompt.push_str("```\n");
                prompt.push_str(content);
                prompt.push_str("\n```\n");
            }

            prompt.push_str("The user expects you to be aware of what's in this file.\n");
            context_added = true;
        }

        if let Some(repo_id) = &meta.repo_id {
            prompt.push_str(&format!("[REPOSITORY: {}]\n", repo_id));
            context_added = true;
        }

        if let Some(selection) = &meta.selection {
            if selection.start_line != selection.end_line {
                prompt.push_str(&format!(
                    "[SELECTED LINES: {}-{}]\n",
                    selection.start_line, selection.end_line
                ));

                if let Some(text) = &selection.text {
                    // NO TRUNCATION - send full selection
                    prompt.push_str(&format!("```\n{}\n```\n", text));
                }
                context_added = true;
            }
        }

        if context_added {
            prompt.push('\n');
        }
    }
}
