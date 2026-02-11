// background/pondering/storage.rs
// Storage of pondering insights as behavior patterns

use super::types::PonderingInsight;
use crate::db::pool::DatabasePool;
use rusqlite::params;
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Delete insights not triggered in the last 30 days.
pub async fn cleanup_stale_insights(pool: &Arc<DatabasePool>) -> Result<usize, String> {
    pool.run(|conn| {
        let deleted = conn
            .execute(
                "DELETE FROM behavior_patterns \
                 WHERE pattern_type LIKE 'insight_%' \
                   AND last_triggered_at < datetime('now', '-30 days')",
                [],
            )
            .map_err(|e| anyhow::anyhow!("Failed to cleanup insights: {}", e))?;
        if deleted > 0 {
            tracing::info!("Cleaned up {} stale insights", deleted);
        }
        Ok::<_, anyhow::Error>(deleted)
    })
    .await
}

/// Normalize a description for dedup hashing.
/// Lowercases, replaces digits with `#`, and collapses whitespace so that
/// "Module X had 3 reverts" and "Module X had 5 reverts" produce the same key.
fn normalize_for_dedup(description: &str) -> String {
    description
        .to_lowercase()
        .replace(|c: char| c.is_ascii_digit(), "#")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Hash a string into a 16-hex-char key.
fn hash_key(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())[..16].to_string()
}

/// Extract the first file/module path from an insight description.
/// Matches patterns like `src/db/pool.rs`, `background/`, `crates/mira-server/src/...`.
fn extract_primary_entity(description: &str) -> Option<String> {
    // Look for path-like tokens: contains '/' or ends with a known extension
    for token in description.split_whitespace() {
        let clean = token.trim_matches(|c: char| c == '`' || c == '\'' || c == '"' || c == ',');
        if clean.contains('/') || clean.ends_with(".rs") || clean.ends_with(".ts") || clean.ends_with(".py") || clean.ends_with(".js") || clean.ends_with(".go") {
            return Some(clean.to_lowercase());
        }
    }
    None
}

/// Extract the first quoted text from a description (for goal titles, etc.).
fn extract_quoted_text(description: &str) -> Option<&str> {
    // Try double quotes first, then single quotes
    for quote in ['"', '\'', '\u{201c}'] {
        let close = match quote {
            '\u{201c}' => '\u{201d}',
            q => q,
        };
        if let Some(start) = description.find(quote)
            && let Some(end) = description[start + quote.len_utf8()..].find(close)
        {
            let inner = &description[start + quote.len_utf8()..start + quote.len_utf8() + end];
            if !inner.is_empty() {
                return Some(inner);
            }
        }
    }
    None
}

/// Compute a 16-hex-char pattern key, using the primary entity for
/// file/module-based insight types to avoid false-unique keys from
/// different LLM phrasings about the same entity.
fn compute_pattern_key(pattern_type: &str, description: &str) -> String {
    // For file/module insights, key on the entity not the prose
    if matches!(
        pattern_type,
        "insight_fragile_code" | "insight_untested" | "insight_revert_cluster"
    ) && let Some(entity) = extract_primary_entity(description)
    {
        return hash_key(&format!("{}:{}", pattern_type, entity));
    }
    // For stale goal insights, key on the goal title
    if pattern_type == "insight_stale_goal"
        && let Some(title) = extract_quoted_text(description)
    {
        return hash_key(&format!("{}:{}", pattern_type, title.to_lowercase()));
    }
    // Default: current behavior (normalized description hash)
    hash_key(&normalize_for_dedup(description))
}

/// Maximum number of insights to keep per pattern_type per project.
const MAX_INSIGHTS_PER_TYPE: i64 = 10;

/// Store insights as behavior patterns, then enforce per-type cap.
pub(super) async fn store_insights(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    insights: &[PonderingInsight],
) -> Result<usize, String> {
    if insights.is_empty() {
        return Ok(0);
    }

    let insights_clone: Vec<PonderingInsight> = insights
        .iter()
        .map(|i| PonderingInsight {
            pattern_type: i.pattern_type.clone(),
            description: i.description.clone(),
            confidence: i.confidence,
            evidence: i.evidence.clone(),
        })
        .collect();

    pool.run(move |conn| {
        let mut stored = 0;
        let mut types_touched = std::collections::HashSet::new();

        for insight in &insights_clone {
            // Generate pattern key using entity-aware hashing
            let pattern_key = compute_pattern_key(&insight.pattern_type, &insight.description);

            // Skip if this pattern was previously dismissed
            let dismissed: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM behavior_patterns \
                     WHERE project_id = ? AND pattern_type = ? AND pattern_key = ? AND dismissed = 1)",
                    params![project_id, insight.pattern_type, pattern_key],
                    |row| row.get(0),
                )
                .unwrap_or(false);
            if dismissed {
                continue;
            }

            let pattern_data = serde_json::json!({
                "description": insight.description,
                "evidence": insight.evidence,
                "generated_by": "pondering",
            });

            // Upsert pattern - increment occurrence if exists, keep latest evidence
            let result = conn.execute(
                r#"
                INSERT INTO behavior_patterns
                    (project_id, pattern_type, pattern_key, pattern_data, confidence,
                     occurrence_count, last_triggered_at, first_seen_at, updated_at)
                VALUES (?, ?, ?, ?, ?, 1, datetime('now'), datetime('now'), datetime('now'))
                ON CONFLICT(project_id, pattern_type, pattern_key) DO UPDATE SET
                    occurrence_count = occurrence_count + 1,
                    confidence = (confidence + excluded.confidence) / 2,
                    pattern_data = excluded.pattern_data,
                    last_triggered_at = datetime('now')
                "#,
                params![
                    project_id,
                    insight.pattern_type,
                    pattern_key,
                    pattern_data.to_string(),
                    insight.confidence,
                ],
            );

            match result {
                Ok(_) => {
                    stored += 1;
                    types_touched.insert(insight.pattern_type.clone());
                }
                Err(e) => tracing::warn!("Failed to store insight: {}", e),
            }
        }

        // Enforce per-type cap: keep the newest MAX_INSIGHTS_PER_TYPE, evict oldest
        for pattern_type in &types_touched {
            let evicted = conn
                .execute(
                    r#"
                    DELETE FROM behavior_patterns
                    WHERE project_id = ? AND pattern_type = ?
                      AND id NOT IN (
                          SELECT id FROM behavior_patterns
                          WHERE project_id = ? AND pattern_type = ?
                          ORDER BY last_triggered_at DESC
                          LIMIT ?
                      )
                    "#,
                    params![
                        project_id,
                        pattern_type,
                        project_id,
                        pattern_type,
                        MAX_INSIGHTS_PER_TYPE,
                    ],
                )
                .unwrap_or(0);
            if evicted > 0 {
                tracing::info!(
                    "Evicted {} old '{}' insights (cap: {})",
                    evicted,
                    pattern_type,
                    MAX_INSIGHTS_PER_TYPE,
                );
            }
        }

        Ok::<_, anyhow::Error>(stored)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_for_dedup() {
        // Numbers replaced with #
        assert_eq!(
            normalize_for_dedup("Module X had 3 reverts"),
            "module x had # reverts"
        );
        assert_eq!(
            normalize_for_dedup("Module X had 5 reverts"),
            "module x had # reverts"
        );

        // Multi-digit numbers
        assert_eq!(
            normalize_for_dedup("Goal 94 stale for 23 days"),
            "goal ## stale for ## days"
        );
        assert_eq!(
            normalize_for_dedup("Goal 94 stale for 45 days"),
            "goal ## stale for ## days"
        );

        // Whitespace collapsed
        assert_eq!(
            normalize_for_dedup("  extra   spaces   here  "),
            "extra spaces here"
        );
    }

    #[test]
    fn test_same_meaning_same_key() {
        // Generic type falls back to normalized description hash
        let key1 = compute_pattern_key("insight_workflow", "Module background/ had 3 reverts in 24h");
        let key2 = compute_pattern_key("insight_workflow", "Module background/ had 7 reverts in 24h");
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_different_meaning_different_key() {
        let key1 = compute_pattern_key("insight_workflow", "Module background/ had reverts");
        let key2 = compute_pattern_key("insight_workflow", "Module db/ had reverts");
        assert_ne!(key1, key2);
    }

    // ── Entity-aware dedup tests ─────────────────────────────────────

    #[test]
    fn test_extract_primary_entity() {
        assert_eq!(
            extract_primary_entity("File src/db/pool.rs has 148 modifications"),
            Some("src/db/pool.rs".to_string()),
        );
        assert_eq!(
            extract_primary_entity("Module background/ has high churn"),
            Some("background/".to_string()),
        );
        assert_eq!(
            extract_primary_entity("No paths here at all"),
            None,
        );
    }

    #[test]
    fn test_extract_quoted_text() {
        assert_eq!(
            extract_quoted_text(r#"Goal "Implement auth" is stale for 30 days"#),
            Some("Implement auth"),
        );
        assert_eq!(
            extract_quoted_text("Goal 'Quick Wins' has no progress"),
            Some("Quick Wins"),
        );
        assert_eq!(extract_quoted_text("No quotes here"), None);
    }

    #[test]
    fn test_entity_dedup_same_file_different_prose() {
        // Two different descriptions about the same file should produce the same key
        let key1 = compute_pattern_key(
            "insight_fragile_code",
            "src/db/factory.rs has 148 modifications and high revert rate",
        );
        let key2 = compute_pattern_key(
            "insight_fragile_code",
            "LLM provider abstraction src/db/factory.rs shows instability with 148 mods",
        );
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_entity_dedup_different_files() {
        let key1 = compute_pattern_key(
            "insight_fragile_code",
            "src/db/factory.rs has high churn",
        );
        let key2 = compute_pattern_key(
            "insight_fragile_code",
            "src/db/pool.rs has high churn",
        );
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_stale_goal_dedup_same_title() {
        let key1 = compute_pattern_key(
            "insight_stale_goal",
            r#"Goal "Implement auth" has been stale for 30 days"#,
        );
        let key2 = compute_pattern_key(
            "insight_stale_goal",
            r#"Goal "Implement auth" still in progress after 45 days with no updates"#,
        );
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_stale_goal_dedup_different_titles() {
        let key1 = compute_pattern_key(
            "insight_stale_goal",
            r#"Goal "Implement auth" is stale"#,
        );
        let key2 = compute_pattern_key(
            "insight_stale_goal",
            r#"Goal "Add caching" is stale"#,
        );
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_session_insight_falls_back_to_description() {
        // Session/workflow insights have no entity, so they use normalized description
        let key1 = compute_pattern_key(
            "insight_session",
            "5 sessions lasted under 5 minutes",
        );
        let key2 = compute_pattern_key(
            "insight_session",
            "8 sessions lasted under 5 minutes",
        );
        // Numbers are normalized to #, so these should match
        assert_eq!(key1, key2);
    }
}
