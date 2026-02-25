// db/project.rs
// Project management operations

use rusqlite::{Connection, OptionalExtension, params};

use crate::utils::normalize_project_path;

// ═══════════════════════════════════════════════════════════════════════════════
// Sync functions for pool.interact() usage
// ═══════════════════════════════════════════════════════════════════════════════

/// Get or create a project, returning (id, name) - sync version for pool.interact()
///
/// The path is normalized via [`normalize_project_path`] before insertion,
/// so `~/project`, `/home/user/project`, and symlinked variants all resolve
/// to the same canonical row.
pub fn get_or_create_project_sync(
    conn: &Connection,
    path: &str,
    name: Option<&str>,
) -> rusqlite::Result<(i64, Option<String>)> {
    let normalized = normalize_project_path(path);
    conn.query_row(
        "INSERT INTO projects (path, name) VALUES (?, ?)
         ON CONFLICT(path) DO UPDATE SET
             name = COALESCE(projects.name, excluded.name),
             created_at = projects.created_at
         RETURNING id, name",
        params![normalized, name],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
}

/// Update project name - sync version
pub fn update_project_name_sync(
    conn: &Connection,
    project_id: i64,
    name: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE projects SET name = ? WHERE id = ?",
        params![name, project_id],
    )?;
    Ok(())
}

/// Get project path by ID - sync version for pool.interact()
pub fn get_project_path_sync(conn: &Connection, project_id: i64) -> rusqlite::Result<String> {
    conn.query_row(
        "SELECT path FROM projects WHERE id = ?",
        [project_id],
        |row| row.get::<_, String>(0),
    )
}

/// Get project info by ID (name, path) - sync version for pool.interact()
pub fn get_project_info_sync(
    conn: &Connection,
    project_id: i64,
) -> rusqlite::Result<Option<(Option<String>, String)>> {
    conn.query_row(
        "SELECT name, path FROM projects WHERE id = ?",
        [project_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
}

/// Create or update a session - sync version for pool.interact()
///
/// Delegates to `create_session_sync` in db/session.rs (identical SQL).
/// Kept as a convenience alias for callers that import from db/project.
pub fn upsert_session_sync(
    conn: &Connection,
    session_id: &str,
    project_id: Option<i64>,
) -> rusqlite::Result<()> {
    super::session::create_session_sync(conn, session_id, project_id)
}

/// Create or update a session with branch - sync version for pool.interact()
/// Note: Sets status='active' on conflict to properly reactivate completed sessions
pub fn upsert_session_with_branch_sync(
    conn: &Connection,
    session_id: &str,
    project_id: Option<i64>,
    branch: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO sessions (id, project_id, branch, status, started_at, last_activity)
         VALUES (?1, ?2, ?3, 'active', datetime('now'), datetime('now'))
         ON CONFLICT(id) DO UPDATE SET
            last_activity = datetime('now'),
            status = 'active',
            branch = COALESCE(?3, sessions.branch)",
        params![session_id, project_id, branch],
    )?;
    Ok(())
}

/// Get indexed projects (projects with codebase_modules) - sync version
///
/// NOTE: After code DB sharding, codebase_modules lives in the code database.
/// This function requires a connection to the code DB. Callers that need
/// project paths should use `get_indexed_project_ids_sync` on the code pool,
/// then `get_project_paths_by_ids_sync` on the main pool.
pub fn get_indexed_projects_sync(conn: &Connection) -> rusqlite::Result<Vec<(i64, String)>> {
    // Try the old single-DB JOIN first (for backwards compat / tests)
    let result = conn.prepare(
        "SELECT DISTINCT p.id, p.path
         FROM projects p
         JOIN codebase_modules m ON m.project_id = p.id",
    );

    match result {
        Ok(mut stmt) => {
            let projects = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .filter_map(super::log_and_discard)
                .collect();
            Ok(projects)
        }
        Err(_) => {
            // Fallback: projects table doesn't exist in this DB (sharded layout).
            // Return empty - caller should use the two-step approach.
            Ok(vec![])
        }
    }
}

/// Get project IDs that have indexed code (from codebase_modules).
/// Run this on the code database pool.
pub fn get_indexed_project_ids_sync(conn: &Connection) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn
        .prepare("SELECT DISTINCT project_id FROM codebase_modules WHERE project_id IS NOT NULL ORDER BY project_id")?;
    let ids = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(super::log_and_discard)
        .collect();
    Ok(ids)
}

/// Get project paths for a list of project IDs.
/// Run this on the main database pool.
pub fn get_project_paths_by_ids_sync(
    conn: &Connection,
    ids: &[i64],
) -> rusqlite::Result<Vec<(i64, String)>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
    let sql = format!(
        "SELECT id, path FROM projects WHERE id IN ({}) ORDER BY id",
        placeholders.join(",")
    );
    let mut stmt = conn.prepare(&sql)?;
    let params: Vec<&dyn rusqlite::types::ToSql> = ids
        .iter()
        .map(|id| id as &dyn rusqlite::types::ToSql)
        .collect();
    let projects = stmt
        .query_map(params.as_slice(), |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(super::log_and_discard)
        .collect();
    Ok(projects)
}

/// Get projects that need briefing checks - sync version for pool.interact()
pub fn get_projects_for_briefing_check_sync(
    conn: &Connection,
) -> rusqlite::Result<Vec<(i64, String, Option<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT p.id, p.path, pb.last_known_commit
         FROM projects p
         LEFT JOIN project_briefings pb ON p.id = pb.project_id
         WHERE p.path IS NOT NULL
         ORDER BY p.id",
    )?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    rows.collect()
}

/// Update project briefing with new git state and summary - sync version for pool.interact()
pub fn update_project_briefing_sync(
    conn: &Connection,
    project_id: i64,
    last_known_commit: &str,
    briefing_text: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO project_briefings (project_id, last_known_commit, briefing_text, generated_at)
         VALUES (?, ?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT(project_id) DO UPDATE SET
            last_known_commit = excluded.last_known_commit,
            briefing_text = excluded.briefing_text,
            generated_at = CURRENT_TIMESTAMP",
        params![project_id, last_known_commit, briefing_text],
    )?;
    Ok(())
}

/// Set a server state value (upsert) - sync version for pool.interact()
pub fn set_server_state_sync(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO server_state (key, value, updated_at)
         VALUES (?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = CURRENT_TIMESTAMP",
        params![key, value],
    )?;
    Ok(())
}

/// Get a server state value by key - sync version for pool.interact()
pub fn get_server_state_sync(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM server_state WHERE key = ?",
        [key],
        |row| row.get(0),
    )
    .optional()
}

/// Get project briefing (What's New since last session) - sync version
pub fn get_project_briefing_sync(
    conn: &Connection,
    project_id: i64,
) -> rusqlite::Result<Option<super::types::ProjectBriefing>> {
    conn.query_row(
        "SELECT project_id, last_known_commit, last_session_at, briefing_text, generated_at
         FROM project_briefings WHERE project_id = ?",
        [project_id],
        |row| {
            Ok(super::types::ProjectBriefing {
                project_id: row.get(0)?,
                last_known_commit: row.get(1)?,
                last_session_at: row.get(2)?,
                briefing_text: row.get(3)?,
                generated_at: row.get(4)?,
            })
        },
    )
    .optional()
}

/// Mark that a session occurred for this project (clears the briefing) - sync version
pub fn mark_session_for_briefing_sync(conn: &Connection, project_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO project_briefings (project_id, last_session_at)
         VALUES (?, CURRENT_TIMESTAMP)
         ON CONFLICT(project_id) DO UPDATE SET
            last_session_at = CURRENT_TIMESTAMP,
            briefing_text = NULL",
        [project_id],
    )?;
    Ok(())
}

/// Save active project path for restart recovery - sync version
pub fn save_active_project_sync(conn: &Connection, path: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO server_state (key, value) VALUES ('active_project', ?)",
        [path],
    )?;
    Ok(())
}

/// Get last active project path for restart recovery - sync version
pub fn get_last_active_project_sync(conn: &Connection) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM server_state WHERE key = 'active_project'",
        [],
        |row| row.get(0),
    )
    .optional()
}

/// List all projects - sync version
pub fn list_projects_sync(
    conn: &Connection,
) -> rusqlite::Result<Vec<(i64, String, Option<String>)>> {
    let mut stmt = conn.prepare("SELECT id, path, name FROM projects ORDER BY id DESC")?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    rows.collect()
}

/// Delete a server state by key - sync version
pub fn delete_server_state_sync(conn: &Connection, key: &str) -> rusqlite::Result<bool> {
    let deleted = conn.execute("DELETE FROM server_state WHERE key = ?", [key])?;
    Ok(deleted > 0)
}

/// Clear active project (for switching/closing) - sync version
pub fn clear_active_project_sync(conn: &Connection) -> rusqlite::Result<()> {
    delete_server_state_sync(conn, "active_project")?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Recent activity helpers (used by background workers)
// ═══════════════════════════════════════════════════════════════════════════════

/// Project IDs with recent session activity (within `hours`)
pub fn get_active_project_ids_sync(conn: &Connection, hours: i64) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT DISTINCT p.id
        FROM projects p
        JOIN sessions s ON s.project_id = p.id
        WHERE s.last_activity > datetime('now', '-' || ? || ' hours')
        "#,
    )?;
    let rows = stmt.query_map(params![hours], |row| row.get::<_, i64>(0))?;
    rows.collect()
}

/// Project info (id, name, path) with recent session activity
pub fn get_active_projects_sync(
    conn: &Connection,
    hours: i64,
) -> rusqlite::Result<Vec<(i64, Option<String>, String)>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT DISTINCT p.id, p.name, p.path
        FROM projects p
        JOIN sessions s ON s.project_id = p.id
        WHERE s.last_activity > datetime('now', '-' || ? || ' hours')
        "#,
    )?;
    let rows = stmt.query_map(params![hours], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    rows.collect()
}
