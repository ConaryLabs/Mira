//! Response chain management
//!
//! Handles response ID persistence and handoff for smooth context resets.
//! Includes hysteresis logic to prevent flappy resets.
//! Uses core::ops::chat_chain for all database operations.

use anyhow::Result;

use crate::core::ops::chat_chain as core_chain;
use crate::core::primitives::{
    CHAIN_RESET_COOLDOWN_TURNS, CHAIN_RESET_HARD_CEILING, CHAIN_RESET_HYSTERESIS_TURNS,
    CHAIN_RESET_MIN_CACHE_PCT, CHAIN_RESET_TOKEN_THRESHOLD,
};
use crate::core::OpContext;

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
    /// Build OpContext for core operations
    fn chain_context(&self) -> OpContext {
        OpContext::new(std::env::current_dir().unwrap_or_default()).with_db(self.db.clone())
    }

    // ========================================================================
    // Response ID Management
    // ========================================================================

    /// Update the previous response ID
    pub async fn set_response_id(&self, response_id: &str) -> Result<()> {
        let ctx = self.chain_context();
        core_chain::set_response_id(&ctx, &self.project_path, response_id).await?;
        Ok(())
    }

    /// Get the previous response ID
    pub async fn get_response_id(&self) -> Result<Option<String>> {
        let ctx = self.chain_context();
        let id = core_chain::get_response_id(&ctx, &self.project_path).await?;
        Ok(id)
    }

    /// Clear the previous response ID (no handoff - hard reset)
    pub async fn clear_response_id(&self) -> Result<()> {
        let ctx = self.chain_context();
        core_chain::clear_response_id(&ctx, &self.project_path).await?;
        Ok(())
    }

    /// Clear the previous response ID and prepare handoff for next turn
    /// Use this when token usage is too high to reset server-side accumulation
    /// The handoff blob preserves continuity so the reset isn't obvious
    pub async fn clear_response_id_with_handoff(&self) -> Result<()> {
        // Build handoff blob BEFORE clearing (captures current state)
        let handoff = self.build_handoff_blob().await?;

        let ctx = self.chain_context();
        core_chain::clear_response_id_with_handoff(&ctx, &self.project_path, &handoff).await?;
        Ok(())
    }

    // ========================================================================
    // Handoff Management
    // ========================================================================

    /// Check if next request needs handoff context
    pub async fn needs_handoff(&self) -> Result<bool> {
        let ctx = self.chain_context();
        let needs = core_chain::needs_handoff(&ctx, &self.project_path).await?;
        Ok(needs)
    }

    /// Get the handoff blob (if any) and clear the flag
    pub async fn consume_handoff(&self) -> Result<Option<String>> {
        let ctx = self.chain_context();
        let blob = core_chain::consume_handoff(&ctx, &self.project_path).await?;
        Ok(blob)
    }

    /// Build a compact handoff blob for continuity after reset
    /// This captures: recent turns, current plan/decisions, persona reminders
    pub(super) async fn build_handoff_blob(&self) -> Result<String> {
        let ctx = self.chain_context();
        let mut sections = Vec::new();

        // 1. Recent conversation (last 3-4 turns for immediate context)
        let recent = core_chain::get_recent_messages(&ctx, 6).await.unwrap_or_default();

        if !recent.is_empty() {
            let mut convo_lines = vec!["## Recent Conversation".to_string()];
            for msg in recent.into_iter().rev() {
                // Extract just text content from blocks
                if let Ok(blocks) = serde_json::from_str::<Vec<serde_json::Value>>(&msg.blocks_json)
                {
                    for block in blocks {
                        if let Some(content) = block.get("content").and_then(|c| c.as_str()) {
                            // Truncate long messages (use chars to avoid UTF-8 boundary panic)
                            let text = if content.chars().count() > 500 {
                                format!("{}...", content.chars().take(500).collect::<String>())
                            } else {
                                content.to_string()
                            };
                            convo_lines.push(format!("**{}**: {}", msg.role, text));
                        }
                    }
                }
            }
            if convo_lines.len() > 1 {
                sections.push(convo_lines.join("\n"));
            }
        }

        // 2. Latest summary (captures older context)
        if let Ok(Some(summary_text)) =
            core_chain::get_latest_summary(&ctx, &self.project_path).await
        {
            sections.push(format!("## Earlier Context (Summary)\n{}", summary_text));
        }

        // 3. Active goals/tasks
        let goals = core_chain::get_active_goals_for_handoff(&ctx, &self.project_path, 3)
            .await
            .unwrap_or_default();

        if !goals.is_empty() {
            let mut goal_lines = vec!["## Active Goals".to_string()];
            for goal in goals {
                goal_lines.push(format!(
                    "- {} [{}] ({}%)",
                    goal.title, goal.status, goal.progress_percent
                ));
            }
            sections.push(goal_lines.join("\n"));
        }

        // 4. Key decisions (recent ones that might be relevant)
        let decisions = core_chain::get_recent_decisions_for_handoff(&ctx, &self.project_path, 5)
            .await
            .unwrap_or_default();

        if !decisions.is_empty() {
            let mut dec_lines = vec!["## Recent Decisions".to_string()];
            for decision in decisions {
                dec_lines.push(format!("- {}", decision.value));
            }
            sections.push(dec_lines.join("\n"));
        }

        // 5. Working set (touched files) - in-memory, not from DB
        let touched = self.get_touched_files();
        if !touched.is_empty() {
            let mut file_lines = vec!["## Working Set (Recently Touched Files)".to_string()];
            for path in touched.iter().take(10) {
                file_lines.push(format!("- {}", path));
            }
            sections.push(file_lines.join("\n"));
        }

        // 6. Last failure (if any)
        if let Ok(Some(failure)) =
            core_chain::get_last_failure(&ctx, &self.project_path).await
        {
            sections.push(format!(
                "## Last Known Failure\n**Command:** `{}`\n**Error (first 500 chars):**\n```\n{}\n```",
                failure.command,
                if failure.error.len() > 500 {
                    &failure.error[..500]
                } else {
                    &failure.error
                }
            ));
        }

        // 7. Recent artifacts
        let artifact_ids = core_chain::get_recent_artifact_ids(&ctx, &self.project_path)
            .await
            .unwrap_or_default();

        if !artifact_ids.is_empty() {
            let mut art_lines = vec!["## Recent Artifacts".to_string()];
            for id in artifact_ids.iter().take(5) {
                art_lines.push(format!("- `{}`", id));
            }
            sections.push(art_lines.join("\n"));
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
        let ctx = self.chain_context();

        // Get current tracking state
        let state = core_chain::get_reset_tracking(&ctx, &self.project_path).await?;

        // Check cooldown first
        if state.turns_since_reset < CHAIN_RESET_COOLDOWN_TURNS {
            return Ok(ResetDecision::Cooldown {
                turns_remaining: CHAIN_RESET_COOLDOWN_TURNS - state.turns_since_reset,
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
            let new_consecutive = state.consecutive_low_cache_turns + 1;
            core_chain::update_consecutive_low_cache(&ctx, &self.project_path, new_consecutive)
                .await?;

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
            if state.consecutive_low_cache_turns > 0 {
                core_chain::update_consecutive_low_cache(&ctx, &self.project_path, 0).await?;
            }
        }

        // Increment turns since reset
        core_chain::increment_turns_since_reset(&ctx, &self.project_path).await?;

        Ok(ResetDecision::NoReset)
    }

    /// Record that a reset occurred
    pub async fn record_reset(&self) -> Result<()> {
        let ctx = self.chain_context();
        core_chain::record_reset(&ctx, &self.project_path).await?;
        Ok(())
    }

    // ========================================================================
    // Failure & Artifact Tracking
    // ========================================================================

    /// Record a failure for handoff context
    pub async fn record_failure(&self, command: &str, error: &str) -> Result<()> {
        let ctx = self.chain_context();
        core_chain::record_failure(&ctx, &self.project_path, command, error).await?;
        Ok(())
    }

    /// Clear failure after success
    pub async fn clear_failure(&self) -> Result<()> {
        let ctx = self.chain_context();
        core_chain::clear_failure(&ctx, &self.project_path).await?;
        Ok(())
    }

    /// Get last failure (command, error) - used internally for handoff
    async fn get_last_failure(&self) -> Result<Option<(String, String)>> {
        let ctx = self.chain_context();
        let info = core_chain::get_last_failure(&ctx, &self.project_path).await?;
        Ok(info.map(|f| (f.command, f.error)))
    }

    /// Add an artifact ID to recent list (keeps last 10)
    pub async fn track_artifact(&self, artifact_id: &str) -> Result<()> {
        let ctx = self.chain_context();
        core_chain::track_artifact(&ctx, &self.project_path, artifact_id, 10).await?;
        Ok(())
    }

    /// Get recent artifact IDs - used internally for handoff
    async fn get_recent_artifact_ids(&self) -> Result<Option<Vec<String>>> {
        let ctx = self.chain_context();
        let ids = core_chain::get_recent_artifact_ids(&ctx, &self.project_path).await?;
        if ids.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ids))
        }
    }
}
