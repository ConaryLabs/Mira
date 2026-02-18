// db/milestones.rs
// Milestone database operations

use rusqlite::{Connection, OptionalExtension, params};

use super::types::Milestone;

/// Parse Milestone from a rusqlite Row with standard column order:
/// (id, goal_id, title, completed, weight, created_at, completed_at, completed_in_session_id)
pub fn parse_milestone_row(row: &rusqlite::Row) -> rusqlite::Result<Milestone> {
    Ok(Milestone {
        id: row.get(0)?,
        goal_id: row.get(1)?,
        title: row.get(2)?,
        completed: row.get::<_, i32>(3)? != 0,
        weight: row.get(4)?,
        created_at: row.get(5)?,
        completed_at: row.get(6)?,
        completed_in_session_id: row.get(7)?,
    })
}

/// Create a new milestone for a goal
pub fn create_milestone_sync(
    conn: &Connection,
    goal_id: i64,
    title: &str,
    weight: Option<i32>,
) -> rusqlite::Result<i64> {
    let weight = weight.unwrap_or(1);
    conn.execute(
        "INSERT INTO milestones (goal_id, title, weight) VALUES (?, ?, ?)",
        params![goal_id, title, weight],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Get all milestones for a goal
pub fn get_milestones_for_goal_sync(
    conn: &Connection,
    goal_id: i64,
) -> rusqlite::Result<Vec<Milestone>> {
    let sql = "SELECT id, goal_id, title, completed, weight, created_at, completed_at, completed_in_session_id
               FROM milestones WHERE goal_id = ?
               ORDER BY id ASC";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([goal_id], parse_milestone_row)?;
    rows.collect()
}

/// Get a milestone by ID
pub fn get_milestone_by_id_sync(conn: &Connection, id: i64) -> rusqlite::Result<Option<Milestone>> {
    let sql = "SELECT id, goal_id, title, completed, weight, created_at, completed_at, completed_in_session_id
               FROM milestones WHERE id = ?";
    conn.query_row(sql, [id], parse_milestone_row).optional()
}

/// Update a milestone
pub fn update_milestone_sync(
    conn: &Connection,
    id: i64,
    title: Option<&str>,
    weight: Option<i32>,
) -> rusqlite::Result<()> {
    if let Some(title) = title {
        conn.execute(
            "UPDATE milestones SET title = ? WHERE id = ?",
            params![title, id],
        )?;
    }
    if let Some(weight) = weight {
        conn.execute(
            "UPDATE milestones SET weight = ? WHERE id = ?",
            params![weight, id],
        )?;
    }
    Ok(())
}

/// Mark a milestone as completed and return the goal_id for progress update
pub fn complete_milestone_sync(
    conn: &Connection,
    id: i64,
    session_id: Option<&str>,
) -> rusqlite::Result<Option<i64>> {
    conn.execute(
        "UPDATE milestones SET completed = 1, completed_at = datetime('now'), completed_in_session_id = ?2 WHERE id = ?1",
        params![id, session_id],
    )?;
    // Return the goal_id so caller can update progress
    conn.query_row("SELECT goal_id FROM milestones WHERE id = ?", [id], |row| {
        row.get(0)
    })
    .optional()
}

/// Delete a milestone and return the goal_id for progress update
pub fn delete_milestone_sync(conn: &Connection, id: i64) -> rusqlite::Result<Option<i64>> {
    // Get goal_id first
    let goal_id: Option<i64> = conn
        .query_row("SELECT goal_id FROM milestones WHERE id = ?", [id], |row| {
            row.get(0)
        })
        .optional()?;

    conn.execute("DELETE FROM milestones WHERE id = ?", [id])?;
    Ok(goal_id)
}

/// Calculate goal progress based on completed milestones
/// Returns progress as percentage (0-100)
pub fn calculate_goal_progress_sync(conn: &Connection, goal_id: i64) -> rusqlite::Result<i32> {
    let (completed_weight, total_weight): (i64, i64) = conn.query_row(
        "SELECT
            COALESCE(SUM(CASE WHEN completed = 1 THEN weight ELSE 0 END), 0),
            COALESCE(SUM(weight), 0)
         FROM milestones WHERE goal_id = ?",
        [goal_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    if total_weight == 0 {
        return Ok(0);
    }

    let progress = (completed_weight as f64 / total_weight as f64 * 100.0).round() as i32;
    Ok(progress.min(100))
}

/// Update a goal's progress based on its milestones
pub fn update_goal_progress_from_milestones_sync(
    conn: &Connection,
    goal_id: i64,
) -> rusqlite::Result<i32> {
    let progress = calculate_goal_progress_sync(conn, goal_id)?;
    conn.execute(
        "UPDATE goals SET progress_percent = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        params![progress, goal_id],
    )?;
    Ok(progress)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::setup_test_connection;

    // Helper: create a goal and return its id
    fn create_test_goal(conn: &Connection) -> i64 {
        crate::db::create_goal_sync(
            conn,
            Some(1), // project_id
            "Test Goal",
            None,
            Some("in_progress"),
            Some("medium"),
            Some(0),
        )
        .expect("create goal should succeed")
    }

    // Helper: ensure project exists
    fn ensure_project(conn: &Connection) -> i64 {
        crate::db::get_or_create_project_sync(conn, "/test/project", Some("test"))
            .expect("create project should succeed")
            .0
    }

    // ========================================================================
    // Happy-path CRUD tests
    // ========================================================================

    #[test]
    fn test_create_milestone_returns_positive_id() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);
        let goal_id = create_test_goal(&conn);

        let id = create_milestone_sync(&conn, goal_id, "First milestone", None)
            .expect("create should succeed");
        assert!(id > 0);
    }

    #[test]
    fn test_create_milestone_default_weight_is_one() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);
        let goal_id = create_test_goal(&conn);

        let id = create_milestone_sync(&conn, goal_id, "Milestone", None)
            .expect("create should succeed");
        let ms = get_milestone_by_id_sync(&conn, id)
            .expect("get should succeed")
            .expect("milestone should exist");
        assert_eq!(ms.weight, 1);
    }

    #[test]
    fn test_create_milestone_custom_weight() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);
        let goal_id = create_test_goal(&conn);

        let id = create_milestone_sync(&conn, goal_id, "Heavy milestone", Some(5))
            .expect("create should succeed");
        let ms = get_milestone_by_id_sync(&conn, id)
            .expect("get should succeed")
            .expect("milestone should exist");
        assert_eq!(ms.weight, 5);
        assert_eq!(ms.title, "Heavy milestone");
    }

    #[test]
    fn test_get_milestones_for_goal_returns_ordered_by_id() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);
        let goal_id = create_test_goal(&conn);

        create_milestone_sync(&conn, goal_id, "First", None).unwrap();
        create_milestone_sync(&conn, goal_id, "Second", None).unwrap();
        create_milestone_sync(&conn, goal_id, "Third", None).unwrap();

        let milestones = get_milestones_for_goal_sync(&conn, goal_id).unwrap();
        assert_eq!(milestones.len(), 3);
        assert_eq!(milestones[0].title, "First");
        assert_eq!(milestones[1].title, "Second");
        assert_eq!(milestones[2].title, "Third");
    }

    #[test]
    fn test_get_milestone_by_id_all_fields() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);
        let goal_id = create_test_goal(&conn);

        let id = create_milestone_sync(&conn, goal_id, "Detailed", Some(3)).unwrap();
        let ms = get_milestone_by_id_sync(&conn, id).unwrap().unwrap();

        assert_eq!(ms.id, id);
        assert_eq!(ms.goal_id, Some(goal_id));
        assert_eq!(ms.title, "Detailed");
        assert_eq!(ms.weight, 3);
        assert!(!ms.completed);
        assert!(ms.completed_at.is_none());
        assert!(ms.completed_in_session_id.is_none());
    }

    #[test]
    fn test_update_milestone_title_only() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);
        let goal_id = create_test_goal(&conn);
        let id = create_milestone_sync(&conn, goal_id, "Old title", Some(2)).unwrap();

        update_milestone_sync(&conn, id, Some("New title"), None).unwrap();

        let ms = get_milestone_by_id_sync(&conn, id).unwrap().unwrap();
        assert_eq!(ms.title, "New title");
        assert_eq!(ms.weight, 2); // unchanged
    }

    #[test]
    fn test_update_milestone_weight_only() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);
        let goal_id = create_test_goal(&conn);
        let id = create_milestone_sync(&conn, goal_id, "Keep title", Some(1)).unwrap();

        update_milestone_sync(&conn, id, None, Some(10)).unwrap();

        let ms = get_milestone_by_id_sync(&conn, id).unwrap().unwrap();
        assert_eq!(ms.title, "Keep title"); // unchanged
        assert_eq!(ms.weight, 10);
    }

    #[test]
    fn test_complete_milestone_sets_fields() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);
        let goal_id = create_test_goal(&conn);
        let id = create_milestone_sync(&conn, goal_id, "Complete me", None).unwrap();

        let returned = complete_milestone_sync(&conn, id, Some("session-abc")).unwrap();
        assert_eq!(returned, Some(goal_id));

        let ms = get_milestone_by_id_sync(&conn, id).unwrap().unwrap();
        assert!(ms.completed);
        assert!(ms.completed_at.is_some());
        assert_eq!(ms.completed_in_session_id, Some("session-abc".to_string()));
    }

    #[test]
    fn test_delete_milestone_removes_it() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);
        let goal_id = create_test_goal(&conn);
        let id = create_milestone_sync(&conn, goal_id, "Delete me", None).unwrap();

        let returned = delete_milestone_sync(&conn, id).unwrap();
        assert_eq!(returned, Some(goal_id));

        let ms = get_milestone_by_id_sync(&conn, id).unwrap();
        assert!(ms.is_none());
    }

    #[test]
    fn test_calculate_progress_partial_completion() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);
        let goal_id = create_test_goal(&conn);

        let id1 = create_milestone_sync(&conn, goal_id, "Done", Some(1)).unwrap();
        create_milestone_sync(&conn, goal_id, "Not done", Some(1)).unwrap();
        complete_milestone_sync(&conn, id1, None).unwrap();

        let progress = calculate_goal_progress_sync(&conn, goal_id).unwrap();
        assert_eq!(progress, 50);
    }

    #[test]
    fn test_calculate_progress_weighted_milestones() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);
        let goal_id = create_test_goal(&conn);

        // weight=3 completed, weight=7 not => 30%
        let id1 = create_milestone_sync(&conn, goal_id, "Light", Some(3)).unwrap();
        create_milestone_sync(&conn, goal_id, "Heavy", Some(7)).unwrap();
        complete_milestone_sync(&conn, id1, None).unwrap();

        let progress = calculate_goal_progress_sync(&conn, goal_id).unwrap();
        assert_eq!(progress, 30);
    }

    #[test]
    fn test_calculate_progress_all_complete_is_100() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);
        let goal_id = create_test_goal(&conn);

        let id1 = create_milestone_sync(&conn, goal_id, "A", Some(2)).unwrap();
        let id2 = create_milestone_sync(&conn, goal_id, "B", Some(3)).unwrap();
        complete_milestone_sync(&conn, id1, None).unwrap();
        complete_milestone_sync(&conn, id2, None).unwrap();

        let progress = calculate_goal_progress_sync(&conn, goal_id).unwrap();
        assert_eq!(progress, 100);
    }

    #[test]
    fn test_update_goal_progress_writes_to_goals_table() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);
        let goal_id = create_test_goal(&conn);

        let id1 = create_milestone_sync(&conn, goal_id, "Done", Some(1)).unwrap();
        create_milestone_sync(&conn, goal_id, "Pending", Some(1)).unwrap();
        complete_milestone_sync(&conn, id1, None).unwrap();

        let progress = update_goal_progress_from_milestones_sync(&conn, goal_id).unwrap();
        assert_eq!(progress, 50);

        // Verify it was written to the goals table
        let stored: i32 = conn
            .query_row(
                "SELECT progress_percent FROM goals WHERE id = ?",
                [goal_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, 50);
    }

    // ========================================================================
    // calculate_goal_progress_sync edge cases
    // ========================================================================

    #[test]
    fn test_calculate_progress_no_milestones() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);
        let goal_id = create_test_goal(&conn);

        // No milestones at all: total_weight == 0 => should return 0
        let progress = calculate_goal_progress_sync(&conn, goal_id)
            .expect("calculate progress should succeed");
        assert_eq!(progress, 0, "progress with no milestones should be 0");
    }

    #[test]
    fn test_calculate_progress_with_zero_weight_milestones() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);
        let goal_id = create_test_goal(&conn);

        // Create milestones with weight=0: total_weight sums to 0
        create_milestone_sync(&conn, goal_id, "Zero weight A", Some(0))
            .expect("create milestone should succeed");
        create_milestone_sync(&conn, goal_id, "Zero weight B", Some(0))
            .expect("create milestone should succeed");

        // total_weight == 0 => should return 0 (no division by zero)
        let progress = calculate_goal_progress_sync(&conn, goal_id)
            .expect("calculate progress should succeed");
        assert_eq!(
            progress, 0,
            "progress with all zero-weight milestones should be 0, not panic"
        );
    }

    #[test]
    fn test_calculate_progress_nonexistent_goal() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);

        // Goal ID 99999 does not exist: query returns COALESCE(SUM(weight),0) = 0
        let progress = calculate_goal_progress_sync(&conn, 99999)
            .expect("calculate progress should succeed even for nonexistent goal");
        assert_eq!(progress, 0);
    }

    // ========================================================================
    // get_milestone_by_id_sync with nonexistent ID
    // ========================================================================

    #[test]
    fn test_get_milestone_by_id_nonexistent() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);

        let result = get_milestone_by_id_sync(&conn, 99999).expect("get by id should succeed");
        assert!(result.is_none(), "nonexistent milestone should return None");
    }

    // ========================================================================
    // complete_milestone_sync with nonexistent ID
    // ========================================================================

    #[test]
    fn test_complete_milestone_nonexistent() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);

        // Completing a nonexistent milestone: UPDATE affects 0 rows,
        // then SELECT goal_id returns None
        let result = complete_milestone_sync(&conn, 99999, Some("session-1"))
            .expect("complete should succeed");
        assert!(
            result.is_none(),
            "completing nonexistent milestone should return None"
        );
    }

    #[test]
    fn test_complete_already_completed_milestone() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);
        let goal_id = create_test_goal(&conn);
        let ms_id = create_milestone_sync(&conn, goal_id, "Done task", None)
            .expect("create milestone should succeed");

        // Complete it once
        let result1 = complete_milestone_sync(&conn, ms_id, Some("s1"))
            .expect("first complete should succeed");
        assert_eq!(result1, Some(goal_id));

        // Complete it again (idempotent — just overwrites completed_at)
        let result2 = complete_milestone_sync(&conn, ms_id, Some("s2"))
            .expect("second complete should succeed");
        assert_eq!(result2, Some(goal_id));

        // Verify it's still completed
        let ms = get_milestone_by_id_sync(&conn, ms_id)
            .expect("get should succeed")
            .expect("milestone should exist");
        assert!(ms.completed);
    }

    // ========================================================================
    // delete_milestone_sync with nonexistent ID
    // ========================================================================

    #[test]
    fn test_delete_milestone_nonexistent() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);

        // Deleting nonexistent: SELECT returns None, DELETE affects 0 rows
        let result = delete_milestone_sync(&conn, 99999).expect("delete should succeed");
        assert!(
            result.is_none(),
            "deleting nonexistent milestone should return None"
        );
    }

    // ========================================================================
    // get_milestones_for_goal_sync with empty list
    // ========================================================================

    #[test]
    fn test_get_milestones_for_nonexistent_goal() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);

        let milestones =
            get_milestones_for_goal_sync(&conn, 99999).expect("get milestones should succeed");
        assert!(milestones.is_empty());
    }

    // ========================================================================
    // update_milestone_sync with nonexistent ID (no-op)
    // ========================================================================

    #[test]
    fn test_update_milestone_nonexistent() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);

        // Updating nonexistent milestone: UPDATE affects 0 rows, returns Ok(())
        let result = update_milestone_sync(&conn, 99999, Some("new title"), Some(5));
        assert!(result.is_ok(), "update nonexistent should not error");
    }

    // ========================================================================
    // update_goal_progress_from_milestones_sync edge case
    // ========================================================================

    #[test]
    fn test_update_goal_progress_zero_weight_milestones() {
        let conn = setup_test_connection();
        let _pid = ensure_project(&conn);
        let goal_id = create_test_goal(&conn);

        create_milestone_sync(&conn, goal_id, "Zero A", Some(0)).expect("create should succeed");
        create_milestone_sync(&conn, goal_id, "Zero B", Some(0)).expect("create should succeed");

        // Complete one of them
        let ms = get_milestones_for_goal_sync(&conn, goal_id).expect("get should succeed");
        complete_milestone_sync(&conn, ms[0].id, None).expect("complete should succeed");

        // Progress should be 0 (total_weight == 0)
        let progress = update_goal_progress_from_milestones_sync(&conn, goal_id)
            .expect("update progress should succeed");
        assert_eq!(progress, 0);
    }
}
