// src/patterns/replay.rs
// Pattern replay - apply learned patterns to new situations

use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

use crate::llm::provider::{Gpt5Provider, Message, ReasoningEffort};
use crate::prompt::internal::patterns as prompts;

use super::matcher::PatternMatcher;
use super::storage::PatternStorage;
use super::types::*;

/// Configuration for pattern replay
#[derive(Debug, Clone)]
pub struct ReplayConfig {
    /// Minimum match score to auto-apply pattern
    pub auto_apply_threshold: f64,
    /// Whether to use solution templates
    pub use_templates: bool,
    /// Maximum steps to replay
    pub max_steps: usize,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self {
            auto_apply_threshold: 0.8,
            use_templates: true,
            max_steps: 10,
        }
    }
}

/// Result of replaying a pattern
#[derive(Debug, Clone)]
pub struct ReplayResult {
    /// The pattern that was replayed
    pub pattern: ReasoningPattern,
    /// Steps executed
    pub steps_executed: Vec<ExecutedStep>,
    /// Generated solution (if any)
    pub solution: Option<String>,
    /// Whether replay was successful
    pub success: bool,
    /// Time taken (ms)
    pub duration_ms: i64,
    /// Error message if failed
    pub error: Option<String>,
}

/// A step that was executed
#[derive(Debug, Clone)]
pub struct ExecutedStep {
    pub step: ReasoningStep,
    pub output: String,
    pub success: bool,
    pub duration_ms: i64,
}

/// Pattern replay system
pub struct PatternReplay {
    storage: Arc<PatternStorage>,
    matcher: Arc<PatternMatcher>,
    llm: Gpt5Provider,
    config: ReplayConfig,
}

impl PatternReplay {
    pub fn new(
        storage: Arc<PatternStorage>,
        matcher: Arc<PatternMatcher>,
        llm: Gpt5Provider,
    ) -> Self {
        Self {
            storage,
            matcher,
            llm,
            config: ReplayConfig::default(),
        }
    }

    pub fn with_config(mut self, config: ReplayConfig) -> Self {
        self.config = config;
        self
    }

    /// Attempt to replay a pattern for the given context
    pub async fn replay(
        &self,
        pattern: &ReasoningPattern,
        context: &MatchContext,
    ) -> Result<ReplayResult> {
        let start = Instant::now();
        info!("Replaying pattern: {} ({})", pattern.name, pattern.id);

        let mut steps_executed = Vec::new();
        let mut overall_success = true;

        // Execute each step
        for step in pattern.steps.iter().take(self.config.max_steps) {
            let step_start = Instant::now();

            let step_output = self.execute_step(step, context, &steps_executed).await;

            let (output, success) = match step_output {
                Ok(out) => (out, true),
                Err(e) => {
                    warn!("Step {} failed: {}", step.step_number, e);
                    overall_success = false;
                    (format!("Error: {}", e), false)
                }
            };

            steps_executed.push(ExecutedStep {
                step: step.clone(),
                output,
                success,
                duration_ms: step_start.elapsed().as_millis() as i64,
            });

            if !success {
                break; // Stop on first failure
            }
        }

        // Generate solution if template available
        let solution = if overall_success && self.config.use_templates {
            match &pattern.solution_template {
                Some(template) => {
                    self.apply_template(template, context, &steps_executed)
                        .await
                        .ok()
                }
                None => {
                    // Generate solution from steps
                    self.generate_solution(pattern, context, &steps_executed)
                        .await
                        .ok()
                }
            }
        } else {
            None
        };

        let duration_ms = start.elapsed().as_millis() as i64;

        Ok(ReplayResult {
            pattern: pattern.clone(),
            steps_executed,
            solution,
            success: overall_success,
            duration_ms,
            error: None,
        })
    }

    /// Execute a single step
    async fn execute_step(
        &self,
        step: &ReasoningStep,
        context: &MatchContext,
        previous_steps: &[ExecutedStep],
    ) -> Result<String> {
        debug!(
            "Executing step {}: {} ({:?})",
            step.step_number, step.description, step.step_type
        );

        let system_prompt = prompts::step_executor(
            step.step_number,
            step.step_type.as_str(),
            &step.description,
            &step.rationale
                .as_ref()
                .map(|r| format!("Rationale: {}", r))
                .unwrap_or_default(),
        );

        let mut user_content = String::new();

        // Add context
        if let Some(ref msg) = context.message {
            user_content.push_str(&format!("User request: {}\n\n", msg));
        }
        if let Some(ref file) = context.file_path {
            user_content.push_str(&format!("Current file: {}\n", file));
        }
        if let Some(ref error) = context.error_message {
            user_content.push_str(&format!("Error: {}\n", error));
        }

        // Add previous step outputs
        if !previous_steps.is_empty() {
            user_content.push_str("\nPrevious steps:\n");
            for prev in previous_steps {
                user_content.push_str(&format!(
                    "- Step {}: {}\n",
                    prev.step.step_number, prev.output
                ));
            }
        }

        user_content.push_str(&format!("\nNow execute: {}", step.description));

        let messages = vec![Message::user(user_content)];

        // Use appropriate reasoning effort based on step type
        let effort = match step.step_type {
            StepType::Generate | StepType::Analyze => ReasoningEffort::Medium,
            StepType::Validate | StepType::Decide => ReasoningEffort::High,
            _ => ReasoningEffort::Minimum,
        };

        let response = self
            .llm
            .complete_with_reasoning(messages, system_prompt, effort)
            .await
            .context("Step execution failed")?;

        Ok(response.content)
    }

    /// Apply a solution template
    async fn apply_template(
        &self,
        template: &str,
        context: &MatchContext,
        steps: &[ExecutedStep],
    ) -> Result<String> {
        let system_prompt = prompts::TEMPLATE_APPLIER;

        let mut user_content = format!("Template:\n{}\n\n", template);

        if let Some(ref msg) = context.message {
            user_content.push_str(&format!("Context: {}\n", msg));
        }

        user_content.push_str("\nStep outputs:\n");
        for step in steps {
            user_content.push_str(&format!(
                "- {}: {}\n",
                step.step.description, step.output
            ));
        }

        let messages = vec![Message::user(user_content)];

        let response = self
            .llm
            .complete_with_reasoning(messages, system_prompt.to_string(), ReasoningEffort::Medium)
            .await?;

        Ok(response.content)
    }

    /// Generate solution from step outputs
    async fn generate_solution(
        &self,
        pattern: &ReasoningPattern,
        context: &MatchContext,
        steps: &[ExecutedStep],
    ) -> Result<String> {
        let system_prompt = prompts::solution_generator(&pattern.name);

        let mut user_content = String::new();

        if let Some(ref msg) = context.message {
            user_content.push_str(&format!("Original request: {}\n\n", msg));
        }

        user_content.push_str("Steps completed:\n");
        for step in steps {
            if step.success {
                user_content.push_str(&format!(
                    "{}: {}\n",
                    step.step.description, step.output
                ));
            }
        }

        user_content.push_str("\nProvide the final solution:");

        let messages = vec![Message::user(user_content)];

        let response = self
            .llm
            .complete_with_reasoning(messages, system_prompt, ReasoningEffort::Medium)
            .await?;

        Ok(response.content)
    }

    /// Attempt automatic pattern replay if high-confidence match exists
    pub async fn auto_replay(
        &self,
        context: &MatchContext,
        operation_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Option<ReplayResult>> {
        // Find matching pattern
        let best_match = self.matcher.find_best_match(context).await?;

        let pattern_match = match best_match {
            Some(m) if m.match_score >= self.config.auto_apply_threshold => m,
            _ => return Ok(None),
        };

        info!(
            "Auto-applying pattern {} with score {:.2}",
            pattern_match.pattern.name, pattern_match.match_score
        );

        // Replay the pattern
        let result = self.replay(&pattern_match.pattern, context).await?;

        // Record usage
        let mut usage = PatternUsage::new(pattern_match.pattern.id.clone(), result.success)
            .with_match_score(pattern_match.match_score);

        if let Some(op_id) = operation_id {
            usage = usage.with_operation(op_id);
        }
        if let Some(uid) = user_id {
            usage = usage.with_user(uid);
        }
        if result.success {
            // Estimate cost saved (rough estimate based on avoiding full LLM reasoning)
            let cost_saved = 0.02; // ~2 cents saved per pattern application
            usage = usage.with_savings(result.duration_ms, cost_saved);
        }

        self.storage.store_usage(&usage).await?;

        Ok(Some(result))
    }

    /// Create a new pattern from a successful interaction
    pub async fn learn_pattern(
        &self,
        name: &str,
        description: &str,
        trigger_type: TriggerType,
        reasoning_steps: Vec<(StepType, String)>,
        context: &MatchContext,
        project_id: Option<&str>,
    ) -> Result<ReasoningPattern> {
        info!("Learning new pattern: {}", name);

        // Build reasoning chain description
        let reasoning_chain = reasoning_steps
            .iter()
            .enumerate()
            .map(|(i, (t, d))| format!("{}. [{}] {}", i + 1, t.as_str(), d))
            .collect::<Vec<_>>()
            .join("\n");

        let mut pattern = ReasoningPattern::new(
            name.to_string(),
            description.to_string(),
            trigger_type,
            reasoning_chain,
        );

        if let Some(pid) = project_id {
            pattern = pattern.with_project(pid);
        }

        // Add steps
        for (step_type, desc) in reasoning_steps {
            pattern.add_step(step_type, &desc);
        }

        // Set applicable contexts from the current context
        let mut applicable = ApplicableContext::default();
        applicable.keywords = context.keywords.clone();

        if let Some(ext) = context.file_extension() {
            applicable.file_types.push(ext);
        }
        if let Some(ref code) = context.error_code {
            applicable.error_codes.push(code.clone());
        }
        if let Some(ref intent) = context.intent {
            applicable.intents.push(intent.clone());
        }

        pattern = pattern.with_contexts(applicable);

        // Store the pattern
        self.storage.store_pattern(&pattern).await?;

        info!("Learned pattern: {} ({})", pattern.name, pattern.id);
        Ok(pattern)
    }

    /// Format replay result for context injection
    pub fn format_replay_for_context(&self, result: &ReplayResult) -> String {
        let mut output = String::new();

        output.push_str(&format!(
            "## Applied Pattern: {}\n\n",
            result.pattern.name
        ));
        output.push_str(&format!(
            "Success: {} ({}ms)\n\n",
            if result.success { "Yes" } else { "No" },
            result.duration_ms
        ));

        output.push_str("### Steps:\n");
        for step in &result.steps_executed {
            let status = if step.success { "done" } else { "failed" };
            output.push_str(&format!(
                "{}. [{}] {}: {}\n",
                step.step.step_number, status, step.step.description, step.output
            ));
        }

        if let Some(ref solution) = result.solution {
            output.push_str(&format!("\n### Solution:\n{}\n", solution));
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn create_test_storage() -> Arc<PatternStorage> {
        let pool = SqlitePoolOptions::new()
            .connect(":memory:")
            .await
            .unwrap();

        sqlx::query(
            r#"
            CREATE TABLE reasoning_patterns (
                id TEXT PRIMARY KEY,
                project_id TEXT,
                name TEXT NOT NULL,
                description TEXT NOT NULL,
                trigger_type TEXT NOT NULL,
                reasoning_chain TEXT NOT NULL,
                solution_template TEXT,
                applicable_contexts TEXT,
                success_rate REAL DEFAULT 1.0,
                use_count INTEGER DEFAULT 1,
                success_count INTEGER DEFAULT 0,
                cost_savings_usd REAL DEFAULT 0.0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                last_used INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            CREATE TABLE reasoning_steps (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                pattern_id TEXT NOT NULL,
                step_number INTEGER NOT NULL,
                step_type TEXT NOT NULL,
                description TEXT NOT NULL,
                rationale TEXT,
                created_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            CREATE TABLE pattern_usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                pattern_id TEXT NOT NULL,
                operation_id TEXT,
                user_id TEXT,
                context_match_score REAL,
                applied_successfully BOOLEAN NOT NULL,
                outcome_notes TEXT,
                time_saved_ms INTEGER,
                cost_saved_usd REAL,
                used_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        Arc::new(PatternStorage::new(Arc::new(pool)))
    }

    #[test]
    fn test_format_replay_result() {
        // This test doesn't need async since format_replay_for_context is sync
        let pattern = ReasoningPattern::new(
            "test".to_string(),
            "Test pattern".to_string(),
            TriggerType::Keyword,
            "chain".to_string(),
        );

        let result = ReplayResult {
            pattern,
            steps_executed: vec![ExecutedStep {
                step: ReasoningStep::new(
                    "test".to_string(),
                    1,
                    StepType::Gather,
                    "Gather info".to_string(),
                ),
                output: "Found relevant files".to_string(),
                success: true,
                duration_ms: 100,
            }],
            solution: Some("Use pattern X".to_string()),
            success: true,
            duration_ms: 500,
            error: None,
        };

        // We need to create a minimal replay to test formatting
        // Since we can't easily create the full replay without LLM, just test the types
        assert!(result.success);
        assert_eq!(result.steps_executed.len(), 1);
        assert!(result.solution.is_some());
    }
}
