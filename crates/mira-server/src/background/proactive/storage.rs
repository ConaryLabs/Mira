// background/proactive/storage.rs
// Storage and cleanup of proactive suggestions

use super::PreGeneratedSuggestion;
use crate::background::is_fallback_content;
use crate::db::pool::DatabasePool;
use crate::utils::ResultExt;
use rusqlite::params;
use std::sync::Arc;

/// Store suggestions in the database
/// Upgrade guard: won't overwrite non-fallback (LLM) suggestions with template suggestions
pub(super) async fn store_suggestions(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    suggestions: &[PreGeneratedSuggestion],
) -> Result<usize, String> {
    if suggestions.is_empty() {
        return Ok(0);
    }

    let suggestions_clone: Vec<_> = suggestions
        .iter()
        .map(|s| PreGeneratedSuggestion {
            pattern_id: s.pattern_id,
            trigger_key: s.trigger_key.clone(),
            suggestion_text: s.suggestion_text.clone(),
            confidence: s.confidence,
        })
        .collect();

    pool.interact(move |conn| {
        let mut stored = 0;

        for suggestion in &suggestions_clone {
            let is_template = is_fallback_content(&suggestion.suggestion_text);

            // Upgrade guard: if this is a template suggestion, don't overwrite LLM output
            if is_template {
                let existing: Option<String> = conn
                    .query_row(
                        "SELECT suggestion_text FROM proactive_suggestions WHERE project_id = ? AND trigger_key = ?",
                        params![project_id, suggestion.trigger_key],
                        |row| row.get(0),
                    )
                    .ok();

                if let Some(ref text) = existing
                    && !text.is_empty() && !is_fallback_content(text) {
                        // Existing is LLM-generated — don't overwrite
                        continue;
                    }
            }

            let result = conn.execute(
                r#"
                INSERT INTO proactive_suggestions
                    (project_id, pattern_id, trigger_key, suggestion_text, confidence, expires_at)
                VALUES (?, ?, ?, ?, ?, datetime('now', '+7 days'))
                ON CONFLICT(project_id, trigger_key) DO UPDATE SET
                    pattern_id = excluded.pattern_id,
                    suggestion_text = excluded.suggestion_text,
                    confidence = excluded.confidence,
                    expires_at = datetime('now', '+7 days')
                "#,
                params![
                    project_id,
                    suggestion.pattern_id,
                    suggestion.trigger_key,
                    suggestion.suggestion_text,
                    suggestion.confidence,
                ],
            );

            match result {
                Ok(_) => stored += 1,
                Err(e) => tracing::warn!("Failed to store suggestion: {}", e),
            }
        }

        Ok::<usize, anyhow::Error>(stored)
    })
    .await
    .str_err()
}

/// Clean up expired suggestions
pub async fn cleanup_expired_suggestions(pool: &Arc<DatabasePool>) -> Result<usize, String> {
    pool.interact(|conn| {
        let deleted = conn
            .execute(
                "DELETE FROM proactive_suggestions WHERE expires_at < datetime('now')",
                [],
            )
            .map_err(|e| anyhow::anyhow!("Failed to cleanup: {}", e))?;
        Ok(deleted)
    })
    .await
    .str_err()
}
