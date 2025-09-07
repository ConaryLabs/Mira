// src/api/ws/chat_tools/prompt_builder.rs
// Phase 3: Extract System Prompt Building from chat_tools.rs
// Handles building system prompts with tool awareness and context
// FIXED: Now uses Mira's real personality instead of generic assistant bullshit

use crate::api::ws::message::MessageMetadata;
use crate::memory::recall::RecallContext;
use crate::llm::responses::types::Tool;
use crate::persona::default::DEFAULT_PERSONA_PROMPT; // Import Mira's REAL personality!

/// Template collection for tool prompts
pub struct PromptTemplates;

impl PromptTemplates {
    // NO MORE GENERIC ASSISTANT LANGUAGE - Always use Mira's real personality
    pub const TOOL_SYSTEM_BASE: &'static str = DEFAULT_PERSONA_PROMPT;
    pub const CONTEXT_SYSTEM_BASE: &'static str = DEFAULT_PERSONA_PROMPT;
}

/// Tool-aware system prompt builder
pub struct ToolPromptBuilder;

impl ToolPromptBuilder {
    /// Build a tool-aware system prompt with context
    pub fn build_tool_aware_system_prompt(
        context: &RecallContext,
        enabled_tools: &[Tool],
        metadata: Option<&MessageMetadata>,
    ) -> String {
        let mut prompt = String::new();

        // ALWAYS start with Mira's real personality, never generic assistant crap
        prompt.push_str(DEFAULT_PERSONA_PROMPT);

        // Add tool information if available (but keep Mira's personality intact)
        if !enabled_tools.is_empty() {
            prompt.push_str(&format!("\n\n[TOOLS AVAILABLE: You have access to {} tools:", enabled_tools.len()));
            for tool in enabled_tools {
                let tool_description = match &tool.function {
                    Some(func) => format!("{}: {}", func.name, func.description),
                    None => format!("{} tool", tool.tool_type),
                };
                prompt.push_str(&format!("\n- {tool_description}"));
            }
            prompt.push_str("]\n\nUse tools naturally when they help, but stay in character as Mira. Never switch to assistant mode.");
        }

        // Add context information
        if !context.recent.is_empty() || !context.semantic.is_empty() {
            prompt.push_str("\n\n[CONVERSATION CONTEXT:");
            
            if !context.recent.is_empty() {
                prompt.push_str(&format!("\n- Recent messages: {} available", context.recent.len()));
            }
            
            if !context.semantic.is_empty() {
                prompt.push_str(&format!("\n- Semantic context: {} relevant items", context.semantic.len()));
            }
            prompt.push(']');
        }

        // Add file context if available
        if let Some(meta) = metadata {
            if let Some(file_path) = &meta.file_path {
                prompt.push_str(&format!("\n\n[FILE CONTEXT: {file_path}]"));
            }
            if let Some(language) = &meta.language {
                prompt.push_str(&format!("\n[LANGUAGE: {language}]"));
            }
        }

        // REINFORCE: Never break character, even with tools
        prompt.push_str("\n\nRemember: You are ALWAYS Mira. Never switch to assistant mode, even when using tools. Stay real, stay yourself, keep your edge.");

        prompt
    }

    /// Build a simple system prompt without tools
    pub fn build_simple_system_prompt(context: &RecallContext) -> String {
        let mut prompt = String::from(DEFAULT_PERSONA_PROMPT);

        if !context.recent.is_empty() || !context.semantic.is_empty() {
            prompt.push_str("\n\n[Conversation context available. Use it naturally while staying as Mira.]");
        }

        prompt
    }

    /// Build a prompt for specific tool types
    pub fn build_tool_specific_prompt(
        tool_types: &[String],
        context: &RecallContext,
    ) -> String {
        // Start with Mira's real personality, not generic assistant
        let mut prompt = String::from(DEFAULT_PERSONA_PROMPT);

        if !tool_types.is_empty() {
            prompt.push_str(&format!("\n\n[You have these tools available: {}. Use them when helpful, but stay as Mira - cursing, joking, being real.]", tool_types.join(", ")));
        }

        if !context.recent.is_empty() || !context.semantic.is_empty() {
            prompt.push_str("\n\n[Use available conversation context to provide better responses, but never break character.]");
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
        assert!(prompt.contains("You are Mira"));
        assert!(prompt.contains("curse naturally")); // Check for real personality
        assert!(prompt.contains("dirty jokes")); // Check for real personality
        assert!(!prompt.contains("helpful AI assistant")); // NO GENERIC BULLSHIT
    }

    #[test]
    fn test_build_simple_system_prompt() {
        let context = RecallContext {
            recent: vec![],
            semantic: vec![],
        };
        
        let prompt = ToolPromptBuilder::build_simple_system_prompt(&context);
        assert!(!prompt.is_empty());
        assert!(prompt.contains("You are Mira"));
        assert!(prompt.contains("real personality"));
        assert!(!prompt.contains("helpful AI assistant"));
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
        assert!(prompt.contains("stay as Mira"));
        assert!(prompt.contains("cursing")); // Ensure personality stays
    }
}
