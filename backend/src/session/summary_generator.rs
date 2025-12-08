// backend/src/session/summary_generator.rs
// Generates summaries for completed Codex sessions
//
// Creates rich summaries by:
// 1. Analyzing session artifacts (files created/modified)
// 2. Extracting key actions from tool calls
// 3. Using LLM to generate natural language summary
// 4. Formatting for Voice session context injection

use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::llm::provider::LlmProvider;
use crate::session::completion::CompletionReason;
use crate::session::types::CodexCompletionMetadata;

/// Configuration for summary generation
#[derive(Debug, Clone)]
pub struct SummaryConfig {
    /// Maximum files to include in summary
    pub max_files_in_summary: usize,
    /// Maximum key actions to include
    pub max_key_actions: usize,
    /// Whether to use LLM for summary generation
    pub use_llm_summary: bool,
    /// Maximum tokens for summary generation
    pub max_summary_tokens: usize,
}

impl Default for SummaryConfig {
    fn default() -> Self {
        Self {
            max_files_in_summary: 15,
            max_key_actions: 10,
            use_llm_summary: true,
            max_summary_tokens: 500,
        }
    }
}

/// Session artifacts and metrics for summary generation
#[derive(Debug, Clone)]
pub struct SessionArtifacts {
    /// Files created during session
    pub files_created: Vec<String>,
    /// Files modified during session
    pub files_modified: Vec<String>,
    /// Files deleted during session
    pub files_deleted: Vec<String>,
    /// Tool calls made (name, success)
    pub tool_calls: Vec<(String, bool)>,
    /// Key actions extracted from LLM responses
    pub key_actions: Vec<String>,
    /// Total duration in seconds
    pub duration_seconds: i64,
    /// Total tokens used
    pub tokens_total: i64,
    /// Cost in USD
    pub cost_usd: f64,
    /// Compaction events
    pub compaction_count: u32,
    /// Completion reason
    pub completion_reason: Option<CompletionReason>,
}

impl SessionArtifacts {
    pub fn new() -> Self {
        Self {
            files_created: Vec::new(),
            files_modified: Vec::new(),
            files_deleted: Vec::new(),
            tool_calls: Vec::new(),
            key_actions: Vec::new(),
            duration_seconds: 0,
            tokens_total: 0,
            cost_usd: 0.0,
            compaction_count: 0,
            completion_reason: None,
        }
    }

    pub fn all_files_changed(&self) -> Vec<String> {
        let mut files = Vec::new();
        files.extend(self.files_created.clone());
        files.extend(self.files_modified.clone());
        files
    }
}

impl Default for SessionArtifacts {
    fn default() -> Self {
        Self::new()
    }
}

/// Generator for Codex session summaries
pub struct SummaryGenerator {
    pool: SqlitePool,
    llm_provider: Option<Arc<dyn LlmProvider>>,
    config: SummaryConfig,
}

impl SummaryGenerator {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            llm_provider: None,
            config: SummaryConfig::default(),
        }
    }

    pub fn with_llm(mut self, provider: Arc<dyn LlmProvider>) -> Self {
        self.llm_provider = Some(provider);
        self
    }

    pub fn with_config(mut self, config: SummaryConfig) -> Self {
        self.config = config;
        self
    }

    /// Collect artifacts from a completed Codex session
    pub async fn collect_artifacts(&self, codex_session_id: &str) -> Result<SessionArtifacts> {
        let mut artifacts = SessionArtifacts::new();

        // Get session info
        let session_info: Option<(i64, Option<i64>, Option<String>)> = sqlx::query_as(
            r#"
            SELECT started_at, completed_at, completion_reason
            FROM chat_sessions
            WHERE id = ? AND session_type = 'codex'
            "#,
        )
        .bind(codex_session_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((started_at, completed_at, completion_reason)) = session_info {
            if let Some(completed) = completed_at {
                artifacts.duration_seconds = completed - started_at;
            }
            if let Some(reason_json) = completion_reason {
                artifacts.completion_reason = parse_completion_reason(&reason_json);
            }
        }

        // Get link stats
        let link_stats: Option<(i64, i64, f64, i32)> = sqlx::query_as(
            r#"
            SELECT tokens_used_input, tokens_used_output, cost_usd, compaction_count
            FROM codex_session_links
            WHERE codex_session_id = ?
            "#,
        )
        .bind(codex_session_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((tokens_in, tokens_out, cost, compaction)) = link_stats {
            artifacts.tokens_total = tokens_in + tokens_out;
            artifacts.cost_usd = cost;
            artifacts.compaction_count = compaction as u32;
        }

        // Get artifacts from database
        let artifact_rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
            r#"
            SELECT artifact_type, file_path, content
            FROM artifacts
            WHERE session_id = ?
            ORDER BY created_at ASC
            "#,
        )
        .bind(codex_session_id)
        .fetch_all(&self.pool)
        .await?;

        for (artifact_type, file_path, _content) in artifact_rows {
            match artifact_type.as_str() {
                "create" | "new_file" => {
                    if !artifacts.files_created.contains(&file_path) {
                        artifacts.files_created.push(file_path);
                    }
                }
                "edit" | "modify" | "update" => {
                    if !artifacts.files_modified.contains(&file_path)
                        && !artifacts.files_created.contains(&file_path)
                    {
                        artifacts.files_modified.push(file_path);
                    }
                }
                "delete" => {
                    artifacts.files_deleted.push(file_path);
                }
                _ => {}
            }
        }

        // Get tool calls from operations
        let tool_rows: Vec<(String, i32)> = sqlx::query_as(
            r#"
            SELECT tool_name, success
            FROM tool_executions
            WHERE session_id = ?
            ORDER BY executed_at ASC
            "#,
        )
        .bind(codex_session_id)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        for (tool_name, success) in tool_rows {
            artifacts.tool_calls.push((tool_name, success != 0));
        }

        // Extract key actions from messages
        let message_rows: Vec<(String, String)> = sqlx::query_as(
            r#"
            SELECT role, content
            FROM messages
            WHERE session_id = ? AND role = 'assistant'
            ORDER BY created_at ASC
            "#,
        )
        .bind(codex_session_id)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        for (_role, content) in message_rows {
            let actions = extract_key_actions(&content);
            for action in actions {
                if !artifacts.key_actions.contains(&action)
                    && artifacts.key_actions.len() < self.config.max_key_actions
                {
                    artifacts.key_actions.push(action);
                }
            }
        }

        debug!(
            codex_session_id = %codex_session_id,
            files_created = artifacts.files_created.len(),
            files_modified = artifacts.files_modified.len(),
            tool_calls = artifacts.tool_calls.len(),
            key_actions = artifacts.key_actions.len(),
            "Collected session artifacts"
        );

        Ok(artifacts)
    }

    /// Generate a summary from collected artifacts
    pub async fn generate_summary(
        &self,
        codex_session_id: &str,
        task_description: &str,
        artifacts: &SessionArtifacts,
    ) -> Result<String> {
        // Try LLM-based summary if available
        if self.config.use_llm_summary {
            if let Some(provider) = &self.llm_provider {
                match self
                    .generate_llm_summary(provider.as_ref(), task_description, artifacts)
                    .await
                {
                    Ok(summary) => {
                        debug!(
                            codex_session_id = %codex_session_id,
                            "Generated LLM-based summary"
                        );
                        return Ok(summary);
                    }
                    Err(e) => {
                        warn!(
                            codex_session_id = %codex_session_id,
                            error = %e,
                            "Failed to generate LLM summary, using fallback"
                        );
                    }
                }
            }
        }

        // Fallback to rule-based summary
        Ok(self.generate_rule_based_summary(task_description, artifacts))
    }

    /// Generate summary using LLM
    async fn generate_llm_summary(
        &self,
        provider: &dyn LlmProvider,
        task_description: &str,
        artifacts: &SessionArtifacts,
    ) -> Result<String> {
        let prompt = format!(
            r#"Summarize this completed coding session concisely (2-3 sentences max).

Task: {task_description}

Files created: {files_created}
Files modified: {files_modified}
Total tool calls: {tool_count}
Duration: {duration}

Key actions taken:
{key_actions}

Provide a brief, factual summary of what was accomplished. Focus on the outcome, not the process."#,
            task_description = task_description,
            files_created = if artifacts.files_created.is_empty() {
                "None".to_string()
            } else {
                artifacts.files_created.join(", ")
            },
            files_modified = if artifacts.files_modified.is_empty() {
                "None".to_string()
            } else {
                artifacts.files_modified.join(", ")
            },
            tool_count = artifacts.tool_calls.len(),
            duration = format_duration(artifacts.duration_seconds),
            key_actions = if artifacts.key_actions.is_empty() {
                "No specific actions recorded".to_string()
            } else {
                artifacts
                    .key_actions
                    .iter()
                    .map(|a| format!("- {}", a))
                    .collect::<Vec<_>>()
                    .join("\n")
            },
        );

        let response = provider
            .chat(
                vec![crate::llm::provider::Message::user(prompt)],
                "You are a technical writer. Provide concise, factual summaries.".to_string(),
            )
            .await?;

        Ok(response.content.trim().to_string())
    }

    /// Generate summary using rules (fallback)
    fn generate_rule_based_summary(
        &self,
        task_description: &str,
        artifacts: &SessionArtifacts,
    ) -> String {
        let mut summary = String::new();

        // Task completion
        let completion_status = match &artifacts.completion_reason {
            Some(CompletionReason::ToolLoopTerminated)
            | Some(CompletionReason::GitCommitDetected { .. })
            | Some(CompletionReason::UserExplicitCompletion { .. }) => "Successfully completed",
            Some(CompletionReason::MaxIterationsReached { .. }) => "Reached iteration limit",
            Some(CompletionReason::InactivityTimeout { .. }) => "Timed out",
            Some(CompletionReason::Failed { .. }) => "Failed",
            Some(CompletionReason::UserCancelled) => "Cancelled",
            None => "Completed",
        };

        summary.push_str(&format!("{}: {}\n\n", completion_status, task_description));

        // Files changed
        let all_files = artifacts.all_files_changed();
        if !all_files.is_empty() {
            summary.push_str("Files changed:\n");
            for file in all_files.iter().take(self.config.max_files_in_summary) {
                summary.push_str(&format!("- {}\n", file));
            }
            if all_files.len() > self.config.max_files_in_summary {
                summary.push_str(&format!(
                    "... and {} more\n",
                    all_files.len() - self.config.max_files_in_summary
                ));
            }
            summary.push('\n');
        }

        // Key actions
        if !artifacts.key_actions.is_empty() {
            summary.push_str("Key actions:\n");
            for action in artifacts.key_actions.iter().take(5) {
                summary.push_str(&format!("- {}\n", action));
            }
            summary.push('\n');
        }

        // Metrics
        summary.push_str(&format!(
            "Duration: {} | Tokens: {} | Cost: ${:.4}",
            format_duration(artifacts.duration_seconds),
            artifacts.tokens_total,
            artifacts.cost_usd
        ));

        summary
    }

    /// Generate completion metadata for injection
    pub fn create_completion_metadata(&self, artifacts: &SessionArtifacts) -> CodexCompletionMetadata {
        CodexCompletionMetadata {
            files_changed: artifacts.all_files_changed(),
            duration_seconds: artifacts.duration_seconds,
            tokens_total: artifacts.tokens_total,
            cost_usd: artifacts.cost_usd,
            tool_calls_count: artifacts.tool_calls.len() as u32,
            compaction_count: artifacts.compaction_count,
            key_actions: artifacts.key_actions.clone(),
        }
    }

    /// Store generated summary in database
    pub async fn store_summary(
        &self,
        codex_session_id: &str,
        summary: &str,
    ) -> Result<()> {
        // Update link with completion summary
        sqlx::query(
            "UPDATE codex_session_links SET completion_summary = ? WHERE codex_session_id = ?",
        )
        .bind(summary)
        .bind(codex_session_id)
        .execute(&self.pool)
        .await?;

        info!(
            codex_session_id = %codex_session_id,
            summary_length = summary.len(),
            "Stored session summary"
        );

        Ok(())
    }

    /// Full summary generation pipeline
    pub async fn generate_and_store(
        &self,
        codex_session_id: &str,
        task_description: &str,
    ) -> Result<(String, CodexCompletionMetadata)> {
        let artifacts = self.collect_artifacts(codex_session_id).await?;
        let summary = self
            .generate_summary(codex_session_id, task_description, &artifacts)
            .await?;
        let metadata = self.create_completion_metadata(&artifacts);

        self.store_summary(codex_session_id, &summary).await?;

        Ok((summary, metadata))
    }
}

/// Extract key actions from LLM response text
fn extract_key_actions(content: &str) -> Vec<String> {
    let mut actions = Vec::new();

    // Look for action-like phrases
    let action_indicators = [
        "created", "implemented", "added", "fixed", "updated", "refactored",
        "modified", "deleted", "removed", "renamed", "moved", "installed",
        "configured", "tested", "wrote", "built",
    ];

    for line in content.lines() {
        let line_lower = line.to_lowercase();
        for indicator in &action_indicators {
            if line_lower.contains(indicator) && line.len() > 10 && line.len() < 200 {
                let action = line.trim().to_string();
                if !action.starts_with("```") && !actions.contains(&action) {
                    actions.push(action);
                    break;
                }
            }
        }
    }

    actions
}

/// Format duration as human-readable string
fn format_duration(seconds: i64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else if seconds < 3600 {
        let minutes = seconds / 60;
        let secs = seconds % 60;
        format!("{}m {}s", minutes, secs)
    } else {
        let hours = seconds / 3600;
        let minutes = (seconds % 3600) / 60;
        format!("{}h {}m", hours, minutes)
    }
}

/// Parse completion reason from JSON string
fn parse_completion_reason(json_str: &str) -> Option<CompletionReason> {
    let value: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let reason_type = value.get("type")?.as_str()?;

    match reason_type {
        "tool_loop_terminated" => Some(CompletionReason::ToolLoopTerminated),
        "git_commit" | "git_commit_detected" => {
            let hash = value
                .get("commit_hash")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            Some(CompletionReason::GitCommitDetected { commit_hash: hash })
        }
        "user_explicit" | "user_explicit_completion" => {
            let phrase = value
                .get("trigger_phrase")
                .and_then(|v| v.as_str())
                .unwrap_or("done")
                .to_string();
            Some(CompletionReason::UserExplicitCompletion {
                trigger_phrase: phrase,
            })
        }
        "inactivity_timeout" => {
            let seconds = value
                .get("idle_seconds")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            Some(CompletionReason::InactivityTimeout {
                idle_seconds: seconds,
            })
        }
        "max_iterations" | "max_iterations_reached" => {
            let iters = value
                .get("iterations")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            Some(CompletionReason::MaxIterationsReached { iterations: iters })
        }
        "user_cancelled" => Some(CompletionReason::UserCancelled),
        "failed" => {
            let error = value
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
                .to_string();
            Some(CompletionReason::Failed { error })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_key_actions() {
        let content = r#"
I'll start by creating the new module.
Created src/auth/login.rs with the login handler.
Implemented password validation logic.
All tests are passing now.
```rust
fn example() {}
```
"#;

        let actions = extract_key_actions(content);
        assert!(actions.len() >= 2);
        assert!(actions.iter().any(|a| a.contains("Created")));
        assert!(actions.iter().any(|a| a.contains("Implemented")));
        // Should not include code blocks
        assert!(!actions.iter().any(|a| a.starts_with("```")));
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(3661), "1h 1m");
    }

    #[test]
    fn test_parse_completion_reason() {
        let json = r#"{"type": "git_commit", "commit_hash": "abc123"}"#;
        let reason = parse_completion_reason(json);
        assert!(matches!(
            reason,
            Some(CompletionReason::GitCommitDetected { .. })
        ));

        let json = r#"{"type": "inactivity_timeout", "idle_seconds": 600}"#;
        let reason = parse_completion_reason(json);
        assert!(matches!(
            reason,
            Some(CompletionReason::InactivityTimeout { idle_seconds: 600 })
        ));
    }

    #[test]
    fn test_session_artifacts_all_files_changed() {
        let mut artifacts = SessionArtifacts::new();
        artifacts.files_created = vec!["new.rs".to_string()];
        artifacts.files_modified = vec!["existing.rs".to_string()];

        let all = artifacts.all_files_changed();
        assert_eq!(all.len(), 2);
        assert!(all.contains(&"new.rs".to_string()));
        assert!(all.contains(&"existing.rs".to_string()));
    }
}
