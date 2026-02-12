// crates/mira-server/src/db/observations.rs
// System observations storage -- ephemeral system-generated data (health, scans, extractions)
//
// Unlike memory_facts (user memories, permanent), observations are TTL-based and
// not embedded or exported.

use rusqlite::{Connection, params};

use super::log_and_discard;

/// Parameters for storing a system observation
pub struct StoreObservationParams<'a> {
    pub project_id: Option<i64>,
    pub key: Option<&'a str>,
    pub content: &'a str,
    pub observation_type: &'a str,
    pub category: Option<&'a str>,
    pub confidence: f64,
    pub source: &'a str,
    pub session_id: Option<&'a str>,
    pub team_id: Option<i64>,
    pub scope: &'a str,
    pub expires_at: Option<&'a str>,
}

/// Store a system observation with UPSERT by (project_id, team_id, scope, key) when key is provided.
///
/// Uses COALESCE on nullable principals to handle NULL safely (SQLite treats NULLs
/// as distinct, so raw comparisons would create duplicate rows).
///
/// `expires_at` accepts either absolute timestamps ("2026-02-18 12:00:00") or
/// SQLite relative modifiers ("+7 days", "-1 hour"). Relative values are resolved
/// to absolute timestamps via `datetime('now', ?)` before storage so that TTL
/// comparison in `cleanup_expired_observations_sync` works correctly.
///
/// Returns the observation ID.
pub fn store_observation_sync(
    conn: &Connection,
    params: StoreObservationParams,
) -> rusqlite::Result<i64> {
    // Resolve relative durations ("+7 days", "-1 hour") to absolute timestamps.
    // Without this, the raw string would be stored and text-compared against
    // datetime('now'), causing immediate false-positive expiration.
    let resolved_expires: Option<String> = match params.expires_at {
        Some(rel) if rel.starts_with('+') || rel.starts_with('-') => conn
            .query_row("SELECT datetime('now', ?1)", [rel], |row| row.get(0))
            .ok(),
        Some(abs) => Some(abs.to_string()),
        None => None,
    };
    let resolved_expires_ref = resolved_expires.as_deref();

    if let Some(key) = params.key {
        // Manual UPSERT: check for existing row first, then update or insert.
        // SQLite doesn't support ON CONFLICT with expression-based indexes.
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM system_observations
                 WHERE COALESCE(project_id, -1) = COALESCE(?1, -1)
                   AND COALESCE(team_id, -1) = COALESCE(?2, -1)
                   AND scope = ?3
                   AND key = ?4",
                params![params.project_id, params.team_id, params.scope, key],
                |row| row.get(0),
            )
            .ok();

        if let Some(id) = existing {
            conn.execute(
                "UPDATE system_observations SET
                    content = ?1, observation_type = ?2, category = ?3,
                    confidence = ?4, source = ?5, session_id = ?6,
                    expires_at = ?7, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?8",
                params![
                    params.content,
                    params.observation_type,
                    params.category,
                    params.confidence,
                    params.source,
                    params.session_id,
                    resolved_expires_ref,
                    id,
                ],
            )?;
            Ok(id)
        } else {
            conn.execute(
                "INSERT INTO system_observations
                    (project_id, key, content, observation_type, category, confidence,
                     source, session_id, team_id, scope, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    params.project_id,
                    key,
                    params.content,
                    params.observation_type,
                    params.category,
                    params.confidence,
                    params.source,
                    params.session_id,
                    params.team_id,
                    params.scope,
                    resolved_expires_ref,
                ],
            )?;
            Ok(conn.last_insert_rowid())
        }
    } else {
        // No key: always INSERT (no upsert target)
        conn.execute(
            "INSERT INTO system_observations
                (project_id, content, observation_type, category, confidence,
                 source, session_id, team_id, scope, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                params.project_id,
                params.content,
                params.observation_type,
                params.category,
                params.confidence,
                params.source,
                params.session_id,
                params.team_id,
                params.scope,
                resolved_expires_ref,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }
}

/// Query observations by type for a project
#[allow(clippy::type_complexity)]
pub fn query_observations_by_type_sync(
    conn: &Connection,
    project_id: i64,
    observation_type: &str,
    limit: usize,
) -> rusqlite::Result<Vec<(i64, String, Option<String>, f64)>> {
    let mut stmt = conn.prepare(
        "SELECT id, content, category, confidence
         FROM system_observations
         WHERE project_id = ?1 AND observation_type = ?2
         ORDER BY updated_at DESC
         LIMIT ?3",
    )?;
    let rows = stmt
        .query_map(params![project_id, observation_type, limit as i64], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .filter_map(log_and_discard)
        .collect();
    Ok(rows)
}

/// Query observations by type and categories (for scoring.rs gather_findings)
pub fn query_observations_by_categories_sync(
    conn: &Connection,
    project_id: i64,
    observation_type: &str,
    categories: &[&str],
) -> rusqlite::Result<Vec<(String, String)>> {
    if categories.is_empty() {
        return Ok(Vec::new());
    }
    // Cap to 50 categories to stay within SQLite's default 999 parameter limit
    let categories = if categories.len() > 50 {
        &categories[..50]
    } else {
        categories
    };
    let placeholders: Vec<String> = categories
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 3))
        .collect();
    let sql = format!(
        "SELECT content, category FROM system_observations
         WHERE project_id = ?1 AND observation_type = ?2
           AND category IN ({})
         ORDER BY updated_at DESC",
        placeholders.join(", ")
    );
    let mut stmt = conn.prepare(&sql)?;

    // Build param list: project_id, observation_type, then each category
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    param_values.push(Box::new(project_id));
    param_values.push(Box::new(observation_type.to_string()));
    for cat in categories {
        param_values.push(Box::new(cat.to_string()));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    let rows = stmt
        .query_map(param_refs.as_slice(), |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(log_and_discard)
        .collect();
    Ok(rows)
}

/// Check if an observation key exists for a project
pub fn observation_key_exists_sync(conn: &Connection, project_id: i64, key: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM system_observations WHERE project_id = ?1 AND key = ?2",
        params![project_id, key],
        |_| Ok(true),
    )
    .unwrap_or(false)
}

/// Get observation info (content, updated_at) for a key -- replaces get_scan_info_sync for system keys
pub fn get_observation_info_sync(
    conn: &Connection,
    project_id: i64,
    key: &str,
) -> Option<(String, String)> {
    conn.query_row(
        "SELECT content, COALESCE(updated_at, created_at) FROM system_observations
         WHERE project_id = ?1 AND key = ?2",
        params![project_id, key],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .ok()
}

/// Delete an observation by key for a project
pub fn delete_observation_by_key_sync(
    conn: &Connection,
    project_id: i64,
    key: &str,
) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM system_observations WHERE project_id = ?1 AND key = ?2",
        params![project_id, key],
    )
}

/// Delete all observations of a given type for a project
pub fn delete_observations_by_type_sync(
    conn: &Connection,
    project_id: i64,
    observation_type: &str,
) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM system_observations WHERE project_id = ?1 AND observation_type = ?2",
        params![project_id, observation_type],
    )
}

/// Delete observations by type and categories for a project
pub fn delete_observations_by_categories_sync(
    conn: &Connection,
    project_id: i64,
    observation_type: &str,
    categories: &[&str],
) -> rusqlite::Result<usize> {
    let mut total = 0;
    for category in categories {
        total += conn.execute(
            "DELETE FROM system_observations
             WHERE project_id = ?1 AND observation_type = ?2 AND category = ?3",
            params![project_id, observation_type, category],
        )?;
    }
    Ok(total)
}

/// Clean up expired observations (TTL-based). Returns count of deleted rows.
pub fn cleanup_expired_observations_sync(conn: &Connection) -> rusqlite::Result<usize> {
    let deleted = conn.execute(
        "DELETE FROM system_observations
         WHERE expires_at IS NOT NULL AND expires_at < datetime('now')",
        [],
    )?;
    if deleted > 0 {
        tracing::info!("[observations] Cleaned up {} expired observations", deleted);
    }
    Ok(deleted)
}

/// Query observations by team scope (for user_prompt.rs team discoveries)
pub fn query_team_observations_sync(
    conn: &Connection,
    team_id: i64,
    since_duration: &str,
    limit: usize,
) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT content, COALESCE(category, 'general')
         FROM system_observations
         WHERE scope = 'team' AND team_id = ?1
           AND COALESCE(updated_at, created_at) > datetime('now', ?2)
         ORDER BY COALESCE(updated_at, created_at) DESC
         LIMIT ?3",
    )?;
    let rows = stmt
        .query_map(params![team_id, since_duration, limit as i64], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .filter_map(log_and_discard)
        .collect();
    Ok(rows)
}

/// Query health alerts from system_observations (replaces get_health_alerts_sync from memory.rs)
pub fn get_health_observations_sync(
    conn: &Connection,
    project_id: i64,
    limit: usize,
) -> rusqlite::Result<Vec<(String, Option<String>, f64)>> {
    let mut stmt = conn.prepare(
        "SELECT content, category, confidence
         FROM system_observations
         WHERE project_id = ?1
           AND observation_type = 'health'
           AND confidence >= 0.7
         ORDER BY confidence DESC, COALESCE(updated_at, created_at) DESC
         LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![project_id, limit as i64], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .filter_map(log_and_discard)
        .collect();
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::setup_test_connection;

    fn insert_project(conn: &Connection) -> i64 {
        conn.execute(
            "INSERT INTO projects (path, name) VALUES ('/test/obs', 'obs-test')",
            [],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn store_and_query_observation() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);

        let id = store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("health:todo:file.rs:10"),
                content: "TODO found at file.rs:10",
                observation_type: "health",
                category: Some("todo"),
                confidence: 0.8,
                source: "code_health",
                session_id: None,
                team_id: None,
                scope: "project",
                expires_at: None,
            },
        )
        .unwrap();
        assert!(id > 0);

        let results = query_observations_by_type_sync(&conn, pid, "health", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, "TODO found at file.rs:10");
    }

    #[test]
    fn upsert_by_key() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);

        let id1 = store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("scan_time"),
                content: "v1",
                observation_type: "system",
                category: Some("health"),
                confidence: 1.0,
                source: "code_health",
                session_id: None,
                team_id: None,
                scope: "project",
                expires_at: None,
            },
        )
        .unwrap();

        let id2 = store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("scan_time"),
                content: "v2",
                observation_type: "system",
                category: Some("health"),
                confidence: 1.0,
                source: "code_health",
                session_id: None,
                team_id: None,
                scope: "project",
                expires_at: None,
            },
        )
        .unwrap();

        assert_eq!(id1, id2, "same key should upsert in place");

        let info = get_observation_info_sync(&conn, pid, "scan_time").unwrap();
        assert_eq!(info.0, "v2");
    }

    #[test]
    fn delete_by_key() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);

        store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("marker"),
                content: "test",
                observation_type: "system",
                category: None,
                confidence: 1.0,
                source: "test",
                session_id: None,
                team_id: None,
                scope: "project",
                expires_at: None,
            },
        )
        .unwrap();

        assert!(observation_key_exists_sync(&conn, pid, "marker"));
        delete_observation_by_key_sync(&conn, pid, "marker").unwrap();
        assert!(!observation_key_exists_sync(&conn, pid, "marker"));
    }

    #[test]
    fn delete_by_type() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);

        for i in 0..3 {
            store_observation_sync(
                &conn,
                StoreObservationParams {
                    project_id: Some(pid),
                    key: Some(&format!("health:{}", i)),
                    content: &format!("issue {}", i),
                    observation_type: "health",
                    category: Some("todo"),
                    confidence: 0.8,
                    source: "test",
                    session_id: None,
                    team_id: None,
                    scope: "project",
                    expires_at: None,
                },
            )
            .unwrap();
        }

        let deleted = delete_observations_by_type_sync(&conn, pid, "health").unwrap();
        assert_eq!(deleted, 3);

        let remaining = query_observations_by_type_sync(&conn, pid, "health", 10).unwrap();
        assert!(remaining.is_empty());
    }

    #[test]
    fn delete_by_categories() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);

        store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("h1"),
                content: "todo issue",
                observation_type: "health",
                category: Some("todo"),
                confidence: 0.8,
                source: "test",
                session_id: None,
                team_id: None,
                scope: "project",
                expires_at: None,
            },
        )
        .unwrap();
        store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("h2"),
                content: "unwrap issue",
                observation_type: "health",
                category: Some("unwrap"),
                confidence: 0.8,
                source: "test",
                session_id: None,
                team_id: None,
                scope: "project",
                expires_at: None,
            },
        )
        .unwrap();
        store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("h3"),
                content: "complexity issue",
                observation_type: "health",
                category: Some("complexity"),
                confidence: 0.8,
                source: "test",
                session_id: None,
                team_id: None,
                scope: "project",
                expires_at: None,
            },
        )
        .unwrap();

        let deleted =
            delete_observations_by_categories_sync(&conn, pid, "health", &["todo", "unwrap"])
                .unwrap();
        assert_eq!(deleted, 2);

        let remaining = query_observations_by_type_sync(&conn, pid, "health", 10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].1, "complexity issue");
    }

    #[test]
    fn ttl_cleanup() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);

        // Insert expired observation via raw SQL (absolute timestamp)
        conn.execute(
            "INSERT INTO system_observations
                (project_id, key, content, observation_type, source, scope, expires_at)
             VALUES (?1, 'expired', 'old', 'session_event', 'test', 'project', datetime('now', '-1 day'))",
            [pid],
        )
        .unwrap();

        // Insert non-expired observation via raw SQL (absolute timestamp)
        conn.execute(
            "INSERT INTO system_observations
                (project_id, key, content, observation_type, source, scope, expires_at)
             VALUES (?1, 'valid', 'new', 'session_event', 'test', 'project', datetime('now', '+7 days'))",
            [pid],
        )
        .unwrap();

        // Insert observation with no expiry
        store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("permanent"),
                content: "no ttl",
                observation_type: "system",
                category: None,
                confidence: 1.0,
                source: "test",
                session_id: None,
                team_id: None,
                scope: "project",
                expires_at: None,
            },
        )
        .unwrap();

        let cleaned = cleanup_expired_observations_sync(&conn).unwrap();
        assert_eq!(cleaned, 1);

        assert!(!observation_key_exists_sync(&conn, pid, "expired"));
        assert!(observation_key_exists_sync(&conn, pid, "valid"));
        assert!(observation_key_exists_sync(&conn, pid, "permanent"));
    }

    /// Regression test: callers pass relative durations like "+7 days" to expires_at.
    /// These must be resolved to absolute timestamps before storage, otherwise
    /// cleanup_expired_observations_sync will purge them immediately ("+7 days" < "2026-..." in text comparison).
    #[test]
    fn ttl_relative_duration_resolved_before_storage() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);

        // Store via the API with a relative "+7 days" TTL (the real caller pattern)
        store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("relative_ttl"),
                content: "should survive cleanup",
                observation_type: "session_event",
                category: Some("compaction"),
                confidence: 0.8,
                source: "test",
                session_id: None,
                team_id: None,
                scope: "project",
                expires_at: Some("+7 days"),
            },
        )
        .unwrap();

        // Store an already-expired observation via the API
        store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("expired_relative"),
                content: "should be cleaned up",
                observation_type: "session_event",
                category: Some("compaction"),
                confidence: 0.8,
                source: "test",
                session_id: None,
                team_id: None,
                scope: "project",
                expires_at: Some("-1 day"),
            },
        )
        .unwrap();

        // Verify the stored expires_at is an absolute timestamp, not "+7 days"
        let stored: String = conn
            .query_row(
                "SELECT expires_at FROM system_observations WHERE key = 'relative_ttl'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            !stored.starts_with('+'),
            "expires_at should be an absolute timestamp, got: {}",
            stored
        );

        // Cleanup should only purge the expired one
        let cleaned = cleanup_expired_observations_sync(&conn).unwrap();
        assert_eq!(cleaned, 1);

        assert!(observation_key_exists_sync(&conn, pid, "relative_ttl"));
        assert!(!observation_key_exists_sync(&conn, pid, "expired_relative"));
    }

    #[test]
    fn query_by_categories() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);

        store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("c1"),
                content: "complexity finding",
                observation_type: "health",
                category: Some("complexity"),
                confidence: 0.8,
                source: "test",
                session_id: None,
                team_id: None,
                scope: "project",
                expires_at: None,
            },
        )
        .unwrap();
        store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("c2"),
                content: "todo finding",
                observation_type: "health",
                category: Some("todo"),
                confidence: 0.8,
                source: "test",
                session_id: None,
                team_id: None,
                scope: "project",
                expires_at: None,
            },
        )
        .unwrap();
        store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("c3"),
                content: "system marker",
                observation_type: "system",
                category: Some("health"),
                confidence: 1.0,
                source: "test",
                session_id: None,
                team_id: None,
                scope: "project",
                expires_at: None,
            },
        )
        .unwrap();

        let results =
            query_observations_by_categories_sync(&conn, pid, "health", &["complexity", "todo"])
                .unwrap();
        assert_eq!(results.len(), 2);

        // System type should not be included
        let results =
            query_observations_by_categories_sync(&conn, pid, "system", &["health"]).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn team_scoped_upsert_isolation() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);

        // Same key, different team_id -> separate rows
        let id1 = store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("convergence:files"),
                content: "team 1 conflict",
                observation_type: "convergence_alert",
                category: Some("convergence_alert"),
                confidence: 0.8,
                source: "team_monitor",
                session_id: None,
                team_id: Some(1),
                scope: "team",
                expires_at: None,
            },
        )
        .unwrap();

        let id2 = store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("convergence:files"),
                content: "team 2 conflict",
                observation_type: "convergence_alert",
                category: Some("convergence_alert"),
                confidence: 0.8,
                source: "team_monitor",
                session_id: None,
                team_id: Some(2),
                scope: "team",
                expires_at: None,
            },
        )
        .unwrap();

        assert_ne!(id1, id2, "different teams should create separate rows");

        // Same team, same key -> upsert
        let id3 = store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("convergence:files"),
                content: "team 1 updated",
                observation_type: "convergence_alert",
                category: Some("convergence_alert"),
                confidence: 0.9,
                source: "team_monitor",
                session_id: None,
                team_id: Some(1),
                scope: "team",
                expires_at: None,
            },
        )
        .unwrap();

        assert_eq!(id1, id3, "same team + same key should upsert");
    }

    #[test]
    fn team_observations_query() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);

        store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("disc:1"),
                content: "team discovery",
                observation_type: "distilled",
                category: Some("distilled"),
                confidence: 0.8,
                source: "distillation",
                session_id: None,
                team_id: Some(42),
                scope: "team",
                expires_at: None,
            },
        )
        .unwrap();

        let results = query_team_observations_sync(&conn, 42, "-1 hour", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "team discovery");

        // Different team should see nothing
        let results = query_team_observations_sync(&conn, 99, "-1 hour", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn health_observations_query() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);

        store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("h:1"),
                content: "high confidence health issue",
                observation_type: "health",
                category: Some("complexity"),
                confidence: 0.9,
                source: "code_health",
                session_id: None,
                team_id: None,
                scope: "project",
                expires_at: None,
            },
        )
        .unwrap();

        store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(pid),
                key: Some("h:2"),
                content: "low confidence health issue",
                observation_type: "health",
                category: Some("todo"),
                confidence: 0.3,
                source: "code_health",
                session_id: None,
                team_id: None,
                scope: "project",
                expires_at: None,
            },
        )
        .unwrap();

        let results = get_health_observations_sync(&conn, pid, 10).unwrap();
        assert_eq!(
            results.len(),
            1,
            "only high-confidence alerts should appear"
        );
        assert_eq!(results[0].0, "high confidence health issue");
    }
}
