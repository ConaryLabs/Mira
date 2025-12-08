// backend/src/session/completion.rs
// Session completion detection for Codex sessions
//
// Detects completion signals:
// 1. Git commit - A commit was made during the session
// 2. Explicit completion - User said "done", "finished", etc.
// 3. Inactivity timeout - No activity for configured duration
// 4. Tool loop termination - LLM stopped making tool calls

use anyhow::Result;
use sqlx::SqlitePool;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::session::types::CodexStatus;

/// Reasons a Codex session completed
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionReason {
    /// LLM stopped making tool calls (natural completion)
    ToolLoopTerminated,
    /// A git commit was detected
    GitCommitDetected { commit_hash: String },
    /// User explicitly said done/finished
    UserExplicitCompletion { trigger_phrase: String },
    /// Session timed out due to inactivity
    InactivityTimeout { idle_seconds: u64 },
    /// Maximum iterations reached
    MaxIterationsReached { iterations: u32 },
    /// User cancelled the session
    UserCancelled,
    /// Session failed with error
    Failed { error: String },
}

impl CompletionReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            CompletionReason::ToolLoopTerminated => "tool_loop_terminated",
            CompletionReason::GitCommitDetected { .. } => "git_commit_detected",
            CompletionReason::UserExplicitCompletion { .. } => "user_explicit_completion",
            CompletionReason::InactivityTimeout { .. } => "inactivity_timeout",
            CompletionReason::MaxIterationsReached { .. } => "max_iterations_reached",
            CompletionReason::UserCancelled => "user_cancelled",
            CompletionReason::Failed { .. } => "failed",
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(
            self,
            CompletionReason::ToolLoopTerminated
                | CompletionReason::GitCommitDetected { .. }
                | CompletionReason::UserExplicitCompletion { .. }
        )
    }
}

/// Configuration for completion detection
#[derive(Debug, Clone)]
pub struct CompletionConfig {
    /// Inactivity timeout in seconds (0 = disabled)
    pub inactivity_timeout_seconds: u64,
    /// Maximum iterations before forced completion
    pub max_iterations: u32,
    /// Phrases that trigger explicit completion detection
    pub completion_phrases: Vec<String>,
    /// Whether to detect git commits
    pub detect_git_commits: bool,
}

impl Default for CompletionConfig {
    fn default() -> Self {
        Self {
            inactivity_timeout_seconds: 600, // 10 minutes
            max_iterations: 1000,
            completion_phrases: vec![
                "done".to_string(),
                "finished".to_string(),
                "complete".to_string(),
                "all done".to_string(),
                "task complete".to_string(),
            ],
            detect_git_commits: true,
        }
    }
}

/// Event signaling a completion signal was detected
#[derive(Debug, Clone)]
pub struct CompletionSignal {
    pub codex_session_id: String,
    pub reason: CompletionReason,
    pub timestamp: i64,
}

/// Detector for session completion signals
pub struct CompletionDetector {
    pool: SqlitePool,
    config: CompletionConfig,
}

impl CompletionDetector {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            config: CompletionConfig::default(),
        }
    }

    pub fn with_config(pool: SqlitePool, config: CompletionConfig) -> Self {
        Self { pool, config }
    }

    /// Check if text contains an explicit completion phrase
    pub fn check_explicit_completion(&self, text: &str) -> Option<String> {
        let text_lower = text.to_lowercase();
        for phrase in &self.config.completion_phrases {
            if text_lower.contains(&phrase.to_lowercase()) {
                return Some(phrase.clone());
            }
        }
        None
    }

    /// Check if a git commit was made for the session's project
    pub async fn check_git_commit(
        &self,
        codex_session_id: &str,
        project_path: Option<&str>,
        since_timestamp: i64,
    ) -> Option<String> {
        if !self.config.detect_git_commits {
            return None;
        }

        let Some(path) = project_path else {
            return None;
        };

        // Check git log for commits since timestamp
        let output = tokio::process::Command::new("git")
            .args([
                "log",
                "--since",
                &format!("@{}", since_timestamp),
                "--format=%H",
                "-1",
            ])
            .current_dir(path)
            .output()
            .await
            .ok()?;

        if output.status.success() {
            let hash = String::from_utf8_lossy(&output.stdout)
                .trim()
                .to_string();
            if !hash.is_empty() {
                debug!(
                    codex_session_id = %codex_session_id,
                    commit_hash = %hash,
                    "Git commit detected"
                );
                return Some(hash);
            }
        }

        None
    }

    /// Check if session has exceeded inactivity timeout
    pub fn check_inactivity_timeout(&self, last_activity: Instant) -> Option<u64> {
        if self.config.inactivity_timeout_seconds == 0 {
            return None;
        }

        let idle_seconds = last_activity.elapsed().as_secs();
        if idle_seconds >= self.config.inactivity_timeout_seconds {
            return Some(idle_seconds);
        }

        None
    }

    /// Check if session has exceeded max iterations
    pub fn check_max_iterations(&self, current_iteration: u32) -> bool {
        current_iteration >= self.config.max_iterations
    }

    /// Record completion in database
    pub async fn record_completion(
        &self,
        codex_session_id: &str,
        reason: &CompletionReason,
    ) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let status = if reason.is_success() {
            CodexStatus::Completed
        } else {
            match reason {
                CompletionReason::UserCancelled => CodexStatus::Cancelled,
                CompletionReason::Failed { .. } => CodexStatus::Failed,
                CompletionReason::InactivityTimeout { .. } => CodexStatus::Completed,
                CompletionReason::MaxIterationsReached { .. } => CodexStatus::Completed,
                _ => CodexStatus::Completed,
            }
        };

        let reason_json = match reason {
            CompletionReason::GitCommitDetected { commit_hash } => {
                serde_json::json!({"type": "git_commit", "commit_hash": commit_hash})
            }
            CompletionReason::UserExplicitCompletion { trigger_phrase } => {
                serde_json::json!({"type": "user_explicit", "trigger_phrase": trigger_phrase})
            }
            CompletionReason::InactivityTimeout { idle_seconds } => {
                serde_json::json!({"type": "inactivity_timeout", "idle_seconds": idle_seconds})
            }
            CompletionReason::MaxIterationsReached { iterations } => {
                serde_json::json!({"type": "max_iterations", "iterations": iterations})
            }
            _ => serde_json::json!({"type": reason.as_str()}),
        };

        sqlx::query(
            r#"
            UPDATE chat_sessions
            SET codex_status = ?, completed_at = ?, last_active = ?,
                completion_reason = ?
            WHERE id = ? AND session_type = 'codex'
            "#,
        )
        .bind(status.as_str())
        .bind(now)
        .bind(now)
        .bind(reason_json.to_string())
        .bind(codex_session_id)
        .execute(&self.pool)
        .await?;

        info!(
            codex_session_id = %codex_session_id,
            reason = %reason.as_str(),
            status = %status.as_str(),
            "Recorded session completion"
        );

        Ok(())
    }

    /// Get pending Codex sessions that may need completion checking
    pub async fn get_active_sessions(&self) -> Result<Vec<(String, Option<String>, i64)>> {
        let rows: Vec<(String, Option<String>, i64)> = sqlx::query_as(
            r#"
            SELECT id, project_path, last_active
            FROM chat_sessions
            WHERE session_type = 'codex' AND codex_status = 'running'
            ORDER BY last_active DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// Background task to check for stale sessions
    pub async fn check_stale_sessions(&self) -> Vec<CompletionSignal> {
        let mut signals = Vec::new();

        let sessions = match self.get_active_sessions().await {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to get active sessions: {}", e);
                return signals;
            }
        };

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        for (session_id, project_path, last_active) in sessions {
            let idle_seconds = (now - last_active).max(0) as u64;

            // Check inactivity timeout
            if self.config.inactivity_timeout_seconds > 0
                && idle_seconds >= self.config.inactivity_timeout_seconds
            {
                signals.push(CompletionSignal {
                    codex_session_id: session_id.clone(),
                    reason: CompletionReason::InactivityTimeout { idle_seconds },
                    timestamp: now,
                });
                continue;
            }

            // Check for git commits
            if let Some(commit_hash) = self
                .check_git_commit(&session_id, project_path.as_deref(), last_active)
                .await
            {
                signals.push(CompletionSignal {
                    codex_session_id: session_id,
                    reason: CompletionReason::GitCommitDetected { commit_hash },
                    timestamp: now,
                });
            }
        }

        signals
    }
}

/// Monitor that runs completion detection in background
pub struct CompletionMonitor {
    detector: CompletionDetector,
    check_interval: Duration,
}

impl CompletionMonitor {
    pub fn new(detector: CompletionDetector) -> Self {
        Self {
            detector,
            check_interval: Duration::from_secs(30),
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.check_interval = interval;
        self
    }

    /// Start background monitoring, returns channel for completion signals
    pub fn start(self) -> mpsc::Receiver<CompletionSignal> {
        let (tx, rx) = mpsc::channel(100);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.check_interval);

            loop {
                interval.tick().await;

                let signals = self.detector.check_stale_sessions().await;
                for signal in signals {
                    // Record completion
                    if let Err(e) = self
                        .detector
                        .record_completion(&signal.codex_session_id, &signal.reason)
                        .await
                    {
                        warn!(
                            codex_session_id = %signal.codex_session_id,
                            error = %e,
                            "Failed to record completion"
                        );
                        continue;
                    }

                    // Send signal
                    if tx.send(signal).await.is_err() {
                        // Receiver dropped, stop monitoring
                        return;
                    }
                }
            }
        });

        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> CompletionConfig {
        CompletionConfig::default()
    }

    #[test]
    fn test_check_explicit_completion() {
        // Test without database pool
        let config = test_config();
        let text_lower = "i'm done with this task".to_lowercase();
        let found = config
            .completion_phrases
            .iter()
            .any(|phrase| text_lower.contains(&phrase.to_lowercase()));
        assert!(found);

        let text_lower = "the feature is finished".to_lowercase();
        let found = config
            .completion_phrases
            .iter()
            .any(|phrase| text_lower.contains(&phrase.to_lowercase()));
        assert!(found);

        let text_lower = "still working on it".to_lowercase();
        let found = config
            .completion_phrases
            .iter()
            .any(|phrase| text_lower.contains(&phrase.to_lowercase()));
        assert!(!found);
    }

    #[test]
    fn test_completion_reason_is_success() {
        assert!(CompletionReason::ToolLoopTerminated.is_success());
        assert!(CompletionReason::GitCommitDetected {
            commit_hash: "abc123".to_string()
        }
        .is_success());
        assert!(CompletionReason::UserExplicitCompletion {
            trigger_phrase: "done".to_string()
        }
        .is_success());
        assert!(!CompletionReason::Failed {
            error: "oops".to_string()
        }
        .is_success());
        assert!(!CompletionReason::UserCancelled.is_success());
    }

    #[test]
    fn test_check_max_iterations() {
        let config = test_config();
        assert!(999 < config.max_iterations);
        assert!(1000 >= config.max_iterations);
        assert!(1001 > config.max_iterations);
    }
}
