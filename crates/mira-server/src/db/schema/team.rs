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
    // Deterministic merge: when multiple duplicates exist for the same (name, project),
    // use ROW_NUMBER to pick exactly one (freshest heartbeat, tie-break on team_id DESC)
    // per (surv_team, session_id) so the UPDATE ... FROM applies a single row.
    conn.execute_batch(
        "UPDATE team_sessions SET
             member_name = best.member_name,
             role = best.role,
             last_heartbeat = best.last_heartbeat
         FROM (
             SELECT session_id, member_name, role, last_heartbeat, surv_team
             FROM (
                 SELECT d.*, ROW_NUMBER() OVER (
                     PARTITION BY d.surv_team, d.session_id
                     ORDER BY d.last_heartbeat DESC, d.orig_team_id DESC
                 ) AS rn
                 FROM (
                     SELECT ts_d.session_id, ts_d.member_name, ts_d.role,
                            ts_d.last_heartbeat, ts_d.team_id AS orig_team_id,
                            (SELECT MIN(t2.id) FROM teams t2
                             WHERE t2.name = t.name
                               AND COALESCE(t2.project_id, 0) = COALESCE(t.project_id, 0)
                            ) AS surv_team
                     FROM team_sessions ts_d
                     JOIN teams t ON ts_d.team_id = t.id
                     WHERE ts_d.team_id NOT IN (
                         SELECT MIN(id) FROM teams GROUP BY name, COALESCE(project_id, 0)
                     )
                 ) d
             )
             WHERE rn = 1
         ) best
         WHERE team_sessions.team_id = best.surv_team
           AND team_sessions.session_id = best.session_id
           AND team_sessions.last_heartbeat < best.last_heartbeat;",
    )?;

    // Deduplicate orphan session rows across duplicate teams: when the same
    // session_id appears in multiple duplicates within a group, keep only the
    // freshest row per (survivor_team, session_id) so the subsequent OR IGNORE
    // remap is deterministic. Scoped to each survivor group to avoid collapsing
    // rows across unrelated duplicate groups that happen to share a session_id.
    conn.execute_batch(
        "DELETE FROM team_sessions
         WHERE team_id NOT IN (
             SELECT MIN(id) FROM teams GROUP BY name, COALESCE(project_id, 0)
         )
         AND id NOT IN (
             SELECT id FROM (
                 SELECT ts.id, ROW_NUMBER() OVER (
                     PARTITION BY surv.surv_team, ts.session_id
                     ORDER BY ts.last_heartbeat DESC, ts.team_id DESC
                 ) AS rn
                 FROM team_sessions ts
                 JOIN (
                     SELECT t.id AS dup_team,
                            (SELECT MIN(t2.id) FROM teams t2
                             WHERE t2.name = t.name
                               AND COALESCE(t2.project_id, 0) = COALESCE(t.project_id, 0)
                            ) AS surv_team
                     FROM teams t
                     WHERE t.id NOT IN (
                         SELECT MIN(id) FROM teams GROUP BY name, COALESCE(project_id, 0)
                     )
                 ) surv ON ts.team_id = surv.dup_team
             )
             WHERE rn = 1
         );",
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

    // Enforce single-active-team-per-session at the DB level.
    // Clean up any legacy rows where a session is active in multiple teams
    // (keep the one with the most recent heartbeat, tie-break on team_id DESC).
    conn.execute_batch(
        "UPDATE team_sessions SET status = 'stopped'
         WHERE status = 'active' AND id NOT IN (
             SELECT id FROM (
                 SELECT id, ROW_NUMBER() OVER (
                     PARTITION BY session_id
                     ORDER BY last_heartbeat DESC, team_id DESC
                 ) AS rn
                 FROM team_sessions
                 WHERE status = 'active'
             )
             WHERE rn = 1
         );
         CREATE UNIQUE INDEX IF NOT EXISTS idx_ts_single_active_session
             ON team_sessions(session_id) WHERE status = 'active';",
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
         COMMIT;",
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
        conn.execute("INSERT INTO projects (id, path) VALUES (1, '/test')", [])
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
        conn.execute("INSERT INTO projects (id, path) VALUES (1, '/test')", [])
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
        conn.execute("INSERT INTO projects (id, path) VALUES (1, '/test')", [])
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
        conn.execute("INSERT INTO projects (id, path) VALUES (1, '/test')", [])
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

    #[test]
    fn test_dedupe_three_way_picks_freshest_deterministically() {
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

        // Three duplicate teams for (delta, project 1). Survivor = team 1 (MIN id).
        // Team 3 has the freshest heartbeat, team 5 has a middle one.
        // Previously non-deterministic: SQLite could pick team 5's row instead of team 3's.
        conn.execute_batch(
            "INSERT INTO teams (id, name, project_id, config_path) VALUES (1, 'delta', 1, '/p1');
             INSERT INTO teams (id, name, project_id, config_path) VALUES (3, 'delta', 1, '/p2');
             INSERT INTO teams (id, name, project_id, config_path) VALUES (5, 'delta', 1, '/p3');
             -- Survivor's row: oldest
             INSERT INTO team_sessions (team_id, session_id, member_name, role, last_heartbeat)
                 VALUES (1, 'sess-Z', 'old_name', 'teammate', '2025-01-01T00:00:00');
             -- Duplicate team 5: middle heartbeat
             INSERT INTO team_sessions (team_id, session_id, member_name, role, last_heartbeat)
                 VALUES (5, 'sess-Z', 'mid_name', 'teammate', '2025-05-01T00:00:00');
             -- Duplicate team 3: freshest heartbeat
             INSERT INTO team_sessions (team_id, session_id, member_name, role, last_heartbeat)
                 VALUES (3, 'sess-Z', 'newest_name', 'lead', '2025-09-01T00:00:00');",
        )
        .unwrap();

        migrate_team_tables(&conn).unwrap();

        // Must deterministically pick team 3's metadata (freshest heartbeat)
        let (name, role, hb): (String, String, String) = conn
            .query_row(
                "SELECT member_name, role, last_heartbeat FROM team_sessions WHERE session_id = 'sess-Z'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            name, "newest_name",
            "should pick freshest duplicate metadata"
        );
        assert_eq!(role, "lead", "should pick freshest duplicate role");
        assert_eq!(hb, "2025-09-01T00:00:00", "should pick freshest heartbeat");

        let sess_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM team_sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(sess_count, 1);
        let team_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM teams", [], |r| r.get(0))
            .unwrap();
        assert_eq!(team_count, 1);
    }

    #[test]
    fn test_dedupe_orphan_session_in_multiple_duplicates() {
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

        // Three duplicate teams. Survivor (team 1) has NO row for sess-orphan,
        // but duplicate teams 3 and 5 both do. Without the orphan dedup step,
        // OR IGNORE would keep whichever row was processed first (rowid-dependent).
        conn.execute_batch(
            "INSERT INTO teams (id, name, project_id, config_path) VALUES (1, 'echo', 1, '/p1');
             INSERT INTO teams (id, name, project_id, config_path) VALUES (3, 'echo', 1, '/p2');
             INSERT INTO teams (id, name, project_id, config_path) VALUES (5, 'echo', 1, '/p3');
             -- Duplicate team 5: older
             INSERT INTO team_sessions (team_id, session_id, member_name, role, last_heartbeat)
                 VALUES (5, 'sess-orphan', 'stale_name', 'teammate', '2025-01-01T00:00:00');
             -- Duplicate team 3: freshest
             INSERT INTO team_sessions (team_id, session_id, member_name, role, last_heartbeat)
                 VALUES (3, 'sess-orphan', 'fresh_name', 'lead', '2025-09-01T00:00:00');",
        )
        .unwrap();

        migrate_team_tables(&conn).unwrap();

        // Orphan session must land in the survivor with the freshest metadata
        let (name, role, hb): (String, String, String) = conn
            .query_row(
                "SELECT member_name, role, last_heartbeat FROM team_sessions WHERE session_id = 'sess-orphan'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(name, "fresh_name", "orphan should keep freshest metadata");
        assert_eq!(role, "lead");
        assert_eq!(hb, "2025-09-01T00:00:00");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM team_sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "should have exactly one session row");
    }

    #[test]
    fn test_dedupe_cross_group_preserves_both_groups() {
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

        // Two UNRELATED duplicate groups (alpha and beta), each with duplicates.
        // Both groups have orphan sessions with the SAME session_id 'sess-shared'.
        // The dedupe must keep one row per group, not collapse across groups.
        conn.execute_batch(
            "-- Group alpha: survivor=1, duplicate=2
             INSERT INTO teams (id, name, project_id, config_path) VALUES (1, 'alpha', 1, '/a1');
             INSERT INTO teams (id, name, project_id, config_path) VALUES (2, 'alpha', 1, '/a2');
             -- Group beta: survivor=3, duplicate=4 (same project, different name)
             INSERT INTO teams (id, name, project_id, config_path) VALUES (3, 'beta', 1, '/b1');
             INSERT INTO teams (id, name, project_id, config_path) VALUES (4, 'beta', 1, '/b2');
             -- Orphan in alpha's duplicate
             INSERT INTO team_sessions (team_id, session_id, member_name, role, last_heartbeat)
                 VALUES (2, 'sess-shared', 'alice', 'lead', '2025-06-01T00:00:00');
             -- Orphan in beta's duplicate
             INSERT INTO team_sessions (team_id, session_id, member_name, role, last_heartbeat)
                 VALUES (4, 'sess-shared', 'bob', 'teammate', '2025-03-01T00:00:00');",
        )
        .unwrap();

        migrate_team_tables(&conn).unwrap();

        // Both groups should have their session remapped to their respective survivor
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM team_sessions WHERE session_id = 'sess-shared'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2, "each group should retain its own session row");

        let alpha_name: String = conn
            .query_row(
                "SELECT member_name FROM team_sessions WHERE team_id = 1 AND session_id = 'sess-shared'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(alpha_name, "alice");

        let beta_name: String = conn
            .query_row(
                "SELECT member_name FROM team_sessions WHERE team_id = 3 AND session_id = 'sess-shared'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(beta_name, "bob");

        let team_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM teams", [], |r| r.get(0))
            .unwrap();
        assert_eq!(team_count, 2, "two survivor teams remain");
    }

    #[test]
    fn test_single_active_team_constraint() {
        let conn = setup_db();
        migrate_team_tables(&conn).unwrap();

        // Create two teams
        conn.execute(
            "INSERT INTO teams (id, name, project_id, config_path) VALUES (1, 'team-a', 1, '/c1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO teams (id, name, project_id, config_path) VALUES (2, 'team-b', 1, '/c2')",
            [],
        )
        .unwrap();

        // Register session in team-a (active)
        conn.execute(
            "INSERT INTO team_sessions (team_id, session_id, member_name, status)
             VALUES (1, 'sess-1', 'alice', 'active')",
            [],
        )
        .unwrap();

        // Attempting a second active row for the same session should fail
        let result = conn.execute(
            "INSERT INTO team_sessions (team_id, session_id, member_name, status)
             VALUES (2, 'sess-1', 'alice', 'active')",
            [],
        );
        assert!(
            result.is_err(),
            "partial unique index should prevent duplicate active sessions"
        );

        // But a stopped session for the same session_id is fine
        conn.execute(
            "INSERT INTO team_sessions (team_id, session_id, member_name, status)
             VALUES (2, 'sess-1', 'alice', 'stopped')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn test_single_active_team_cleanup_keeps_freshest() {
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

        // Legacy corrupt state: session active in two different teams
        conn.execute_batch(
            "INSERT INTO teams (id, name, project_id, config_path) VALUES (1, 'team-x', 1, '/c1');
             INSERT INTO teams (id, name, project_id, config_path) VALUES (2, 'team-y', 1, '/c2');
             INSERT INTO team_sessions (team_id, session_id, member_name, status, last_heartbeat)
                 VALUES (1, 'sess-dup', 'alice', 'active', '2025-01-01T00:00:00');
             INSERT INTO team_sessions (team_id, session_id, member_name, status, last_heartbeat)
                 VALUES (2, 'sess-dup', 'alice', 'active', '2025-06-01T00:00:00');",
        )
        .unwrap();

        migrate_team_tables(&conn).unwrap();

        // Only one active membership should remain (the one with freshest heartbeat)
        let active_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM team_sessions WHERE session_id = 'sess-dup' AND status = 'active'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            active_count, 1,
            "cleanup should leave exactly one active membership"
        );

        // The surviving active one should be in team 2 (fresher heartbeat)
        let team_id: i64 = conn
            .query_row(
                "SELECT team_id FROM team_sessions WHERE session_id = 'sess-dup' AND status = 'active'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            team_id, 2,
            "should keep the membership with freshest heartbeat"
        );
    }
}
