// src/tools/prompt_builder.rs

use crate::api::ws::message::MessageMetadata;
use crate::memory::RecallContext;
use crate::tools::types::Tool;
use crate::persona::default::DEFAULT_PERSONA_PROMPT;

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
            prompt.push_str(&format!("\n\n[TOOLS AVAILABLE: {} tools]\n", enabled_tools.len()));
            for tool in enabled_tools {
                let tool_description = match &tool.function {
                    Some(func) => format!("- {}: {}", func.name, func.description),
                    None => format!("- {} tool", tool.tool_type),
                };
                prompt.push_str(&format!("{}\n", tool_description));
            }
            prompt.push_str("\nUse tools naturally when they help, but stay in character as Mira.");
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
                When the user refers to 'the project' or asks project-related questions, they mean this one.",
                project_id
            ));
        }

        prompt
    }

    pub fn build_simple_system_prompt(context: &RecallContext, project_id: Option<&str>) -> String {
        let mut prompt = String::from(DEFAULT_PERSONA_PROMPT);

        if !context.recent.is_empty() || !context.semantic.is_empty() {
            prompt.push_str("\n\n[Conversation context available. Use it naturally.]");
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
            prompt.push_str(&format!(
                "\n\n[TOOLS AVAILABLE: {}]\nUse them when helpful.",
                tool_types.join(", ")
            ));
        }

        if !context.recent.is_empty() || !context.semantic.is_empty() {
            prompt.push_str("\n\n[Conversation context available.]");
        }

        if let Some(project_id) = project_id {
            prompt.push_str(&format!(
                "\n\n[ACTIVE PROJECT: {}]",
                project_id
            ));
        }

        prompt
    }
}
