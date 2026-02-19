// crates/mira-server/src/hooks/task_completed.rs
// Hook handler for TaskCompleted events - auto-links tasks to Mira goals

use crate::hooks::{HookTimer, read_hook_input, write_hook_output};
use anyhow::{Context, Result};

/// TaskCompleted hook input from Claude Code
#[derive(Debug)]
struct TaskCompletedInput {
    session_id: String,
    task_id: String,
    task_subject: String,
    task_description: Option<String>,
}

impl TaskCompletedInput {
    fn from_json(json: &serde_json::Value) -> Self {
        Self {
            session_id: json
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            task_id: json
                .get("task_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            task_subject: json
                .get("task_subject")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            task_description: json
                .get("task_description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        }
    }
}

/// Run TaskCompleted hook
///
/// This hook fires when a task is marked completed. We:
/// 1. Log task completion to session_behavior_log
/// 2. Check for matching milestones in active goals
/// 3. Auto-complete matching milestones
pub async fn run() -> Result<()> {
    let _timer = HookTimer::start("TaskCompleted");
    let input = read_hook_input().context("Failed to parse hook input from stdin")?;
    let task_input = TaskCompletedInput::from_json(&input);

    tracing::debug!(
        task_id = %task_input.task_id,
        subject = %task_input.task_subject,
        "TaskCompleted hook triggered"
    );

    // Connect via IPC (falls back to direct DB)
    let mut client = crate::ipc::client::HookClient::connect().await;

    // Get current project
    let sid = Some(task_input.session_id.as_str()).filter(|s| !s.is_empty());
    let Some((project_id, _)) = client.resolve_project(None, sid).await else {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    // Log task completion event
    {
        let mut data = serde_json::json!({
            "behavior_type": "task_completed",
            "task_id": task_input.task_id,
            "task_subject": task_input.task_subject,
        });
        if let Some(ref desc) = task_input.task_description {
            data["task_description"] = serde_json::Value::String(desc.clone());
        }
        client
            .log_behavior(&task_input.session_id, project_id, "goal_update", data)
            .await;
    }

    // Try to auto-link task completion to goal milestones
    let session_id = if task_input.session_id.is_empty() {
        None
    } else {
        Some(task_input.session_id.as_str())
    };
    client
        .auto_link_milestone(
            project_id,
            &task_input.task_subject,
            task_input.task_description.as_deref(),
            session_id,
        )
        .await;

    write_hook_output(&serde_json::json!({}));
    Ok(())
}

/// Check if a completed task matches any active goal milestones and auto-complete them.
/// Matches against both the task subject and optional description for better coverage.
pub fn auto_link_milestone(
    conn: &rusqlite::Connection,
    project_id: i64,
    task_subject: &str,
    task_description: Option<&str>,
    session_id: Option<&str>,
) -> Result<()> {
    // Get active goals for this project
    let goals_sql = r#"
        SELECT id, title
        FROM goals
        WHERE project_id = ?
          AND status IN ('planning', 'in_progress')
    "#;
    let mut goals_stmt = conn.prepare(goals_sql)?;
    let goal_ids: Vec<(i64, String)> = goals_stmt
        .query_map([project_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    if goal_ids.is_empty() {
        return Ok(());
    }

    // Get incomplete milestones for those goals
    let placeholders: Vec<String> = goal_ids.iter().map(|_| "?".to_string()).collect();
    let milestones_sql = format!(
        "SELECT id, goal_id, title FROM milestones WHERE goal_id IN ({}) AND completed = 0",
        placeholders.join(", ")
    );
    let mut milestones_stmt = conn.prepare(&milestones_sql)?;
    let params: Vec<&dyn rusqlite::ToSql> = goal_ids
        .iter()
        .map(|(id, _)| id as &dyn rusqlite::ToSql)
        .collect();

    let milestones: Vec<(i64, i64, String)> = milestones_stmt
        .query_map(params.as_slice(), |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    if milestones.is_empty() {
        return Ok(());
    }

    // Fuzzy match: check if task subject or description contains milestone title or vice versa.
    // To prevent overly aggressive matching (e.g., short milestone "fix" matching everything),
    // require a minimum length for substring matching. Below the threshold, require exact match.
    const MIN_SUBSTRING_LEN: usize = 4;
    let task_lower = task_subject.to_lowercase();
    let desc_lower = task_description.map(|d| d.to_lowercase());
    for (milestone_id, goal_id, milestone_title) in &milestones {
        let ms_lower = milestone_title.to_lowercase();
        let subject_matches =
            if ms_lower.len() < MIN_SUBSTRING_LEN || task_lower.len() < MIN_SUBSTRING_LEN {
                // Short strings: require exact case-insensitive match
                task_lower == ms_lower
            } else {
                task_lower.contains(&ms_lower) || ms_lower.contains(&task_lower)
            };
        let desc_matches = desc_lower.as_ref().is_some_and(|d| {
            if ms_lower.len() < MIN_SUBSTRING_LEN || d.len() < MIN_SUBSTRING_LEN {
                *d == ms_lower
            } else {
                d.contains(&ms_lower) || ms_lower.contains(d)
            }
        });
        if subject_matches || desc_matches {
            // Mark milestone as completed (with temporal fields matching explicit path)
            crate::db::complete_milestone_sync(conn, *milestone_id, session_id)?;

            // Find the goal title for logging
            let goal_title = goal_ids
                .iter()
                .find(|(id, _)| id == goal_id)
                .map(|(_, t)| t.as_str())
                .unwrap_or("unknown");

            tracing::info!(
                task = task_subject,
                milestone = %milestone_title,
                goal = goal_title,
                "Auto-linked task to milestone"
            );
            // Only match the first milestone
            break;
        }
    }

    Ok(())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_input_parses_all_fields() {
        let input = TaskCompletedInput::from_json(&serde_json::json!({
            "session_id": "sess-1",
            "task_id": "task-42",
            "task_subject": "Fix login bug",
            "task_description": "The login form crashes on empty input"
        }));
        assert_eq!(input.session_id, "sess-1");
        assert_eq!(input.task_id, "task-42");
        assert_eq!(input.task_subject, "Fix login bug");
        assert_eq!(
            input.task_description.as_deref(),
            Some("The login form crashes on empty input")
        );
    }

    #[test]
    fn task_input_defaults_on_empty_json() {
        let input = TaskCompletedInput::from_json(&serde_json::json!({}));
        assert!(input.session_id.is_empty());
        assert!(input.task_id.is_empty());
        assert!(input.task_subject.is_empty());
        assert!(input.task_description.is_none());
    }

    #[test]
    fn task_input_ignores_wrong_types() {
        let input = TaskCompletedInput::from_json(&serde_json::json!({
            "task_id": 42,
            "task_subject": false
        }));
        assert!(input.task_id.is_empty());
        assert!(input.task_subject.is_empty());
    }

    #[test]
    fn auto_link_no_goals() {
        let conn = crate::db::test_support::setup_test_connection();
        crate::db::get_or_create_project_sync(&conn, "/tmp/test-proj", None).unwrap();
        // Should not error even with no goals
        let result = auto_link_milestone(&conn, 1, "Some task", None, None);
        assert!(result.is_ok());
    }

    #[test]
    fn auto_link_matches_milestone() {
        let conn = crate::db::test_support::setup_test_connection();
        let (pid, _) =
            crate::db::get_or_create_project_sync(&conn, "/tmp/link-test", None).unwrap();
        crate::db::test_support::seed_goal(&conn, pid, "Auth System", "in_progress", 0);

        // Get the goal ID
        let goal_id: i64 = conn
            .query_row("SELECT id FROM goals WHERE project_id = ?", [pid], |row| {
                row.get(0)
            })
            .unwrap();

        // Add a milestone
        conn.execute(
            "INSERT INTO milestones (goal_id, title, completed, weight) VALUES (?, ?, 0, 1)",
            rusqlite::params![goal_id, "Fix login bug"],
        )
        .unwrap();

        // Auto-link with matching task subject
        auto_link_milestone(&conn, pid, "Fix login bug", None, Some("test-session")).unwrap();

        // Check milestone was completed with temporal fields
        let (completed, completed_at, completed_in_session): (bool, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT completed, completed_at, completed_in_session_id FROM milestones WHERE goal_id = ?",
                [goal_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert!(completed);
        assert!(
            completed_at.is_some(),
            "completed_at should be set by auto-link"
        );
        assert_eq!(completed_in_session.as_deref(), Some("test-session"));
    }

    #[test]
    fn auto_link_case_insensitive() {
        let conn = crate::db::test_support::setup_test_connection();
        let (pid, _) =
            crate::db::get_or_create_project_sync(&conn, "/tmp/case-test", None).unwrap();
        crate::db::test_support::seed_goal(&conn, pid, "API Work", "in_progress", 0);

        let goal_id: i64 = conn
            .query_row("SELECT id FROM goals WHERE project_id = ?", [pid], |row| {
                row.get(0)
            })
            .unwrap();

        conn.execute(
            "INSERT INTO milestones (goal_id, title, completed, weight) VALUES (?, ?, 0, 1)",
            rusqlite::params![goal_id, "Add Authentication"],
        )
        .unwrap();

        // Task subject contains milestone title (case-insensitive)
        auto_link_milestone(
            &conn,
            pid,
            "add authentication endpoint",
            None,
            Some("test-session"),
        )
        .unwrap();

        let completed: bool = conn
            .query_row(
                "SELECT completed FROM milestones WHERE goal_id = ?",
                [goal_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(completed);
    }

    #[test]
    fn auto_link_no_match_leaves_milestone() {
        let conn = crate::db::test_support::setup_test_connection();
        let (pid, _) =
            crate::db::get_or_create_project_sync(&conn, "/tmp/nomatch-test", None).unwrap();
        crate::db::test_support::seed_goal(&conn, pid, "Refactor", "in_progress", 0);

        let goal_id: i64 = conn
            .query_row("SELECT id FROM goals WHERE project_id = ?", [pid], |row| {
                row.get(0)
            })
            .unwrap();

        conn.execute(
            "INSERT INTO milestones (goal_id, title, completed, weight) VALUES (?, ?, 0, 1)",
            rusqlite::params![goal_id, "Extract config module"],
        )
        .unwrap();

        // Unrelated task
        auto_link_milestone(&conn, pid, "Fix CSS styling", None, Some("test-session")).unwrap();

        let completed: bool = conn
            .query_row(
                "SELECT completed FROM milestones WHERE goal_id = ?",
                [goal_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!completed);
    }

    #[test]
    fn auto_link_matches_via_description() {
        let conn = crate::db::test_support::setup_test_connection();
        let (pid, _) =
            crate::db::get_or_create_project_sync(&conn, "/tmp/desc-test", None).unwrap();
        crate::db::test_support::seed_goal(&conn, pid, "Backend Work", "in_progress", 0);

        let goal_id: i64 = conn
            .query_row("SELECT id FROM goals WHERE project_id = ?", [pid], |row| {
                row.get(0)
            })
            .unwrap();

        conn.execute(
            "INSERT INTO milestones (goal_id, title, completed, weight) VALUES (?, ?, 0, 1)",
            rusqlite::params![goal_id, "Add rate limiting"],
        )
        .unwrap();

        // Subject doesn't match, but description does
        auto_link_milestone(
            &conn,
            pid,
            "Implement middleware",
            Some("Add rate limiting to API endpoints"),
            Some("test-session"),
        )
        .unwrap();

        let completed: bool = conn
            .query_row(
                "SELECT completed FROM milestones WHERE goal_id = ?",
                [goal_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(completed);
    }

    #[test]
    fn auto_link_short_milestone_no_false_positive() {
        // A short milestone like "fix" should NOT match via substring
        let conn = crate::db::test_support::setup_test_connection();
        let (pid, _) =
            crate::db::get_or_create_project_sync(&conn, "/tmp/short-test", None).unwrap();
        crate::db::test_support::seed_goal(&conn, pid, "Bug Fixes", "in_progress", 0);

        let goal_id: i64 = conn
            .query_row("SELECT id FROM goals WHERE project_id = ?", [pid], |row| {
                row.get(0)
            })
            .unwrap();

        conn.execute(
            "INSERT INTO milestones (goal_id, title, completed, weight) VALUES (?, ?, 0, 1)",
            rusqlite::params![goal_id, "fix"],
        )
        .unwrap();

        // Task "Fix login bug" should NOT match short milestone "fix" via substring
        auto_link_milestone(&conn, pid, "Fix login bug", None, Some("test-session")).unwrap();

        let completed: bool = conn
            .query_row(
                "SELECT completed FROM milestones WHERE goal_id = ?",
                [goal_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            !completed,
            "Short milestone 'fix' should not match via substring"
        );
    }

    #[test]
    fn auto_link_short_milestone_exact_match() {
        // A short milestone should still match if the task subject is exactly the same
        let conn = crate::db::test_support::setup_test_connection();
        let (pid, _) =
            crate::db::get_or_create_project_sync(&conn, "/tmp/exact-test", None).unwrap();
        crate::db::test_support::seed_goal(&conn, pid, "Quick Tasks", "in_progress", 0);

        let goal_id: i64 = conn
            .query_row("SELECT id FROM goals WHERE project_id = ?", [pid], |row| {
                row.get(0)
            })
            .unwrap();

        conn.execute(
            "INSERT INTO milestones (goal_id, title, completed, weight) VALUES (?, ?, 0, 1)",
            rusqlite::params![goal_id, "Fix"],
        )
        .unwrap();

        // Exact match (case-insensitive) should still work
        auto_link_milestone(&conn, pid, "fix", None, Some("test-session")).unwrap();

        let completed: bool = conn
            .query_row(
                "SELECT completed FROM milestones WHERE goal_id = ?",
                [goal_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            completed,
            "Short milestone should match on exact case-insensitive match"
        );
    }
}
