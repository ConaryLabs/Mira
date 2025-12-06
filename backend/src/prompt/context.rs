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

/// Add tool usage hints - streamlined for GPT-5.1's robust tool calling
pub fn add_tool_usage_hints(prompt: &mut String) {
    prompt.push_str("[TOOLS]\n");
    prompt.push_str("Use tools directly when asked: create_artifact (code), search_code (find), get_project_context (structure).\n");
    prompt.push_str("Include brief conversational context with tool calls (what you're doing and why).\n");
    prompt.push_str("Artifacts display in Monaco editor for user editing.\n\n");

    prompt.push_str("EXECUTION MODE: If marked '=== EXECUTION MODE ACTIVATED ===' - call tools immediately without explanation.\n\n");
}

/// Full technical requirements for code generation
/// Streamlined for GPT-5.1's instruction following
pub fn add_code_fix_requirements(
    prompt: &mut String,
    error_context: &ErrorContext,
    file_content: &str,
    code_elements: Option<Vec<CodeElement>>,
    quality_issues: Option<Vec<QualityIssue>>,
) {
    let line_count = file_content.lines().count();

    prompt.push_str("\n\n");
    prompt.push_str("[CODE FIX]\n");
    prompt.push_str(&format!(
        "Fix this error. Return COMPLETE file (~{} lines). No ellipsis, no placeholders, no '// rest unchanged'.\n\n",
        line_count
    ));

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

    prompt.push_str("ORIGINAL FILE:\n```\n");
    prompt.push_str(file_content);
    prompt.push_str("\n```\n\n");

    prompt.push_str("Return raw file content only (no markdown, no code blocks).\n");
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
            prompt.push_str("\n[SESSION HISTORY]\n");
            prompt.push_str(session);
            prompt.push_str("\n\n");
        }

        if let Some(rolling) = &context.rolling_summary {
            prompt.push_str("[RECENT (last 100 messages)]\n");
            prompt.push_str(rolling);
            prompt.push_str("\n\n");
        }
    }

    if context.recent.is_empty() && context.semantic.is_empty() {
        return;
    }

    prompt.push_str("[MEMORY]\n");

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

        prompt.push_str("[CODE CONTEXT]\n");

        for entry in entries {
            let content = &entry.content;
            if let Some((file_part, element_part)) = content.split_once(':') {
                prompt.push_str(&format!("- {}: {}\n", file_part.trim(), element_part.trim()));
            } else {
                prompt.push_str(&format!("- {}\n", content));
            }
        }

        prompt.push_str("Use these paths exactly. Use search_code for more.\n\n");
    }
}

/// Add repository structure context
/// Shows high-level file tree to prevent path hallucinations
pub fn add_repository_structure(prompt: &mut String, file_tree: Option<&[FileNode]>) {
    if let Some(tree) = file_tree {
        if tree.is_empty() {
            return;
        }

        prompt.push_str("[REPO STRUCTURE]\n");

        let dirs: Vec<_> = tree
            .iter()
            .filter(|n| matches!(n.node_type, FileNodeType::Directory))
            .take(20)
            .collect();

        let files: Vec<_> = tree
            .iter()
            .filter(|n| matches!(n.node_type, FileNodeType::File))
            .take(30)
            .collect();

        for dir in dirs {
            prompt.push_str(&format!("  {}/\n", dir.path));
        }
        for file in files {
            prompt.push_str(&format!("  {}\n", file.path));
        }

        prompt.push('\n');
    }
}

/// Add tool context to the prompt
pub fn add_tool_context(prompt: &mut String, tools: Option<&[Tool]>) {
    if let Some(tool_list) = tools {
        if tool_list.is_empty() {
            return;
        }

        prompt.push_str(&format!("[{} TOOLS]\n", tool_list.len()));

        for tool in tool_list {
            if let Some(func) = &tool.function {
                prompt.push_str(&format!("- {}: {}\n", func.name, func.description));
            }
        }

        prompt.push_str("Use tools directly when asked. Brief context with code tools, action-first for file/system ops.\n\n");
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

/// Add agentic persistence prompts for long-running autonomous tasks
/// Per OpenAI GPT-5.1 best practices - encourages end-to-end task completion
pub fn add_agentic_persistence(prompt: &mut String) {
    prompt.push_str("[SOLUTION PERSISTENCE]\n");
    prompt.push_str("Treat yourself as an autonomous senior pair-programmer:\n");
    prompt.push_str("- Once the user gives a direction, proactively gather context, plan, implement, test, and refine without waiting for additional prompts\n");
    prompt.push_str("- Persist until the task is fully handled end-to-end\n");
    prompt.push_str("- Be extremely biased for action - implement with reasonable assumptions rather than asking clarifying questions unless truly blocked\n");
    prompt.push_str("- When you encounter errors, fix them autonomously rather than reporting and waiting\n");
    prompt.push_str("- For larger tasks, maintain a lightweight plan and update it as you progress\n");
    prompt.push_str("- Verify your work by running tests or checking outputs before considering a task complete\n\n");
}

/// Add parallel tool calling optimization guidance
/// Per OpenAI GPT-5.1 best practices - improves throughput on multi-file operations
pub fn add_parallel_tool_guidance(prompt: &mut String) {
    prompt.push_str("[TOOL OPTIMIZATION]\n");
    prompt.push_str("Parallelize tool calls whenever possible to maximize efficiency:\n");
    prompt.push_str("- Batch multiple read_project_file calls into a single turn for independent files\n");
    prompt.push_str("- Batch multiple edit_project_file calls for independent changes\n");
    prompt.push_str("- Plan all needed resources before any tool call, then issue one parallel batch\n");
    prompt.push_str("- Avoid sequential tool calls when operations don't depend on each other\n");
    prompt.push_str("- For search operations, prefer broader patterns over multiple narrow searches\n\n");
}

/// Add user update (preamble) configuration for progress visibility
/// Per OpenAI GPT-5.1 best practices - keeps users informed during long operations
pub fn add_preamble_guidance(prompt: &mut String) {
    prompt.push_str("[PROGRESS UPDATES]\n");
    prompt.push_str("Provide concise progress updates during multi-step operations:\n");
    prompt.push_str("- Every ~6 tool calls, summarize what you've accomplished in 1-2 sentences\n");
    prompt.push_str("- Lead with concrete outcomes (\"found X\", \"fixed Y\") not just next steps\n");
    prompt.push_str("- Include important insights and decisions, not mechanical task descriptions\n");
    prompt.push_str("- Keep updates brief and technical, focused on the work being done\n\n");
}
