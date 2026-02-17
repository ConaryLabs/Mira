// crates/mira-server/src/background/team_monitor.rs
// Background convergence detection for Agent Teams
//
// Runs periodically to detect:
// 1. File conflicts: multiple sessions editing the same file
// 2. Stale sessions: mark sessions with no heartbeat as stopped
// Stores alerts as team-scoped memories.

use crate::db::pool::DatabasePool;
use std::sync::Arc;

/// Stale session threshold in minutes (no heartbeat for this long → stopped)
const STALE_THRESHOLD_MINUTES: i64 = 30;

/// Process team monitoring tasks.
/// Returns count of items processed.
pub async fn process_team_monitor(pool: &Arc<DatabasePool>) -> Result<usize, String> {
    pool.run(move |conn| {
        // Cheap EXISTS check before doing heavier work
        if !has_active_teams(conn) {
            return Ok(0);
        }

        let mut processed = 0;
        let team_ids = get_active_team_ids(conn);

        for team_id in team_ids {
            // 1. Clean up stale sessions
            if let Ok(cleaned) =
                crate::db::cleanup_stale_sessions_sync(conn, team_id, STALE_THRESHOLD_MINUTES)
                && cleaned > 0
            {
                tracing::info!(
                    "Team monitor: cleaned {} stale session(s) for team {}",
                    cleaned,
                    team_id
                );
                processed += cleaned;
            }

            // 2. Detect file conflicts across active sessions
            let conflicts = detect_team_file_conflicts(conn, team_id);
            if !conflicts.is_empty() {
                // Store as team-scoped convergence alert
                let alert_content = format!(
                    "File conflict detected: {}",
                    conflicts
                        .iter()
                        .map(|(file, editors)| format!(
                            "{} (edited by {})",
                            file,
                            editors.join(", ")
                        ))
                        .collect::<Vec<_>>()
                        .join("; ")
                );

                let key = format!("convergence:files:{}", team_id);
                if let Err(e) = crate::db::store_observation_sync(
                    conn,
                    crate::db::StoreObservationParams {
                        project_id: None,
                        key: Some(&key),
                        content: &alert_content,
                        observation_type: "convergence_alert",
                        category: Some("convergence_alert"),
                        confidence: 0.7,
                        source: "team_monitor",
                        session_id: None,
                        team_id: Some(team_id),
                        scope: "team",
                        expires_at: Some("+1 day"),
                    },
                ) {
                    tracing::warn!("Failed to store convergence alert: {}", e);
                }
                processed += 1;
                tracing::info!(
                    "Team monitor: {} file conflict(s) for team {}",
                    conflicts.len(),
                    team_id
                );
            }
        }

        Ok::<usize, rusqlite::Error>(processed)
    })
    .await
    .map_err(Into::into)
}

/// Get IDs of all active teams.
fn get_active_team_ids(conn: &rusqlite::Connection) -> Vec<i64> {
    let mut stmt = match conn.prepare(
        "SELECT DISTINCT t.id FROM teams t
         JOIN team_sessions ts ON ts.team_id = t.id
         WHERE t.status = 'active' AND ts.status = 'active'",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    stmt.query_map([], |row| row.get::<_, i64>(0))
        .map(|rows| rows.filter_map(crate::db::log_and_discard).collect())
        .unwrap_or_default()
}

/// Detect files edited by multiple active sessions in the last 30 minutes.
/// Returns map of file_path → list of editor names.
fn detect_team_file_conflicts(
    conn: &rusqlite::Connection,
    team_id: i64,
) -> Vec<(String, Vec<String>)> {
    let mut stmt = match conn.prepare(
        "SELECT tfo.file_path, GROUP_CONCAT(DISTINCT tfo.member_name)
         FROM team_file_ownership tfo
         JOIN team_sessions ts ON ts.team_id = tfo.team_id AND ts.session_id = tfo.session_id
         WHERE tfo.team_id = ?1
           AND ts.status = 'active'
           AND tfo.timestamp > datetime('now', '-30 minutes')
         GROUP BY tfo.file_path
         HAVING COUNT(DISTINCT tfo.session_id) > 1",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    stmt.query_map(rusqlite::params![team_id], |row| {
        let file_path: String = row.get(0)?;
        let editors_str: String = row.get(1)?;
        let editors: Vec<String> = editors_str.split(',').map(|s| s.to_string()).collect();
        Ok((file_path, editors))
    })
    .map(|rows| rows.filter_map(crate::db::log_and_discard).collect())
    .unwrap_or_default()
}

/// Lightweight check: are there any active teams?
/// Used to skip team monitoring when no teams exist.
pub fn has_active_teams(conn: &rusqlite::Connection) -> bool {
    conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM teams t
            JOIN team_sessions ts ON ts.team_id = t.id
            WHERE t.status = 'active' AND ts.status = 'active'
        )",
        [],
        |row| row.get::<_, bool>(0),
    )
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup_pool() -> Arc<DatabasePool> {
        // open_in_memory runs all migrations (including team tables and memory_facts)
        Arc::new(DatabasePool::open_in_memory().await.unwrap())
    }

    async fn setup_pool_with_project(pool: &Arc<DatabasePool>) -> i64 {
        pool.interact(|conn| {
            crate::db::get_or_create_project_sync(conn, "/test", Some("test"))
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .unwrap()
        .0
    }

    #[tokio::test]
    async fn test_no_active_teams() {
        let pool = setup_pool().await;
        let result = pool
            .interact(|conn| {
                Ok::<_, anyhow::Error>((has_active_teams(conn), get_active_team_ids(conn)))
            })
            .await
            .unwrap();
        assert!(!result.0);
        assert!(result.1.is_empty());
    }

    #[tokio::test]
    async fn test_has_active_teams() {
        let pool = setup_pool().await;
        let pid = setup_pool_with_project(&pool).await;
        let tid = pool
            .interact(move |conn| {
                let tid = crate::db::get_or_create_team_sync(conn, "t1", Some(pid), "/c")?;
                crate::db::register_team_session_sync(conn, tid, "s1", "alice", "lead", None)?;
                Ok::<_, anyhow::Error>(tid)
            })
            .await
            .unwrap();

        let result = pool
            .interact(move |conn| {
                Ok::<_, anyhow::Error>((has_active_teams(conn), get_active_team_ids(conn)))
            })
            .await
            .unwrap();
        assert!(result.0);
        assert_eq!(result.1, vec![tid]);
    }

    #[tokio::test]
    async fn test_detect_no_conflicts() {
        let pool = setup_pool().await;
        let pid = setup_pool_with_project(&pool).await;
        let tid = pool
            .interact(move |conn| {
                let tid = crate::db::get_or_create_team_sync(conn, "t1", Some(pid), "/c")?;
                crate::db::register_team_session_sync(conn, tid, "s1", "alice", "lead", None)?;
                crate::db::register_team_session_sync(conn, tid, "s2", "bob", "teammate", None)?;
                crate::db::record_file_ownership_sync(
                    conn, tid, "s1", "alice", "src/a.rs", "Edit",
                )?;
                Ok::<_, anyhow::Error>(tid)
            })
            .await
            .unwrap();

        let conflicts = pool
            .interact(move |conn| Ok::<_, anyhow::Error>(detect_team_file_conflicts(conn, tid)))
            .await
            .unwrap();
        assert!(conflicts.is_empty());
    }

    #[tokio::test]
    async fn test_detect_file_conflict() {
        let pool = setup_pool().await;
        let pid = setup_pool_with_project(&pool).await;
        let tid = pool
            .interact(move |conn| {
                let tid = crate::db::get_or_create_team_sync(conn, "t1", Some(pid), "/c")?;
                crate::db::register_team_session_sync(conn, tid, "s1", "alice", "lead", None)?;
                crate::db::register_team_session_sync(conn, tid, "s2", "bob", "teammate", None)?;
                crate::db::record_file_ownership_sync(
                    conn,
                    tid,
                    "s1",
                    "alice",
                    "src/shared.rs",
                    "Edit",
                )?;
                crate::db::record_file_ownership_sync(
                    conn,
                    tid,
                    "s2",
                    "bob",
                    "src/shared.rs",
                    "Write",
                )?;
                Ok::<_, anyhow::Error>(tid)
            })
            .await
            .unwrap();

        let conflicts = pool
            .interact(move |conn| Ok::<_, anyhow::Error>(detect_team_file_conflicts(conn, tid)))
            .await
            .unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].0, "src/shared.rs");
        assert!(conflicts[0].1.contains(&"alice".to_string()));
        assert!(conflicts[0].1.contains(&"bob".to_string()));
    }
}
