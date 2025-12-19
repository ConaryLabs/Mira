//! Response chain management
//!
//! Handles response ID persistence and handoff for smooth context resets.
//! Includes hysteresis logic to prevent flappy resets.

use anyhow::Result;
use chrono::Utc;
use mira_core::{
    CHAIN_RESET_HARD_CEILING, CHAIN_RESET_HYSTERESIS_TURNS, CHAIN_RESET_MIN_CACHE_PCT,
    CHAIN_RESET_TOKEN_THRESHOLD, CHAIN_RESET_COOLDOWN_TURNS,
};

use super::SessionManager;

/// Reset decision result
#[derive(Debug, Clone)]
pub enum ResetDecision {
    /// No reset needed
    NoReset,
    /// Soft reset (token threshold + low cache for N consecutive turns)
    SoftReset { reason: String },
    /// Hard reset (approaching context limits)
    HardReset { reason: String },
    /// Skip due to cooldown
    Cooldown { turns_remaining: u32 },
}

impl SessionManager {
    /// Update the previous response ID
    pub async fn set_response_id(&self, response_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE chat_context
            SET last_response_id = $1, updated_at = $2
            WHERE project_path = $3
            "#,
        )
        .bind(response_id)
        .bind(Utc::now().timestamp())
        .bind(&self.project_path)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Clear the previous response ID and prepare handoff for next turn
    /// Use this when token usage is too high to reset server-side accumulation
    /// The handoff blob preserves continuity so the reset isn't obvious
    pub async fn clear_response_id_with_handoff(&self) -> Result<()> {
        // Build handoff blob BEFORE clearing (captures current state)
        let handoff = self.build_handoff_blob().await?;

        sqlx::query(
            r#"
            UPDATE chat_context
            SET last_response_id = NULL, needs_handoff = 1, handoff_blob = $1, updated_at = $2
            WHERE project_path = $3
            "#,
        )
        .bind(&handoff)
        .bind(Utc::now().timestamp())
        .bind(&self.project_path)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Clear the previous response ID (no handoff - hard reset)
    pub async fn clear_response_id(&self) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE chat_context
            SET last_response_id = NULL, updated_at = $1
            WHERE project_path = $2
            "#,
        )
        .bind(Utc::now().timestamp())
        .bind(&self.project_path)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Check if next request needs handoff context
    pub async fn needs_handoff(&self) -> Result<bool> {
        let row: Option<(i32,)> = sqlx::query_as(
            "SELECT needs_handoff FROM chat_context WHERE project_path = $1",
        )
        .bind(&self.project_path)
        .fetch_optional(&self.db)
        .await?;
        Ok(row.map(|(v,)| v != 0).unwrap_or(false))
    }

    /// Get the handoff blob (if any) and clear the flag
    pub async fn consume_handoff(&self) -> Result<Option<String>> {
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT handoff_blob FROM chat_context WHERE project_path = $1 AND needs_handoff = 1",
        )
        .bind(&self.project_path)
        .fetch_optional(&self.db)
        .await?;

        if let Some((blob,)) = row {
            // Clear the handoff flag
            sqlx::query(
                "UPDATE chat_context SET needs_handoff = 0, handoff_blob = NULL, updated_at = $1 WHERE project_path = $2",
            )
            .bind(Utc::now().timestamp())
            .bind(&self.project_path)
            .execute(&self.db)
            .await?;
            Ok(blob)
        } else {
            Ok(None)
        }
    }

    /// Get the previous response ID
    pub async fn get_response_id(&self) -> Result<Option<String>> {
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT last_response_id FROM chat_context WHERE project_path = $1",
        )
        .bind(&self.project_path)
        .fetch_optional(&self.db)
        .await?;

        Ok(row.and_then(|(id,)| id))
    }

    /// Build a compact handoff blob for continuity after reset
    /// This captures: recent turns, current plan/decisions, persona reminders
    pub(super) async fn build_handoff_blob(&self) -> Result<String> {
        let mut sections = Vec::new();

        // 1. Recent conversation (last 3-4 turns for immediate context)
        let recent: Vec<(String, String, i64)> = sqlx::query_as(
            r#"
            SELECT role, blocks, created_at FROM chat_messages
            WHERE archived_at IS NULL
            ORDER BY created_at DESC
            LIMIT 6
            "#,
        )
        .fetch_all(&self.db)
        .await?;

        if !recent.is_empty() {
            let mut convo_lines = vec!["## Recent Conversation".to_string()];
            for (role, blocks_json, _) in recent.into_iter().rev() {
                // Extract just text content from blocks
                if let Ok(blocks) = serde_json::from_str::<Vec<serde_json::Value>>(&blocks_json) {
                    for block in blocks {
                        if let Some(content) = block.get("content").and_then(|c| c.as_str()) {
                            // Truncate long messages (use chars to avoid UTF-8 boundary panic)
                            let text = if content.chars().count() > 500 {
                                format!("{}...", content.chars().take(500).collect::<String>())
                            } else {
                                content.to_string()
                            };
                            convo_lines.push(format!("**{}**: {}", role, text));
                        }
                    }
                }
            }
            if convo_lines.len() > 1 {
                sections.push(convo_lines.join("\n"));
            }
        }

        // 2. Latest summary (captures older context)
        let summary: Option<(String,)> = sqlx::query_as(
            "SELECT summary FROM chat_summaries WHERE project_path = $1 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(&self.project_path)
        .fetch_optional(&self.db)
        .await?;

        if let Some((summary_text,)) = summary {
            sections.push(format!("## Earlier Context (Summary)\n{}", summary_text));
        }

        // 3. Active goals/tasks
        let goals: Vec<(String, String, i32)> = sqlx::query_as(
            r#"
            SELECT title, status, progress_percent FROM goals
            WHERE project_id = (SELECT id FROM projects WHERE path = $1)
              AND status IN ('planning', 'in_progress', 'blocked')
            ORDER BY updated_at DESC LIMIT 3
            "#,
        )
        .bind(&self.project_path)
        .fetch_all(&self.db)
        .await?;

        if !goals.is_empty() {
            let mut goal_lines = vec!["## Active Goals".to_string()];
            for (title, status, progress) in goals {
                goal_lines.push(format!("- {} [{}] ({}%)", title, status, progress));
            }
            sections.push(goal_lines.join("\n"));
        }

        // 4. Key decisions (recent ones that might be relevant)
        let decisions: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT value FROM memory_facts
            WHERE (project_id = (SELECT id FROM projects WHERE path = $1) OR project_id IS NULL)
              AND fact_type = 'decision'
            ORDER BY updated_at DESC LIMIT 5
            "#,
        )
        .bind(&self.project_path)
        .fetch_all(&self.db)
        .await?;

        if !decisions.is_empty() {
            let mut dec_lines = vec!["## Recent Decisions".to_string()];
            for (decision,) in decisions {
                dec_lines.push(format!("- {}", decision));
            }
            sections.push(dec_lines.join("\n"));
        }

        // 5. Working set (touched files)
        let touched = self.get_touched_files();
        if !touched.is_empty() {
            let mut file_lines = vec!["## Working Set (Recently Touched Files)".to_string()];
            for path in touched.iter().take(10) {
                file_lines.push(format!("- {}", path));
            }
            sections.push(file_lines.join("\n"));
        }

        // 6. Last failure (if any)
        if let Ok(Some(failure)) = self.get_last_failure().await {
            sections.push(format!(
                "## Last Known Failure\n**Command:** `{}`\n**Error (first 500 chars):**\n```\n{}\n```",
                failure.0,
                if failure.1.len() > 500 { &failure.1[..500] } else { &failure.1 }
            ));
        }

        // 7. Recent artifacts
        if let Ok(Some(artifact_ids)) = self.get_recent_artifact_ids().await {
            if !artifact_ids.is_empty() {
                let mut art_lines = vec!["## Recent Artifacts".to_string()];
                for id in artifact_ids.iter().take(5) {
                    art_lines.push(format!("- `{}`", id));
                }
                sections.push(art_lines.join("\n"));
            }
        }

        // 8. Continuity note
        sections.push(
            "## Continuity Note\nThis is a continuation of an ongoing conversation. \
             The context above summarizes where we left off. Continue naturally without \
             re-introducing yourself or asking what we're working on."
                .to_string(),
        );

        Ok(format!(
            "# Handoff Context (Thread Reset)\n\n{}",
            sections.join("\n\n")
        ))
    }

    // ========================================================================
    // Reset Hysteresis & State Tracking
    // ========================================================================

    /// Evaluate whether a reset should occur based on token count and cache%
    /// Uses hysteresis to prevent flappy resets
    pub async fn should_reset(&self, input_tokens: u32, cache_pct: u32) -> Result<ResetDecision> {
        // Get current tracking state
        let (consecutive_low, turns_since) = self.get_reset_tracking().await?;

        // Check cooldown first
        if turns_since < CHAIN_RESET_COOLDOWN_TURNS {
            return Ok(ResetDecision::Cooldown {
                turns_remaining: CHAIN_RESET_COOLDOWN_TURNS - turns_since,
            });
        }

        // Hard ceiling - always reset regardless of cache (quality guard)
        if input_tokens > CHAIN_RESET_HARD_CEILING {
            return Ok(ResetDecision::HardReset {
                reason: format!(
                    "{}k tokens exceeds hard ceiling of {}k",
                    input_tokens / 1000,
                    CHAIN_RESET_HARD_CEILING / 1000
                ),
            });
        }

        // Soft reset with hysteresis
        let is_low_cache = cache_pct < CHAIN_RESET_MIN_CACHE_PCT;
        let above_threshold = input_tokens > CHAIN_RESET_TOKEN_THRESHOLD;

        if above_threshold && is_low_cache {
            // Check if we've had enough consecutive low-cache turns
            let new_consecutive = consecutive_low + 1;
            self.update_consecutive_low_cache(new_consecutive).await?;

            if new_consecutive >= CHAIN_RESET_HYSTERESIS_TURNS {
                return Ok(ResetDecision::SoftReset {
                    reason: format!(
                        "{}k tokens with {}% cache for {} consecutive turns",
                        input_tokens / 1000,
                        cache_pct,
                        new_consecutive
                    ),
                });
            }
        } else {
            // Reset the consecutive counter if conditions aren't met
            if consecutive_low > 0 {
                self.update_consecutive_low_cache(0).await?;
            }
        }

        // Increment turns since reset
        self.increment_turns_since_reset().await?;

        Ok(ResetDecision::NoReset)
    }

    /// Record that a reset occurred
    pub async fn record_reset(&self) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE chat_context
            SET consecutive_low_cache_turns = 0, turns_since_reset = 0, updated_at = $1
            WHERE project_path = $2
            "#,
        )
        .bind(Utc::now().timestamp())
        .bind(&self.project_path)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Get current reset tracking state
    async fn get_reset_tracking(&self) -> Result<(u32, u32)> {
        let row: Option<(i32, i32)> = sqlx::query_as(
            "SELECT consecutive_low_cache_turns, turns_since_reset FROM chat_context WHERE project_path = $1",
        )
        .bind(&self.project_path)
        .fetch_optional(&self.db)
        .await?;
        Ok(row.map(|(c, t)| (c as u32, t as u32)).unwrap_or((0, 999))) // 999 = no cooldown on first run
    }

    /// Update consecutive low-cache counter
    async fn update_consecutive_low_cache(&self, count: u32) -> Result<()> {
        sqlx::query(
            "UPDATE chat_context SET consecutive_low_cache_turns = $1, updated_at = $2 WHERE project_path = $3",
        )
        .bind(count as i32)
        .bind(Utc::now().timestamp())
        .bind(&self.project_path)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Increment turns since last reset
    async fn increment_turns_since_reset(&self) -> Result<()> {
        sqlx::query(
            "UPDATE chat_context SET turns_since_reset = turns_since_reset + 1, updated_at = $1 WHERE project_path = $2",
        )
        .bind(Utc::now().timestamp())
        .bind(&self.project_path)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    // ========================================================================
    // Failure & Artifact Tracking
    // ========================================================================

    /// Record a failure for handoff context
    pub async fn record_failure(&self, command: &str, error: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE chat_context
            SET last_failure_command = $1, last_failure_error = $2, last_failure_at = $3, updated_at = $3
            WHERE project_path = $4
            "#,
        )
        .bind(command)
        .bind(error)
        .bind(Utc::now().timestamp())
        .bind(&self.project_path)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Clear failure after success
    pub async fn clear_failure(&self) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE chat_context
            SET last_failure_command = NULL, last_failure_error = NULL, last_failure_at = NULL, updated_at = $1
            WHERE project_path = $2
            "#,
        )
        .bind(Utc::now().timestamp())
        .bind(&self.project_path)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Get last failure (command, error)
    async fn get_last_failure(&self) -> Result<Option<(String, String)>> {
        let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT last_failure_command, last_failure_error FROM chat_context WHERE project_path = $1",
        )
        .bind(&self.project_path)
        .fetch_optional(&self.db)
        .await?;
        Ok(row.and_then(|(c, e)| c.zip(e)))
    }

    /// Add an artifact ID to recent list (keeps last 10)
    pub async fn track_artifact(&self, artifact_id: &str) -> Result<()> {
        // Get current list
        let current = self.get_recent_artifact_ids().await?.unwrap_or_default();
        let mut ids: Vec<String> = current;

        // Add new, keep last 10
        ids.push(artifact_id.to_string());
        if ids.len() > 10 {
            let skip_count = ids.len() - 10;
            ids = ids.into_iter().skip(skip_count).collect();
        }

        sqlx::query(
            "UPDATE chat_context SET recent_artifact_ids = $1, updated_at = $2 WHERE project_path = $3",
        )
        .bind(serde_json::to_string(&ids)?)
        .bind(Utc::now().timestamp())
        .bind(&self.project_path)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Get recent artifact IDs
    async fn get_recent_artifact_ids(&self) -> Result<Option<Vec<String>>> {
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT recent_artifact_ids FROM chat_context WHERE project_path = $1",
        )
        .bind(&self.project_path)
        .fetch_optional(&self.db)
        .await?;

        match row {
            Some((Some(json),)) => Ok(Some(serde_json::from_str(&json)?)),
            _ => Ok(None),
        }
    }
}
