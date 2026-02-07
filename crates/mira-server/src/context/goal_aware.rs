// crates/mira-server/src/context/goal_aware.rs
// Goal-aware context injection

use crate::db::pool::DatabasePool;
use std::sync::Arc;

pub struct GoalAwareInjector {
    pool: Arc<DatabasePool>,
}

impl GoalAwareInjector {
    pub fn new(pool: Arc<DatabasePool>) -> Self {
        Self { pool }
    }

    /// Get active goal IDs scoped to the current project
    pub async fn get_active_goal_ids(&self, project_id: Option<i64>) -> Vec<i64> {
        match self
            .pool
            .interact(move |conn| {
                let ids = if let Some(pid) = project_id {
                    let mut stmt = conn.prepare(
                        "SELECT id FROM goals WHERE project_id = ? AND status NOT IN ('completed', 'abandoned') ORDER BY created_at DESC LIMIT 10"
                    )?;
                    stmt.query_map(rusqlite::params![pid], |row| row.get::<_, i64>(0))?
                        .filter_map(|r| r.ok())
                        .collect::<Vec<_>>()
                } else {
                    let mut stmt = conn.prepare(
                        "SELECT id FROM goals WHERE status NOT IN ('completed', 'abandoned') ORDER BY created_at DESC LIMIT 10"
                    )?;
                    stmt.query_map([], |row| row.get::<_, i64>(0))?
                        .filter_map(|r| r.ok())
                        .collect::<Vec<_>>()
                };
                Ok::<_, anyhow::Error>(ids)
            })
            .await
        {
            Ok(ids) => ids,
            Err(e) => {
                tracing::debug!("Failed to get active goals: {}", e);
                Vec::new()
            }
        }
    }

    /// Inject context about active goals and their milestones.
    /// Fetches goal details for the provided `goal_ids`.
    pub async fn inject_goal_context(&self, goal_ids: Vec<i64>) -> String {
        if goal_ids.is_empty() {
            return String::new();
        }

        // Fetch goals by their IDs directly
        let ids = goal_ids.clone();
        let goals = match self
            .pool
            .interact(move |conn| {
                let placeholders: String = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                let sql = format!(
                    "SELECT id, project_id, title, description, status, priority, progress_percent, created_at \
                     FROM goals WHERE id IN ({}) AND status NOT IN ('completed', 'abandoned') \
                     ORDER BY created_at DESC LIMIT 5",
                    placeholders
                );
                let mut stmt = conn.prepare(&sql)?;
                let params: Vec<Box<dyn rusqlite::types::ToSql>> =
                    ids.iter().map(|id| Box::new(*id) as Box<dyn rusqlite::types::ToSql>).collect();
                let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                    params.iter().map(|p| p.as_ref()).collect();
                let goals = stmt
                    .query_map(param_refs.as_slice(), crate::db::parse_goal_row)?
                    .filter_map(|r| r.ok())
                    .collect::<Vec<_>>();
                Ok::<_, anyhow::Error>(goals)
            })
            .await
        {
            Ok(goals) => goals,
            Err(_) => return String::new(),
        };

        if goals.is_empty() {
            return String::new();
        }

        // Batch-fetch milestones for all goal IDs in one query
        let gids = goals.iter().map(|g| g.id).collect::<Vec<_>>();
        let milestones_by_goal = self
            .pool
            .interact(move |conn| -> anyhow::Result<std::collections::HashMap<i64, (usize, usize)>> {
                if gids.is_empty() {
                    return Ok(std::collections::HashMap::new());
                }
                let placeholders: String = gids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                let sql = format!(
                    "SELECT goal_id, completed FROM milestones WHERE goal_id IN ({})",
                    placeholders
                );
                let mut stmt = conn.prepare(&sql)?;
                let params: Vec<Box<dyn rusqlite::types::ToSql>> =
                    gids.iter().map(|id| Box::new(*id) as Box<dyn rusqlite::types::ToSql>).collect();
                let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                    params.iter().map(|p| p.as_ref()).collect();
                let rows = stmt.query_map(param_refs.as_slice(), |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?))
                })?;
                let mut map: std::collections::HashMap<i64, (usize, usize)> =
                    std::collections::HashMap::new();
                for row in rows {
                    let (gid, completed) = row?;
                    let entry = map.entry(gid).or_insert((0, 0));
                    entry.1 += 1; // total
                    if completed {
                        entry.0 += 1; // completed
                    }
                }
                Ok(map)
            })
            .await
            .unwrap_or_default();

        let mut context = String::new();
        context.push_str("Active goals:\n");

        for goal in &goals {
            let milestone_summary = if let Some((completed, total)) = milestones_by_goal.get(&goal.id) {
                if *total > 0 {
                    format!(" - {}/{} milestones", completed, total)
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            context.push_str(&format!(
                "  - Goal: {} ({}%){}\n",
                goal.title, goal.progress_percent, milestone_summary
            ));
        }

        context.trim_end().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn create_test_injector() -> GoalAwareInjector {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());
        GoalAwareInjector::new(pool)
    }

    #[tokio::test]
    async fn test_empty_goals() {
        let injector = create_test_injector().await;

        let ids = injector.get_active_goal_ids(None).await;
        assert!(ids.is_empty());

        let context = injector.inject_goal_context(vec![]).await;
        assert!(context.is_empty());
    }

    #[tokio::test]
    async fn test_with_goals() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        // Create a project first (via pool)
        let project_id = pool
            .interact(|conn| {
                crate::db::get_or_create_project_sync(conn, "/test/project", Some("test"))
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
            .unwrap()
            .0;

        // Create some goals
        pool.interact(move |conn| {
            conn.execute(
                "INSERT INTO goals (project_id, title, description, status, priority, progress_percent) VALUES (?, ?, ?, ?, ?, ?)",
                rusqlite::params![project_id, "Launch v1.0", Some("First stable release"), "in_progress", "high", 50],
            )?;
            conn.execute(
                "INSERT INTO goals (project_id, title, description, status, priority, progress_percent) VALUES (?, ?, ?, ?, ?, ?)",
                rusqlite::params![project_id, "Add documentation", Option::<String>::None, "planning", "medium", 0],
            )?;
            Ok::<_, anyhow::Error>(())
        }).await.unwrap();

        let injector = GoalAwareInjector::new(pool);

        let ids = injector.get_active_goal_ids(Some(project_id)).await;
        assert_eq!(ids.len(), 2);

        let context = injector.inject_goal_context(ids).await;
        assert!(context.contains("Launch v1.0"));
        assert!(context.contains("50%"));
        assert!(context.contains("Add documentation"));
    }

    #[tokio::test]
    async fn test_with_milestones() {
        let pool = Arc::new(DatabasePool::open_in_memory().await.unwrap());

        let project_id = pool
            .interact(|conn| {
                crate::db::get_or_create_project_sync(conn, "/test/project", Some("test"))
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
            .unwrap()
            .0;

        // Create a goal with milestones
        pool.interact(move |conn| {
            conn.execute(
                "INSERT INTO goals (project_id, title, description, status, priority, progress_percent) VALUES (?, ?, ?, ?, ?, ?)",
                rusqlite::params![project_id, "Feature X", Some("New feature"), "in_progress", "high", 33],
            )?;
            let goal_id = conn.last_insert_rowid();
            // Add milestones
            conn.execute(
                "INSERT INTO milestones (goal_id, title, completed, weight) VALUES (?, ?, ?, ?)",
                rusqlite::params![goal_id, "Design", 1, 1],
            )?;
            conn.execute(
                "INSERT INTO milestones (goal_id, title, completed, weight) VALUES (?, ?, ?, ?)",
                rusqlite::params![goal_id, "Implement", 0, 2],
            )?;
            Ok::<_, anyhow::Error>(())
        }).await.unwrap();

        let injector = GoalAwareInjector::new(pool);

        let context = injector.inject_goal_context(vec![1]).await;
        assert!(context.contains("Feature X"));
        assert!(context.contains("1/2 milestones"));
    }
}
