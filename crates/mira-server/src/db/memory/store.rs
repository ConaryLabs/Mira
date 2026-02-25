// db/memory/store.rs
// Memory storage operations: create, import, embed

use rusqlite::OptionalExtension;

/// Parameters for storing a memory with full scope support
pub struct StoreMemoryParams<'a> {
    pub project_id: Option<i64>,
    pub key: Option<&'a str>,
    pub content: &'a str,
    pub fact_type: &'a str,
    pub category: Option<&'a str>,
    pub confidence: f64,
    pub session_id: Option<&'a str>,
    pub user_id: Option<&'a str>,
    pub scope: &'a str,
    pub branch: Option<&'a str>,
    pub team_id: Option<i64>,
    pub suspicious: bool,
}

/// Store a memory with full scope/user support (sync version for pool.interact())
/// Returns the memory ID
pub fn store_memory_sync(
    conn: &rusqlite::Connection,
    params: StoreMemoryParams,
) -> rusqlite::Result<i64> {
    // Upsert by key if provided — includes scope principal to prevent cross-scope overwrites
    if let Some(key) = params.key {
        let existing: Option<(i64, Option<String>)> = match conn.query_row(
            "SELECT id, last_session_id FROM memory_facts
                 WHERE key = ?1 AND project_id IS ?2
                   AND COALESCE(scope, 'project') = ?3
                   AND COALESCE(team_id, 0) = COALESCE(?4, 0)
                   AND (?3 != 'personal' OR COALESCE(user_id, '') = COALESCE(?5, ''))",
            rusqlite::params![
                key,
                params.project_id,
                params.scope,
                params.team_id,
                params.user_id
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ) {
            Ok(v) => Some(v),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => {
                tracing::warn!("Failed to update memory embedding: {e}");
                None
            }
        };

        if let Some((id, last_session)) = existing {
            let is_new_session = params
                .session_id
                .map(|s| last_session.as_deref() != Some(s))
                .unwrap_or(false);

            if is_new_session {
                conn.execute(
                    "UPDATE memory_facts SET content = ?, fact_type = ?, category = ?, confidence = ?,
                     session_count = session_count + 1, last_session_id = ?, user_id = COALESCE(user_id, ?),
                     scope = ?, branch = ?, team_id = ?, suspicious = ?,
                     stale_since = NULL, stale_file_path = NULL,
                     updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                    rusqlite::params![
                        params.content, params.fact_type, params.category, params.confidence,
                        params.session_id, params.user_id, params.scope, params.branch, params.team_id,
                        params.suspicious as i32, id
                    ],
                )?;
                // Check for promotion
                conn.execute(
                    "UPDATE memory_facts SET status = 'confirmed', confidence = MIN(confidence + 0.2, 1.0)
                     WHERE id = ? AND status = 'candidate' AND session_count >= 3",
                    [id],
                )?;
            } else {
                conn.execute(
                    "UPDATE memory_facts SET content = ?, fact_type = ?, category = ?, confidence = ?,
                     user_id = COALESCE(user_id, ?), scope = ?, branch = ?, team_id = ?, suspicious = ?,
                     stale_since = NULL, stale_file_path = NULL,
                     updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                    rusqlite::params![
                        params.content, params.fact_type, params.category, params.confidence,
                        params.user_id, params.scope, params.branch, params.team_id,
                        params.suspicious as i32, id
                    ],
                )?;
            }
            return Ok(id);
        }
    }

    // New memory - starts as candidate with capped confidence
    let initial_confidence = if params.confidence < 1.0 {
        params.confidence
    } else {
        0.5
    };
    conn.execute(
        "INSERT INTO memory_facts (project_id, key, content, fact_type, category, confidence,
         session_count, first_session_id, last_session_id, status, user_id, scope, branch, team_id, suspicious)
         VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, 'candidate', ?, ?, ?, ?, ?)",
        rusqlite::params![
            params.project_id,
            params.key,
            params.content,
            params.fact_type,
            params.category,
            initial_confidence,
            params.session_id,
            params.session_id,
            params.user_id,
            params.scope,
            params.branch,
            params.team_id,
            params.suspicious as i32
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Import a confirmed memory (bypasses evidence-based promotion)
/// Used for importing from CLAUDE.local.md where entries are already high-confidence.
/// On re-import, updates existing memories matched by (key, project_id) instead of duplicating.
pub fn import_confirmed_memory_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
    key: &str,
    content: &str,
    fact_type: &str,
    category: Option<&str>,
    confidence: f64,
) -> rusqlite::Result<i64> {
    // Check for existing memory with same key and project_id
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM memory_facts WHERE key = ?1 AND project_id IS ?2",
            rusqlite::params![key, project_id],
            |row| row.get(0),
        )
        .optional()
        .unwrap_or(None);

    if let Some(id) = existing {
        conn.execute(
            "UPDATE memory_facts SET content = ?, fact_type = ?, category = ?, confidence = ?,
             status = 'confirmed', updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            rusqlite::params![content, fact_type, category, confidence, id],
        )?;
        Ok(id)
    } else {
        conn.execute(
            "INSERT INTO memory_facts (project_id, key, content, fact_type, category, confidence,
             session_count, first_session_id, last_session_id, status)
             VALUES (?, ?, ?, ?, ?, ?, 1, NULL, NULL, 'confirmed')",
            rusqlite::params![project_id, key, content, fact_type, category, confidence],
        )?;
        Ok(conn.last_insert_rowid())
    }
}

/// Mark memories referencing a file as stale when that file changes.
///
/// Searches memory_facts content for references to the file path (both full path
/// and basename). Only marks not-yet-stale memories (stale_since IS NULL) to avoid
/// resetting the staleness clock on already-stale memories.
///
/// Returns the number of memories marked stale.
pub fn mark_memories_stale_for_file_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
    file_path: &str,
) -> rusqlite::Result<usize> {
    // Extract basename for matching (e.g., "main.rs" from "/src/main.rs")
    let basename = std::path::Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(file_path);

    // Match memories that reference this file by full path or basename.
    // Only mark memories that aren't already stale.
    let updated = conn.execute(
        "UPDATE memory_facts
         SET stale_since = CURRENT_TIMESTAMP, stale_file_path = ?1
         WHERE project_id = ?2
           AND stale_since IS NULL
           AND (content LIKE '%' || ?1 || '%' OR content LIKE '%' || ?3 || '%')",
        rusqlite::params![file_path, project_id, basename],
    )?;

    if updated > 0 {
        tracing::debug!(
            count = updated,
            file = file_path,
            "Marked memories as stale for changed file"
        );
    }

    Ok(updated)
}

/// Store embedding for a fact and mark as embedded (sync version for pool.interact())
pub fn store_fact_embedding_sync(
    conn: &rusqlite::Connection,
    fact_id: i64,
    content: &str,
    embedding_bytes: &[u8],
) -> rusqlite::Result<()> {
    // Insert or update embedding
    conn.execute(
        "INSERT OR REPLACE INTO vec_memory (rowid, embedding, fact_id, content) VALUES (?, ?, ?, ?)",
        rusqlite::params![fact_id, embedding_bytes, fact_id, content],
    )?;

    // Mark fact as having embedding
    conn.execute(
        "UPDATE memory_facts SET has_embedding = 1 WHERE id = ?",
        [fact_id],
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::setup_test_connection;

    fn insert_project(conn: &rusqlite::Connection) -> i64 {
        conn.execute(
            "INSERT INTO projects (path, name) VALUES ('/test/staleness', 'staleness-test')",
            [],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn store_memory(conn: &rusqlite::Connection, project_id: i64, content: &str) -> i64 {
        store_memory_sync(
            conn,
            StoreMemoryParams {
                project_id: Some(project_id),
                key: None,
                content,
                fact_type: "decision",
                category: None,
                confidence: 0.8,
                session_id: Some("test-session"),
                user_id: None,
                scope: "project",
                branch: None,
                team_id: None,
                suspicious: false,
            },
        )
        .unwrap()
    }

    fn get_stale_since(conn: &rusqlite::Connection, id: i64) -> Option<String> {
        conn.query_row(
            "SELECT stale_since FROM memory_facts WHERE id = ?",
            [id],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn test_mark_stale_matches_full_path() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);
        let id = store_memory(&conn, pid, "The config in /src/config.rs uses builder pattern");
        assert!(get_stale_since(&conn, id).is_none());

        let count = mark_memories_stale_for_file_sync(&conn, pid, "/src/config.rs").unwrap();
        assert_eq!(count, 1);
        assert!(get_stale_since(&conn, id).is_some());
    }

    #[test]
    fn test_mark_stale_matches_basename() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);
        let id = store_memory(&conn, pid, "The config.rs file uses builder pattern");
        assert!(get_stale_since(&conn, id).is_none());

        let count = mark_memories_stale_for_file_sync(&conn, pid, "/src/config.rs").unwrap();
        assert_eq!(count, 1);
        assert!(get_stale_since(&conn, id).is_some());
    }

    #[test]
    fn test_mark_stale_skips_unrelated_memories() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);
        let related = store_memory(&conn, pid, "auth.rs handles authentication");
        let unrelated = store_memory(&conn, pid, "database uses connection pooling");

        let count = mark_memories_stale_for_file_sync(&conn, pid, "/src/auth.rs").unwrap();
        assert_eq!(count, 1);
        assert!(get_stale_since(&conn, related).is_some());
        assert!(get_stale_since(&conn, unrelated).is_none());
    }

    #[test]
    fn test_mark_stale_does_not_restale_already_stale() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);
        let id = store_memory(&conn, pid, "The config.rs uses builder pattern");

        // Mark stale first time
        mark_memories_stale_for_file_sync(&conn, pid, "/src/config.rs").unwrap();
        let first_stale = get_stale_since(&conn, id).unwrap();

        // Manually backdate the stale_since to verify it doesn't get overwritten
        conn.execute(
            "UPDATE memory_facts SET stale_since = '2020-01-01 00:00:00' WHERE id = ?",
            [id],
        )
        .unwrap();

        // Second call should not update (already stale)
        let count = mark_memories_stale_for_file_sync(&conn, pid, "/src/config.rs").unwrap();
        assert_eq!(count, 0);
        let second_stale = get_stale_since(&conn, id).unwrap();
        assert_eq!(second_stale, "2020-01-01 00:00:00");
        assert_ne!(second_stale, first_stale);
    }

    #[test]
    fn test_upsert_clears_staleness() {
        let conn = setup_test_connection();
        let pid = insert_project(&conn);

        // Create a keyed memory and mark it stale
        let id = store_memory_sync(
            &conn,
            StoreMemoryParams {
                project_id: Some(pid),
                key: Some("config_pattern"),
                content: "config.rs uses builder pattern",
                fact_type: "decision",
                category: None,
                confidence: 0.8,
                session_id: Some("session-1"),
                user_id: None,
                scope: "project",
                branch: None,
                team_id: None,
                suspicious: false,
            },
        )
        .unwrap();

        mark_memories_stale_for_file_sync(&conn, pid, "/src/config.rs").unwrap();
        assert!(get_stale_since(&conn, id).is_some());

        // Re-confirm the memory via upsert (new session)
        let id2 = store_memory_sync(
            &conn,
            StoreMemoryParams {
                project_id: Some(pid),
                key: Some("config_pattern"),
                content: "config.rs uses builder pattern (confirmed)",
                fact_type: "decision",
                category: None,
                confidence: 0.8,
                session_id: Some("session-2"),
                user_id: None,
                scope: "project",
                branch: None,
                team_id: None,
                suspicious: false,
            },
        )
        .unwrap();

        assert_eq!(id, id2, "upsert should update in place");
        assert!(
            get_stale_since(&conn, id).is_none(),
            "upsert should clear staleness"
        );
    }

    #[test]
    fn test_mark_stale_respects_project_boundary() {
        let conn = setup_test_connection();
        let pid1 = insert_project(&conn);
        // Insert second project
        conn.execute(
            "INSERT INTO projects (path, name) VALUES ('/test/other', 'other-test')",
            [],
        )
        .unwrap();
        let pid2 = conn.last_insert_rowid();

        let id1 = store_memory(&conn, pid1, "config.rs in project 1");
        let id2 = store_memory(&conn, pid2, "config.rs in project 2");

        // Only mark stale for project 1
        let count = mark_memories_stale_for_file_sync(&conn, pid1, "/src/config.rs").unwrap();
        assert_eq!(count, 1);
        assert!(get_stale_since(&conn, id1).is_some());
        assert!(get_stale_since(&conn, id2).is_none());
    }
}
