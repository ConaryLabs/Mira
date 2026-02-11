// background/proactive/mining.rs
// Pattern mining from behavior logs (SQL only, no LLM)

use crate::db::get_active_project_ids_sync;
use crate::db::pool::DatabasePool;
use crate::proactive::patterns::run_pattern_mining;
use std::sync::Arc;

/// Mine patterns from behavior logs - SQL only, no LLM
pub(super) async fn mine_patterns(pool: &Arc<DatabasePool>) -> Result<usize, String> {
    // Get all projects with recent activity
    let projects = pool
        .run(|conn| {
            get_active_project_ids_sync(conn, 24)
                .map_err(|e| anyhow::anyhow!("Failed to get active projects: {}", e))
        })
        .await?;

    let mut total_patterns = 0;

    for project_id in projects {
        let pool_clone = pool.clone();
        let patterns_stored = pool_clone
            .run(move |conn| {
                run_pattern_mining(conn, project_id)
                    .map_err(|e| anyhow::anyhow!("Mining failed: {}", e))
            })
            .await?;

        if patterns_stored > 0 {
            tracing::debug!(
                "Proactive: mined {} patterns for project {}",
                patterns_stored,
                project_id
            );
            total_patterns += patterns_stored;
        }
    }

    if total_patterns > 0 {
        tracing::info!("Proactive: mined {} total patterns", total_patterns);
    }

    Ok(total_patterns)
}
