// db/tasks.rs
// Task and goal database operations

use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use rusqlite::Connection;
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

// Sync functions for pool.interact() usage

/// Get pending tasks (sync version for pool.interact)
pub fn get_pending_tasks_sync(conn: &Connection, project_id: Option<i64>, limit: usize) -> Result<Vec<Task>> {
    let sql = "SELECT id, project_id, goal_id, title, description, status, priority, created_at
               FROM tasks
               WHERE (project_id = ? OR project_id IS NULL) AND status != 'completed'
               ORDER BY created_at DESC, id DESC
               LIMIT ?";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![project_id, limit as i64], parse_task_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Get a task by ID (sync version for pool.interact)
pub fn get_task_by_id_sync(conn: &Connection, id: i64) -> Result<Option<Task>> {
    let sql = "SELECT id, project_id, goal_id, title, description, status, priority, created_at
               FROM tasks WHERE id = ?";
    conn.query_row(sql, [id], parse_task_row)
        .optional()
        .map_err(Into::into)
}

/// Get active goals (sync version for pool.interact)
pub fn get_active_goals_sync(conn: &Connection, project_id: Option<i64>, limit: usize) -> Result<Vec<Goal>> {
    let sql = "SELECT id, project_id, title, description, status, priority, progress_percent, created_at
               FROM goals
               WHERE (project_id = ? OR project_id IS NULL) AND status NOT IN ('completed', 'abandoned')
               ORDER BY created_at DESC, id DESC
               LIMIT ?";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![project_id, limit as i64], parse_goal_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Create a new task (sync version for pool.interact)
pub fn create_task_sync(
    conn: &Connection,
    project_id: Option<i64>,
    goal_id: Option<i64>,
    title: &str,
    description: Option<&str>,
    status: Option<&str>,
    priority: Option<&str>,
) -> rusqlite::Result<i64> {
    let status = status.unwrap_or("pending");
    let priority = priority.unwrap_or("medium");
    conn.execute(
        "INSERT INTO tasks (project_id, goal_id, title, description, status, priority) VALUES (?, ?, ?, ?, ?, ?)",
        params![project_id, goal_id, title, description, status, priority],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Get tasks with optional status filter (sync version for pool.interact)
pub fn get_tasks_sync(conn: &Connection, project_id: Option<i64>, status_filter: Option<&str>) -> rusqlite::Result<Vec<Task>> {
    let (negate, status_value) = match status_filter {
        Some(s) if s.starts_with('!') => (true, Some(&s[1..])),
        Some(s) => (false, Some(s)),
        None => (false, None),
    };

    let sql = match (status_value, negate) {
        (Some(_), true) => "SELECT id, project_id, goal_id, title, description, status, priority, created_at
                           FROM tasks WHERE (project_id = ? OR project_id IS NULL) AND status != ?
                           ORDER BY created_at DESC, id DESC LIMIT 100",
        (Some(_), false) => "SELECT id, project_id, goal_id, title, description, status, priority, created_at
                            FROM tasks WHERE (project_id = ? OR project_id IS NULL) AND status = ?
                            ORDER BY created_at DESC, id DESC LIMIT 100",
        (None, _) => "SELECT id, project_id, goal_id, title, description, status, priority, created_at
                     FROM tasks WHERE (project_id = ? OR project_id IS NULL)
                     ORDER BY created_at DESC, id DESC LIMIT 100",
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = match status_value {
        Some(status) => stmt.query_map(params![project_id, status], parse_task_row)?,
        None => stmt.query_map(params![project_id], parse_task_row)?,
    };
    rows.collect()
}

/// Update a task (sync version for pool.interact)
pub fn update_task_sync(
    conn: &Connection,
    id: i64,
    title: Option<&str>,
    status: Option<&str>,
    priority: Option<&str>,
) -> rusqlite::Result<()> {
    if let Some(title) = title {
        conn.execute("UPDATE tasks SET title = ? WHERE id = ?", params![title, id])?;
    }
    if let Some(status) = status {
        conn.execute("UPDATE tasks SET status = ? WHERE id = ?", params![status, id])?;
    }
    if let Some(priority) = priority {
        conn.execute("UPDATE tasks SET priority = ? WHERE id = ?", params![priority, id])?;
    }
    Ok(())
}

/// Delete a task (sync version for pool.interact)
pub fn delete_task_sync(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM tasks WHERE id = ?", [id])?;
    Ok(())
}

/// Get a goal by ID (sync version for pool.interact)
pub fn get_goal_by_id_sync(conn: &Connection, id: i64) -> Result<Option<Goal>> {
    let sql = "SELECT id, project_id, title, description, status, priority, progress_percent, created_at
               FROM goals WHERE id = ?";
    conn.query_row(sql, [id], parse_goal_row)
        .optional()
        .map_err(Into::into)
}

/// Create a new goal (sync version for pool.interact)
pub fn create_goal_sync(
    conn: &Connection,
    project_id: Option<i64>,
    title: &str,
    description: Option<&str>,
    status: Option<&str>,
    priority: Option<&str>,
    progress_percent: Option<i64>,
) -> rusqlite::Result<i64> {
    let status = status.unwrap_or("planning");
    let priority = priority.unwrap_or("medium");
    let progress_percent = progress_percent.unwrap_or(0);
    conn.execute(
        "INSERT INTO goals (project_id, title, description, status, priority, progress_percent) VALUES (?, ?, ?, ?, ?, ?)",
        params![project_id, title, description, status, priority, progress_percent],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Get goals with optional status filter (sync version for pool.interact)
pub fn get_goals_sync(conn: &Connection, project_id: Option<i64>, status_filter: Option<&str>) -> rusqlite::Result<Vec<Goal>> {
    let (negate, status_value) = match status_filter {
        Some(s) if s.starts_with('!') => (true, Some(&s[1..])),
        Some(s) => (false, Some(s)),
        None => (false, None),
    };

    let sql = match (status_value, negate) {
        (Some(_), true) => "SELECT id, project_id, title, description, status, priority, progress_percent, created_at
                           FROM goals WHERE (project_id = ? OR project_id IS NULL) AND status != ?
                           ORDER BY created_at DESC, id DESC LIMIT 100",
        (Some(_), false) => "SELECT id, project_id, title, description, status, priority, progress_percent, created_at
                            FROM goals WHERE (project_id = ? OR project_id IS NULL) AND status = ?
                            ORDER BY created_at DESC, id DESC LIMIT 100",
        (None, _) => "SELECT id, project_id, title, description, status, priority, progress_percent, created_at
                     FROM goals WHERE (project_id = ? OR project_id IS NULL)
                     ORDER BY created_at DESC, id DESC LIMIT 100",
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = match status_value {
        Some(status) => stmt.query_map(params![project_id, status], parse_goal_row)?,
        None => stmt.query_map(params![project_id], parse_goal_row)?,
    };
    rows.collect()
}

/// Update a goal (sync version for pool.interact)
pub fn update_goal_sync(
    conn: &Connection,
    id: i64,
    title: Option<&str>,
    status: Option<&str>,
    priority: Option<&str>,
    progress: Option<i64>,
) -> rusqlite::Result<()> {
    if let Some(title) = title {
        conn.execute("UPDATE goals SET title = ? WHERE id = ?", params![title, id])?;
    }
    if let Some(status) = status {
        conn.execute("UPDATE goals SET status = ? WHERE id = ?", params![status, id])?;
    }
    if let Some(priority) = priority {
        conn.execute("UPDATE goals SET priority = ? WHERE id = ?", params![priority, id])?;
    }
    if let Some(progress) = progress {
        conn.execute("UPDATE goals SET progress_percent = ? WHERE id = ?", params![progress, id])?;
    }
    Ok(())
}

/// Delete a goal (sync version for pool.interact)
pub fn delete_goal_sync(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM goals WHERE id = ?", [id])?;
    Ok(())
}

impl Database {
    /// Get a single task by ID
    pub fn get_task_by_id(&self, id: i64) -> Result<Option<Task>> {
        let conn = self.conn();
        let sql = "SELECT id, project_id, goal_id, title, description, status, priority, created_at
                   FROM tasks WHERE id = ?";
        conn.query_row(sql, [id], parse_task_row)
            .optional()
            .map_err(Into::into)
    }

    /// Get a single goal by ID
    pub fn get_goal_by_id(&self, id: i64) -> Result<Option<Goal>> {
        let conn = self.conn();
        let sql = "SELECT id, project_id, title, description, status, priority, progress_percent, created_at
                   FROM goals WHERE id = ?";
        conn.query_row(sql, [id], parse_goal_row)
            .optional()
            .map_err(Into::into)
    }

    /// Get pending tasks for a project (status != 'completed')
    pub fn get_pending_tasks(&self, project_id: Option<i64>, limit: usize) -> Result<Vec<Task>> {
        let conn = self.conn();
        let sql = "SELECT id, project_id, goal_id, title, description, status, priority, created_at
                   FROM tasks
                   WHERE (project_id = ? OR project_id IS NULL) AND status != 'completed'
                   ORDER BY created_at DESC, id DESC
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
                   ORDER BY created_at DESC, id DESC
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
                   ORDER BY created_at DESC, id DESC
                   LIMIT ?";
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![project_id, limit as i64], parse_task_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Create a new task
    pub fn create_task(
        &self,
        project_id: Option<i64>,
        goal_id: Option<i64>,
        title: &str,
        description: Option<&str>,
        status: Option<&str>,
        priority: Option<&str>,
    ) -> Result<i64> {
        let conn = self.conn();
        let status = status.unwrap_or("pending");
        let priority = priority.unwrap_or("medium");
        
        conn.execute(
            "INSERT INTO tasks (project_id, goal_id, title, description, status, priority) VALUES (?, ?, ?, ?, ?, ?)",
            params![project_id, goal_id, title, description, status, priority],
        )?;
        
        Ok(conn.last_insert_rowid())
    }

    /// Get tasks with optional status filter
    /// Prefix with '!' to negate (e.g., "!completed" = status != 'completed')
    pub fn get_tasks(&self, project_id: Option<i64>, status_filter: Option<&str>) -> Result<Vec<Task>> {
        let conn = self.conn();

        // Parse negation prefix
        let (negate, status_value) = match status_filter {
            Some(s) if s.starts_with('!') => (true, Some(&s[1..])),
            Some(s) => (false, Some(s)),
            None => (false, None),
        };

        let sql = match (status_value, negate) {
            (Some(_), true) => "SELECT id, project_id, goal_id, title, description, status, priority, created_at
                               FROM tasks
                               WHERE (project_id = ? OR project_id IS NULL) AND status != ?
                               ORDER BY created_at DESC, id DESC
                               LIMIT 100",
            (Some(_), false) => "SELECT id, project_id, goal_id, title, description, status, priority, created_at
                                FROM tasks
                                WHERE (project_id = ? OR project_id IS NULL) AND status = ?
                                ORDER BY created_at DESC, id DESC
                                LIMIT 100",
            (None, _) => "SELECT id, project_id, goal_id, title, description, status, priority, created_at
                         FROM tasks
                         WHERE (project_id = ? OR project_id IS NULL)
                         ORDER BY created_at DESC, id DESC
                         LIMIT 100",
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = match status_value {
            Some(status) => stmt.query_map(params![project_id, status], parse_task_row)?,
            None => stmt.query_map(params![project_id], parse_task_row)?,
        };
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }



    /// Update a task
    /// Update a task
    pub fn update_task(
        &self,
        id: i64,
        title: Option<&str>,
        status: Option<&str>,
        priority: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn();
        
        if let Some(title) = title {
            conn.execute("UPDATE tasks SET title = ? WHERE id = ?", params![title, id])?;
        }
        if let Some(status) = status {
            conn.execute("UPDATE tasks SET status = ? WHERE id = ?", params![status, id])?;
        }
        if let Some(priority) = priority {
            conn.execute("UPDATE tasks SET priority = ? WHERE id = ?", params![priority, id])?;
        }
        
        Ok(())
    }



    /// Delete a task
    pub fn delete_task(&self, id: i64) -> Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM tasks WHERE id = ?", [id])?;
        Ok(())
    }

    /// Create a new goal
    pub fn create_goal(
        &self,
        project_id: Option<i64>,
        title: &str,
        description: Option<&str>,
        status: Option<&str>,
        priority: Option<&str>,
        progress_percent: Option<i64>,
    ) -> Result<i64> {
        let conn = self.conn();
        let status = status.unwrap_or("planning");
        let priority = priority.unwrap_or("medium");
        let progress_percent = progress_percent.unwrap_or(0);
        
        conn.execute(
            "INSERT INTO goals (project_id, title, description, status, priority, progress_percent) VALUES (?, ?, ?, ?, ?, ?)",
            params![project_id, title, description, status, priority, progress_percent],
        )?;
        
        Ok(conn.last_insert_rowid())
    }

    /// Get goals with optional status filter
    /// Prefix with '!' to negate (e.g., "!finished" = status != 'finished')
    pub fn get_goals(&self, project_id: Option<i64>, status_filter: Option<&str>) -> Result<Vec<Goal>> {
        let conn = self.conn();

        // Parse negation prefix
        let (negate, status_value) = match status_filter {
            Some(s) if s.starts_with('!') => (true, Some(&s[1..])),
            Some(s) => (false, Some(s)),
            None => (false, None),
        };

        let sql = match (status_value, negate) {
            (Some(_), true) => "SELECT id, project_id, title, description, status, priority, progress_percent, created_at
                               FROM goals
                               WHERE (project_id = ? OR project_id IS NULL) AND status != ?
                               ORDER BY created_at DESC, id DESC
                               LIMIT 100",
            (Some(_), false) => "SELECT id, project_id, title, description, status, priority, progress_percent, created_at
                                FROM goals
                                WHERE (project_id = ? OR project_id IS NULL) AND status = ?
                                ORDER BY created_at DESC, id DESC
                                LIMIT 100",
            (None, _) => "SELECT id, project_id, title, description, status, priority, progress_percent, created_at
                         FROM goals
                         WHERE (project_id = ? OR project_id IS NULL)
                         ORDER BY created_at DESC, id DESC
                         LIMIT 100",
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = match status_value {
            Some(status) => stmt.query_map(params![project_id, status], parse_goal_row)?,
            None => stmt.query_map(params![project_id], parse_goal_row)?,
        };
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }



    /// Update a goal
    /// Update a goal
    pub fn update_goal(
        &self,
        id: i64,
        title: Option<&str>,
        status: Option<&str>,
        priority: Option<&str>,
        progress_percent: Option<i64>,
    ) -> Result<()> {
        let conn = self.conn();
        
        if let Some(title) = title {
            conn.execute("UPDATE goals SET title = ? WHERE id = ?", params![title, id])?;
        }
        if let Some(status) = status {
            conn.execute("UPDATE goals SET status = ? WHERE id = ?", params![status, id])?;
        }
        if let Some(priority) = priority {
            conn.execute("UPDATE goals SET priority = ? WHERE id = ?", params![priority, id])?;
        }
        if let Some(progress_percent) = progress_percent {
            conn.execute("UPDATE goals SET progress_percent = ? WHERE id = ?", params![progress_percent, id])?;
        }
        
        Ok(())
    }



    /// Delete a goal
    pub fn delete_goal(&self, id: i64) -> Result<()> {
        let conn = self.conn();
        // First, orphan any tasks referencing this goal
        conn.execute("UPDATE tasks SET goal_id = NULL WHERE goal_id = ?", [id])?;
        // Also orphan any milestones
        conn.execute("UPDATE milestones SET goal_id = NULL WHERE goal_id = ?", [id])?;
        // Now delete the goal
        conn.execute("DELETE FROM goals WHERE id = ?", [id])?;
        Ok(())
    }
}
