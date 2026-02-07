// crates/mira-server/src/db/team.rs
// Team intelligence layer database operations

use rusqlite::{Connection, OptionalExtension, params};

/// Team info returned from DB queries
#[derive(Debug, Clone)]
pub struct TeamInfo {
    pub id: i64,
    pub name: String,
    pub project_id: Option<i64>,
    pub config_path: String,
    pub status: String,
}

/// Team member info returned from DB queries
#[derive(Debug, Clone)]
pub struct TeamMemberInfo {
    pub session_id: String,
    pub member_name: String,
    pub role: String,
    pub agent_type: Option<String>,
    pub last_heartbeat: String,
    pub status: String,
}

/// File conflict: another teammate edited the same file recently
#[derive(Debug, Clone)]
pub struct FileConflict {
    pub file_path: String,
    pub other_session_id: String,
    pub other_member_name: String,
    pub operation: String,
    pub timestamp: String,
}

/// Get or create a team (race-safe: INSERT OR IGNORE + SELECT).
/// Uses COALESCE(project_id, 0) to enforce uniqueness even when project_id is NULL.
pub fn get_or_create_team_sync(
    conn: &Connection,
    name: &str,
    project_id: Option<i64>,
    config_path: &str,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT OR IGNORE INTO teams (name, project_id, config_path) VALUES (?1, ?2, ?3)",
        params![name, project_id, config_path],
    )?;

    conn.query_row(
        "SELECT id FROM teams WHERE name = ?1 AND COALESCE(project_id, 0) = COALESCE(?2, 0)",
        params![name, project_id],
        |row| row.get(0),
    )
}

/// Register a team session (UPSERT on (team_id, session_id)).
/// Enforces single active team per session: deactivates any prior team
/// memberships for this session before registering in the new team.
pub fn register_team_session_sync(
    conn: &Connection,
    team_id: i64,
    session_id: &str,
    member_name: &str,
    role: &str,
    agent_type: Option<&str>,
) -> rusqlite::Result<()> {
    // Use an explicit transaction so the deactivation + upsert are atomic.
    // Without this, a concurrent registration could see a window where the
    // old membership is deactivated but the new one isn't yet inserted.
    let tx = conn.unchecked_transaction()?;

    // Deactivate any existing active memberships in OTHER teams
    tx.execute(
        "UPDATE team_sessions SET status = 'stopped'
         WHERE session_id = ?1 AND team_id != ?2 AND status = 'active'",
        params![session_id, team_id],
    )?;

    tx.execute(
        "INSERT INTO team_sessions (team_id, session_id, member_name, role, agent_type)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(team_id, session_id) DO UPDATE SET
           member_name = excluded.member_name,
           role = excluded.role,
           agent_type = COALESCE(excluded.agent_type, team_sessions.agent_type),
           last_heartbeat = CURRENT_TIMESTAMP,
           status = 'active'",
        params![team_id, session_id, member_name, role, agent_type],
    )?;

    tx.commit()?;
    Ok(())
}

/// Get team info for a session (if any).
pub fn get_team_for_session_sync(conn: &Connection, session_id: &str) -> Option<TeamInfo> {
    conn.query_row(
        "SELECT t.id, t.name, t.project_id, t.config_path, t.status
         FROM teams t
         JOIN team_sessions ts ON ts.team_id = t.id
         WHERE ts.session_id = ?1 AND ts.status = 'active' AND t.status = 'active'",
        params![session_id],
        |row| {
            Ok(TeamInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                project_id: row.get(2)?,
                config_path: row.get(3)?,
                status: row.get(4)?,
            })
        },
    )
    .optional()
    .ok()
    .flatten()
}

/// Get full team membership for a session (team info + member details).
/// Preferred over filesystem-based read_team_membership() for session isolation.
pub fn get_team_membership_for_session_sync(
    conn: &Connection,
    session_id: &str,
) -> Option<crate::hooks::session::TeamMembership> {
    conn.query_row(
        "SELECT t.id, t.name, t.config_path, ts.member_name, ts.role
         FROM teams t
         JOIN team_sessions ts ON ts.team_id = t.id
         WHERE ts.session_id = ?1 AND ts.status = 'active' AND t.status = 'active'
         LIMIT 1",
        params![session_id],
        |row| {
            Ok(crate::hooks::session::TeamMembership {
                team_id: row.get(0)?,
                team_name: row.get(1)?,
                config_path: row.get(2)?,
                member_name: row.get(3)?,
                role: row.get(4)?,
            })
        },
    )
    .optional()
    .ok()
    .flatten()
}

/// Get active team members.
pub fn get_active_team_members_sync(conn: &Connection, team_id: i64) -> Vec<TeamMemberInfo> {
    let mut stmt = match conn.prepare(
        "SELECT session_id, member_name, role, agent_type, last_heartbeat, status
         FROM team_sessions
         WHERE team_id = ?1 AND status = 'active'
         ORDER BY joined_at",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    stmt.query_map(params![team_id], |row| {
        Ok(TeamMemberInfo {
            session_id: row.get(0)?,
            member_name: row.get(1)?,
            role: row.get(2)?,
            agent_type: row.get(3)?,
            last_heartbeat: row.get(4)?,
            status: row.get(5)?,
        })
    })
    .map(|rows| rows.filter_map(super::log_and_discard).collect())
    .unwrap_or_default()
}

/// Update heartbeat for a team session.
/// Also reactivates sessions that were marked stale by the background monitor.
pub fn heartbeat_team_session_sync(
    conn: &Connection,
    team_id: i64,
    session_id: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE team_sessions SET last_heartbeat = CURRENT_TIMESTAMP, status = 'active'
         WHERE team_id = ?1 AND session_id = ?2",
        params![team_id, session_id],
    )?;
    Ok(())
}

/// Deactivate a team session (set status='stopped').
pub fn deactivate_team_session_sync(conn: &Connection, session_id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE team_sessions SET status = 'stopped' WHERE session_id = ?1 AND status = 'active'",
        params![session_id],
    )?;
    Ok(())
}

/// Validate that a session is an active member of a team.
pub fn validate_team_membership_sync(conn: &Connection, team_id: i64, session_id: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM team_sessions WHERE team_id = ?1 AND session_id = ?2 AND status = 'active'",
        params![team_id, session_id],
        |_| Ok(true),
    )
    .unwrap_or(false)
}

/// Record a file ownership event.
pub fn record_file_ownership_sync(
    conn: &Connection,
    team_id: i64,
    session_id: &str,
    member_name: &str,
    file_path: &str,
    operation: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO team_file_ownership (team_id, session_id, member_name, file_path, operation)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![team_id, session_id, member_name, file_path, operation],
    )?;
    Ok(())
}

/// Get file conflicts: files edited by OTHER active sessions in the last 30 minutes.
pub fn get_file_conflicts_sync(
    conn: &Connection,
    team_id: i64,
    session_id: &str,
) -> Vec<FileConflict> {
    let mut stmt = match conn.prepare(
        "SELECT tfo.file_path, tfo.session_id, tfo.member_name, tfo.operation, tfo.timestamp
         FROM team_file_ownership tfo
         JOIN team_sessions ts ON ts.team_id = tfo.team_id AND ts.session_id = tfo.session_id
         WHERE tfo.team_id = ?1
           AND tfo.session_id != ?2
           AND ts.status = 'active'
           AND tfo.timestamp > datetime('now', '-30 minutes')
           AND tfo.file_path IN (
               SELECT DISTINCT file_path FROM team_file_ownership
               WHERE team_id = ?1 AND session_id = ?2
                 AND timestamp > datetime('now', '-30 minutes')
           )
         ORDER BY tfo.timestamp DESC",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    stmt.query_map(params![team_id, session_id], |row| {
        Ok(FileConflict {
            file_path: row.get(0)?,
            other_session_id: row.get(1)?,
            other_member_name: row.get(2)?,
            operation: row.get(3)?,
            timestamp: row.get(4)?,
        })
    })
    .map(|rows| rows.filter_map(super::log_and_discard).collect())
    .unwrap_or_default()
}

/// Get files modified by a specific session.
pub fn get_member_files_sync(conn: &Connection, team_id: i64, session_id: &str) -> Vec<String> {
    let mut stmt = match conn.prepare(
        "SELECT DISTINCT file_path FROM team_file_ownership
         WHERE team_id = ?1 AND session_id = ?2
         ORDER BY timestamp DESC",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    stmt.query_map(params![team_id, session_id], |row| row.get::<_, String>(0))
        .map(|rows| rows.filter_map(super::log_and_discard).collect())
        .unwrap_or_default()
}

/// Mark sessions with no heartbeat for longer than threshold as stopped.
pub fn cleanup_stale_sessions_sync(
    conn: &Connection,
    team_id: i64,
    stale_threshold_minutes: i64,
) -> rusqlite::Result<usize> {
    let count = conn.execute(
        "UPDATE team_sessions SET status = 'stopped'
         WHERE team_id = ?1
           AND status = 'active'
           AND last_heartbeat < datetime('now', ?2)",
        params![team_id, format!("-{} minutes", stale_threshold_minutes)],
    )?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        conn.execute(
            "CREATE TABLE projects (id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL)",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO projects (id, path) VALUES (1, '/test')", [])
            .unwrap();
        crate::db::schema::team::migrate_team_tables(&conn).unwrap();
        conn
    }

    #[test]
    fn test_get_or_create_team() {
        let conn = setup_db();

        let id1 = get_or_create_team_sync(&conn, "my-team", Some(1), "/config").unwrap();
        assert!(id1 > 0);

        // Same name + project → same ID (idempotent)
        let id2 = get_or_create_team_sync(&conn, "my-team", Some(1), "/config2").unwrap();
        assert_eq!(id1, id2);

        // Different name → different ID
        let id3 = get_or_create_team_sync(&conn, "other-team", Some(1), "/config").unwrap();
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_register_and_get_team_session() {
        let conn = setup_db();
        let team_id = get_or_create_team_sync(&conn, "t1", Some(1), "/c").unwrap();

        register_team_session_sync(&conn, team_id, "sess-a", "alice", "lead", Some("lead"))
            .unwrap();
        register_team_session_sync(&conn, team_id, "sess-b", "bob", "teammate", None).unwrap();

        let members = get_active_team_members_sync(&conn, team_id);
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].member_name, "alice");
        assert_eq!(members[1].member_name, "bob");
    }

    #[test]
    fn test_register_upsert() {
        let conn = setup_db();
        let team_id = get_or_create_team_sync(&conn, "t1", Some(1), "/c").unwrap();

        register_team_session_sync(&conn, team_id, "sess-a", "alice", "teammate", None).unwrap();

        // Re-register with updated role — should upsert
        register_team_session_sync(&conn, team_id, "sess-a", "alice", "lead", Some("lead"))
            .unwrap();

        let members = get_active_team_members_sync(&conn, team_id);
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].role, "lead");
    }

    #[test]
    fn test_get_team_for_session() {
        let conn = setup_db();
        let team_id = get_or_create_team_sync(&conn, "t1", Some(1), "/c").unwrap();
        register_team_session_sync(&conn, team_id, "sess-a", "alice", "lead", None).unwrap();

        let info = get_team_for_session_sync(&conn, "sess-a");
        assert!(info.is_some());
        assert_eq!(info.unwrap().name, "t1");

        // Unknown session
        assert!(get_team_for_session_sync(&conn, "unknown").is_none());
    }

    #[test]
    fn test_heartbeat() {
        let conn = setup_db();
        let team_id = get_or_create_team_sync(&conn, "t1", Some(1), "/c").unwrap();
        register_team_session_sync(&conn, team_id, "sess-a", "alice", "lead", None).unwrap();

        heartbeat_team_session_sync(&conn, team_id, "sess-a").unwrap();
        // Should not error on non-existent session
        heartbeat_team_session_sync(&conn, team_id, "nonexistent").unwrap();
    }

    #[test]
    fn test_deactivate() {
        let conn = setup_db();
        let team_id = get_or_create_team_sync(&conn, "t1", Some(1), "/c").unwrap();
        register_team_session_sync(&conn, team_id, "sess-a", "alice", "lead", None).unwrap();

        deactivate_team_session_sync(&conn, "sess-a").unwrap();

        let members = get_active_team_members_sync(&conn, team_id);
        assert!(members.is_empty());

        // Team lookup should also fail now
        assert!(get_team_for_session_sync(&conn, "sess-a").is_none());
    }

    #[test]
    fn test_validate_membership() {
        let conn = setup_db();
        let team_id = get_or_create_team_sync(&conn, "t1", Some(1), "/c").unwrap();
        register_team_session_sync(&conn, team_id, "sess-a", "alice", "lead", None).unwrap();

        assert!(validate_team_membership_sync(&conn, team_id, "sess-a"));
        assert!(!validate_team_membership_sync(&conn, team_id, "unknown"));

        deactivate_team_session_sync(&conn, "sess-a").unwrap();
        assert!(!validate_team_membership_sync(&conn, team_id, "sess-a"));
    }

    #[test]
    fn test_file_ownership_and_conflicts() {
        let conn = setup_db();
        let team_id = get_or_create_team_sync(&conn, "t1", Some(1), "/c").unwrap();
        register_team_session_sync(&conn, team_id, "sess-a", "alice", "lead", None).unwrap();
        register_team_session_sync(&conn, team_id, "sess-b", "bob", "teammate", None).unwrap();

        // Alice edits file.rs
        record_file_ownership_sync(&conn, team_id, "sess-a", "alice", "src/file.rs", "Edit")
            .unwrap();
        // Bob also edits file.rs
        record_file_ownership_sync(&conn, team_id, "sess-b", "bob", "src/file.rs", "Write")
            .unwrap();

        // Alice should see Bob's conflict
        let conflicts = get_file_conflicts_sync(&conn, team_id, "sess-a");
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].other_member_name, "bob");
        assert_eq!(conflicts[0].file_path, "src/file.rs");

        // Bob should see Alice's conflict
        let conflicts = get_file_conflicts_sync(&conn, team_id, "sess-b");
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].other_member_name, "alice");
    }

    #[test]
    fn test_get_member_files() {
        let conn = setup_db();
        let team_id = get_or_create_team_sync(&conn, "t1", Some(1), "/c").unwrap();
        register_team_session_sync(&conn, team_id, "sess-a", "alice", "lead", None).unwrap();

        record_file_ownership_sync(&conn, team_id, "sess-a", "alice", "src/a.rs", "Write").unwrap();
        record_file_ownership_sync(&conn, team_id, "sess-a", "alice", "src/b.rs", "Edit").unwrap();
        record_file_ownership_sync(&conn, team_id, "sess-a", "alice", "src/a.rs", "Edit").unwrap(); // duplicate

        let files = get_member_files_sync(&conn, team_id, "sess-a");
        assert_eq!(files.len(), 2); // DISTINCT
    }

    #[test]
    fn test_cleanup_stale_sessions() {
        let conn = setup_db();
        let team_id = get_or_create_team_sync(&conn, "t1", Some(1), "/c").unwrap();
        register_team_session_sync(&conn, team_id, "sess-a", "alice", "lead", None).unwrap();

        // Set heartbeat to 2 hours ago
        conn.execute(
            "UPDATE team_sessions SET last_heartbeat = datetime('now', '-2 hours') WHERE session_id = 'sess-a'",
            [],
        ).unwrap();

        let cleaned = cleanup_stale_sessions_sync(&conn, team_id, 30).unwrap();
        assert_eq!(cleaned, 1);

        let members = get_active_team_members_sync(&conn, team_id);
        assert!(members.is_empty());
    }
}
