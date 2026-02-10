// background/pondering/
// Active Reasoning Loops - "Nightly Pondering"
//
// Analyzes project data during idle time to discover
// actionable insights about goals, code stability, and workflow.

mod heuristic;
mod llm;
mod queries;
mod storage;
pub(crate) mod types;

use crate::db::get_active_projects_sync;
use crate::db::pool::DatabasePool;
use crate::llm::LlmClient;
use crate::utils::ResultExt;
use rusqlite::params;
use std::sync::Arc;

/// Minimum number of tool calls as fallback gate when project data is empty
const MIN_TOOL_CALLS: usize = 10;

pub use storage::cleanup_stale_insights;

/// Process pondering - analyze recent activity for patterns
pub async fn process_pondering(
    pool: &Arc<DatabasePool>,
    client: Option<&Arc<dyn LlmClient>>,
) -> Result<usize, String> {
    // Get all projects with recent activity
    let projects = pool
        .interact(|conn| {
            get_active_projects_sync(conn, 24)
                .map_err(|e| anyhow::anyhow!("Failed to get active projects: {}", e))
        })
        .await
        .str_err()?;

    let mut processed = 0;

    for (project_id, project_name, _project_path) in projects {
        let name = project_name.unwrap_or_else(|| "Unknown".to_string());

        // Check if we should ponder this project
        let should_ponder = should_ponder_project(pool, project_id).await?;
        if !should_ponder {
            continue;
        }

        // Gather project-aware data (new rich queries)
        let data = queries::get_project_insight_data(pool, project_id).await?;

        // Gather legacy data (still used as secondary context for LLM)
        let (tool_history, memories, existing_insights) = tokio::join!(
            queries::get_recent_tool_history(pool, project_id),
            queries::get_recent_memories(pool, project_id),
            queries::get_existing_insights(pool, project_id),
        );
        let tool_history = tool_history?;
        let memories = memories?;
        let existing_insights = existing_insights.unwrap_or_default();

        // Gate: need either project data OR sufficient tool history
        if !data.has_data() && tool_history.len() < MIN_TOOL_CALLS {
            tracing::debug!(
                "Project {} has insufficient data (no project signals, {} tool calls), skipping pondering",
                name,
                tool_history.len()
            );
            continue;
        }

        // Generate insights
        let insights = match client {
            Some(c) => {
                llm::generate_insights(
                    pool,
                    project_id,
                    &name,
                    &data,
                    &tool_history,
                    &memories,
                    &existing_insights,
                    c,
                )
                .await?
            }
            None => heuristic::generate_insights_heuristic(&data),
        };

        // Store insights as behavior patterns, then advance cooldown.
        // Cooldown must come AFTER persistence so transient DB failures
        // don't suppress reprocessing for the full cooldown window.
        match storage::store_insights(pool, project_id, &insights).await {
            Ok(stored) => {
                if stored > 0 {
                    tracing::info!(
                        "Pondering: generated {} insights for project {}",
                        stored,
                        name
                    );
                    processed += stored;
                }
                // Advance cooldown only after successful storage
                update_last_pondering(pool, project_id).await?;
            }
            Err(e) => {
                tracing::warn!(
                    "Pondering: failed to store insights for project {}: {}",
                    name,
                    e
                );
                // Don't advance cooldown — retry next cycle
            }
        }
    }

    Ok(processed)
}

/// Check if enough time has passed since last pondering
async fn should_ponder_project(pool: &Arc<DatabasePool>, project_id: i64) -> Result<bool, String> {
    pool.interact(move |conn| {
        // Check server_state for last pondering time
        let last_pondering: Option<String> = conn
            .query_row(
                "SELECT value FROM server_state WHERE key = ?",
                params![format!("last_pondering_{}", project_id)],
                |row| row.get(0),
            )
            .ok();

        match last_pondering {
            Some(timestamp) => {
                // Only ponder if >6 hours since last time
                let should = conn
                    .query_row(
                        "SELECT datetime(?) < datetime('now', '-6 hours')",
                        params![timestamp],
                        |row| row.get::<_, bool>(0),
                    )
                    .unwrap_or(true);
                Ok(should)
            }
            None => Ok(true), // Never pondered before
        }
    })
    .await
    .str_err()
}

/// Update last pondering timestamp
async fn update_last_pondering(pool: &Arc<DatabasePool>, project_id: i64) -> Result<(), String> {
    pool.interact(move |conn| {
        conn.execute(
            r#"
            INSERT INTO server_state (key, value, updated_at)
            VALUES (?, datetime('now'), datetime('now'))
            ON CONFLICT(key) DO UPDATE SET value = datetime('now'), updated_at = datetime('now')
            "#,
            params![format!("last_pondering_{}", project_id)],
        )
        .map_err(|e| anyhow::anyhow!("Failed to update: {}", e))?;
        Ok(())
    })
    .await
    .str_err()
}
