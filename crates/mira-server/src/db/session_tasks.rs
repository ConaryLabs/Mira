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
        let raw = serde_json::to_string(task)?;

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

/// A lightweight snapshot of an incomplete task for resume context.
pub struct IncompleteTask {
    pub subject: String,
    pub status: String,
}

/// Fetch incomplete (non-completed) tasks for a given session.
/// Used by session resume to show what was in progress.
pub fn get_incomplete_tasks_for_session_sync(
    conn: &Connection,
    session_id: &str,
) -> Vec<IncompleteTask> {
    let mut stmt = match conn.prepare(
        "SELECT subject, status FROM session_tasks \
         WHERE session_id = ? AND status != 'completed' \
         ORDER BY id",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    stmt.query_map(params![session_id], |row| {
        Ok(IncompleteTask {
            subject: row.get(0)?,
            status: row.get(1)?,
        })
    })
    .ok()
    .map(|rows| rows.flatten().collect())
    .unwrap_or_default()
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
