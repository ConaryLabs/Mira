// db/milestones.rs
// Milestone database operations

use rusqlite::{Connection, OptionalExtension, params};

use super::types::Milestone;

/// Parse Milestone from a rusqlite Row with standard column order:
/// (id, goal_id, title, completed, weight)
pub fn parse_milestone_row(row: &rusqlite::Row) -> rusqlite::Result<Milestone> {
    Ok(Milestone {
        id: row.get(0)?,
        goal_id: row.get(1)?,
        title: row.get(2)?,
        completed: row.get::<_, i32>(3)? != 0,
        weight: row.get(4)?,
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
    let sql = "SELECT id, goal_id, title, completed, weight
               FROM milestones WHERE goal_id = ?
               ORDER BY id ASC";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([goal_id], parse_milestone_row)?;
    rows.collect()
}

/// Get a milestone by ID
pub fn get_milestone_by_id_sync(conn: &Connection, id: i64) -> rusqlite::Result<Option<Milestone>> {
    let sql = "SELECT id, goal_id, title, completed, weight
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
pub fn complete_milestone_sync(conn: &Connection, id: i64) -> rusqlite::Result<Option<i64>> {
    conn.execute("UPDATE milestones SET completed = 1 WHERE id = ?", [id])?;
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
        "UPDATE goals SET progress_percent = ? WHERE id = ?",
        params![progress, goal_id],
    )?;
    Ok(progress)
}

