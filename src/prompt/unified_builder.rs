// src/prompt/unified_builder.rs
// FIXED: Enforce explanatory text with all tool calls

use crate::api::ws::message::MessageMetadata;
use crate::memory::features::recall_engine::RecallContext;
use crate::tools::types::Tool;
use crate::persona::PersonaOverlay;
use crate::config::CONFIG;
use chrono::Utc;

// Code intelligence types for context formatting
#[derive(Debug, Clone)]
pub struct CodeElement {
    pub element_type: String,
    pub name: String,
    pub start_line: i64,
    pub end_line: i64,
    pub complexity: Option<i64>,
    pub is_async: Option<bool>,
    pub is_public: Option<bool>,
    pub documentation: Option<String>,
}

#[derive(Debug, Clone)]
pub struct QualityIssue {
    pub severity: String,
    pub category: String,
    pub description: String,
    pub element_name: Option<String>,
    pub suggestion: Option<String>,
}

// Error context for code fix operations
#[derive(Debug, Clone)]
pub struct ErrorContext {
    pub error_message: String,
    pub file_path: String,
    pub error_type: String,
    pub error_severity: String,
    pub original_line_count: usize,
}

pub struct UnifiedPromptBuilder;

impl UnifiedPromptBuilder {
    /// Build system prompt for Mira (conversational AI)
    /// Pure personality from persona/default.rs - no system meta-info
    pub fn build_system_prompt(
        persona: &PersonaOverlay,
        context: &RecallContext,
        tools: Option<&[Tool]>,
        metadata: Option<&MessageMetadata>,
        project_id: Option<&str>,
    ) -> String {
        let mut prompt = String::new();
        
        // 1. Core personality - pure, unmodified
        prompt.push_str(persona.prompt());
        prompt.push_str("\n\n");
        
        // 2. Context only - no system architecture notes
        Self::add_project_context(&mut prompt, metadata, project_id);
        Self::add_memory_context(&mut prompt, context);
        Self::add_tool_context(&mut prompt, tools);
        Self::add_file_context(&mut prompt, metadata);
        
        // 3. Light tool usage hints (if code-related)
        if Self::is_code_related(metadata) {
            Self::add_tool_usage_hints(&mut prompt);
        }
        
        prompt
    }
    
    /// Build prompt for code fixes with personality intact
    /// Used when Mira needs to provide technical fixes
    pub fn build_code_fix_prompt(
        persona: &PersonaOverlay,
        context: &RecallContext,
        error_context: &ErrorContext,
        file_content: &str,
        metadata: Option<&MessageMetadata>,
        project_id: Option<&str>,
        code_elements: Option<Vec<CodeElement>>,
        quality_issues: Option<Vec<QualityIssue>>,
    ) -> String {
        let mut prompt = String::new();
        
        // Keep persona for Mira's direct code fixes
        prompt.push_str(persona.prompt());
        prompt.push_str("\n\n");
        
        Self::add_project_context(&mut prompt, metadata, project_id);
        Self::add_memory_context(&mut prompt, context);
        Self::add_code_fix_requirements(
            &mut prompt, 
            error_context, 
            file_content,
            code_elements,
            quality_issues,
        );
        
        prompt
    }
    
    /// Build prompt for pure technical code operations (no personality)
    /// Only used when personality would interfere with technical accuracy
    pub fn build_technical_code_prompt(
        error_context: &ErrorContext,
        file_content: &str,
        code_elements: Option<Vec<CodeElement>>,
        quality_issues: Option<Vec<QualityIssue>>,
    ) -> String {
        let mut prompt = String::new();
        
        prompt.push_str("You are a code fix specialist.\n");
        prompt.push_str("Generate complete, working file fixes with no personality or commentary.\n\n");
        
        Self::add_code_fix_requirements(
            &mut prompt,
            error_context,
            file_content,
            code_elements,
            quality_issues,
        );
        
        prompt
    }
    
    /// Tool usage hints with mandatory conversational context
    fn add_tool_usage_hints(prompt: &mut String) {
        prompt.push_str("[CODE HANDLING]\n");
        prompt.push_str("For code-related tasks, use the appropriate tools:\n");
        prompt.push_str("- 'create_artifact' - For any code you write (examples, new files, fixes, etc)\n");
        prompt.push_str("- 'search_code' - For finding code elements in projects\n");
        prompt.push_str("- 'get_project_context' - For understanding project structure\n\n");
        
        prompt.push_str("CRITICAL CONVERSATION REQUIREMENTS:\n");
        prompt.push_str("NEVER respond with ONLY a tool call and no explanatory text.\n");
        prompt.push_str("Every response with a tool call MUST include conversational text that:\n");
        prompt.push_str("- Explains what you're doing and why\n");
        prompt.push_str("- Describes the approach or solution\n");
        prompt.push_str("- Guides the user on what to expect or do next\n");
        prompt.push_str("- Maintains your personality and connection with the user\n\n");
        
        prompt.push_str("Example BAD response: [creates artifact with zero text]\n");
        prompt.push_str("Example GOOD response: \"Alright, here's a streamBuffer utility that'll batch those 3500 chunks...\"\n");
        prompt.push_str("[then creates artifact]\n\n");
        
        prompt.push_str("Artifacts display in a Monaco editor where users can edit and apply changes.\n\n");
    }
    
    /// Full technical requirements for code generation
    /// Model-agnostic - works with any LLM backend
    fn add_code_fix_requirements(
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
        prompt.push_str("This is not a code review or partial fix - you must provide COMPLETE files.\n\n");
        
        prompt.push_str("REQUIREMENTS:\n");
        prompt.push_str(&format!("1. The original file has {} lines. Your fixed file should have a similar line count.\n", line_count));
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
        let language = error_context.file_path
            .rsplit('.')
            .next()
            .map(|ext| match ext {
                "rs" => "rust",
                "ts" | "tsx" => "typescript",
                "js" | "jsx" => "javascript",
                "py" => "python",
                "go" => "go",
                "java" => "java",
                "cpp" | "cc" => "cpp",
                "c" => "c",
                _ => "text"
            })
            .unwrap_or("text");
        
        prompt.push_str(&format!("- Language: {}\n", language));
        prompt.push_str("\n");
        
        prompt.push_str("ERROR MESSAGE:\n");
        prompt.push_str("```\n");
        prompt.push_str(&error_context.error_message);
        prompt.push_str("\n```\n\n");
        
        // Add code intelligence context if available
        Self::add_code_intelligence_context(prompt, &code_elements, &quality_issues);
        
        prompt.push_str("COMPLETE ORIGINAL FILE CONTENT:\n");
        prompt.push_str("```");
        prompt.push_str(language);
        prompt.push_str("\n");
        prompt.push_str(file_content);
        prompt.push_str("\n```\n\n");
        
        prompt.push_str("VALIDATION:\n");
        prompt.push_str("- Count the lines in your response before submitting.\n");
        prompt.push_str(&format!("- Your fixed file should have approximately {} lines.\n", line_count));
        prompt.push_str("- If your output is significantly shorter, you have omitted code.\n");
        prompt.push_str("- The system will reject responses with ellipsis or incomplete code.\n\n");
        
        prompt.push_str("MULTI-FILE FIXES:\n");
        prompt.push_str("If fixing this error requires changes to other files:\n");
        prompt.push_str("1. Include ALL affected files as COMPLETE files.\n");
        prompt.push_str("2. Mark the primary file (with the error) as change_type: 'primary'.\n");
        prompt.push_str("3. Mark import updates as change_type: 'import'.\n");
        prompt.push_str("4. Mark type definition updates as change_type: 'type'.\n");
        prompt.push_str("5. Mark other cascading changes as change_type: 'cascade'.\n\n");
        
        prompt.push_str("Remember: Users cannot merge partial code. Provide complete, working files.\n");
        prompt.push_str("=========================================\n\n");
    }
    
    fn add_code_intelligence_context(
        prompt: &mut String,
        code_elements: &Option<Vec<CodeElement>>,
        quality_issues: &Option<Vec<QualityIssue>>,
    ) {
        let has_elements = code_elements.as_ref().map_or(false, |e| !e.is_empty());
        let has_issues = quality_issues.as_ref().map_or(false, |i| !i.is_empty());
        
        if !has_elements && !has_issues {
            return;
        }
        
        prompt.push_str("==== CODE STRUCTURE ANALYSIS ====\n\n");
        
        // Format code elements
        if let Some(elements) = code_elements {
            if !elements.is_empty() {
                prompt.push_str("Code Elements Found:\n");
                
                for element in elements {
                    // Basic element info
                    prompt.push_str(&format!(
                        "  - {} `{}` (lines {}-{})",
                        element.element_type,
                        element.name,
                        element.start_line,
                        element.end_line
                    ));
                    
                    // Add complexity if available
                    if let Some(complexity) = element.complexity {
                        prompt.push_str(&format!(" [complexity: {}]", complexity));
                    }
                    
                    // Add async/public flags
                    let mut flags = Vec::new();
                    if element.is_async == Some(true) {
                        flags.push("async");
                    }
                    if element.is_public == Some(true) {
                        flags.push("pub");
                    }
                    if !flags.is_empty() {
                        prompt.push_str(&format!(" [{}]", flags.join(", ")));
                    }
                    
                    prompt.push('\n');
                    
                    // Add full documentation - NO TRUNCATION
                    if let Some(doc) = &element.documentation {
                        prompt.push_str(&format!("    Doc: {}\n", doc));
                    }
                }
                
                prompt.push('\n');
            }
        }
        
        // Format quality issues
        if let Some(issues) = quality_issues {
            if !issues.is_empty() {
                prompt.push_str("Detected Quality Issues:\n");
                
                for issue in issues {
                    prompt.push_str(&format!(
                        "  - [{}] {}: {}\n",
                        issue.severity.to_uppercase(),
                        issue.category,
                        issue.description
                    ));
                    
                    if let Some(element) = &issue.element_name {
                        prompt.push_str(&format!("    In: {}\n", element));
                    }
                    
                    if let Some(suggestion) = &issue.suggestion {
                        prompt.push_str(&format!("    Suggested: {}\n", suggestion));
                    }
                }
                
                prompt.push('\n');
            }
        }
        
        prompt.push_str("Use this structural understanding when generating your fix.\n");
        prompt.push_str("====================================\n\n");
    }
    
    pub fn is_code_related(metadata: Option<&MessageMetadata>) -> bool {
        if let Some(meta) = metadata {
            if meta.file_path.is_some() || meta.file_content.is_some() {
                return true;
            }
            
            if let Some(lang) = &meta.language {
                let code_languages = ["rust", "typescript", "javascript", "python", "go", "java", "cpp", "c"];
                if code_languages.contains(&lang.to_lowercase().as_str()) {
                    return true;
                }
            }
            
            if meta.repo_id.is_some() || meta.has_repository == Some(true) {
                return true;
            }
        }
        false
    }
    
    fn add_project_context(prompt: &mut String, metadata: Option<&MessageMetadata>, project_id: Option<&str>) {
        if let Some(meta) = metadata {
            if let Some(project_name) = &meta.project_name {
                prompt.push_str(&format!(
                    "[ACTIVE PROJECT: {}]\n",
                    project_name
                ));
                
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
    
    fn add_memory_context(prompt: &mut String, context: &RecallContext) {
        // Add summaries if config enabled
        if CONFIG.use_rolling_summaries_in_context {
            if let Some(session) = &context.session_summary {
                prompt.push_str("\n## SESSION OVERVIEW (Entire Conversation)\n");
                prompt.push_str("This is a comprehensive summary of your entire conversation history:\n\n");
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
                
                prompt.push_str(&format!("[{}] {} ({})\n", entry.role, entry.content, time_str));
            }
            prompt.push('\n');
        }
        
        // Semantic memories - filter by salience >= 0.6
        if !context.semantic.is_empty() {
            let important_memories: Vec<_> = context.semantic.iter()
                .filter(|m| m.salience.unwrap_or(0.0) >= 0.6)
                .collect();
            
            if !important_memories.is_empty() {
                prompt.push_str("Key memories that might be relevant:\n");
                for memory in important_memories {
                    let content = if let Some(summary) = &memory.summary {
                        summary.clone()
                    } else {
                        memory.content.split('.').next().unwrap_or(&memory.content).to_string()
                    };
                    
                    let salience = memory.salience.unwrap_or(0.0);
                    prompt.push_str(&format!("- {} (importance: {:.1})\n", content, salience));
                }
                prompt.push('\n');
            }
        }
    }
    
    fn add_tool_context(prompt: &mut String, tools: Option<&[Tool]>) {
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
            
            prompt.push_str("\nCRITICAL: Always provide conversational text explaining what you're doing when using tools.\n");
            prompt.push_str("Never respond with only a tool call - the user needs context and explanation.\n");
            prompt.push_str("Integrate tool usage naturally into your responses with proper setup and follow-through.\n\n");
        }
    }
    
    fn add_file_context(prompt: &mut String, metadata: Option<&MessageMetadata>) {
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
                        selection.start_line, 
                        selection.end_line
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
    
    pub fn build_simple_prompt(
        persona: &PersonaOverlay,
        context: &RecallContext,
        project_id: Option<&str>,
    ) -> String {
        Self::build_system_prompt(
            persona,
            context,
            None,
            None,
            project_id,
        )
    }
}
