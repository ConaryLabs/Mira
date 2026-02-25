// db/test_support.rs
// Shared test helpers and macros for database tests

use super::get_or_create_project_sync;
use super::pool::DatabasePool;
use rusqlite::Connection;
use std::sync::Arc;

/// Run a sync database operation in the test pool, unwrapping the result.
///
/// Wraps `pool.interact(move |conn| body).await.unwrap()` into a single expression.
/// The body must return `anyhow::Result<T>` (use `.map_err(Into::into)` for rusqlite errors).
///
/// ```ignore
/// let id = db!(pool, |conn| create_task_sync(conn, args).map_err(Into::into));
/// let recap = db!(pool, |conn| Ok::<_, anyhow::Error>(build_recap(conn)));
/// ```
macro_rules! db {
    ($pool:expr, |$conn:ident| $body:expr) => {
        $pool.interact(move |$conn| $body).await.unwrap()
    };
}

/// Create a test pool (in-memory DB, no project)
pub async fn setup_test_pool() -> Arc<DatabasePool> {
    Arc::new(
        DatabasePool::open_in_memory()
            .await
            .expect("Failed to open in-memory pool"),
    )
}

/// Create a test pool with a default project
pub async fn setup_test_pool_with_project() -> (Arc<DatabasePool>, i64) {
    let pool = setup_test_pool().await;
    let project_id = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/test/path", Some("test")).map_err(Into::into)
    })
    .0;
    (pool, project_id)
}

/// Create a second project in the test pool (for isolation tests)
pub async fn setup_second_project(pool: &Arc<DatabasePool>) -> i64 {
    pool.interact(|conn| {
        get_or_create_project_sync(conn, "/other/path", Some("other")).map_err(Into::into)
    })
    .await
    .unwrap()
    .0
}

/// Create a sync in-memory connection with all migrations applied.
/// Use this for sync tests that don't need async pool semantics.
/// Loads sqlite-vec and runs all migrations.
pub fn setup_test_connection() -> rusqlite::Connection {
    use super::pool::ensure_sqlite_vec_registered;
    use super::schema::run_all_migrations;
    ensure_sqlite_vec_registered();
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    run_all_migrations(&conn).unwrap();
    conn
}

// ═══════════════════════════════════════════════════════════════════════════════
// Seed helpers for integration tests
// ═══════════════════════════════════════════════════════════════════════════════

/// Seed a session. If status is "completed", closes it immediately.
pub fn seed_session(conn: &Connection, session_id: &str, project_id: i64, status: &str) {
    super::create_session_sync(conn, session_id, Some(project_id)).unwrap();
    if status == "completed" {
        super::close_session_sync(conn, session_id, None).unwrap();
    }
}

/// Seed a tool history entry for a session.
pub fn seed_tool_history(
    conn: &Connection,
    session_id: &str,
    tool_name: &str,
    arguments_json: &str,
    output: &str,
) -> i64 {
    super::log_tool_call_sync(
        conn,
        session_id,
        tool_name,
        arguments_json,
        output,
        None,
        true,
    )
    .unwrap()
}

/// Seed a session snapshot (structured JSON metadata from stop hook).
pub fn seed_session_snapshot(conn: &Connection, session_id: &str, snapshot_json: &str) {
    conn.execute(
        "INSERT INTO session_snapshots (session_id, snapshot, created_at)
         VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(session_id) DO UPDATE SET snapshot = ?2, created_at = datetime('now')",
        rusqlite::params![session_id, snapshot_json],
    )
    .unwrap();
}

/// Seed a goal with sensible defaults.
pub fn seed_goal(
    conn: &Connection,
    project_id: i64,
    title: &str,
    status: &str,
    progress: i32,
) -> i64 {
    super::create_goal_sync(
        conn,
        Some(project_id),
        title,
        None,
        Some(status),
        Some("medium"),
        Some(progress as i64),
    )
    .unwrap()
}

/// Seed a code symbol. Returns the symbol ID.
pub fn seed_symbol(
    conn: &Connection,
    project_id: i64,
    name: &str,
    file_path: &str,
    symbol_type: &str,
    start_line: u32,
    end_line: u32,
) -> i64 {
    conn.execute(
        "INSERT INTO code_symbols (project_id, file_path, name, symbol_type, start_line, end_line)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            project_id,
            file_path,
            name,
            symbol_type,
            start_line,
            end_line
        ],
    )
    .unwrap();
    conn.last_insert_rowid()
}

/// Seed a call edge in the call graph.
pub fn seed_call_edge(conn: &Connection, caller_id: i64, callee_name: &str) {
    conn.execute(
        "INSERT INTO call_graph (caller_id, callee_name) VALUES (?1, ?2)",
        rusqlite::params![caller_id, callee_name],
    )
    .unwrap();
}

/// Seed a team. Returns team ID.
#[allow(dead_code)]
pub fn seed_team(conn: &Connection, name: &str, project_id: i64) -> i64 {
    super::get_or_create_team_sync(conn, name, Some(project_id), "/test/config.json").unwrap()
}

/// Seed a team member registration.
#[allow(dead_code)]
pub fn seed_team_member(conn: &Connection, team_id: i64, session_id: &str, name: &str, role: &str) {
    super::register_team_session_sync(conn, team_id, session_id, name, role, None).unwrap();
}
