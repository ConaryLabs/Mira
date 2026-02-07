// background/pondering/storage.rs
// Storage of pondering insights as behavior patterns

use super::types::PonderingInsight;
use crate::db::pool::DatabasePool;
use crate::utils::ResultExt;
use rusqlite::params;
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Delete insights not triggered in the last 30 days.
pub async fn cleanup_stale_insights(
    pool: &Arc<DatabasePool>,
) -> Result<usize, String> {
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

/// Store insights as behavior patterns
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

        for insight in &insights_clone {
            // Generate pattern key from description hash
            let pattern_key = {
                let mut hasher = Sha256::new();
                hasher.update(insight.description.as_bytes());
                format!("{:x}", hasher.finalize())[..16].to_string()
            };

            let pattern_data = serde_json::json!({
                "description": insight.description,
                "evidence": insight.evidence,
                "generated_by": "pondering",
            });

            // Upsert pattern - increment occurrence if exists
            let result = conn.execute(
                r#"
                INSERT INTO behavior_patterns
                    (project_id, pattern_type, pattern_key, pattern_data, confidence,
                     occurrence_count, last_triggered_at, first_seen_at, updated_at)
                VALUES (?, ?, ?, ?, ?, 1, datetime('now'), datetime('now'), datetime('now'))
                ON CONFLICT(project_id, pattern_type, pattern_key) DO UPDATE SET
                    occurrence_count = occurrence_count + 1,
                    confidence = (confidence + excluded.confidence) / 2,
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
                Ok(_) => stored += 1,
                Err(e) => tracing::warn!("Failed to store insight: {}", e),
            }
        }

        Ok(stored)
    })
    .await
    .str_err()
}
