// crates/mira-server/src/db/schema/team.rs
// Team intelligence layer tables

use crate::db::migration_helpers::create_table_if_missing;
use anyhow::Result;
use rusqlite::Connection;

/// Create team tables for the team intelligence layer.
///
/// Tables:
/// - `teams`: Team metadata (one row per Agent Teams team)
/// - `team_sessions`: Active teammate sessions within a team
/// - `team_file_ownership`: Tracks which teammate modified which files
pub fn migrate_team_tables(conn: &Connection) -> Result<()> {
    create_table_if_missing(
        conn,
        "teams",
        r#"
        CREATE TABLE IF NOT EXISTS teams (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            project_id INTEGER REFERENCES projects(id),
            config_path TEXT NOT NULL,
            status TEXT DEFAULT 'active',
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(name, project_id)
        );
        CREATE INDEX IF NOT EXISTS idx_teams_status ON teams(status);
    "#,
    )?;

    create_table_if_missing(
        conn,
        "team_sessions",
        r#"
        CREATE TABLE IF NOT EXISTS team_sessions (
            id INTEGER PRIMARY KEY,
            team_id INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
            session_id TEXT NOT NULL,
            member_name TEXT NOT NULL,
            role TEXT DEFAULT 'teammate',
            agent_type TEXT,
            joined_at TEXT DEFAULT CURRENT_TIMESTAMP,
            last_heartbeat TEXT DEFAULT CURRENT_TIMESTAMP,
            status TEXT DEFAULT 'active',
            UNIQUE(team_id, session_id)
        );
        CREATE INDEX IF NOT EXISTS idx_ts_team_status ON team_sessions(team_id, status);
        CREATE INDEX IF NOT EXISTS idx_ts_session ON team_sessions(session_id);
        CREATE INDEX IF NOT EXISTS idx_ts_heartbeat ON team_sessions(status, last_heartbeat);
    "#,
    )?;

    create_table_if_missing(
        conn,
        "team_file_ownership",
        r#"
        CREATE TABLE IF NOT EXISTS team_file_ownership (
            id INTEGER PRIMARY KEY,
            team_id INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
            session_id TEXT NOT NULL,
            member_name TEXT NOT NULL,
            file_path TEXT NOT NULL,
            operation TEXT NOT NULL,
            timestamp TEXT DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_tfo_team_file ON team_file_ownership(team_id, file_path);
        CREATE INDEX IF NOT EXISTS idx_tfo_session ON team_file_ownership(team_id, session_id);
        CREATE INDEX IF NOT EXISTS idx_tfo_timestamp ON team_file_ownership(team_id, timestamp);
    "#,
    )?;

    // NULL-safe unique index: SQLite UNIQUE treats NULLs as distinct,
    // so UNIQUE(name, project_id) doesn't enforce uniqueness when project_id IS NULL.
    // This expression index uses COALESCE to normalize NULLs to 0.
    //
    // Before creating the index, deduplicate any legacy rows with the same
    // (name, COALESCE(project_id,0)) so the CREATE UNIQUE INDEX doesn't fail.
    // Remap dependent rows to the surviving (MIN id) row before deleting
    // duplicates, to avoid ON DELETE CASCADE data loss.
    conn.execute_batch(
        // Remap team_sessions to the surviving row. Use OR IGNORE to skip
        // rows that would violate UNIQUE(team_id, session_id) — those already
        // exist in the target team and can safely be dropped with the duplicate.
        "UPDATE OR IGNORE team_sessions SET team_id = (
             SELECT MIN(t2.id) FROM teams t2
             WHERE t2.name = (SELECT name FROM teams WHERE id = team_sessions.team_id)
               AND COALESCE(t2.project_id, 0) = COALESCE(
                   (SELECT project_id FROM teams WHERE id = team_sessions.team_id), 0)
         )
         WHERE team_id NOT IN (
             SELECT MIN(id) FROM teams GROUP BY name, COALESCE(project_id, 0)
         );

         UPDATE OR IGNORE team_file_ownership SET team_id = (
             SELECT MIN(t2.id) FROM teams t2
             WHERE t2.name = (SELECT name FROM teams WHERE id = team_file_ownership.team_id)
               AND COALESCE(t2.project_id, 0) = COALESCE(
                   (SELECT project_id FROM teams WHERE id = team_file_ownership.team_id), 0)
         )
         WHERE team_id NOT IN (
             SELECT MIN(id) FROM teams GROUP BY name, COALESCE(project_id, 0)
         );

         DELETE FROM teams WHERE id NOT IN (
             SELECT MIN(id) FROM teams GROUP BY name, COALESCE(project_id, 0)
         );",
    )?;
    conn.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_teams_name_project
         ON teams(name, COALESCE(project_id, 0));",
    )?;

    // Migration: remove CHECK constraint on operation column for existing databases.
    // SQLite doesn't support ALTER TABLE DROP CONSTRAINT, so we recreate the table.
    migrate_drop_file_ownership_check(conn)?;

    Ok(())
}

/// Remove the CHECK(operation IN (...)) constraint from team_file_ownership.
/// For existing DBs that already have the constraint, we recreate the table.
fn migrate_drop_file_ownership_check(conn: &Connection) -> Result<()> {
    // Use schema introspection instead of a probe INSERT.
    // The probe approach fails under FK enforcement (team_id=0 doesn't exist in teams),
    // causing a false-positive that triggers table rebuild every startup.
    let create_sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='team_file_ownership'",
            [],
            |row| row.get(0),
        )
        .unwrap_or_default();

    let needs_migration = create_sql.contains("CHECK");

    if !needs_migration {
        return Ok(());
    }

    conn.execute_batch(
        "BEGIN;
         CREATE TABLE team_file_ownership_new (
             id INTEGER PRIMARY KEY,
             team_id INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
             session_id TEXT NOT NULL,
             member_name TEXT NOT NULL,
             file_path TEXT NOT NULL,
             operation TEXT NOT NULL,
             timestamp TEXT DEFAULT CURRENT_TIMESTAMP
         );
         INSERT INTO team_file_ownership_new SELECT * FROM team_file_ownership;
         DROP TABLE team_file_ownership;
         ALTER TABLE team_file_ownership_new RENAME TO team_file_ownership;
         CREATE INDEX IF NOT EXISTS idx_tfo_team_file ON team_file_ownership(team_id, file_path);
         CREATE INDEX IF NOT EXISTS idx_tfo_session ON team_file_ownership(team_id, session_id);
         CREATE INDEX IF NOT EXISTS idx_tfo_timestamp ON team_file_ownership(team_id, timestamp);
         COMMIT;"
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migrate_team_tables_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        // Need projects table for FK
        conn.execute(
            "CREATE TABLE projects (id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL)",
            [],
        )
        .unwrap();

        // Run twice — should be idempotent
        migrate_team_tables(&conn).unwrap();
        migrate_team_tables(&conn).unwrap();

        // Verify tables exist
        let teams: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='teams'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);
        assert!(teams);

        let sessions: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='team_sessions'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);
        assert!(sessions);

        let ownership: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='team_file_ownership'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);
        assert!(ownership);
    }

    #[test]
    fn test_teams_unique_constraint() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE projects (id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects (id, path) VALUES (1, '/test')",
            [],
        )
        .unwrap();
        migrate_team_tables(&conn).unwrap();

        conn.execute(
            "INSERT INTO teams (name, project_id, config_path) VALUES ('team1', 1, '/path')",
            [],
        )
        .unwrap();

        // Duplicate should fail
        let result = conn.execute(
            "INSERT INTO teams (name, project_id, config_path) VALUES ('team1', 1, '/path2')",
            [],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_team_sessions_unique_constraint() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE projects (id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects (id, path) VALUES (1, '/test')",
            [],
        )
        .unwrap();
        migrate_team_tables(&conn).unwrap();

        conn.execute(
            "INSERT INTO teams (id, name, project_id, config_path) VALUES (1, 'team1', 1, '/p')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO team_sessions (team_id, session_id, member_name) VALUES (1, 'sess1', 'alice')",
            [],
        )
        .unwrap();

        // Duplicate (team_id, session_id) should fail
        let result = conn.execute(
            "INSERT INTO team_sessions (team_id, session_id, member_name) VALUES (1, 'sess1', 'bob')",
            [],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_file_ownership_accepts_any_operation() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE projects (id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects (id, path) VALUES (1, '/test')",
            [],
        )
        .unwrap();
        migrate_team_tables(&conn).unwrap();

        conn.execute(
            "INSERT INTO teams (id, name, project_id, config_path) VALUES (1, 'team1', 1, '/p')",
            [],
        )
        .unwrap();

        // All operation names are accepted (filtering is done in the hook layer)
        for op in &["Write", "Edit", "NotebookEdit", "MultiEdit"] {
            conn.execute(
                "INSERT INTO team_file_ownership (team_id, session_id, member_name, file_path, operation) VALUES (1, 's1', 'a', '/f', ?1)",
                [op],
            ).unwrap();
        }
    }
}
