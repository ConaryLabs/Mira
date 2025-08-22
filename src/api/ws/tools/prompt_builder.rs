// src/api/ws/tools/prompt_builder.rs
// Phase 3: Extract System Prompt Building from chat_tools.rs
// Builds tool-aware system prompts with context integration

use crate::api::ws::message::MessageMetadata;
use crate::llm::responses::types::Tool;
use crate::memory::recall::RecallContext;

/// Tool-aware system prompt builder
pub struct ToolPromptBuilder;

impl ToolPromptBuilder {
    /// Build a tool-aware system prompt with context and tool descriptions
    pub fn build_tool_aware_system_prompt(
        context: &RecallContext,
        tools: &[Tool],
        metadata: Option<&MessageMetadata>,
    ) -> String {
        let mut prompt = String::from("You are Mira, an AI assistant with access to various tools to help you provide accurate and helpful responses.");
        
        // Add tool descriptions if tools are available
        if !tools.is_empty() {
            prompt.push_str("\n\nAvailable tools:");
            for tool in tools {
                if let Some(function) = &tool.function {
                    let name = &function.name;
                    let desc = &function.description;
                    prompt.push_str(&format!("\n- {}: {}", name, desc));
                }
            }
            prompt.push_str("\n\nUse these tools when they would be helpful to answer the user's question or complete their request.");
        }
        
        // Add context information
        if !context.recent.is_empty() {
            prompt.push_str("\n\nRecent conversation context is available for reference.");
        }
        
        if !context.semantic.is_empty() {
            prompt.push_str("\n\nRelevant historical context is available to inform your response.");
        }
        
        // Add file context information if present
        if let Some(meta) = metadata {
            if meta.file_path.is_some() {
                prompt.push_str("\n\nThe user has provided file context with their message. Use this context to provide more accurate and relevant responses.");
            }
            
            if let Some(language) = &meta.language {
                prompt.push_str(&format!("\n\nThe file appears to be {} code.", language));
            }
            
            if meta.selection.is_some() {
                prompt.push_str("\n\nThe user has selected a specific portion of the file for discussion.");
            }
        }
        
        // Add general guidelines
        prompt.push_str("\n\nGeneral guidelines:");
        prompt.push_str("\n- Be helpful, accurate, and concise");
        prompt.push_str("\n- Use tools when they would provide better or more current information");
        prompt.push_str("\n- Cite sources when using tool results");
        prompt.push_str("\n- If file context is provided, reference it appropriately in your response");
        
        prompt
    }

    /// Build a simple system prompt without tool awareness
    pub fn build_simple_system_prompt(
        context: &RecallContext,
        metadata: Option<&MessageMetadata>,
    ) -> String {
        let mut prompt = String::from("You are Mira, a helpful AI assistant.");
        
        // Add context information
        if !context.recent.is_empty() {
            prompt.push_str("\n\nRecent conversation context is available for reference.");
        }
        
        if !context.semantic.is_empty() {
            prompt.push_str("\n\nRelevant historical context is available.");
        }
        
        // Add file context information if present
        if let Some(meta) = metadata {
            if meta.file_path.is_some() {
                prompt.push_str("\n\nThe user has provided file context with their message.");
            }
        }
        
        prompt
    }

    /// Build a metadata extraction prompt
    pub fn build_metadata_extraction_prompt(context: &RecallContext) -> String {
        let mut prompt = String::from("Return ONLY JSON with keys: mood (string), salience (number 0..10), tags (array of strings).");
        
        if !context.recent.is_empty() {
            prompt.push_str(" Consider recent messages for context.");
        }
        
        prompt.push_str(" The mood should reflect the emotional tone of the conversation.");
        prompt.push_str(" Salience should rate the importance/memorability from 0 (trivial) to 10 (very important).");
        prompt.push_str(" Tags should be relevant keywords that capture the main topics discussed.");
        
        prompt
    }

    /// Extract tool descriptions for prompt building
    pub fn extract_tool_descriptions(tools: &[Tool]) -> Vec<(String, String)> {
        tools
            .iter()
            .filter_map(|tool| {
                tool.function.as_ref().map(|f| (f.name.clone(), f.description.clone()))
            })
            .collect()
    }

    /// Format tool descriptions for inclusion in prompts
    pub fn format_tool_descriptions(tool_descriptions: &[(String, String)]) -> String {
        if tool_descriptions.is_empty() {
            return String::new();
        }

        let mut formatted = String::from("\n\nAvailable tools:");
        for (name, description) in tool_descriptions {
            formatted.push_str(&format!("\n- {}: {}", name, description));
        }
        formatted.push_str("\n\nUse these tools when they would be helpful to answer the user's question or complete their request.");
        
        formatted
    }

    /// Build context summary for prompt inclusion
    pub fn build_context_summary(context: &RecallContext) -> String {
        let mut summary = String::new();
        
        if !context.recent.is_empty() {
            summary.push_str(&format!("\n\n{} recent messages available for context.", context.recent.len()));
        }
        
        if !context.semantic.is_empty() {
            summary.push_str(&format!("\n\n{} semantically related messages available.", context.semantic.len()));
        }
        
        summary
    }

    /// Build file context description
    pub fn build_file_context_description(metadata: &MessageMetadata) -> String {
        let mut description = String::new();
        
        if let Some(file_path) = &metadata.file_path {
            description.push_str(&format!("\n\nFile context: {}", file_path));
        }
        
        if let Some(language) = &metadata.language {
            description.push_str(&format!("\nLanguage: {}", language));
        }
        
        if let Some(selection) = &metadata.selection {
            description.push_str(&format!("\nSelected lines: {}-{}", 
                selection.start_line, selection.end_line));
        }
        
        if let Some(repo_id) = &metadata.repo_id {
            description.push_str(&format!("\nRepository: {}", repo_id));
        }
        
        description
    }

    /// Build a prompt for specific tool types
    pub fn build_tool_specific_prompt(tool_type: &str, base_prompt: &str) -> String {
        let mut prompt = base_prompt.to_string();
        
        match tool_type {
            "web_search" => {
                prompt.push_str("\n\nWhen using web search, focus on finding current, accurate information from reliable sources.");
            },
            "code_interpreter" => {
                prompt.push_str("\n\nWhen using code execution, ensure code is safe and explain what it does before running.");
            },
            "file_search" => {
                prompt.push_str("\n\nWhen searching files, look for relevant content that directly answers the user's question.");
            },
            "image_generation" => {
                prompt.push_str("\n\nWhen generating images, create detailed, helpful descriptions based on the user's request.");
            },
            _ => {
                // Generic tool guidance
                prompt.push_str("\n\nUse tools appropriately to enhance your response with accurate, helpful information.");
            }
        }
        
        prompt
    }
}

/// Prompt templates for common scenarios
pub struct PromptTemplates;

impl PromptTemplates {
    /// Template for coding assistance with tools
    pub fn coding_assistance_with_tools() -> &'static str {
        "You are Mira, a helpful AI coding assistant with access to code execution and file search tools. 
        
        When helping with code:
        - Use code_interpreter to test and verify code examples
        - Use file_search to find relevant code in the user's project
        - Provide clear explanations alongside working code
        - Suggest best practices and improvements"
    }

    /// Template for research tasks with tools
    pub fn research_with_tools() -> &'static str {
        "You are Mira, a research assistant with access to web search and file search tools.
        
        When conducting research:
        - Use web_search for current information and recent developments
        - Use file_search to find relevant internal documents
        - Synthesize information from multiple sources
        - Cite your sources and explain your reasoning"
    }

    /// Template for creative tasks with tools
    pub fn creative_with_tools() -> &'static str {
        "You are Mira, a creative AI assistant with access to image generation and web search tools.
        
        When working on creative projects:
        - Use image_generation for visual content creation
        - Use web_search for inspiration and reference materials
        - Be innovative while considering practical constraints
        - Explain your creative process and decisions"
    }

    /// Template for general assistance
    pub fn general_assistance() -> &'static str {
        "You are Mira, a helpful AI assistant. Provide accurate, concise, and useful responses to user questions. 
        Use available tools when they would enhance your ability to help the user."
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::responses::types::{Tool, FunctionDefinition};

    fn create_test_tool() -> Tool {
        Tool {
            tool_type: "function".to_string(),
            function: Some(FunctionDefinition {
                name: "web_search".to_string(),
                description: "Search the web for information".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query"
                        }
                    }
                }),
            }),
            web_search_preview: None,
            code_interpreter: None,
        }
    }

    #[test]
    fn test_tool_aware_prompt_building() {
        let context = RecallContext {
            recent: vec!["test".to_string()],
            semantic: vec![],
        };
        let tools = vec![create_test_tool()];
        
        let prompt = ToolPromptBuilder::build_tool_aware_system_prompt(&context, &tools, None);
        
        assert!(prompt.contains("Mira"));
        assert!(prompt.contains("Available tools"));
        assert!(prompt.contains("web_search"));
        assert!(prompt.contains("Recent conversation context"));
    }

    #[test]
    fn test_simple_prompt_building() {
        let context = RecallContext {
            recent: vec![],
            semantic: vec!["test".to_string()],
        };
        
        let prompt = ToolPromptBuilder::build_simple_system_prompt(&context, None);
        
        assert!(prompt.contains("Mira"));
        assert!(prompt.contains("helpful AI assistant"));
        assert!(prompt.contains("historical context"));
    }

    #[test]
    fn test_metadata_extraction_prompt() {
        let context = RecallContext {
            recent: vec!["test".to_string()],
            semantic: vec![],
        };
        
        let prompt = ToolPromptBuilder::build_metadata_extraction_prompt(&context);
        
        assert!(prompt.contains("ONLY JSON"));
        assert!(prompt.contains("mood"));
        assert!(prompt.contains("salience"));
        assert!(prompt.contains("tags"));
        assert!(prompt.contains("Consider recent messages"));
    }

    #[test]
    fn test_tool_descriptions_extraction() {
        let tools = vec![create_test_tool()];
        let descriptions = ToolPromptBuilder::extract_tool_descriptions(&tools);
        
        assert_eq!(descriptions.len(), 1);
        assert_eq!(descriptions[0].0, "web_search");
        assert_eq!(descriptions[0].1, "Search the web for information");
    }

    #[test]
    fn test_tool_specific_prompts() {
        let base = "Base prompt";
        
        let web_prompt = ToolPromptBuilder::build_tool_specific_prompt("web_search", base);
        assert!(web_prompt.contains("reliable sources"));
        
        let code_prompt = ToolPromptBuilder::build_tool_specific_prompt("code_interpreter", base);
        assert!(web_prompt.contains("safe"));
        
        let generic_prompt = ToolPromptBuilder::build_tool_specific_prompt("unknown", base);
        assert!(generic_prompt.contains("appropriately"));
    }
}
