// background/pondering/
// Active Reasoning Loops - "Nightly Pondering"
//
// Analyzes project data during idle time to discover
// actionable insights about goals, code stability, and workflow.

mod heuristic;
mod queries;
mod storage;
pub(crate) mod types;

use crate::db::get_active_projects_sync;
use crate::db::pool::DatabasePool;
use rusqlite::params;
use std::sync::Arc;

/// Minimum number of tool calls as fallback gate when project data is empty
const MIN_TOOL_CALLS: usize = 10;

pub use storage::cleanup_stale_insights;

/// Process pondering - analyze recent activity for patterns
pub async fn process_pondering(
    pool: &Arc<DatabasePool>,
) -> Result<usize, String> {
    // Get all projects with recent activity
    let projects = pool.run(|conn| get_active_projects_sync(conn, 24)).await?;

    let mut processed = 0;

    for (project_id, project_name, _project_path) in projects {
        let name = project_name.unwrap_or_else(|| "Unknown".to_string());

        // Check if we should ponder this project
        let should_ponder = should_ponder_project(pool, project_id).await?;
        if !should_ponder {
            continue;
        }

        // Gather project-aware data (rich queries)
        let data = queries::get_project_insight_data(pool, project_id).await?;

        // Gate: need project data OR sufficient tool history
        let tool_history = queries::get_recent_tool_history(pool, project_id).await?;

        if !data.has_data() && tool_history.len() < MIN_TOOL_CALLS {
            tracing::debug!(
                "Project {} has insufficient data (no project signals, {} tool calls), skipping pondering",
                name,
                tool_history.len()
            );
            // Update timestamp even when skipping -- prevents the per-project
            // pondering timestamp from going stale on low-activity projects.
            let _ = update_last_pondering(pool, project_id).await;
            continue;
        }

        // Generate insights using heuristic analysis
        let insights = heuristic::generate_insights_heuristic(&data);

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
                // Don't advance cooldown -- retry next cycle
            }
        }
    }

    Ok(processed)
}

/// Check if enough time has passed since last pondering
async fn should_ponder_project(pool: &Arc<DatabasePool>, project_id: i64) -> Result<bool, String> {
    pool.run(move |conn| {
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
            None => Ok::<_, rusqlite::Error>(true), // Never pondered before
        }
    })
    .await
    .map_err(Into::into)
}

/// Update last pondering timestamp
async fn update_last_pondering(pool: &Arc<DatabasePool>, project_id: i64) -> Result<(), String> {
    pool.run(move |conn| {
        conn.execute(
            r#"
            INSERT INTO server_state (key, value, updated_at)
            VALUES (?, datetime('now'), datetime('now'))
            ON CONFLICT(key) DO UPDATE SET value = datetime('now'), updated_at = datetime('now')
            "#,
            params![format!("last_pondering_{}", project_id)],
        )?;
        Ok::<_, rusqlite::Error>(())
    })
    .await
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::setup_test_pool;

    // =========================================================================
    // should_ponder_project Tests
    // =========================================================================

    #[tokio::test]
    async fn test_should_ponder_project_never_pondered_returns_true() {
        let pool = setup_test_pool().await;

        // Create a project but never set any pondering timestamp
        let project_id = pool
            .run(|conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, "/tmp/ponder-never", None)
                        .unwrap()
                        .0,
                )
            })
            .await
            .unwrap();

        let result = should_ponder_project(&pool, project_id).await;
        assert!(result.is_ok());
        assert!(
            result.unwrap(),
            "Project that has never been pondered should return true"
        );
    }

    #[tokio::test]
    async fn test_should_ponder_project_recently_pondered_returns_false() {
        let pool = setup_test_pool().await;

        let project_id = pool
            .run(|conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, "/tmp/ponder-recent", None)
                        .unwrap()
                        .0,
                )
            })
            .await
            .unwrap();

        // Set the pondering timestamp to now
        update_last_pondering(&pool, project_id)
            .await
            .expect("update_last_pondering should succeed");

        let result = should_ponder_project(&pool, project_id).await;
        assert!(result.is_ok());
        assert!(
            !result.unwrap(),
            "Project pondered just now should return false (within 6-hour cooldown)"
        );
    }

    #[tokio::test]
    async fn test_should_ponder_project_pondered_over_6h_ago_returns_true() {
        let pool = setup_test_pool().await;

        let project_id = pool
            .run(|conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, "/tmp/ponder-old", None)
                        .unwrap()
                        .0,
                )
            })
            .await
            .unwrap();

        // Set the pondering timestamp to 7 hours ago
        pool.run(move |conn| {
            conn.execute(
                "INSERT INTO server_state (key, value, updated_at) \
                 VALUES (?, datetime('now', '-7 hours'), datetime('now'))",
                params![format!("last_pondering_{}", project_id)],
            )
            .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .unwrap();

        let result = should_ponder_project(&pool, project_id).await;
        assert!(result.is_ok());
        assert!(
            result.unwrap(),
            "Project pondered >6 hours ago should return true"
        );
    }

    // =========================================================================
    // update_last_pondering Tests
    // =========================================================================

    #[tokio::test]
    async fn test_update_last_pondering_inserts_new_record() {
        let pool = setup_test_pool().await;

        let project_id = pool
            .run(|conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, "/tmp/ponder-insert", None)
                        .unwrap()
                        .0,
                )
            })
            .await
            .unwrap();

        let result = update_last_pondering(&pool, project_id).await;
        assert!(result.is_ok(), "First insert should succeed");

        // Verify the record exists
        let value: Option<String> = pool
            .run(move |conn| {
                conn.query_row(
                    "SELECT value FROM server_state WHERE key = ?",
                    params![format!("last_pondering_{}", project_id)],
                    |row| row.get(0),
                )
                .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
            .ok();

        assert!(
            value.is_some(),
            "server_state should contain the pondering timestamp after update"
        );
    }

    #[tokio::test]
    async fn test_update_last_pondering_upserts_existing_record() {
        let pool = setup_test_pool().await;

        let project_id = pool
            .run(|conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, "/tmp/ponder-upsert", None)
                        .unwrap()
                        .0,
                )
            })
            .await
            .unwrap();

        // Insert an old timestamp
        pool.run(move |conn| {
            conn.execute(
                "INSERT INTO server_state (key, value, updated_at) \
                 VALUES (?, datetime('now', '-12 hours'), datetime('now', '-12 hours'))",
                params![format!("last_pondering_{}", project_id)],
            )
            .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .unwrap();

        // Update should upsert (overwrite with current time)
        let result = update_last_pondering(&pool, project_id).await;
        assert!(result.is_ok(), "Upsert should succeed");

        // The updated value should be recent (not the old -12 hours one)
        let is_recent: bool = pool
            .run(move |conn| {
                conn.query_row(
                    "SELECT datetime(value) > datetime('now', '-1 minute') \
                     FROM server_state WHERE key = ?",
                    params![format!("last_pondering_{}", project_id)],
                    |row| row.get(0),
                )
                .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
            .unwrap();

        assert!(
            is_recent,
            "Upserted timestamp should be recent (within last minute)"
        );
    }

    #[tokio::test]
    async fn test_update_last_pondering_does_not_create_duplicate_rows() {
        let pool = setup_test_pool().await;

        let project_id = pool
            .run(|conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, "/tmp/ponder-nodup", None)
                        .unwrap()
                        .0,
                )
            })
            .await
            .unwrap();

        // Call twice
        update_last_pondering(&pool, project_id).await.unwrap();
        update_last_pondering(&pool, project_id).await.unwrap();

        // Should have exactly one row
        let count: i64 = pool
            .run(move |conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM server_state WHERE key = ?",
                    params![format!("last_pondering_{}", project_id)],
                    |row| row.get(0),
                )
                .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
            .unwrap();

        assert_eq!(
            count, 1,
            "Multiple updates should upsert, not create duplicate rows"
        );
    }
}
