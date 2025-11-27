// src/synthesis/evolver.rs
// Tool effectiveness tracking and evolution

use anyhow::{Context, Result};
use chrono::Utc;
use std::sync::Arc;
use tracing::{info, warn};

use crate::llm::provider::{Gpt5Provider, Message, ReasoningEffort};
use crate::prompt::internal::synthesis as prompts;

use super::storage::SynthesisStorage;
use super::types::*;

/// Configuration for tool evolution
#[derive(Debug, Clone)]
pub struct EvolverConfig {
    /// Minimum effectiveness threshold (0.0 to 1.0)
    pub evolution_threshold: f64,
    /// Minimum executions before evolution consideration
    pub min_executions_for_evolution: i64,
    /// Maximum retries for code improvement
    pub max_improvement_retries: u32,
}

impl Default for EvolverConfig {
    fn default() -> Self {
        Self {
            evolution_threshold: 0.7,
            min_executions_for_evolution: 10,
            max_improvement_retries: 2,
        }
    }
}

/// Tool evolver improves tools based on effectiveness metrics
pub struct ToolEvolver {
    llm: Gpt5Provider,
    storage: Arc<SynthesisStorage>,
    config: EvolverConfig,
}

impl ToolEvolver {
    pub fn new(llm: Gpt5Provider, storage: Arc<SynthesisStorage>) -> Self {
        Self {
            llm,
            storage,
            config: EvolverConfig::default(),
        }
    }

    pub fn with_config(mut self, config: EvolverConfig) -> Self {
        self.config = config;
        self
    }

    /// Evolve a tool to improve its effectiveness
    pub async fn evolve_tool(
        &self,
        tool_name: &str,
        reason: EvolutionReason,
        force: bool,
    ) -> Result<SynthesizedTool> {
        info!("Evolving tool: {} (reason: {:?})", tool_name, reason);

        // Get current tool
        let mut tool = self
            .storage
            .get_tool(tool_name)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Tool not found: {}", tool_name))?;

        // Get effectiveness metrics
        let effectiveness = self
            .storage
            .get_effectiveness(&tool.id)
            .await?
            .unwrap_or_else(|| ToolEffectiveness {
                tool_id: tool.id.clone(),
                total_executions: 0,
                successful_executions: 0,
                failed_executions: 0,
                average_duration_ms: None,
                total_time_saved_ms: 0,
                last_executed: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            });

        // Check if evolution is warranted
        if !force && !self.should_evolve(&tool, &effectiveness, &reason) {
            return Err(anyhow::anyhow!(
                "Tool does not meet evolution criteria (success rate: {:.1}%, executions: {})",
                effectiveness.success_rate() * 100.0,
                effectiveness.total_executions
            ));
        }

        // Analyze for improvements
        let suggestions = self
            .analyze_for_improvements(&tool, &effectiveness)
            .await?;

        if suggestions.is_empty() {
            return Err(anyhow::anyhow!("No improvement suggestions identified"));
        }

        // Generate improved code
        let improved_code = self
            .generate_improved_code(&tool, &suggestions)
            .await
            .context("Failed to generate improved code")?;

        // Record evolution
        let evolution = ToolEvolution {
            id: None,
            tool_id: tool.id.clone(),
            old_version: tool.version,
            new_version: tool.version + 1,
            change_description: suggestions.join("; "),
            motivation: Some(reason.as_str().to_string()),
            source_code_diff: None, // Could compute diff here
            evolved_at: Utc::now(),
        };

        self.storage.record_evolution(&evolution).await?;

        // Update tool
        tool.version += 1;
        tool.source_code = improved_code;
        tool.compilation_status = CompilationStatus::Pending;
        tool.updated_at = Utc::now();

        self.storage.update_tool(&tool).await?;

        info!(
            "Tool {} evolved to version {}",
            tool_name, tool.version
        );

        Ok(tool)
    }

    /// Check if a tool should be evolved
    fn should_evolve(
        &self,
        _tool: &SynthesizedTool,
        effectiveness: &ToolEffectiveness,
        reason: &EvolutionReason,
    ) -> bool {
        // Require minimum executions
        if effectiveness.total_executions < self.config.min_executions_for_evolution {
            return false;
        }

        match reason {
            EvolutionReason::LowEffectiveness => {
                effectiveness.is_below_threshold(self.config.evolution_threshold)
            }
            EvolutionReason::UserFeedback => true,
            EvolutionReason::Manual => true,
            EvolutionReason::PatternChange => true,
        }
    }

    /// Analyze tool for potential improvements
    async fn analyze_for_improvements(
        &self,
        _tool: &SynthesizedTool,
        effectiveness: &ToolEffectiveness,
    ) -> Result<Vec<String>> {
        let mut suggestions = Vec::new();

        // Check failure rate
        if effectiveness.failed_executions > 0 {
            let failure_rate =
                effectiveness.failed_executions as f64 / effectiveness.total_executions as f64;

            if failure_rate > 0.3 {
                suggestions.push(format!(
                    "High failure rate ({:.1}%) - improve error handling and edge cases",
                    failure_rate * 100.0
                ));
            }
        }

        // Check performance
        if let Some(avg_duration) = effectiveness.average_duration_ms {
            if avg_duration > 1000.0 {
                suggestions.push(format!(
                    "Slow execution (avg {:.0}ms) - optimize performance",
                    avg_duration
                ));
            }
        }

        // If no specific issues, suggest general improvements
        if suggestions.is_empty() {
            suggestions.push("General code quality improvements".to_string());
        }

        Ok(suggestions)
    }

    /// Generate improved code using LLM
    async fn generate_improved_code(
        &self,
        tool: &SynthesizedTool,
        suggestions: &[String],
    ) -> Result<String> {
        let system_prompt = prompts::CODE_EVOLVER;

        let user_prompt = format!(
            r#"Improve the following tool implementation:

Tool: {}
Description: {}

Current code:
```rust
{}
```

Improvement suggestions:
{}

Generate an improved implementation that addresses these suggestions while maintaining compatibility.
"#,
            tool.name,
            tool.description,
            tool.source_code,
            suggestions
                .iter()
                .map(|s| format!("- {}", s))
                .collect::<Vec<_>>()
                .join("\n")
        );

        let messages = vec![Message::user(user_prompt)];

        let response = self
            .llm
            .complete_with_reasoning(messages, system_prompt.to_string(), ReasoningEffort::High)
            .await
            .context("LLM improvement generation failed")?;

        // Extract code from response
        self.extract_code(&response.content)
    }

    /// Extract code from LLM response
    fn extract_code(&self, content: &str) -> Result<String> {
        if content.contains("```rust") {
            if let Some(code) = content.split("```rust").nth(1) {
                if let Some(code) = code.split("```").next() {
                    return Ok(code.trim().to_string());
                }
            }
        }

        if content.contains("```") {
            if let Some(code) = content.split("```").nth(1) {
                if let Some(code) = code.split("```").next() {
                    return Ok(code.trim().to_string());
                }
            }
        }

        Ok(content.trim().to_string())
    }

    /// Find all tools that need evolution
    pub async fn find_tools_needing_evolution(&self) -> Result<Vec<String>> {
        self.storage
            .get_tools_below_threshold(self.config.evolution_threshold)
            .await
    }

    /// Batch evolve all underperforming tools
    pub async fn evolve_underperforming(&self) -> Result<Vec<(String, Result<SynthesizedTool>)>> {
        let tools_to_evolve = self.find_tools_needing_evolution().await?;

        let mut results = Vec::new();

        for tool_name in tools_to_evolve {
            let result = self
                .evolve_tool(&tool_name, EvolutionReason::LowEffectiveness, false)
                .await;

            if result.is_err() {
                warn!("Failed to evolve tool {}: {:?}", tool_name, result);
            }

            results.push((tool_name, result));
        }

        Ok(results)
    }
}
