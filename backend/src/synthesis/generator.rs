// src/synthesis/generator.rs
// Tool code generation with LLM and validation

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};

use crate::llm::provider::{Gemini3Provider, Message, ThinkingLevel};
use crate::prompt::internal::synthesis as prompts;

use super::storage::SynthesisStorage;
use super::types::*;

/// Configuration for tool generation
#[derive(Debug, Clone)]
pub struct GeneratorConfig {
    pub max_retries: u32,
    pub workspace_root: PathBuf,
    pub save_failures: bool,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            workspace_root: PathBuf::from("."),
            save_failures: false,
        }
    }
}

/// LLM-based tool generator
pub struct ToolGenerator {
    llm: Gemini3Provider,
    storage: Arc<SynthesisStorage>,
    config: GeneratorConfig,
}

impl ToolGenerator {
    pub fn new(llm: Gemini3Provider, storage: Arc<SynthesisStorage>) -> Self {
        Self {
            llm,
            storage,
            config: GeneratorConfig::default(),
        }
    }

    pub fn with_config(mut self, config: GeneratorConfig) -> Self {
        self.config = config;
        self
    }

    /// Generate a tool from a pattern
    pub async fn generate(&self, pattern: &ToolPattern, project_id: &str) -> Result<GenerationResult> {
        info!("Generating tool from pattern: {}", pattern.pattern_name);

        // Generate tool name from pattern
        let tool_name = self.generate_tool_name(pattern);

        // Check if tool already exists
        if let Ok(Some(_)) = self.storage.get_tool(&tool_name).await {
            return Ok(GenerationResult::failure(
                tool_name,
                "Tool already exists".to_string(),
                Vec::new(),
            ));
        }

        // Generate code with retry loop (escalating reasoning effort)
        let result = self.generate_with_retry(pattern, &tool_name).await?;

        // Store the tool if generation succeeded
        if result.success {
            if let Some(ref source_code) = result.source_code {
                let tool = SynthesizedTool {
                    id: uuid::Uuid::new_v4().to_string(),
                    project_id: project_id.to_string(),
                    tool_pattern_id: pattern.id,
                    name: tool_name.clone(),
                    description: pattern.description.clone(),
                    version: 1,
                    source_code: source_code.clone(),
                    language: "rust".to_string(),
                    compilation_status: CompilationStatus::Pending,
                    compilation_error: None,
                    binary_path: None,
                    enabled: false,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                };

                self.storage.store_tool(&tool).await?;

                // Mark pattern as having generated tool
                if let Some(pattern_id) = pattern.id {
                    self.storage.mark_pattern_tool_generated(pattern_id).await?;
                }

                info!("Tool {} generated successfully", tool_name);
            }
        }

        Ok(result)
    }

    /// Generate tool name from pattern
    fn generate_tool_name(&self, pattern: &ToolPattern) -> String {
        // Convert pattern name to valid tool name
        let name = pattern
            .pattern_name
            .to_lowercase()
            .replace(' ', "_")
            .replace('-', "_");

        // Add suffix based on pattern type
        match pattern.pattern_type {
            PatternType::ApiCall => format!("{}_client", name),
            PatternType::DatabaseQuery => format!("{}_query", name),
            PatternType::FileOperation => format!("{}_file", name),
            PatternType::Validation => format!("{}_validator", name),
            _ => format!("{}_tool", name),
        }
    }

    /// Generate code with retry loop using escalating reasoning
    async fn generate_with_retry(
        &self,
        pattern: &ToolPattern,
        tool_name: &str,
    ) -> Result<GenerationResult> {
        let mut all_errors = Vec::new();

        for attempt in 1..=self.config.max_retries {
            // Escalate thinking level with each attempt
            let thinking_level = match attempt {
                1 => ThinkingLevel::Low,
                _ => ThinkingLevel::High,
            };

            info!(
                "Generation attempt {} for {} with {:?} thinking",
                attempt, tool_name, thinking_level
            );

            // Generate code
            let code = if attempt == 1 {
                self.generate_initial_code(pattern, tool_name, thinking_level)
                    .await?
            } else {
                self.generate_retry_code(pattern, tool_name, &all_errors, thinking_level)
                    .await?
            };

            // Validate code (basic syntax check)
            let validation = self.validate_code(&code, tool_name);

            if validation.is_valid {
                return Ok(GenerationResult::success(
                    tool_name.to_string(),
                    code,
                    attempt,
                ));
            }

            // Collect errors for next attempt
            all_errors.extend(validation.errors);
            warn!(
                "Generation attempt {} failed with {} errors",
                attempt,
                all_errors.len()
            );
        }

        Ok(GenerationResult::failure(
            tool_name.to_string(),
            format!("Failed after {} attempts", self.config.max_retries),
            all_errors,
        ))
    }

    /// Generate initial code
    async fn generate_initial_code(
        &self,
        pattern: &ToolPattern,
        tool_name: &str,
        thinking_level: ThinkingLevel,
    ) -> Result<String> {
        let system_prompt = self.get_generation_system_prompt();
        let user_prompt = self.get_initial_generation_prompt(pattern, tool_name);

        let messages = vec![Message::user(user_prompt)];

        let response = self
            .llm
            .complete_with_thinking(messages, system_prompt, thinking_level)
            .await
            .context("LLM code generation failed")?;

        // Extract code from response
        self.extract_code(&response.content)
    }

    /// Generate retry code with error feedback
    async fn generate_retry_code(
        &self,
        pattern: &ToolPattern,
        tool_name: &str,
        errors: &[String],
        thinking_level: ThinkingLevel,
    ) -> Result<String> {
        let system_prompt = self.get_generation_system_prompt();
        let user_prompt = self.get_retry_generation_prompt(pattern, tool_name, errors);

        let messages = vec![Message::user(user_prompt)];

        let response = self
            .llm
            .complete_with_thinking(messages, system_prompt, thinking_level)
            .await
            .context("LLM code generation failed")?;

        self.extract_code(&response.content)
    }

    /// Get system prompt for code generation
    fn get_generation_system_prompt(&self) -> String {
        prompts::CODE_GENERATOR.to_string()
    }

    /// Get initial generation prompt
    fn get_initial_generation_prompt(&self, pattern: &ToolPattern, tool_name: &str) -> String {
        let locations = pattern
            .example_locations
            .iter()
            .map(|l| format!("- {} (lines {}-{})", l.file_path, l.start_line, l.end_line))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"Generate a Rust tool with the following specifications:

Tool Name: {}
Pattern Type: {}
Description: {}
Frequency: {} occurrences
Confidence: {:.0}%

Example Locations:
{}

Create a complete, working implementation that automates this pattern.
"#,
            tool_name,
            pattern.pattern_type.as_str(),
            pattern.description,
            pattern.detected_occurrences,
            pattern.confidence_score * 100.0,
            locations
        )
    }

    /// Get retry generation prompt with errors
    fn get_retry_generation_prompt(
        &self,
        pattern: &ToolPattern,
        tool_name: &str,
        errors: &[String],
    ) -> String {
        let error_list = errors
            .iter()
            .map(|e| format!("- {}", e))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            r#"Fix the following errors in the {} tool implementation:

Pattern: {} - {}

Errors to fix:
{}

Generate a corrected, complete implementation that resolves all errors.
"#,
            tool_name,
            pattern.pattern_name,
            pattern.description,
            error_list
        )
    }

    /// Extract code from LLM response
    fn extract_code(&self, content: &str) -> Result<String> {
        // Extract code from markdown code blocks
        if content.contains("```rust") {
            if let Some(code) = content.split("```rust").nth(1) {
                if let Some(code) = code.split("```").next() {
                    return Ok(code.trim().to_string());
                }
            }
        }

        // Try generic code block
        if content.contains("```") {
            if let Some(code) = content.split("```").nth(1) {
                if let Some(code) = code.split("```").next() {
                    return Ok(code.trim().to_string());
                }
            }
        }

        // Return as-is if no code blocks
        Ok(content.trim().to_string())
    }

    /// Validate generated code (basic syntax check)
    fn validate_code(&self, code: &str, tool_name: &str) -> ValidationResult {
        let mut errors = Vec::new();

        // Check for required elements
        if !code.contains("impl Tool for") {
            errors.push("Missing Tool trait implementation".to_string());
        }

        if !code.contains("fn name(&self)") {
            errors.push("Missing name() method".to_string());
        }

        if !code.contains("fn definition(&self)") {
            errors.push("Missing definition() method".to_string());
        }

        if !code.contains("async fn execute") {
            errors.push("Missing execute() method".to_string());
        }

        if !code.contains("#[async_trait]") {
            errors.push("Missing #[async_trait] attribute".to_string());
        }

        if !code.contains("ToolResult") {
            errors.push("Missing ToolResult return type".to_string());
        }

        // Check for the tool name in the code
        if !code.contains(tool_name) && !code.contains(&tool_name.replace("_", "")) {
            errors.push(format!("Tool name '{}' not found in code", tool_name));
        }

        ValidationResult {
            is_valid: errors.is_empty(),
            errors,
        }
    }
}

/// Validation result
struct ValidationResult {
    is_valid: bool,
    errors: Vec<String>,
}
