// db/session_goals.rs
// Session-goal linkage database operations

use super::types::SessionGoalLink;
use rusqlite::{Connection, params};

/// Parse a SessionGoalLink from a row with column order:
/// (id, session_id, goal_id, interaction_type, created_at)
pub fn parse_session_goal_row(row: &rusqlite::Row) -> rusqlite::Result<SessionGoalLink> {
    Ok(SessionGoalLink {
        id: row.get(0)?,
        session_id: row.get(1)?,
        goal_id: row.get(2)?,
        interaction_type: row.get(3)?,
        created_at: row.get(4)?,
    })
}

/// Record a session-goal interaction. Idempotent via INSERT OR IGNORE.
/// Returns Ok(true) if a new link was created, Ok(false) if it already existed.
pub fn record_session_goal_sync(
    conn: &Connection,
    session_id: &str,
    goal_id: i64,
    interaction_type: &str,
) -> rusqlite::Result<bool> {
    let rows = conn.execute(
        "INSERT OR IGNORE INTO session_goals (session_id, goal_id, interaction_type)
         VALUES (?1, ?2, ?3)",
        params![session_id, goal_id, interaction_type],
    )?;
    Ok(rows > 0)
}

/// Get distinct sessions that worked on a specific goal, ordered by most recent first.
/// Returns one row per session with the latest interaction type and timestamp.
pub fn get_sessions_for_goal_sync(
    conn: &Connection,
    goal_id: i64,
    limit: usize,
) -> rusqlite::Result<Vec<SessionGoalLink>> {
    let limit = if limit == 0 { 20 } else { limit };
    let mut stmt = conn.prepare(
        "SELECT MAX(sg.id), sg.session_id, sg.goal_id,
                GROUP_CONCAT(DISTINCT sg.interaction_type) AS interaction_types,
                MAX(sg.created_at) AS last_activity
         FROM session_goals sg
         WHERE sg.goal_id = ?1
         GROUP BY sg.session_id
         ORDER BY last_activity DESC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![goal_id, limit], parse_session_goal_row)?;
    rows.collect()
}

/// Get all goals linked to a specific session.
pub fn get_goals_for_session_sync(
    conn: &Connection,
    session_id: &str,
) -> rusqlite::Result<Vec<SessionGoalLink>> {
    let mut stmt = conn.prepare(
        "SELECT sg.id, sg.session_id, sg.goal_id, sg.interaction_type, sg.created_at
         FROM session_goals sg
         WHERE sg.session_id = ?1
         ORDER BY sg.created_at DESC",
    )?;
    let rows = stmt.query_map(params![session_id], parse_session_goal_row)?;
    rows.collect()
}

/// Count distinct sessions that worked on a goal.
pub fn count_sessions_for_goal_sync(conn: &Connection, goal_id: i64) -> rusqlite::Result<usize> {
    conn.query_row(
        "SELECT COUNT(DISTINCT session_id) FROM session_goals WHERE goal_id = ?1",
        params![goal_id],
        |row| row.get(0),
    )
}

/// Delete all session_goals links for a goal (manual cascade).
pub fn delete_session_goals_for_goal_sync(
    conn: &Connection,
    goal_id: i64,
) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM session_goals WHERE goal_id = ?1",
        params![goal_id],
    )
}
