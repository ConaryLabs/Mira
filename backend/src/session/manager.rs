// backend/src/session/manager.rs
// Manages Voice and Codex session lifecycle

use crate::session::types::*;
use anyhow::Result;
use sqlx::SqlitePool;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

/// Manages Voice and Codex session lifecycle
pub struct SessionManager {
    pool: SqlitePool,
}

fn now_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl SessionManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Get session type for a session ID
    pub async fn get_session_type(&self, session_id: &str) -> Result<SessionType> {
        let result: Option<(String,)> = sqlx::query_as(
            "SELECT session_type FROM chat_sessions WHERE id = ?",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;

        match result {
            Some((session_type,)) => {
                Ok(SessionType::from_str(&session_type).unwrap_or_default())
            }
            None => Ok(SessionType::Voice), // Default for non-existent sessions
        }
    }

    /// Get the Voice session ID for a given session (returns self if already Voice)
    pub async fn get_voice_session_id(&self, session_id: &str) -> Result<String> {
        let result: Option<(String, Option<String>)> = sqlx::query_as(
            "SELECT session_type, parent_session_id FROM chat_sessions WHERE id = ?",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;

        match result {
            Some((session_type, parent_id)) => {
                if session_type == "codex" {
                    // Return parent Voice session
                    parent_id.ok_or_else(|| {
                        anyhow::anyhow!("Codex session {} has no parent", session_id)
                    })
                } else {
                    // Already a Voice session
                    Ok(session_id.to_string())
                }
            }
            None => Ok(session_id.to_string()), // Session doesn't exist yet
        }
    }

    /// Get or create a Voice session for a user/project
    pub async fn get_or_create_voice_session(
        &self,
        user_id: Option<&str>,
        project_path: Option<&str>,
    ) -> Result<String> {
        // Try to find existing Voice session
        let existing: Option<(String,)> = sqlx::query_as(
            r#"
            SELECT id FROM chat_sessions
            WHERE session_type = 'voice'
            AND (user_id = ? OR (user_id IS NULL AND ? IS NULL))
            AND (project_path = ? OR (project_path IS NULL AND ? IS NULL))
            ORDER BY last_active DESC
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .bind(user_id)
        .bind(project_path)
        .bind(project_path)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((id,)) = existing {
            debug!(session_id = %id, "Found existing Voice session");
            return Ok(id);
        }

        // Create new Voice session
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_timestamp();

        sqlx::query(
            r#"
            INSERT INTO chat_sessions (id, user_id, project_path, session_type, message_count, created_at, last_active)
            VALUES (?, ?, ?, 'voice', 0, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(user_id)
        .bind(project_path)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        info!(session_id = %id, "Created new Voice session");
        Ok(id)
    }

    /// Spawn a Codex session from a Voice session
    pub async fn spawn_codex_session(
        &self,
        voice_session_id: &str,
        task_description: &str,
        trigger: &CodexSpawnTrigger,
        voice_context_summary: Option<&str>,
    ) -> Result<String> {
        // Verify parent is a Voice session
        let session_type = self.get_session_type(voice_session_id).await?;
        if session_type != SessionType::Voice {
            return Err(anyhow::anyhow!(
                "Cannot spawn Codex from non-Voice session: {}",
                voice_session_id
            ));
        }

        let codex_id = uuid::Uuid::new_v4().to_string();
        let now = now_timestamp();

        // Get project_path from parent session
        let parent_path: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT project_path FROM chat_sessions WHERE id = ?",
        )
        .bind(voice_session_id)
        .fetch_optional(&self.pool)
        .await?;
        let project_path = parent_path.and_then(|(p,)| p);

        // Create Codex session
        sqlx::query(
            r#"
            INSERT INTO chat_sessions (
                id, session_type, parent_session_id, codex_status, codex_task_description,
                project_path, message_count, started_at, created_at, last_active
            )
            VALUES (?, 'codex', ?, 'running', ?, ?, 0, ?, ?, ?)
            "#,
        )
        .bind(&codex_id)
        .bind(voice_session_id)
        .bind(task_description)
        .bind(&project_path)
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        // Create link record
        let confidence = match trigger {
            CodexSpawnTrigger::RouterDetection { confidence, .. } => Some(*confidence),
            _ => None,
        };

        sqlx::query(
            r#"
            INSERT INTO codex_session_links (
                voice_session_id, codex_session_id, spawn_trigger, spawn_confidence,
                voice_context_summary, created_at
            )
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(voice_session_id)
        .bind(&codex_id)
        .bind(trigger.trigger_type())
        .bind(confidence)
        .bind(voice_context_summary)
        .bind(now)
        .execute(&self.pool)
        .await?;

        info!(
            voice_session_id = %voice_session_id,
            codex_session_id = %codex_id,
            task = %task_description,
            trigger = %trigger.trigger_type(),
            "Spawned Codex session"
        );

        Ok(codex_id)
    }

    /// Get active Codex sessions for a Voice session
    pub async fn get_active_codex_sessions(
        &self,
        voice_session_id: &str,
    ) -> Result<Vec<CodexSessionInfo>> {
        let rows: Vec<(
            String,          // id
            String,          // codex_status
            String,          // codex_task_description
            i64,             // started_at
            Option<i64>,     // completed_at
        )> = sqlx::query_as(
            r#"
            SELECT id, codex_status, codex_task_description, started_at, completed_at
            FROM chat_sessions
            WHERE parent_session_id = ? AND session_type = 'codex' AND codex_status = 'running'
            ORDER BY started_at DESC
            "#,
        )
        .bind(voice_session_id)
        .fetch_all(&self.pool)
        .await?;

        let mut sessions = Vec::new();
        for (id, status, task, started_at, completed_at) in rows {
            // Get usage stats from link table
            let link: Option<(i64, i64, f64, i32)> = sqlx::query_as(
                r#"
                SELECT tokens_used_input, tokens_used_output, cost_usd, compaction_count
                FROM codex_session_links WHERE codex_session_id = ?
                "#,
            )
            .bind(&id)
            .fetch_optional(&self.pool)
            .await?;

            let (tokens_in, tokens_out, cost, compaction) = link.unwrap_or((0, 0, 0.0, 0));

            sessions.push(CodexSessionInfo {
                id: id.clone(),
                parent_voice_session_id: voice_session_id.to_string(),
                status: CodexStatus::from_str(&status).unwrap_or(CodexStatus::Running),
                task_description: task,
                started_at,
                completed_at,
                progress_percent: None,
                current_activity: None,
                tokens_used: tokens_in + tokens_out,
                cost_usd: cost,
                compaction_count: compaction as u32,
            });
        }

        Ok(sessions)
    }

    /// Update Codex session status
    pub async fn update_codex_status(
        &self,
        codex_session_id: &str,
        status: CodexStatus,
    ) -> Result<()> {
        let now = now_timestamp();
        let completed_at = if status.is_terminal() {
            Some(now)
        } else {
            None
        };

        sqlx::query(
            r#"
            UPDATE chat_sessions
            SET codex_status = ?, completed_at = ?, last_active = ?
            WHERE id = ? AND session_type = 'codex'
            "#,
        )
        .bind(status.as_str())
        .bind(completed_at)
        .bind(now)
        .bind(codex_session_id)
        .execute(&self.pool)
        .await?;

        if status.is_terminal() {
            sqlx::query(
                "UPDATE codex_session_links SET completed_at = ? WHERE codex_session_id = ?",
            )
            .bind(now)
            .bind(codex_session_id)
            .execute(&self.pool)
            .await?;
        }

        debug!(
            codex_session_id = %codex_session_id,
            status = %status.as_str(),
            "Updated Codex session status"
        );

        Ok(())
    }

    /// Complete a Codex session and record summary
    pub async fn complete_codex_session(
        &self,
        codex_session_id: &str,
        completion_summary: &str,
        tokens_input: i64,
        tokens_output: i64,
        cost_usd: f64,
        compaction_count: i32,
    ) -> Result<String> {
        let now = now_timestamp();

        // Update session status
        sqlx::query(
            r#"
            UPDATE chat_sessions
            SET codex_status = 'completed', completed_at = ?, last_active = ?
            WHERE id = ? AND session_type = 'codex'
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(codex_session_id)
        .execute(&self.pool)
        .await?;

        // Update link with completion info
        sqlx::query(
            r#"
            UPDATE codex_session_links
            SET completion_summary = ?, tokens_used_input = ?, tokens_used_output = ?,
                cost_usd = ?, compaction_count = ?, completed_at = ?
            WHERE codex_session_id = ?
            "#,
        )
        .bind(completion_summary)
        .bind(tokens_input)
        .bind(tokens_output)
        .bind(cost_usd)
        .bind(compaction_count)
        .bind(now)
        .bind(codex_session_id)
        .execute(&self.pool)
        .await?;

        // Get parent Voice session ID
        let parent: Option<(String,)> = sqlx::query_as(
            "SELECT parent_session_id FROM chat_sessions WHERE id = ?",
        )
        .bind(codex_session_id)
        .fetch_optional(&self.pool)
        .await?;

        let voice_session_id = parent
            .and_then(|(p,)| Some(p))
            .ok_or_else(|| anyhow::anyhow!("Codex session has no parent"))?;

        info!(
            codex_session_id = %codex_session_id,
            voice_session_id = %voice_session_id,
            tokens_total = tokens_input + tokens_output,
            cost_usd = cost_usd,
            "Completed Codex session"
        );

        Ok(voice_session_id)
    }

    /// Fail a Codex session with error
    pub async fn fail_codex_session(
        &self,
        codex_session_id: &str,
        error: &str,
    ) -> Result<String> {
        let now = now_timestamp();

        sqlx::query(
            r#"
            UPDATE chat_sessions
            SET codex_status = 'failed', completed_at = ?, last_active = ?
            WHERE id = ? AND session_type = 'codex'
            "#,
        )
        .bind(now)
        .bind(now)
        .bind(codex_session_id)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "UPDATE codex_session_links SET completed_at = ? WHERE codex_session_id = ?",
        )
        .bind(now)
        .bind(codex_session_id)
        .execute(&self.pool)
        .await?;

        // Get parent Voice session ID
        let parent: Option<(String,)> = sqlx::query_as(
            "SELECT parent_session_id FROM chat_sessions WHERE id = ?",
        )
        .bind(codex_session_id)
        .fetch_optional(&self.pool)
        .await?;

        let voice_session_id = parent
            .and_then(|(p,)| Some(p))
            .ok_or_else(|| anyhow::anyhow!("Codex session has no parent"))?;

        warn!(
            codex_session_id = %codex_session_id,
            voice_session_id = %voice_session_id,
            error = %error,
            "Failed Codex session"
        );

        Ok(voice_session_id)
    }

    /// Update OpenAI response_id for a session (for compaction continuity)
    pub async fn update_response_id(
        &self,
        session_id: &str,
        response_id: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE chat_sessions SET openai_response_id = ?, last_active = ? WHERE id = ?",
        )
        .bind(response_id)
        .bind(now_timestamp())
        .bind(session_id)
        .execute(&self.pool)
        .await?;

        debug!(session_id = %session_id, response_id = %response_id, "Updated response_id");
        Ok(())
    }

    /// Get OpenAI response_id for a session
    pub async fn get_response_id(&self, session_id: &str) -> Result<Option<String>> {
        let result: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT openai_response_id FROM chat_sessions WHERE id = ?",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.and_then(|(r,)| r))
    }

    /// Clear response_id (e.g., after rolling summary generation)
    pub async fn clear_response_id(&self, session_id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE chat_sessions SET openai_response_id = NULL WHERE id = ?",
        )
        .bind(session_id)
        .execute(&self.pool)
        .await?;

        debug!(session_id = %session_id, "Cleared response_id");
        Ok(())
    }

    /// Update usage stats for a Codex session
    pub async fn update_codex_usage(
        &self,
        codex_session_id: &str,
        tokens_input: i64,
        tokens_output: i64,
        cost_usd: f64,
        compaction_triggered: bool,
    ) -> Result<()> {
        let compaction_inc = if compaction_triggered { 1 } else { 0 };

        sqlx::query(
            r#"
            UPDATE codex_session_links
            SET tokens_used_input = tokens_used_input + ?,
                tokens_used_output = tokens_used_output + ?,
                cost_usd = cost_usd + ?,
                compaction_count = compaction_count + ?
            WHERE codex_session_id = ?
            "#,
        )
        .bind(tokens_input)
        .bind(tokens_output)
        .bind(cost_usd)
        .bind(compaction_inc)
        .bind(codex_session_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();

        // Run migrations
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .unwrap();

        pool
    }

    #[tokio::test]
    async fn test_session_type_default() {
        let pool = test_pool().await;
        let manager = SessionManager::new(pool);

        // Non-existent session should return Voice
        let session_type = manager.get_session_type("nonexistent").await.unwrap();
        assert_eq!(session_type, SessionType::Voice);
    }

    #[tokio::test]
    async fn test_spawn_codex_session() {
        let pool = test_pool().await;
        let manager = SessionManager::new(pool);

        // Create Voice session (no user_id to avoid FK constraint on users table)
        let voice_id = manager
            .get_or_create_voice_session(None, Some("/test/path"))
            .await
            .unwrap();

        // Spawn Codex session
        let trigger = CodexSpawnTrigger::RouterDetection {
            confidence: 0.9,
            detected_patterns: vec!["implement".to_string()],
        };

        let codex_id = manager
            .spawn_codex_session(&voice_id, "Implement feature X", &trigger, Some("Context summary"))
            .await
            .unwrap();

        // Verify Codex session
        let session_type = manager.get_session_type(&codex_id).await.unwrap();
        assert_eq!(session_type, SessionType::Codex);

        // Verify parent lookup
        let parent = manager.get_voice_session_id(&codex_id).await.unwrap();
        assert_eq!(parent, voice_id);

        // Verify active sessions
        let active = manager.get_active_codex_sessions(&voice_id).await.unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, codex_id);
        assert_eq!(active[0].status, CodexStatus::Running);
    }
}
