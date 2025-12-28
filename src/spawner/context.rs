//! Context snapshot builder for Claude Code sessions
//!
//! Builds a ContextSnapshot from Mira's database to hand off strategic
//! context when spawning a new Claude Code session.

use anyhow::Result;
use sqlx::SqlitePool;

use super::types::{ContextSnapshot, CorrectionSummary, GoalSummary};

/// Build a context snapshot for a task
///
/// Gathers relevant context from Mira's database:
/// - Active goals with progress
/// - Relevant decisions
/// - Corrections to apply
/// - Rejected approaches (anti-patterns)
pub async fn build_context_snapshot(
    db: &SqlitePool,
    task_overview: &str,
    project_id: Option<i64>,
    key_files: Vec<String>,
) -> Result<ContextSnapshot> {
    // 1. Get active goals
    let active_goals = get_active_goals(db, project_id).await?;

    // 2. Get relevant decisions (based on task keywords)
    let relevant_decisions = get_relevant_decisions(db, task_overview, project_id).await?;

    // 3. Get corrections
    let corrections = get_corrections(db, project_id).await?;

    // 4. Get rejected approaches as anti-patterns
    let anti_patterns = get_anti_patterns(db, task_overview, project_id).await?;

    Ok(ContextSnapshot {
        task_overview: task_overview.to_string(),
        relevant_decisions,
        active_goals,
        corrections,
        key_files,
        anti_patterns,
    })
}

/// Get active goals for the project
async fn get_active_goals(db: &SqlitePool, project_id: Option<i64>) -> Result<Vec<GoalSummary>> {
    let rows: Vec<(String, String, String, i32)> = sqlx::query_as(
        r#"
        SELECT id, title, status, progress_percent
        FROM goals
        WHERE status IN ('planning', 'in_progress', 'blocked')
          AND (project_id IS NULL OR project_id = $1)
        ORDER BY
            CASE priority
                WHEN 'critical' THEN 0
                WHEN 'high' THEN 1
                WHEN 'medium' THEN 2
                WHEN 'low' THEN 3
                ELSE 4
            END,
            created_at DESC
        LIMIT 5
        "#,
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(id, title, status, progress)| GoalSummary {
            id,
            title,
            status,
            progress_percent: progress,
        })
        .collect())
}

/// Get relevant decisions based on task keywords
async fn get_relevant_decisions(
    db: &SqlitePool,
    task: &str,
    project_id: Option<i64>,
) -> Result<Vec<String>> {
    // Extract key terms from task for matching
    let terms: Vec<&str> = task
        .split_whitespace()
        .filter(|w| w.len() > 3)
        .take(5)
        .collect();

    if terms.is_empty() {
        // Return recent decisions if no useful terms
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT value FROM memory_facts
            WHERE fact_type = 'decision'
              AND (project_id IS NULL OR project_id = $1)
              AND key NOT LIKE 'compaction-%'
            ORDER BY updated_at DESC
            LIMIT 5
            "#,
        )
        .bind(project_id)
        .fetch_all(db)
        .await?;

        return Ok(rows.into_iter().map(|(v,)| v).collect());
    }

    // Search for matching decisions
    let pattern = format!("%{}%", terms.join("%"));
    let rows: Vec<(String,)> = sqlx::query_as(
        r#"
        SELECT value FROM memory_facts
        WHERE fact_type = 'decision'
          AND (project_id IS NULL OR project_id = $1)
          AND (value LIKE $2 OR key LIKE $2)
          AND key NOT LIKE 'compaction-%'
        ORDER BY updated_at DESC
        LIMIT 5
        "#,
    )
    .bind(project_id)
    .bind(&pattern)
    .fetch_all(db)
    .await?;

    Ok(rows.into_iter().map(|(v,)| v).collect())
}

/// Get active corrections
async fn get_corrections(db: &SqlitePool, project_id: Option<i64>) -> Result<Vec<CorrectionSummary>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        r#"
        SELECT what_was_wrong, what_is_right
        FROM corrections
        WHERE status = 'active'
          AND (project_id IS NULL OR project_id = $1)
        ORDER BY times_applied DESC, updated_at DESC
        LIMIT 5
        "#,
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(wrong, right)| CorrectionSummary {
            what_was_wrong: wrong,
            what_is_right: right,
        })
        .collect())
}

/// Get rejected approaches as anti-patterns
async fn get_anti_patterns(
    db: &SqlitePool,
    task: &str,
    project_id: Option<i64>,
) -> Result<Vec<String>> {
    // Get rejected approaches relevant to this task
    let terms: Vec<&str> = task
        .split_whitespace()
        .filter(|w| w.len() > 3)
        .take(3)
        .collect();

    let pattern = if terms.is_empty() {
        "%".to_string()
    } else {
        format!("%{}%", terms.join("%"))
    };

    let rows: Vec<(String, String)> = sqlx::query_as(
        r#"
        SELECT approach, rejection_reason
        FROM rejected_approaches
        WHERE (project_id IS NULL OR project_id = $1)
          AND (problem_context LIKE $2 OR approach LIKE $2)
        ORDER BY created_at DESC
        LIMIT 5
        "#,
    )
    .bind(project_id)
    .bind(&pattern)
    .fetch_all(db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(approach, reason)| format!("{} ({})", approach, reason))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_snapshot_format() {
        let snapshot = ContextSnapshot {
            task_overview: "Implement user auth".to_string(),
            relevant_decisions: vec!["Use JWT tokens".to_string()],
            active_goals: vec![GoalSummary {
                id: "g1".to_string(),
                title: "Auth System".to_string(),
                status: "in_progress".to_string(),
                progress_percent: 30,
            }],
            corrections: vec![CorrectionSummary {
                what_was_wrong: "Storing passwords in plain text".to_string(),
                what_is_right: "Use bcrypt for password hashing".to_string(),
            }],
            key_files: vec!["src/auth.rs".to_string()],
            anti_patterns: vec!["Rolling own crypto".to_string()],
        };

        let prompt = snapshot.to_system_prompt();
        assert!(prompt.contains("Implement user auth"));
        assert!(prompt.contains("Use JWT tokens"));
        assert!(prompt.contains("Auth System"));
        assert!(prompt.contains("30% complete"));
        assert!(prompt.contains("bcrypt"));
    }
}
