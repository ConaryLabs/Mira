// src/api/ws/chat_tools/prompt_builder.rs
// Phase 3: Extract System Prompt Building from chat_tools.rs
// Handles building system prompts with tool awareness and context

use crate::api::ws::message::MessageMetadata;
use crate::memory::recall::RecallContext;
use crate::llm::responses::types::Tool; // FIXED: Use Tool instead of EnabledTool

/// Template collection for tool prompts
pub struct PromptTemplates;

impl PromptTemplates {
    pub const TOOL_SYSTEM_BASE: &'static str = "You are Mira, a helpful AI assistant with access to tools.";
    pub const CONTEXT_SYSTEM_BASE: &'static str = "You are Mira, a helpful AI assistant. Use available context naturally.";
}

/// Tool-aware system prompt builder
pub struct ToolPromptBuilder;

impl ToolPromptBuilder {
    /// Build a tool-aware system prompt with context
    pub fn build_tool_aware_system_prompt(
        context: &RecallContext,
        enabled_tools: &[Tool], // FIXED: Use Tool instead of EnabledTool
        metadata: Option<&MessageMetadata>,
    ) -> String {
        let mut prompt = String::new();

        // Base prompt
        if enabled_tools.is_empty() {
            prompt.push_str(PromptTemplates::CONTEXT_SYSTEM_BASE);
        } else {
            prompt.push_str(PromptTemplates::TOOL_SYSTEM_BASE);
        }

        // Add tool information
        if !enabled_tools.is_empty() {
            prompt.push_str(&format!("\n\nYou have access to {} tools:", enabled_tools.len()));
            for tool in enabled_tools {
                let tool_description = match &tool.function {
                    Some(func) => format!("{}: {}", func.name, func.description),
                    None => format!("{} tool", tool.tool_type),
                };
                prompt.push_str(&format!("\n- {}", tool_description));
            }
        }

        // Add context information
        if !context.recent.is_empty() || !context.semantic.is_empty() {
            prompt.push_str("\n\nConversation context:");
            
            if !context.recent.is_empty() {
                prompt.push_str(&format!("\n- Recent messages: {} available", context.recent.len()));
            }
            
            if !context.semantic.is_empty() {
                prompt.push_str(&format!("\n- Semantic context: {} relevant items", context.semantic.len()));
            }
        }

        // Add file context if available
        if let Some(meta) = metadata {
            if let Some(file_path) = &meta.file_path {
                prompt.push_str(&format!("\n\nFile context: {}", file_path));
            }
            if let Some(language) = &meta.language {
                prompt.push_str(&format!("\nLanguage: {}", language));
            }
        }

        prompt
    }

    /// Build a simple system prompt without tools
    pub fn build_simple_system_prompt(context: &RecallContext) -> String {
        let mut prompt = String::from(PromptTemplates::CONTEXT_SYSTEM_BASE);

        if !context.recent.is_empty() || !context.semantic.is_empty() {
            prompt.push_str("\n\nConversation context available.");
        }

        prompt
    }

    /// Build a prompt for specific tool types
    pub fn build_tool_specific_prompt(
        tool_types: &[String],
        context: &RecallContext,
    ) -> String {
        let mut prompt = String::from("You are Mira, a helpful AI assistant.");

        if !tool_types.is_empty() {
            prompt.push_str(&format!("\n\nYou have access to these tools: {}", tool_types.join(", ")));
        }

        if !context.recent.is_empty() || !context.semantic.is_empty() {
            prompt.push_str("\n\nUse available conversation context to provide better responses.");
        }

        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_tool_aware_system_prompt() {
        let context = RecallContext {
            recent: vec![], // Empty for test
            semantic: vec![],
        };
        let tools: Vec<Tool> = vec![];
        
        let prompt = ToolPromptBuilder::build_tool_aware_system_prompt(&context, &tools, None);
        assert!(!prompt.is_empty());
        assert!(prompt.contains("Mira"));
    }

    #[test]
    fn test_build_simple_system_prompt() {
        let context = RecallContext {
            recent: vec![],
            semantic: vec![],
        };
        
        let prompt = ToolPromptBuilder::build_simple_system_prompt(&context);
        assert!(!prompt.is_empty());
        assert!(prompt.contains("Mira"));
    }

    #[test]
    fn test_build_tool_specific_prompt() {
        let context = RecallContext {
            recent: vec![],
            semantic: vec![],
        };
        let tool_types = vec!["search".to_string(), "calculator".to_string()];
        
        let prompt = ToolPromptBuilder::build_tool_specific_prompt(&tool_types, &context);
        assert!(!prompt.is_empty());
        assert!(prompt.contains("search"));
        assert!(prompt.contains("calculator"));
    }
}
