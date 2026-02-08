// background/pondering/storage.rs
// Storage of pondering insights as behavior patterns

use super::types::PonderingInsight;
use crate::db::pool::DatabasePool;
use crate::utils::ResultExt;
use rusqlite::params;
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Delete insights not triggered in the last 30 days.
pub async fn cleanup_stale_insights(pool: &Arc<DatabasePool>) -> Result<usize, String> {
    pool.interact(|conn| {
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
        Ok(deleted)
    })
    .await
    .str_err()
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

/// Compute a 16-hex-char pattern key from a normalized description.
fn compute_pattern_key(description: &str) -> String {
    let normalized = normalize_for_dedup(description);
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    format!("{:x}", hasher.finalize())[..16].to_string()
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

    pool.interact(move |conn| {
        let mut stored = 0;
        let mut types_touched = std::collections::HashSet::new();

        for insight in &insights_clone {
            // Generate pattern key from normalized description hash
            let pattern_key = compute_pattern_key(&insight.description);

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

        Ok(stored)
    })
    .await
    .str_err()
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
        let key1 = compute_pattern_key("Module background/ had 3 reverts in 24h");
        let key2 = compute_pattern_key("Module background/ had 7 reverts in 24h");
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_different_meaning_different_key() {
        let key1 = compute_pattern_key("Module background/ had reverts");
        let key2 = compute_pattern_key("Module db/ had reverts");
        assert_ne!(key1, key2);
    }
}
