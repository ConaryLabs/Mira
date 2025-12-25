//! Anti-amnesia: Load rejected approaches and past decisions

use sqlx::sqlite::SqlitePool;
use tracing::debug;

use super::types::{RejectedApproach, PastDecision};

/// Load rejected approaches relevant to the query (anti-amnesia)
/// Searches by keyword matching in problem_context, approach, and related_topics
pub async fn load_rejected_approaches(
    db: &SqlitePool,
    query: &str,
    limit: usize,
) -> Vec<RejectedApproach> {
    // Extract keywords from query for matching
    let keywords: Vec<&str> = query
        .split_whitespace()
        .filter(|w| w.len() > 3)  // Skip short words
        .take(5)
        .collect();

    if keywords.is_empty() {
        return Vec::new();
    }

    // Build LIKE conditions for each keyword
    let like_conditions: Vec<String> = keywords
        .iter()
        .map(|_| "(problem_context LIKE ? OR approach LIKE ? OR related_topics LIKE ?)".to_string())
        .collect();

    let query_str = format!(
        r#"
        SELECT problem_context, approach, rejection_reason
        FROM rejected_approaches
        WHERE {}
        ORDER BY created_at DESC
        LIMIT ?
        "#,
        like_conditions.join(" OR ")
    );

    // Build the query with bound parameters
    let mut sql_query = sqlx::query_as::<_, (String, String, String)>(&query_str);

    for kw in &keywords {
        let pattern = format!("%{}%", kw);
        sql_query = sql_query.bind(pattern.clone()).bind(pattern.clone()).bind(pattern);
    }
    sql_query = sql_query.bind(limit as i64);

    match sql_query.fetch_all(db).await {
        Ok(rows) => rows
            .into_iter()
            .map(|(problem_context, approach, rejection_reason)| {
                RejectedApproach {
                    problem_context,
                    approach,
                    rejection_reason,
                }
            })
            .collect(),
        Err(e) => {
            debug!("Failed to load rejected approaches: {}", e);
            Vec::new()
        }
    }
}

/// Load past decisions relevant to the query (anti-amnesia)
/// Uses semantic search when available, falls back to keyword matching
pub async fn load_past_decisions(
    db: &SqlitePool,
    query: &str,
    limit: usize,
) -> Vec<PastDecision> {
    // Try keyword-based search first (simpler, always works)
    let keywords: Vec<&str> = query
        .split_whitespace()
        .filter(|w| w.len() > 3)
        .take(5)
        .collect();

    if keywords.is_empty() {
        return Vec::new();
    }

    // Build LIKE conditions
    let like_conditions: Vec<String> = keywords
        .iter()
        .map(|_| "(key LIKE ? OR decision LIKE ? OR context LIKE ?)".to_string())
        .collect();

    let query_str = format!(
        r#"
        SELECT key, decision, context
        FROM decisions
        WHERE {}
        ORDER BY created_at DESC
        LIMIT ?
        "#,
        like_conditions.join(" OR ")
    );

    let mut sql_query = sqlx::query_as::<_, (String, String, Option<String>)>(&query_str);

    for kw in &keywords {
        let pattern = format!("%{}%", kw);
        sql_query = sql_query.bind(pattern.clone()).bind(pattern.clone()).bind(pattern);
    }
    sql_query = sql_query.bind(limit as i64);

    match sql_query.fetch_all(db).await {
        Ok(rows) => rows
            .into_iter()
            .map(|(key, decision, context)| {
                PastDecision {
                    key,
                    decision,
                    context,
                }
            })
            .collect(),
        Err(e) => {
            debug!("Failed to load past decisions: {}", e);
            Vec::new()
        }
    }
}
