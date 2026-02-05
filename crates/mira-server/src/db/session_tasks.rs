// crates/mira-server/src/db/session_tasks.rs
// Session task snapshot CRUD operations
//
// Snapshots Claude Code's native task files into the database for
// history tracking, goal linking, and cross-session context.

use crate::tasks::NativeTask;
use rusqlite::{Connection, params};

/// Snapshot native tasks into the database via UPSERT.
/// Uses the unique index on (native_task_list_id, native_task_id) for idempotency.
pub fn snapshot_native_tasks_sync(
    conn: &Connection,
    project_id: i64,
    task_list_id: &str,
    session_id: Option<&str>,
    tasks: &[NativeTask],
) -> anyhow::Result<usize> {
    let sql = r#"
        INSERT INTO session_tasks
            (project_id, session_id, native_task_list_id, native_task_id,
             subject, description, status, raw_payload, goal_id, milestone_id, updated_at, completed_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'),
                CASE WHEN ? = 'completed' THEN datetime('now') ELSE NULL END)
        ON CONFLICT(native_task_list_id, native_task_id)
            WHERE native_task_list_id IS NOT NULL
        DO UPDATE SET
            subject = excluded.subject,
            description = excluded.description,
            status = excluded.status,
            raw_payload = excluded.raw_payload,
            goal_id = COALESCE(excluded.goal_id, session_tasks.goal_id),
            milestone_id = COALESCE(excluded.milestone_id, session_tasks.milestone_id),
            version = session_tasks.version + 1,
            updated_at = datetime('now'),
            completed_at = CASE
                WHEN excluded.status = 'completed' AND session_tasks.completed_at IS NULL
                THEN datetime('now')
                ELSE session_tasks.completed_at
            END
    "#;

    let mut stmt = conn.prepare(sql)?;
    let mut count = 0;

    for task in tasks {
        let (goal_id, milestone_id) = parse_link_tags(&task.subject);
        let raw = serde_json::to_string(task).unwrap_or_default();

        stmt.execute(params![
            project_id,
            session_id,
            task_list_id,
            task.id,
            task.subject,
            task.description,
            task.status,
            raw,
            goal_id,
            milestone_id,
            task.status, // for the CASE in completed_at
        ])?;
        count += 1;
    }

    Ok(count)
}

/// Get pending/in_progress session tasks for a project.
pub fn get_pending_session_tasks_sync(
    conn: &Connection,
    project_id: i64,
    limit: usize,
) -> anyhow::Result<Vec<SessionTask>> {
    let sql = r#"
        SELECT id, project_id, session_id, native_task_list_id, native_task_id,
               subject, description, status, goal_id, milestone_id, created_at
        FROM session_tasks
        WHERE project_id = ? AND status IN ('pending', 'in_progress')
        ORDER BY id ASC
        LIMIT ?
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![project_id, limit as i64], parse_session_task_row)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Count completed vs remaining session tasks for a project.
/// Returns (completed, remaining).
pub fn count_session_tasks_sync(
    conn: &Connection,
    project_id: i64,
) -> anyhow::Result<(usize, usize)> {
    let sql = r#"
        SELECT
            COALESCE(SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN status != 'completed' THEN 1 ELSE 0 END), 0)
        FROM session_tasks
        WHERE project_id = ?
    "#;

    let (completed, remaining): (i64, i64) =
        conn.query_row(sql, [project_id], |row| Ok((row.get(0)?, row.get(1)?)))?;

    Ok((completed as usize, remaining as usize))
}

/// Log an iteration snapshot for audit trail.
pub fn log_iteration_sync(
    conn: &Connection,
    project_id: i64,
    session_id: Option<&str>,
    iteration: i32,
    completed: usize,
    remaining: usize,
    summary: Option<&str>,
) -> anyhow::Result<i64> {
    conn.execute(
        r#"INSERT INTO session_task_iterations
            (project_id, session_id, iteration, tasks_completed, tasks_remaining, summary)
           VALUES (?, ?, ?, ?, ?, ?)"#,
        params![
            project_id,
            session_id,
            iteration,
            completed as i64,
            remaining as i64,
            summary,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Parse `[goal:ID]` and `[milestone:ID]` tags from task subject text.
/// Returns (goal_id, milestone_id).
pub fn parse_link_tags(text: &str) -> (Option<i64>, Option<i64>) {
    let goal_id = parse_tag(text, "goal");
    let milestone_id = parse_tag(text, "milestone");
    (goal_id, milestone_id)
}

fn parse_tag(text: &str, tag: &str) -> Option<i64> {
    let prefix = format!("[{}:", tag);
    let start = text.find(&prefix)?;
    let after = &text[start + prefix.len()..];
    let end = after.find(']')?;
    after[..end].trim().parse().ok()
}

/// Lightweight session task representation for query results.
#[derive(Debug, Clone)]
pub struct SessionTask {
    pub id: i64,
    pub project_id: i64,
    pub session_id: Option<String>,
    pub native_task_list_id: Option<String>,
    pub native_task_id: Option<String>,
    pub subject: String,
    pub description: Option<String>,
    pub status: String,
    pub goal_id: Option<i64>,
    pub milestone_id: Option<i64>,
    pub created_at: Option<String>,
}

fn parse_session_task_row(row: &rusqlite::Row) -> rusqlite::Result<SessionTask> {
    Ok(SessionTask {
        id: row.get(0)?,
        project_id: row.get(1)?,
        session_id: row.get(2)?,
        native_task_list_id: row.get(3)?,
        native_task_id: row.get(4)?,
        subject: row.get(5)?,
        description: row.get(6)?,
        status: row.get(7)?,
        goal_id: row.get(8)?,
        milestone_id: row.get(9)?,
        created_at: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_link_tags_goal() {
        let (goal, milestone) = parse_link_tags("Fix auth bug [goal:42]");
        assert_eq!(goal, Some(42));
        assert_eq!(milestone, None);
    }

    #[test]
    fn test_parse_link_tags_milestone() {
        let (goal, milestone) = parse_link_tags("Add tests [milestone:7]");
        assert_eq!(goal, None);
        assert_eq!(milestone, Some(7));
    }

    #[test]
    fn test_parse_link_tags_both() {
        let (goal, milestone) = parse_link_tags("Deploy [goal:10] [milestone:3]");
        assert_eq!(goal, Some(10));
        assert_eq!(milestone, Some(3));
    }

    #[test]
    fn test_parse_link_tags_none() {
        let (goal, milestone) = parse_link_tags("Just a regular task title");
        assert_eq!(goal, None);
        assert_eq!(milestone, None);
    }

    #[test]
    fn test_parse_link_tags_invalid_id() {
        let (goal, milestone) = parse_link_tags("[goal:abc] [milestone:]");
        assert_eq!(goal, None);
        assert_eq!(milestone, None);
    }

    #[test]
    fn test_parse_link_tags_with_spaces() {
        let (goal, _) = parse_link_tags("[goal: 42 ]");
        assert_eq!(goal, Some(42));
    }
}
