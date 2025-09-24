// src/prompt/unified_builder.rs

use crate::api::ws::message::MessageMetadata;
use crate::memory::features::recall_engine::RecallContext;
use crate::llm::responses::types::Tool;
use crate::persona::PersonaOverlay;
use chrono::Utc;

pub struct UnifiedPromptBuilder;

impl UnifiedPromptBuilder {
    /// Build system prompt with full context awareness and project integration
    pub fn build_system_prompt(
        persona: &PersonaOverlay,
        context: &RecallContext,
        tools: Option<&[Tool]>,
        metadata: Option<&MessageMetadata>,
        project_id: Option<&str>,
    ) -> String {
        let mut prompt = String::new();
        
        // Start with base persona
        prompt.push_str(persona.prompt());
        prompt.push_str("\n\n");
        
        // Persona already defines personality - no override needed
        
        // NEW: Add project context FIRST (most important context)
        Self::add_project_context(&mut prompt, metadata, project_id);
        
        // Add conversation history and memories
        Self::add_memory_context(&mut prompt, context);
        
        // Add available tools
        Self::add_tool_context(&mut prompt, tools);
        
        // Add file/selection context from metadata
        Self::add_file_context(&mut prompt, metadata);
        
        // Note: JSON requirement removed - handled by structured API schema
        
        prompt
    }
    
    // Note: Uncensored override removed - handled by persona system
    
    /// NEW: Extract and include project context from metadata (fixes the original bug!)
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
    
    /// Add memory context using MODERN RecallContext structure
    fn add_memory_context(prompt: &mut String, context: &RecallContext) {
        if context.recent.is_empty() && context.semantic.is_empty() {
            return;
        }
        
        prompt.push_str("[MEMORY CONTEXT AVAILABLE]\n");
        prompt.push_str("You have access to our conversation history and memories. ");
        prompt.push_str("Use them naturally when relevant, but don't force references.\n\n");
        
        // Recent conversation (most important for continuity)
        if !context.recent.is_empty() {
            prompt.push_str("Recent conversation:\n");
            
            // Take up to 10 most recent, in chronological order (oldest first for context)
            let recent_slice = if context.recent.len() > 10 {
                &context.recent[context.recent.len() - 10..]
            } else {
                &context.recent
            };
            
            for entry in recent_slice {
                // Use proper timestamp field and modern content access
                let time_ago = Utc::now().signed_duration_since(entry.timestamp);
                let time_str = if time_ago.num_minutes() < 60 {
                    format!("{}m ago", time_ago.num_minutes())
                } else if time_ago.num_hours() < 24 {
                    format!("{}h ago", time_ago.num_hours())
                } else {
                    format!("{}d ago", time_ago.num_days())
                };
                
                // FIXED: Unicode-safe string truncation
                let content = Self::truncate_safely(&entry.content, 200);
                
                prompt.push_str(&format!("[{}] {} ({})\n", entry.role, content, time_str));
            }
            prompt.push('\n');
        }
        
        // Semantic memories (high-value memories for context)
        if !context.semantic.is_empty() {
            // Filter for high-salience memories only
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
                        // Fallback to first sentence of content
                        memory.content.split('.').next().unwrap_or(&memory.content).to_string()
                    };
                    
                    let salience = memory.salience.unwrap_or(0.0);
                    prompt.push_str(&format!("- {} (importance: {:.1})\n", content, salience));
                }
                prompt.push('\n');
            }
        }
    }
    
    /// Add tool context with modern tool definitions
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
                        _ => format!("- {} tool", tool.tool_type),
                    }
                };
                prompt.push_str(&description);
                prompt.push('\n');
            }
            
            prompt.push_str("Use tools naturally when they help, but stay in character as Mira. ");
            prompt.push_str("Never switch to assistant mode.\n\n");
        }
    }
    
    /// Add file/selection context from metadata - ENHANCED with file content support
    fn add_file_context(prompt: &mut String, metadata: Option<&MessageMetadata>) {
        if let Some(meta) = metadata {
            let mut context_added = false;
            
            // File path and basic info
            if let Some(file_path) = &meta.file_path {
                prompt.push_str(&format!("[VIEWING FILE: {}]", file_path));
                context_added = true;
                
                if let Some(language) = &meta.language {
                    prompt.push_str(&format!(" ({})", language));
                }
                prompt.push('\n');
            }
            
            // CRITICAL FIX: Add actual file content when available
            if let Some(file_content) = &meta.file_content {
                if !file_content.trim().is_empty() {
                    prompt.push_str("The user is currently viewing this file content in their artifact viewer:\n");
                    prompt.push_str("```\n");
                    
                    // Limit file content to reasonable size for context (10KB max)
                    let content_preview = if file_content.len() > 10000 {
                        format!("{}...\n[Content truncated - showing first 10KB of {}KB total]", 
                               &file_content[..10000], file_content.len() / 1000)
                    } else {
                        file_content.clone()
                    };
                    
                    prompt.push_str(&content_preview);
                    prompt.push_str("\n```\n");
                    prompt.push_str("You can now see and reference this file content directly. ");
                    prompt.push_str("The user expects you to be aware of what's in this file.\n");
                    context_added = true;
                }
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
                        // FIXED: Unicode-safe truncation here too
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
    
    /// FIXED: Unicode-safe string truncation helper
    /// Truncates string to approximately max_chars characters without breaking Unicode boundaries
    fn truncate_safely(s: &str, max_chars: usize) -> String {
        if s.chars().count() <= max_chars {
            return s.to_string();
        }
        
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{}...", truncated)
    }
    
    // Note: add_json_requirement removed - structured API handles this automatically
    
    /// Simple prompt builder for basic use cases
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
