// db/tasks.rs
// Task and goal database operations

use anyhow::Result;
use rusqlite::params;

use super::types::{Task, Goal};
use super::Database;

/// Parse Task from a rusqlite Row with standard column order:
/// (id, project_id, goal_id, title, description, status, priority, created_at)
pub fn parse_task_row(row: &rusqlite::Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        project_id: row.get(1)?,
        goal_id: row.get(2)?,
        title: row.get(3)?,
        description: row.get(4)?,
        status: row.get(5)?,
        priority: row.get(6)?,
        created_at: row.get(7)?,
    })
}

/// Parse Goal from a rusqlite Row with standard column order:
/// (id, project_id, title, description, status, priority, progress_percent, created_at)
pub fn parse_goal_row(row: &rusqlite::Row) -> rusqlite::Result<Goal> {
    Ok(Goal {
        id: row.get(0)?,
        project_id: row.get(1)?,
        title: row.get(2)?,
        description: row.get(3)?,
        status: row.get(4)?,
        priority: row.get(5)?,
        progress_percent: row.get(6)?,
        created_at: row.get(7)?,
    })
}

impl Database {
    /// Get pending tasks for a project (status != 'completed')
    pub fn get_pending_tasks(&self, project_id: Option<i64>, limit: usize) -> Result<Vec<Task>> {
        let conn = self.conn();
        let sql = "SELECT id, project_id, goal_id, title, description, status, priority, created_at
                   FROM tasks
                   WHERE (project_id = ? OR project_id IS NULL) AND status != 'completed'
                   ORDER BY created_at DESC
                   LIMIT ?";
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![project_id, limit as i64], parse_task_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get active goals for a project (status NOT IN ('completed', 'abandoned'))
    pub fn get_active_goals(&self, project_id: Option<i64>, limit: usize) -> Result<Vec<Goal>> {
        let conn = self.conn();
        let sql = "SELECT id, project_id, title, description, status, priority, progress_percent, created_at
                   FROM goals
                   WHERE (project_id = ? OR project_id IS NULL) AND status NOT IN ('completed', 'abandoned')
                   ORDER BY created_at DESC
                   LIMIT ?";
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![project_id, limit as i64], parse_goal_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get recent tasks for a project (any status)
    pub fn get_recent_tasks(&self, project_id: Option<i64>, limit: usize) -> Result<Vec<Task>> {
        let conn = self.conn();
        let sql = "SELECT id, project_id, goal_id, title, description, status, priority, created_at
                   FROM tasks
                   WHERE project_id = ? OR project_id IS NULL
                   ORDER BY created_at DESC
                   LIMIT ?";
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![project_id, limit as i64], parse_task_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}
