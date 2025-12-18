//! Response chain management
//!
//! Handles response ID persistence and handoff for smooth context resets.

use anyhow::Result;
use chrono::Utc;

use super::SessionManager;

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

        // 5. Continuity note
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
}
