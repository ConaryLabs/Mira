// src/prompt/unified_builder.rs

use crate::api::ws::message::MessageMetadata;
use crate::memory::features::recall_engine::RecallContext;
use crate::llm::responses::types::Tool;
use crate::llm::structured::code_fix_processor::ErrorContext;
use crate::persona::PersonaOverlay;
use chrono::Utc;

pub struct UnifiedPromptBuilder;

impl UnifiedPromptBuilder {
    pub fn build_system_prompt(
        persona: &PersonaOverlay,
        context: &RecallContext,
        tools: Option<&[Tool]>,
        metadata: Option<&MessageMetadata>,
        project_id: Option<&str>,
    ) -> String {
        let mut prompt = String::new();
        
        prompt.push_str(persona.prompt());
        prompt.push_str("\n\n");
        
        Self::add_project_context(&mut prompt, metadata, project_id);
        Self::add_memory_context(&mut prompt, context);
        Self::add_tool_context(&mut prompt, tools);
        Self::add_file_context(&mut prompt, metadata);
        
        if Self::is_code_related(metadata) {
            Self::add_code_best_practices(&mut prompt);
        }
        
        prompt
    }
    
    pub fn build_code_fix_prompt(
        persona: &PersonaOverlay,
        context: &RecallContext,
        error_context: &ErrorContext,
        file_content: &str,
        metadata: Option<&MessageMetadata>,
        project_id: Option<&str>,
    ) -> String {
        let mut prompt = String::new();
        
        prompt.push_str(persona.prompt());
        prompt.push_str("\n\n");
        
        Self::add_project_context(&mut prompt, metadata, project_id);
        Self::add_memory_context(&mut prompt, context);
        Self::add_code_fix_requirements(&mut prompt, error_context, file_content);
        
        prompt
    }
    
    fn add_code_fix_requirements(prompt: &mut String, error_context: &ErrorContext, file_content: &str) {
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
        prompt.push_str(&format!("- Error Type: {}\n", error_context.error_type));
        prompt.push_str(&format!("- File: {}\n", error_context.file_path));
        if let Some(line) = error_context.line_number {
            prompt.push_str(&format!("- Error Line: {}\n", line));
        }
        if let Some(lang) = &error_context.language {
            prompt.push_str(&format!("- Language: {}\n", lang));
        }
        prompt.push_str("\n");
        
        prompt.push_str("ERROR MESSAGE:\n");
        prompt.push_str("```\n");
        prompt.push_str(&error_context.error_message);
        prompt.push_str("\n```\n\n");
        
        prompt.push_str("COMPLETE ORIGINAL FILE CONTENT:\n");
        prompt.push_str("```");
        if let Some(lang) = &error_context.language {
            prompt.push_str(lang);
        }
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
    
    fn add_code_best_practices(prompt: &mut String) {
        prompt.push_str("\n\n");
        prompt.push_str("==== CODE GENERATION GUIDELINES ====\n");
        prompt.push_str("When providing code:\n");
        prompt.push_str("- Always provide complete, runnable code\n");
        prompt.push_str("- Include necessary imports and dependencies\n");
        prompt.push_str("- Add helpful comments for complex logic\n");
        prompt.push_str("- Follow language-specific best practices and idioms\n");
        prompt.push_str("- Consider error handling and edge cases\n");
        prompt.push_str("- If modifying existing code, provide the complete updated version\n");
        prompt.push_str("=====================================\n\n");
    }
    
    fn add_project_context(prompt: &mut String, metadata: Option<&MessageMetadata>, project_id: Option<&str>) {
        if let Some(meta) = metadata {
            if let Some(project_name) = &meta.project_name {
                prompt.push_str(&format!("[ACTIVE PROJECT: {}]", project_name));
                
                if meta.has_repository == Some(true) {
                    prompt.push_str(" - Git repository attached");
                    
                    if let Some(branch) = &meta.branch {
                        prompt.push_str(&format!(" (branch: {})", branch));
                    }
                    
                    if let Some(repo_root) = &meta.repo_root {
                        prompt.push_str(&format!(" at {}", repo_root));
                    }
                }
                
                prompt.push_str("\n");
                prompt.push_str("When the user refers to 'the project', 'this project', asks about files, ");
                prompt.push_str("code, repository, or project-related questions without specifying which ");
                prompt.push_str("project, they mean this one. ");
                
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
        if context.recent.is_empty() && context.semantic.is_empty() {
            return;
        }
        
        prompt.push_str("[MEMORY CONTEXT AVAILABLE]\n");
        prompt.push_str("You have access to our conversation history and memories. ");
        prompt.push_str("Use them naturally when relevant, but don't force references.\n\n");
        
        if !context.recent.is_empty() {
            prompt.push_str("Recent conversation:\n");
            
            let recent_slice = if context.recent.len() > 10 {
                &context.recent[context.recent.len() - 10..]
            } else {
                &context.recent
            };
            
            for entry in recent_slice {
                let time_ago = Utc::now().signed_duration_since(entry.timestamp);
                let time_str = if time_ago.num_minutes() < 60 {
                    format!("{}m ago", time_ago.num_minutes())
                } else if time_ago.num_hours() < 24 {
                    format!("{}h ago", time_ago.num_hours())
                } else {
                    format!("{}d ago", time_ago.num_days())
                };
                
                let content = Self::truncate_safely(&entry.content, 200);
                
                prompt.push_str(&format!("[{}] {} ({})\n", entry.role, content, time_str));
            }
            prompt.push('\n');
        }
        
        if !context.semantic.is_empty() {
            let important_memories: Vec<_> = context.semantic.iter()
                .filter(|m| m.salience.unwrap_or(0.0) >= 7.0)
                .take(3)
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
                let description = if let Some(func) = &tool.function {
                    format!("- {}: {}", func.name, func.description)
                } else {
                    match tool.tool_type.as_str() {
                        "code_interpreter" => "- Code Interpreter: Execute Python code and analyze data".to_string(),
                        "image_generation" => "- Image Generation: Create images from text descriptions".to_string(),
                        "file_search" => "- File Search: Search through uploaded documents".to_string(),
                        "web_search" => "- Web Search: Search the internet for information".to_string(),
                        _ => format!("- {}: Available tool", tool.tool_type),
                    }
                };
                prompt.push_str(&format!("{}\n", description));
            }
            
            prompt.push_str("Use tools as appropriate. You should integrate tool results naturally into the conversation.\n");
            prompt.push('\n');
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
                    let preview = Self::truncate_safely(content, 1000);
                    prompt.push_str("Current file content (truncated if large):\n");
                    prompt.push_str("```\n");
                    prompt.push_str(&preview);
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
                        let preview = Self::truncate_safely(text, 500);
                        prompt.push_str(&format!("```\n{}\n```\n", preview));
                    }
                    context_added = true;
                }
            }
            
            if context_added {
                prompt.push('\n');
            }
        }
    }
    
    fn truncate_safely(s: &str, max_chars: usize) -> String {
        if s.chars().count() <= max_chars {
            return s.to_string();
        }
        
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{}...", truncated)
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
