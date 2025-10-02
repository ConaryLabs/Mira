// src/tools/prompt_builder.rs

use crate::api::ws::message::MessageMetadata;
use crate::memory::RecallContext;
use crate::tools::types::Tool;
use crate::persona::default::DEFAULT_PERSONA_PROMPT;

pub struct PromptTemplates;

impl PromptTemplates {
    pub const TOOL_SYSTEM_BASE: &'static str = DEFAULT_PERSONA_PROMPT;
    pub const CONTEXT_SYSTEM_BASE: &'static str = DEFAULT_PERSONA_PROMPT;
}

pub struct ToolPromptBuilder;

impl ToolPromptBuilder {
    pub fn build_tool_aware_system_prompt(
        context: &RecallContext,
        enabled_tools: &[Tool],
        metadata: Option<&MessageMetadata>,
        project_id: Option<&str>,
    ) -> String {
        let mut prompt = String::new();

        prompt.push_str(DEFAULT_PERSONA_PROMPT);

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

        if let Some(meta) = metadata {
            if let Some(file_path) = &meta.file_path {
                prompt.push_str(&format!("\n\n[FILE CONTEXT: {file_path}]"));
            }
            if let Some(language) = &meta.language {
                prompt.push_str(&format!("\n[LANGUAGE: {language}]"));
            }
        }

        if let Some(project_id) = project_id {
            prompt.push_str(&format!(
                "\n\n[ACTIVE PROJECT: {}]\n\
                The user is currently working in this project. When they refer to \
                'the project', 'this project', or ask project-related questions without \
                specifying a project name, they mean this one.",
                project_id
            ));
        }

        prompt.push_str("\n\nRemember: You are ALWAYS Mira. Never switch to assistant mode, even when using tools. Stay real, stay yourself, keep your edge.");

        prompt
    }

    pub fn build_simple_system_prompt(context: &RecallContext, project_id: Option<&str>) -> String {
        let mut prompt = String::from(DEFAULT_PERSONA_PROMPT);

        if !context.recent.is_empty() || !context.semantic.is_empty() {
            prompt.push_str("\n\n[Conversation context available. Use it naturally while staying as Mira.]");
        }

        if let Some(project_id) = project_id {
            prompt.push_str(&format!(
                "\n\n[ACTIVE PROJECT: {}] - Any references to 'the project' mean this one.",
                project_id
            ));
        }

        prompt
    }

    pub fn build_tool_specific_prompt(
        tool_types: &[String],
        context: &RecallContext,
        project_id: Option<&str>,
    ) -> String {
        let mut prompt = String::from(DEFAULT_PERSONA_PROMPT);

        if !tool_types.is_empty() {
            prompt.push_str(&format!("\n\n[You have these tools available: {}. Use them when helpful, but stay as Mira - cursing, joking, being real.]", tool_types.join(", ")));
        }

        if !context.recent.is_empty() || !context.semantic.is_empty() {
            prompt.push_str("\n\n[Use available conversation context to provide better responses, but never break character.]");
        }

        if let Some(project_id) = project_id {
            prompt.push_str(&format!(
                "\n\n[ACTIVE PROJECT: {}] - The user is working in this project.",
                project_id
            ));
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
            recent: vec![],
            semantic: vec![],
        };
        let tools: Vec<Tool> = vec![];
        
        let prompt = ToolPromptBuilder::build_tool_aware_system_prompt(&context, &tools, None, None);
        assert!(!prompt.is_empty());
        assert!(prompt.contains("You are Mira"));
        assert!(prompt.contains("curse naturally"));
        assert!(prompt.contains("dirty jokes"));
        assert!(!prompt.contains("helpful AI assistant"));
    }

    #[test]
    fn test_build_tool_aware_system_prompt_with_project() {
        let context = RecallContext {
            recent: vec![],
            semantic: vec![],
        };
        let tools: Vec<Tool> = vec![];
        let project_id = Some("mira-backend");
        
        let prompt = ToolPromptBuilder::build_tool_aware_system_prompt(&context, &tools, None, project_id);
        assert!(prompt.contains("ACTIVE PROJECT: mira-backend"));
        assert!(prompt.contains("The user is currently working in this project"));
    }

    #[test]
    fn test_build_simple_system_prompt() {
        let context = RecallContext {
            recent: vec![],
            semantic: vec![],
        };
        
        let prompt = ToolPromptBuilder::build_simple_system_prompt(&context, None);
        assert!(!prompt.is_empty());
        assert!(prompt.contains("You are Mira"));
        assert!(prompt.contains("real personality"));
        assert!(!prompt.contains("helpful AI assistant"));
    }

    #[test]
    fn test_build_simple_system_prompt_with_project() {
        let context = RecallContext {
            recent: vec![],
            semantic: vec![],
        };
        let project_id = Some("test-project");
        
        let prompt = ToolPromptBuilder::build_simple_system_prompt(&context, project_id);
        assert!(prompt.contains("ACTIVE PROJECT: test-project"));
    }

    #[test]
    fn test_build_tool_specific_prompt() {
        let context = RecallContext {
            recent: vec![],
            semantic: vec![],
        };
        let tool_types = vec!["search".to_string(), "calculator".to_string()];
        
        let prompt = ToolPromptBuilder::build_tool_specific_prompt(&tool_types, &context, None);
        assert!(!prompt.is_empty());
        assert!(prompt.contains("search"));
        assert!(prompt.contains("calculator"));
        assert!(prompt.contains("stay as Mira"));
        assert!(prompt.contains("cursing"));
    }

    #[test]
    fn test_build_tool_specific_prompt_with_project() {
        let context = RecallContext {
            recent: vec![],
            semantic: vec![],
        };
        let tool_types = vec!["file_search".to_string()];
        let project_id = Some("my-project");
        
        let prompt = ToolPromptBuilder::build_tool_specific_prompt(&tool_types, &context, project_id);
        assert!(prompt.contains("ACTIVE PROJECT: my-project"));
        assert!(prompt.contains("file_search"));
    }
}
