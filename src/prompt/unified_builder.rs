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
                
                // Truncate very long messages
                let content = if entry.content.len() > 200 {
                    format!("{}...", &entry.content[..200])
                } else {
                    entry.content.clone()
                };
                
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
    
    /// Add file/selection context from metadata
    fn add_file_context(prompt: &mut String, metadata: Option<&MessageMetadata>) {
        if let Some(meta) = metadata {
            let mut context_added = false;
            
            if let Some(file_path) = &meta.file_path {
                prompt.push_str(&format!("[FILE CONTEXT: {}]", file_path));
                context_added = true;
                
                if let Some(language) = &meta.language {
                    prompt.push_str(&format!(" ({})", language));
                }
                prompt.push('\n');
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
                        let preview = if text.len() > 500 {
                            format!("{}...", &text[..500])
                        } else {
                            text.clone()
                        };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::core::types::MemoryEntry;
    use crate::api::ws::message::{MessageMetadata, TextSelection};
    use chrono::Utc;
    
    #[test]
    fn test_unified_prompt_basic() {
        let persona = PersonaOverlay::Default;
        let context = RecallContext {
            recent: vec![],
            semantic: vec![],
        };
        
        let prompt = UnifiedPromptBuilder::build_simple_prompt(&persona, &context, None);
        
        assert!(prompt.contains("You are Mira"));
        assert!(!prompt.contains("CRITICAL OVERRIDE"));
        assert!(prompt.contains("Remember: You are ALWAYS Mira"));
        
        assert!(!prompt.contains("TOOLS AVAILABLE"));
        assert!(!prompt.contains("ACTIVE PROJECT"));
        assert!(!prompt.contains("JSON object"));
    }
    
    #[test]
    fn test_unified_prompt_with_project_metadata() {
        let persona = PersonaOverlay::Default;
        let context = RecallContext {
            recent: vec![],
            semantic: vec![],
        };
        
        let metadata = MessageMetadata {
            file_path: None,
            repo_id: None,
            attachment_id: None,
            language: None,
            selection: None,
            project_name: Some("mira-backend".to_string()),
            has_repository: Some(true),
            repo_root: Some("./repos/test".to_string()),
            branch: Some("main".to_string()),
            request_repo_context: Some(true),
        };
        
        let prompt = UnifiedPromptBuilder::build_system_prompt(
            &persona,
            &context,
            None,
            Some(&metadata),
            None,
        );
        
        assert!(prompt.contains("ACTIVE PROJECT: mira-backend"));
        assert!(prompt.contains("Git repository attached"));
        assert!(prompt.contains("branch: main"));
        assert!(prompt.contains("repository context"));
    }
    
    #[test] 
    fn test_unified_prompt_with_modern_memory() {
        let persona = PersonaOverlay::Default;
        
        // Create a MemoryEntry with MODERN structure
        let memory_entry = MemoryEntry {
            id: Some(1),
            session_id: "test-session".to_string(),
            response_id: None,
            parent_id: None,
            role: "user".to_string(),
            content: "Hello Mira, how's the code analysis going?".to_string(),
            timestamp: Utc::now(), // MODERN: timestamp not created_at
            tags: Some(vec!["greeting".to_string(), "code".to_string()]),
            salience: Some(8.5), // High importance
            topics: Some(vec!["code_analysis".to_string()]),
            contains_code: Some(false),
            programming_lang: None,
            // ... other fields with None defaults
            mood: None,
            intensity: None,
            intent: None,
            summary: None,
            relationship_impact: None,
            language: Some("en".to_string()),
            analyzed_at: None,
            analysis_version: None,
            routed_to_heads: None,
            last_recalled: None,
            recall_count: None,
            model_version: None,
            prompt_tokens: None,
            completion_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
            latency_ms: None,
            generation_time_ms: None,
            finish_reason: None,
            tool_calls: None,
            temperature: None,
            max_tokens: None,
            reasoning_effort: None,
            verbosity: None,
            embedding: None,
            embedding_heads: None,
            qdrant_point_ids: None,
        };
        
        let context = RecallContext {
            recent: vec![memory_entry.clone()],
            semantic: vec![memory_entry],
        };
        
        let prompt = UnifiedPromptBuilder::build_simple_prompt(&persona, &context, None);
        
        assert!(prompt.contains("MEMORY CONTEXT AVAILABLE"));
        assert!(prompt.contains("Recent conversation:"));
        assert!(prompt.contains("[user] Hello Mira"));
        assert!(prompt.contains("code analysis"));
        assert!(prompt.contains("Key memories"));
        assert!(prompt.contains("importance: 8.5"));
    }
    
    #[test]
    fn test_unified_prompt_with_json_requirement() {
        let persona = PersonaOverlay::Default;
        let context = RecallContext {
            recent: vec![],
            semantic: vec![],
        };
        
        assert!(!prompt.contains("JSON object"));
        assert!(!prompt.contains("STRUCTURED RESPONSE"));
    }
}
