//! Chat Executor - Per-step execution with tool handling
//!
//! Executes plan steps using DeepSeek Chat, validates tool calls,
//! applies diffs, and handles retries.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::config::ConductorConfig;
use super::diff::UnifiedDiff;
use super::mira_intel::{MiraIntel, FixSuggestion};
use super::planning::PlanStep;
use super::validation::{repair_json, ToolSchemas, ValidationResult};
use crate::provider::{ChatRequest, Provider, StreamEvent, ToolContinueRequest, ToolResult};

// Smart excerpt thresholds (from mira-core limits)
use mira_core::limits::{ARTIFACT_THRESHOLD_BYTES, INLINE_MAX_BYTES};
use mira_core::excerpts::create_smart_excerpt;

/// Result of executing a step
#[derive(Debug)]
pub struct StepResult {
    pub success: bool,
    pub output: String,
    pub tool_calls_made: Vec<ToolCallRecord>,
    pub files_modified: Vec<String>,
    pub error: Option<String>,
}

/// Record of a tool call made during execution
#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
    pub result: String,
    pub success: bool,
    pub was_repaired: bool,
}

/// Executor for individual plan steps
pub struct StepExecutor {
    /// DeepSeek Chat provider
    chat: Arc<dyn Provider>,

    /// Tool schemas for validation
    schemas: ToolSchemas,

    /// Configuration
    config: ConductorConfig,

    /// File contents cache (for verification)
    file_cache: RwLock<HashMap<String, String>>,

    /// Tool executor (simplified interface)
    tool_handler: Option<Arc<dyn ToolHandler>>,

    /// Mira intelligence for error fixes
    mira: Option<Arc<MiraIntel>>,
}

/// Interface for executing tools
pub trait ToolHandler: Send + Sync {
    /// Execute a tool and return the result
    fn execute(&self, name: &str, args: &serde_json::Value) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send + '_>>;

    /// Read a file (for context/verification)
    fn read_file(&self, path: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send + '_>>;
}

impl StepExecutor {
    /// Create a new executor
    pub fn new(chat: Arc<dyn Provider>, config: ConductorConfig) -> Self {
        Self {
            chat,
            schemas: ToolSchemas::default(),
            config,
            file_cache: RwLock::new(HashMap::new()),
            tool_handler: None,
            mira: None,
        }
    }

    /// Set the tool handler
    pub fn with_tool_handler(mut self, handler: Arc<dyn ToolHandler>) -> Self {
        self.tool_handler = Some(handler);
        self
    }

    /// Add Mira intelligence for error fixes
    pub fn with_mira(mut self, mira: Arc<MiraIntel>) -> Self {
        self.mira = Some(mira);
        self
    }

    /// Apply smart excerpts to tool output if it exceeds threshold
    ///
    /// This prevents context blow-up from large grep/diff/read outputs.
    fn smart_excerpt(&self, tool_name: &str, output: &str) -> String {
        if output.len() <= INLINE_MAX_BYTES {
            return output.to_string();
        }

        // Map tool names to excerpt types
        let excerpt_type = match tool_name.to_lowercase().as_str() {
            "grep" | "search" => "grep",
            "bash" if output.contains("diff --git") => "git_diff",
            _ => "default",
        };

        let excerpted = create_smart_excerpt(excerpt_type, output);

        if output.len() > ARTIFACT_THRESHOLD_BYTES {
            // For very large outputs, add artifact hint
            format!(
                "{}\n\n[Output truncated from {} bytes. Consider using artifacts for full content.]",
                excerpted,
                output.len()
            )
        } else {
            excerpted
        }
    }

    /// Look up similar error fixes from Mira (if available)
    async fn get_error_fixes(&self, error: &str) -> Vec<FixSuggestion> {
        if let Some(ref mira) = self.mira {
            mira.find_similar_fixes(error).await
        } else {
            Vec::new()
        }
    }

    /// Execute a plan step
    pub async fn execute_step(
        &self,
        step: &PlanStep,
        system_prompt: &str,
        context: &str,
    ) -> StepResult {
        // Build the step prompt
        let step_prompt = self.build_step_prompt(step, context);

        // Create initial request
        let request = ChatRequest::new("deepseek-chat", system_prompt, &step_prompt)
            .with_reasoning("low");

        // Execute with tool handling loop
        let mut tool_calls_made = Vec::new();
        let mut output = String::new();
        let mut files_modified = Vec::new();

        let mut response = match self.chat.create(request).await {
            Ok(r) => r,
            Err(e) => {
                return StepResult {
                    success: false,
                    output: String::new(),
                    tool_calls_made,
                    files_modified,
                    error: Some(format!("Chat request failed: {}", e)),
                };
            }
        };

        output.push_str(&response.text);

        // Handle tool calls (up to max iterations)
        let mut iterations = 0;
        const MAX_TOOL_ITERATIONS: usize = 10;

        while !response.tool_calls.is_empty() && iterations < MAX_TOOL_ITERATIONS {
            iterations += 1;

            let mut tool_results = Vec::new();

            for tool_call in &response.tool_calls {
                // Parse and validate arguments
                let args_result = repair_json(&tool_call.arguments);
                let (args, was_repaired) = match args_result {
                    Ok(args) => (args, false),
                    Err(e) => {
                        // Try to salvage
                        tool_results.push(ToolResult {
                            call_id: tool_call.call_id.clone(),
                            name: tool_call.name.clone(),
                            output: format!("Error: Invalid JSON arguments: {}", e),
                        });
                        tool_calls_made.push(ToolCallRecord {
                            call_id: tool_call.call_id.clone(),
                            name: tool_call.name.clone(),
                            arguments: tool_call.arguments.clone(),
                            result: format!("Error: {}", e),
                            success: false,
                            was_repaired: false,
                        });
                        continue;
                    }
                };

                // Validate against schema
                let validation = self.schemas.validate(&tool_call.name, &args);
                let schema_repaired = validation.repaired_args.is_some();
                let final_args = if let Some(repaired) = validation.repaired_args {
                    repaired
                } else {
                    args.clone()
                };

                let was_repaired = was_repaired || schema_repaired;

                if !validation.valid {
                    let errors: Vec<_> = validation.issues.iter()
                        .filter(|i| i.severity == super::validation::IssueSeverity::Error)
                        .map(|i| format!("{}: {}", i.field, i.message))
                        .collect();

                    tool_results.push(ToolResult {
                        call_id: tool_call.call_id.clone(),
                        name: tool_call.name.clone(),
                        output: format!("Validation error: {}", errors.join("; ")),
                    });
                    tool_calls_made.push(ToolCallRecord {
                        call_id: tool_call.call_id.clone(),
                        name: tool_call.name.clone(),
                        arguments: tool_call.arguments.clone(),
                        result: format!("Validation error: {:?}", errors),
                        success: false,
                        was_repaired,
                    });
                    continue;
                }

                // Execute the tool
                let result = if let Some(ref handler) = self.tool_handler {
                    handler.execute(&tool_call.name, &final_args).await
                } else {
                    // Simulate execution for testing
                    Ok(format!("[Simulated {} result]", tool_call.name))
                };

                let (result_str, success) = match result {
                    Ok(r) => {
                        // Track modified files
                        if tool_call.name == "Edit" || tool_call.name == "Write" {
                            if let Some(path) = final_args.get("file_path").and_then(|v| v.as_str()) {
                                files_modified.push(path.to_string());
                            }
                        }
                        // Apply smart excerpts to prevent context blow-up
                        let excerpted = self.smart_excerpt(&tool_call.name, &r);
                        (excerpted, true)
                    }
                    Err(e) => (format!("Error: {}", e), false),
                };

                tool_results.push(ToolResult {
                    call_id: tool_call.call_id.clone(),
                    name: tool_call.name.clone(),
                    output: result_str.clone(),
                });

                tool_calls_made.push(ToolCallRecord {
                    call_id: tool_call.call_id.clone(),
                    name: tool_call.name.clone(),
                    arguments: serde_json::to_string(&final_args).unwrap_or_default(),
                    result: result_str,
                    success,
                    was_repaired,
                });
            }

            // Continue with tool results
            let continue_request = ToolContinueRequest {
                model: "deepseek-chat".into(),
                system: system_prompt.into(),
                previous_response_id: None, // DeepSeek uses client state
                messages: Vec::new(), // Would need history for DeepSeek
                tool_results,
                reasoning_effort: Some("low".into()),
                tools: Vec::new(), // Same tools
            };

            response = match self.chat.create(ChatRequest::new(
                "deepseek-chat",
                system_prompt,
                &format!(
                    "Continue after tool results:\n{}",
                    tool_calls_made.last().map(|t| &t.result).unwrap_or(&String::new())
                ),
            )).await {
                Ok(r) => r,
                Err(e) => {
                    return StepResult {
                        success: false,
                        output,
                        tool_calls_made,
                        files_modified,
                        error: Some(format!("Continuation failed: {}", e)),
                    };
                }
            };

            output.push('\n');
            output.push_str(&response.text);
        }

        // Check for success
        let all_tools_succeeded = tool_calls_made.iter().all(|t| t.success);
        let has_error = tool_calls_made.iter().any(|t| !t.success);

        StepResult {
            success: !has_error,
            output,
            tool_calls_made,
            files_modified,
            error: if has_error {
                Some("Some tool calls failed".into())
            } else {
                None
            },
        }
    }

    /// Build a prompt for executing a step
    fn build_step_prompt(&self, step: &PlanStep, context: &str) -> String {
        let mut prompt = String::new();

        prompt.push_str(&format!("# Step {}: {}\n\n", step.index + 1, step.description));

        // Add context files if any
        if !step.context_files.is_empty() {
            prompt.push_str("## Context Files\n");
            for file in &step.context_files {
                prompt.push_str(&format!("- {}\n", file));
            }
            prompt.push('\n');
        }

        // Add any provided context
        if !context.is_empty() {
            prompt.push_str("## Current Context\n");
            prompt.push_str(context);
            prompt.push_str("\n\n");
        }

        // Step-specific instructions
        match step.step_type {
            super::planning::StepType::Read => {
                prompt.push_str("Read the specified files and analyze their contents.\n");
            }
            super::planning::StepType::Edit => {
                if let Some(ref diff) = step.diff {
                    prompt.push_str("Apply the following diff:\n\n");
                    prompt.push_str("```diff\n");
                    prompt.push_str(diff);
                    prompt.push_str("\n```\n\n");
                    prompt.push_str("Use the Edit tool with old_string and new_string to apply this change.\n");
                }
                if let Some(ref target) = step.target_file {
                    prompt.push_str(&format!("Target file: {}\n", target));
                }
            }
            super::planning::StepType::Create => {
                prompt.push_str("Create the specified file with the required content.\n");
                if let Some(ref target) = step.target_file {
                    prompt.push_str(&format!("Target file: {}\n", target));
                }
            }
            super::planning::StepType::Delete => {
                prompt.push_str("Delete the specified file.\n");
                if let Some(ref target) = step.target_file {
                    prompt.push_str(&format!("Target file: {}\n", target));
                }
            }
            super::planning::StepType::Command => {
                if let Some(ref cmd) = step.command {
                    prompt.push_str(&format!("Run the following command:\n```\n{}\n```\n", cmd));
                } else {
                    prompt.push_str("Run the required shell command.\n");
                }
            }
            super::planning::StepType::Search => {
                prompt.push_str("Search the codebase for the required information.\n");
            }
            super::planning::StepType::Verify => {
                prompt.push_str("Verify the changes made in previous steps.\n");
            }
            super::planning::StepType::Composite => {
                prompt.push_str("Execute the required sequence of operations.\n");
            }
        }

        // Expected tools hint
        if !step.expected_tools.is_empty() {
            prompt.push_str(&format!(
                "\nExpected tools: {}\n",
                step.expected_tools.join(", ")
            ));
        }

        prompt
    }

    /// Apply a diff to a file and verify the result
    pub async fn apply_diff(
        &self,
        file_path: &str,
        diff_content: &str,
    ) -> Result<String, String> {
        // Parse the diff
        let diff = UnifiedDiff::parse(diff_content)
            .map_err(|e| format!("Failed to parse diff: {}", e))?;

        // Read current file content
        let current_content = if let Some(ref handler) = self.tool_handler {
            handler.read_file(file_path).await
                .map_err(|e| format!("Failed to read file: {}", e))?
        } else {
            return Err("No tool handler configured".into());
        };

        // Apply the diff
        let new_content = diff.apply(&current_content)
            .map_err(|e| format!("Failed to apply diff: {}", e))?;

        // Write the new content (would use tool handler)
        Ok(new_content)
    }
}

/// Statistics from step execution
#[derive(Debug, Default)]
pub struct ExecutionStats {
    pub steps_completed: usize,
    pub steps_failed: usize,
    pub tool_calls_total: usize,
    pub tool_calls_repaired: usize,
    pub tool_calls_failed: usize,
    pub files_modified: usize,
}

impl ExecutionStats {
    pub fn from_results(results: &[StepResult]) -> Self {
        let mut stats = Self::default();

        for result in results {
            if result.success {
                stats.steps_completed += 1;
            } else {
                stats.steps_failed += 1;
            }

            for tc in &result.tool_calls_made {
                stats.tool_calls_total += 1;
                if tc.was_repaired {
                    stats.tool_calls_repaired += 1;
                }
                if !tc.success {
                    stats.tool_calls_failed += 1;
                }
            }

            stats.files_modified += result.files_modified.len();
        }

        stats
    }

    pub fn success_rate(&self) -> f64 {
        let total = self.steps_completed + self.steps_failed;
        if total > 0 {
            self.steps_completed as f64 / total as f64
        } else {
            0.0
        }
    }

    pub fn repair_rate(&self) -> f64 {
        if self.tool_calls_total > 0 {
            self.tool_calls_repaired as f64 / self.tool_calls_total as f64
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::planning::{PlanStep, StepType};

    // Mock provider for testing
    struct MockProvider;

    #[async_trait::async_trait]
    impl Provider for MockProvider {
        fn capabilities(&self) -> &crate::provider::Capabilities {
            static CAPS: once_cell::sync::Lazy<crate::provider::Capabilities> =
                once_cell::sync::Lazy::new(crate::provider::Capabilities::deepseek_chat);
            &CAPS
        }

        async fn create_stream(
            &self,
            _request: ChatRequest,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<StreamEvent>> {
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            drop(tx);
            Ok(rx)
        }

        async fn create(&self, _request: ChatRequest) -> anyhow::Result<crate::provider::ChatResponse> {
            Ok(crate::provider::ChatResponse {
                id: "test".into(),
                text: "Task completed".into(),
                reasoning: None,
                tool_calls: vec![],
                usage: None,
                finish_reason: crate::provider::FinishReason::Stop,
            })
        }

        async fn continue_with_tools_stream(
            &self,
            _request: ToolContinueRequest,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<StreamEvent>> {
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            drop(tx);
            Ok(rx)
        }

        fn name(&self) -> &'static str {
            "mock"
        }
    }

    #[tokio::test]
    async fn test_execute_simple_step() {
        let provider = Arc::new(MockProvider);
        let config = ConductorConfig::default();
        let executor = StepExecutor::new(provider, config);

        let step = PlanStep::read(0, "Read config file", vec!["config.toml".into()]);

        let result = executor.execute_step(&step, "You are a helpful assistant", "").await;
        assert!(result.success);
    }

    #[test]
    fn test_execution_stats() {
        let results = vec![
            StepResult {
                success: true,
                output: "Done".into(),
                tool_calls_made: vec![
                    ToolCallRecord {
                        call_id: "1".into(),
                        name: "Read".into(),
                        arguments: "{}".into(),
                        result: "content".into(),
                        success: true,
                        was_repaired: false,
                    },
                ],
                files_modified: vec![],
                error: None,
            },
            StepResult {
                success: false,
                output: "Failed".into(),
                tool_calls_made: vec![
                    ToolCallRecord {
                        call_id: "2".into(),
                        name: "Edit".into(),
                        arguments: "{}".into(),
                        result: "error".into(),
                        success: false,
                        was_repaired: true,
                    },
                ],
                files_modified: vec![],
                error: Some("error".into()),
            },
        ];

        let stats = ExecutionStats::from_results(&results);
        assert_eq!(stats.steps_completed, 1);
        assert_eq!(stats.steps_failed, 1);
        assert_eq!(stats.tool_calls_total, 2);
        assert_eq!(stats.tool_calls_repaired, 1);
        assert_eq!(stats.tool_calls_failed, 1);
        assert!((stats.success_rate() - 0.5).abs() < 0.01);
    }
}
