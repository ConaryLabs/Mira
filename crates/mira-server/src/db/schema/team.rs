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
    // Before remapping, merge fresher metadata into the surviving team's rows.
    // For session_ids that exist in both a surviving (MIN id) and duplicate team,
    // keep the newest member_name/role/last_heartbeat so OR IGNORE doesn't
    // silently discard fresher data from the duplicate.
    conn.execute_batch(
        "UPDATE team_sessions SET
             member_name = dup.member_name,
             role = dup.role,
             last_heartbeat = dup.last_heartbeat
         FROM (
             SELECT ts_d.session_id, ts_d.member_name, ts_d.role, ts_d.last_heartbeat,
                    (SELECT MIN(t2.id) FROM teams t2
                     WHERE t2.name = t.name
                       AND COALESCE(t2.project_id, 0) = COALESCE(t.project_id, 0)
                    ) AS surv_team
             FROM team_sessions ts_d
             JOIN teams t ON ts_d.team_id = t.id
             WHERE ts_d.team_id NOT IN (
                 SELECT MIN(id) FROM teams GROUP BY name, COALESCE(project_id, 0)
             )
         ) dup
         WHERE team_sessions.team_id = dup.surv_team
           AND team_sessions.session_id = dup.session_id
           AND team_sessions.last_heartbeat < dup.last_heartbeat;",
    )?;

    conn.execute_batch(
        // Remap team_sessions to the surviving row. Use OR IGNORE to skip
        // rows that would violate UNIQUE(team_id, session_id) — those already
        // exist in the target team (now with merged metadata) and the duplicate
        // row can safely be dropped with the duplicate team.
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

    /// Helper: set up an in-memory DB with the projects table and a test project.
    fn setup_db() -> Connection {
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
        conn
    }

    #[test]
    fn test_dedupe_remaps_sessions_to_surviving_team() {
        let conn = setup_db();
        // Create tables WITHOUT the unique index (simulates pre-migration state)
        conn.execute_batch(
            "CREATE TABLE teams (
                 id INTEGER PRIMARY KEY, name TEXT NOT NULL,
                 project_id INTEGER REFERENCES projects(id),
                 config_path TEXT NOT NULL, status TEXT DEFAULT 'active',
                 created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                 updated_at TEXT DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TABLE team_sessions (
                 id INTEGER PRIMARY KEY,
                 team_id INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
                 session_id TEXT NOT NULL, member_name TEXT NOT NULL,
                 role TEXT DEFAULT 'teammate', agent_type TEXT,
                 joined_at TEXT DEFAULT CURRENT_TIMESTAMP,
                 last_heartbeat TEXT DEFAULT CURRENT_TIMESTAMP,
                 status TEXT DEFAULT 'active',
                 UNIQUE(team_id, session_id)
             );
             CREATE TABLE team_file_ownership (
                 id INTEGER PRIMARY KEY,
                 team_id INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
                 session_id TEXT NOT NULL, member_name TEXT NOT NULL,
                 file_path TEXT NOT NULL, operation TEXT NOT NULL,
                 timestamp TEXT DEFAULT CURRENT_TIMESTAMP
             );",
        )
        .unwrap();

        // Two duplicate teams (same name + project)
        conn.execute_batch(
            "INSERT INTO teams (id, name, project_id, config_path) VALUES (1, 'alpha', 1, '/p1');
             INSERT INTO teams (id, name, project_id, config_path) VALUES (5, 'alpha', 1, '/p2');
             -- Non-conflicting sessions: sess-A only in team 5
             INSERT INTO team_sessions (team_id, session_id, member_name) VALUES (5, 'sess-A', 'bob');
             -- File ownership in duplicate team
             INSERT INTO team_file_ownership (team_id, session_id, member_name, file_path, operation)
                 VALUES (5, 'sess-A', 'bob', '/src/lib.rs', 'Edit');",
        )
        .unwrap();

        // Run migration — should remap team 5's rows to team 1
        migrate_team_tables(&conn).unwrap();

        // Session remapped to surviving team (id=1)
        let team_id: i64 = conn
            .query_row(
                "SELECT team_id FROM team_sessions WHERE session_id = 'sess-A'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(team_id, 1);

        // File ownership remapped too
        let fo_team: i64 = conn
            .query_row(
                "SELECT team_id FROM team_file_ownership WHERE session_id = 'sess-A'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fo_team, 1);

        // Duplicate team deleted
        let team_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM teams", [], |r| r.get(0))
            .unwrap();
        assert_eq!(team_count, 1);
    }

    #[test]
    fn test_dedupe_keeps_freshest_metadata_on_conflict() {
        let conn = setup_db();
        conn.execute_batch(
            "CREATE TABLE teams (
                 id INTEGER PRIMARY KEY, name TEXT NOT NULL,
                 project_id INTEGER REFERENCES projects(id),
                 config_path TEXT NOT NULL, status TEXT DEFAULT 'active',
                 created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                 updated_at TEXT DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TABLE team_sessions (
                 id INTEGER PRIMARY KEY,
                 team_id INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
                 session_id TEXT NOT NULL, member_name TEXT NOT NULL,
                 role TEXT DEFAULT 'teammate', agent_type TEXT,
                 joined_at TEXT DEFAULT CURRENT_TIMESTAMP,
                 last_heartbeat TEXT DEFAULT CURRENT_TIMESTAMP,
                 status TEXT DEFAULT 'active',
                 UNIQUE(team_id, session_id)
             );
             CREATE TABLE team_file_ownership (
                 id INTEGER PRIMARY KEY,
                 team_id INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
                 session_id TEXT NOT NULL, member_name TEXT NOT NULL,
                 file_path TEXT NOT NULL, operation TEXT NOT NULL,
                 timestamp TEXT DEFAULT CURRENT_TIMESTAMP
             );",
        )
        .unwrap();

        // Two duplicate teams, BOTH have session 'sess-X' (conflicting)
        conn.execute_batch(
            "INSERT INTO teams (id, name, project_id, config_path) VALUES (1, 'beta', 1, '/p1');
             INSERT INTO teams (id, name, project_id, config_path) VALUES (7, 'beta', 1, '/p2');
             -- Surviving team's row: older heartbeat
             INSERT INTO team_sessions (team_id, session_id, member_name, role, last_heartbeat)
                 VALUES (1, 'sess-X', 'alice_old', 'teammate', '2025-01-01T00:00:00');
             -- Duplicate team's row: newer heartbeat with updated metadata
             INSERT INTO team_sessions (team_id, session_id, member_name, role, last_heartbeat)
                 VALUES (7, 'sess-X', 'alice_new', 'lead', '2025-06-15T12:00:00');",
        )
        .unwrap();

        migrate_team_tables(&conn).unwrap();

        // Surviving row should have the fresher metadata from the duplicate
        let (name, role, hb): (String, String, String) = conn
            .query_row(
                "SELECT member_name, role, last_heartbeat FROM team_sessions WHERE session_id = 'sess-X'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(name, "alice_new", "should keep newer member_name");
        assert_eq!(role, "lead", "should keep newer role");
        assert_eq!(hb, "2025-06-15T12:00:00", "should keep newer heartbeat");

        // Only one session row remains (duplicate's row dropped)
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM team_sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_dedupe_preserves_surviving_metadata_when_already_newest() {
        let conn = setup_db();
        conn.execute_batch(
            "CREATE TABLE teams (
                 id INTEGER PRIMARY KEY, name TEXT NOT NULL,
                 project_id INTEGER REFERENCES projects(id),
                 config_path TEXT NOT NULL, status TEXT DEFAULT 'active',
                 created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                 updated_at TEXT DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TABLE team_sessions (
                 id INTEGER PRIMARY KEY,
                 team_id INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
                 session_id TEXT NOT NULL, member_name TEXT NOT NULL,
                 role TEXT DEFAULT 'teammate', agent_type TEXT,
                 joined_at TEXT DEFAULT CURRENT_TIMESTAMP,
                 last_heartbeat TEXT DEFAULT CURRENT_TIMESTAMP,
                 status TEXT DEFAULT 'active',
                 UNIQUE(team_id, session_id)
             );
             CREATE TABLE team_file_ownership (
                 id INTEGER PRIMARY KEY,
                 team_id INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
                 session_id TEXT NOT NULL, member_name TEXT NOT NULL,
                 file_path TEXT NOT NULL, operation TEXT NOT NULL,
                 timestamp TEXT DEFAULT CURRENT_TIMESTAMP
             );",
        )
        .unwrap();

        // Surviving team already has the newest data
        conn.execute_batch(
            "INSERT INTO teams (id, name, project_id, config_path) VALUES (1, 'gamma', 1, '/p1');
             INSERT INTO teams (id, name, project_id, config_path) VALUES (3, 'gamma', 1, '/p2');
             INSERT INTO team_sessions (team_id, session_id, member_name, role, last_heartbeat)
                 VALUES (1, 'sess-Y', 'charlie', 'lead', '2025-12-01T00:00:00');
             INSERT INTO team_sessions (team_id, session_id, member_name, role, last_heartbeat)
                 VALUES (3, 'sess-Y', 'charlie_stale', 'teammate', '2025-01-01T00:00:00');",
        )
        .unwrap();

        migrate_team_tables(&conn).unwrap();

        // Surviving row should keep its own (already newest) metadata
        let (name, role): (String, String) = conn
            .query_row(
                "SELECT member_name, role FROM team_sessions WHERE session_id = 'sess-Y'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(name, "charlie", "should keep surviving row's newer data");
        assert_eq!(role, "lead", "should keep surviving row's newer role");
    }
}
