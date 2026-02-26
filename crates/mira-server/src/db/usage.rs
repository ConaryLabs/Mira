// db/usage.rs
// Database operations for usage tracking (embeddings and LLM)

use anyhow::Result;
use rusqlite::{Connection, params};

// ============================================================================
// Embedding Usage
// ============================================================================

/// Insert an embedding usage record - sync version for pool.interact()
pub fn insert_embedding_usage_sync(
    conn: &Connection,
    record: &EmbeddingUsageRecord,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO embeddings_usage (
            provider, model, tokens, text_count, cost_estimate, project_id
        ) VALUES (?, ?, ?, ?, ?, ?)",
        params![
            record.provider,
            record.model,
            record.tokens as i64,
            record.text_count as i64,
            record.cost_estimate,
            record.project_id,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Embedding usage record
#[derive(Debug, Clone)]
pub struct EmbeddingUsageRecord {
    pub provider: String,
    pub model: String,
    pub tokens: u64,
    pub text_count: u64,
    pub cost_estimate: Option<f64>,
    pub project_id: Option<i64>,
}

// ============================================================================
// LLM Usage
// ============================================================================

/// Insert an LLM usage record - sync version for pool.interact()
pub fn insert_llm_usage_sync(conn: &Connection, record: &LlmUsageRecord) -> Result<i64> {
    conn.execute(
        "INSERT INTO llm_usage (
            provider, model, role, prompt_tokens, completion_tokens, total_tokens,
            cache_hit_tokens, cache_miss_tokens, cost_estimate, duration_ms,
            project_id, session_id
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            record.provider,
            record.model,
            record.role,
            record.prompt_tokens as i64,
            record.completion_tokens as i64,
            record.total_tokens as i64,
            record.cache_hit_tokens.unwrap_or(0) as i64,
            record.cache_miss_tokens.unwrap_or(0) as i64,
            record.cost_estimate,
            record.duration_ms.map(|d| d as i64),
            record.project_id,
            record.session_id,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// LLM usage record for cost/token tracking
#[derive(Debug, Clone)]
pub struct LlmUsageRecord {
    pub provider: String,
    pub model: String,
    pub role: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub cache_hit_tokens: Option<u32>,
    pub cache_miss_tokens: Option<u32>,
    pub cost_estimate: Option<f64>,
    pub duration_ms: Option<u64>,
    pub project_id: Option<i64>,
    pub session_id: Option<String>,
}

impl LlmUsageRecord {
    /// Create a new LLM usage record with cost calculated from pricing
    pub fn new(
        provider: crate::llm::Provider,
        model: &str,
        role: &str,
        usage: &crate::llm::Usage,
        duration_ms: u64,
        project_id: Option<i64>,
        session_id: Option<String>,
    ) -> Self {
        // Calculate cost using pricing module
        let cost_estimate = crate::llm::get_pricing(provider, model).map(|pricing| {
            pricing.calculate_cost(
                usage.prompt_tokens,
                usage.completion_tokens,
                usage.prompt_cache_hit_tokens,
            )
        });

        Self {
            provider: provider.to_string(),
            model: model.to_string(),
            role: role.to_string(),
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
            cache_hit_tokens: usage.prompt_cache_hit_tokens,
            cache_miss_tokens: usage.prompt_cache_miss_tokens,
            cost_estimate,
            duration_ms: Some(duration_ms),
            project_id,
            session_id,
        }
    }
}

// ============================================================================
// Usage Statistics Queries
// ============================================================================

/// Usage statistics grouped by a dimension
#[derive(Debug, Clone)]
pub struct UsageStats {
    pub group_key: String,
    pub total_requests: u64,
    pub total_tokens: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_cost: f64,
    pub avg_duration_ms: Option<f64>,
}

/// Query LLM usage stats grouped by a dimension
pub fn query_llm_usage_stats(
    conn: &Connection,
    group_by: &str,
    project_id: Option<i64>,
    since_days: Option<u32>,
) -> Result<Vec<UsageStats>> {
    let group_column = match group_by {
        "role" => "role",
        "provider" => "provider",
        "model" => "model",
        "provider_model" => "provider || '/' || model",
        _ => "role", // default
    };

    let mut sql = format!(
        "SELECT
            {group_column} as group_key,
            COUNT(*) as total_requests,
            SUM(total_tokens) as total_tokens,
            SUM(prompt_tokens) as prompt_tokens,
            SUM(completion_tokens) as completion_tokens,
            COALESCE(SUM(cost_estimate), 0) as total_cost,
            AVG(duration_ms) as avg_duration_ms
        FROM llm_usage
        WHERE 1=1"
    );

    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(pid) = project_id {
        sql.push_str(" AND project_id = ?");
        params_vec.push(Box::new(pid));
    }

    if let Some(days) = since_days {
        sql.push_str(" AND created_at >= datetime('now', ? || ' days')");
        params_vec.push(Box::new(-(days as i64)));
    }

    sql.push_str(&format!(
        " GROUP BY {group_column} ORDER BY total_cost DESC"
    ));

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(UsageStats {
            group_key: row.get(0)?,
            total_requests: row.get::<_, i64>(1)? as u64,
            total_tokens: row.get::<_, i64>(2)? as u64,
            prompt_tokens: row.get::<_, i64>(3)? as u64,
            completion_tokens: row.get::<_, i64>(4)? as u64,
            total_cost: row.get(5)?,
            avg_duration_ms: row.get(6)?,
        })
    })?;

    let mut stats = Vec::new();
    for row in rows {
        stats.push(row?);
    }
    Ok(stats)
}

// ============================================================================
// Embedding Usage Statistics Queries
// ============================================================================

/// Embedding usage statistics grouped by a dimension
#[derive(Debug, Clone)]
pub struct EmbeddingUsageStats {
    pub group_key: String,
    pub total_requests: u64,
    pub total_tokens: u64,
    pub total_texts: u64,
    pub total_cost: f64,
}

/// Query embedding usage stats grouped by a dimension
pub fn query_embedding_usage_stats(
    conn: &Connection,
    group_by: &str,
    project_id: Option<i64>,
    since_days: Option<u32>,
) -> Result<Vec<EmbeddingUsageStats>> {
    let group_column = match group_by {
        "provider" => "provider",
        "model" => "model",
        "provider_model" => "provider || '/' || model",
        _ => "provider",
    };

    let mut sql = format!(
        "SELECT
            {group_column} as group_key,
            COUNT(*) as total_requests,
            COALESCE(SUM(tokens), 0) as total_tokens,
            COALESCE(SUM(text_count), 0) as total_texts,
            COALESCE(SUM(cost_estimate), 0) as total_cost
        FROM embeddings_usage
        WHERE 1=1"
    );

    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(pid) = project_id {
        sql.push_str(" AND project_id = ?");
        params_vec.push(Box::new(pid));
    }

    if let Some(days) = since_days {
        sql.push_str(" AND created_at >= datetime('now', ? || ' days')");
        params_vec.push(Box::new(-(days as i64)));
    }

    sql.push_str(&format!(
        " GROUP BY {group_column} ORDER BY total_cost DESC"
    ));

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(EmbeddingUsageStats {
            group_key: row.get(0)?,
            total_requests: row.get::<_, i64>(1)? as u64,
            total_tokens: row.get::<_, i64>(2)? as u64,
            total_texts: row.get::<_, i64>(3)? as u64,
            total_cost: row.get(4)?,
        })
    })?;

    let mut stats = Vec::new();
    for row in rows {
        stats.push(row?);
    }
    Ok(stats)
}

/// Get total embedding usage summary
pub fn get_embedding_usage_summary(
    conn: &Connection,
    project_id: Option<i64>,
    since_days: Option<u32>,
) -> Result<EmbeddingUsageStats> {
    let mut sql = String::from(
        "SELECT
            'total' as group_key,
            COUNT(*) as total_requests,
            COALESCE(SUM(tokens), 0) as total_tokens,
            COALESCE(SUM(text_count), 0) as total_texts,
            COALESCE(SUM(cost_estimate), 0) as total_cost
        FROM embeddings_usage
        WHERE 1=1",
    );

    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(pid) = project_id {
        sql.push_str(" AND project_id = ?");
        params_vec.push(Box::new(pid));
    }

    if let Some(days) = since_days {
        sql.push_str(" AND created_at >= datetime('now', ? || ' days')");
        params_vec.push(Box::new(-(days as i64)));
    }

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let stats = stmt.query_row(params_refs.as_slice(), |row| {
        Ok(EmbeddingUsageStats {
            group_key: row.get(0)?,
            total_requests: row.get::<_, i64>(1)? as u64,
            total_tokens: row.get::<_, i64>(2)? as u64,
            total_texts: row.get::<_, i64>(3)? as u64,
            total_cost: row.get(4)?,
        })
    })?;

    Ok(stats)
}

/// Get total LLM usage summary
pub fn get_llm_usage_summary(
    conn: &Connection,
    project_id: Option<i64>,
    since_days: Option<u32>,
) -> Result<UsageStats> {
    let mut sql = String::from(
        "SELECT
            'total' as group_key,
            COUNT(*) as total_requests,
            COALESCE(SUM(total_tokens), 0) as total_tokens,
            COALESCE(SUM(prompt_tokens), 0) as prompt_tokens,
            COALESCE(SUM(completion_tokens), 0) as completion_tokens,
            COALESCE(SUM(cost_estimate), 0) as total_cost,
            AVG(duration_ms) as avg_duration_ms
        FROM llm_usage
        WHERE 1=1",
    );

    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(pid) = project_id {
        sql.push_str(" AND project_id = ?");
        params_vec.push(Box::new(pid));
    }

    if let Some(days) = since_days {
        sql.push_str(" AND created_at >= datetime('now', ? || ' days')");
        params_vec.push(Box::new(-(days as i64)));
    }

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let stats = stmt.query_row(params_refs.as_slice(), |row| {
        Ok(UsageStats {
            group_key: row.get(0)?,
            total_requests: row.get::<_, i64>(1)? as u64,
            total_tokens: row.get::<_, i64>(2)? as u64,
            prompt_tokens: row.get::<_, i64>(3)? as u64,
            completion_tokens: row.get::<_, i64>(4)? as u64,
            total_cost: row.get(5)?,
            avg_duration_ms: row.get(6)?,
        })
    })?;

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_usage_db() -> rusqlite::Connection {
        let conn = crate::db::test_support::setup_test_connection();
        crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();
        conn
    }

    // ========================================================================
    // Happy-path: embedding usage insert and query
    // ========================================================================

    fn make_embedding_record(project_id: Option<i64>) -> EmbeddingUsageRecord {
        EmbeddingUsageRecord {
            provider: "openai".to_string(),
            model: "text-embedding-3-small".to_string(),
            tokens: 500,
            text_count: 10,
            cost_estimate: Some(0.002),
            project_id,
        }
    }

    fn make_llm_record(role: &str, project_id: Option<i64>) -> LlmUsageRecord {
        LlmUsageRecord {
            provider: "deepseek".to_string(),
            model: "deepseek-chat".to_string(),
            role: role.to_string(),
            prompt_tokens: 200,
            completion_tokens: 100,
            total_tokens: 300,
            cache_hit_tokens: Some(50),
            cache_miss_tokens: Some(150),
            cost_estimate: Some(0.005),
            duration_ms: Some(1200),
            project_id,
            session_id: Some("session-1".to_string()),
        }
    }

    #[test]
    fn test_insert_embedding_usage_returns_id() {
        let conn = setup_usage_db();
        let record = make_embedding_record(Some(1));

        let id = insert_embedding_usage_sync(&conn, &record).expect("insert should succeed");
        assert!(id > 0);
    }

    #[test]
    fn test_insert_llm_usage_returns_id() {
        let conn = setup_usage_db();
        let record = make_llm_record("pondering", Some(1));

        let id = insert_llm_usage_sync(&conn, &record).expect("insert should succeed");
        assert!(id > 0);
    }

    #[test]
    fn test_query_llm_usage_stats_grouped_by_role() {
        let conn = setup_usage_db();

        insert_llm_usage_sync(&conn, &make_llm_record("pondering", Some(1))).unwrap();
        insert_llm_usage_sync(&conn, &make_llm_record("pondering", Some(1))).unwrap();
        insert_llm_usage_sync(&conn, &make_llm_record("summary", Some(1))).unwrap();

        let stats = query_llm_usage_stats(&conn, "role", None, None).unwrap();
        assert_eq!(stats.len(), 2);

        let pondering = stats.iter().find(|s| s.group_key == "pondering").unwrap();
        assert_eq!(pondering.total_requests, 2);
        assert_eq!(pondering.total_tokens, 600);
        assert_eq!(pondering.prompt_tokens, 400);
        assert_eq!(pondering.completion_tokens, 200);
    }

    #[test]
    fn test_query_llm_usage_stats_grouped_by_provider() {
        let conn = setup_usage_db();

        insert_llm_usage_sync(&conn, &make_llm_record("pondering", Some(1))).unwrap();

        let stats = query_llm_usage_stats(&conn, "provider", None, None).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].group_key, "deepseek");
    }

    #[test]
    fn test_query_llm_usage_stats_filtered_by_project() {
        let conn = setup_usage_db();
        crate::db::get_or_create_project_sync(&conn, "/other/path", Some("other")).unwrap();

        insert_llm_usage_sync(&conn, &make_llm_record("pondering", Some(1))).unwrap();
        insert_llm_usage_sync(&conn, &make_llm_record("pondering", Some(2))).unwrap();

        let stats = query_llm_usage_stats(&conn, "role", Some(1), None).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].total_requests, 1);
    }

    #[test]
    fn test_get_llm_usage_summary_with_data() {
        let conn = setup_usage_db();

        insert_llm_usage_sync(&conn, &make_llm_record("pondering", Some(1))).unwrap();
        insert_llm_usage_sync(&conn, &make_llm_record("summary", Some(1))).unwrap();

        let summary = get_llm_usage_summary(&conn, None, None).unwrap();
        assert_eq!(summary.total_requests, 2);
        assert_eq!(summary.total_tokens, 600);
        assert_eq!(summary.prompt_tokens, 400);
        assert_eq!(summary.completion_tokens, 200);
        assert!((summary.total_cost - 0.01).abs() < 0.001);
        assert!(summary.avg_duration_ms.is_some());
    }

    #[test]
    fn test_query_embedding_usage_stats_grouped_by_model() {
        let conn = setup_usage_db();

        insert_embedding_usage_sync(&conn, &make_embedding_record(Some(1))).unwrap();
        insert_embedding_usage_sync(&conn, &make_embedding_record(Some(1))).unwrap();

        let stats = query_embedding_usage_stats(&conn, "model", None, None).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].group_key, "text-embedding-3-small");
        assert_eq!(stats[0].total_requests, 2);
        assert_eq!(stats[0].total_tokens, 1000);
        assert_eq!(stats[0].total_texts, 20);
    }

    #[test]
    fn test_get_embedding_usage_summary_with_data() {
        let conn = setup_usage_db();

        insert_embedding_usage_sync(&conn, &make_embedding_record(Some(1))).unwrap();

        let summary = get_embedding_usage_summary(&conn, None, None).unwrap();
        assert_eq!(summary.total_requests, 1);
        assert_eq!(summary.total_tokens, 500);
        assert_eq!(summary.total_texts, 10);
        assert!((summary.total_cost - 0.002).abs() < 0.001);
    }

    #[test]
    fn test_query_embedding_usage_stats_filtered_by_project() {
        let conn = setup_usage_db();
        crate::db::get_or_create_project_sync(&conn, "/other/path", Some("other")).unwrap();

        insert_embedding_usage_sync(&conn, &make_embedding_record(Some(1))).unwrap();
        insert_embedding_usage_sync(&conn, &make_embedding_record(Some(2))).unwrap();

        let stats = query_embedding_usage_stats(&conn, "provider", Some(1), None).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].total_requests, 1);
    }

    // ========================================================================
    // query_llm_usage_stats: unknown/invalid group_by
    // ========================================================================

    #[test]
    fn test_query_llm_usage_stats_unknown_group_by_defaults_to_role() {
        let conn = setup_usage_db();
        insert_llm_usage_sync(&conn, &make_llm_record("pondering", Some(1))).unwrap();
        insert_llm_usage_sync(&conn, &make_llm_record("summary", Some(1))).unwrap();

        let stats_unknown = query_llm_usage_stats(&conn, "nonexistent_column", None, None)
            .expect("query should succeed with unknown group_by");
        let stats_role = query_llm_usage_stats(&conn, "role", None, None)
            .expect("query with explicit role group_by should succeed");

        // Unknown group_by should default to role -- same grouping, same results
        assert_eq!(
            stats_unknown.len(),
            stats_role.len(),
            "unknown group_by should default to role grouping"
        );
        assert!(
            !stats_unknown.is_empty(),
            "inserted records should appear in results"
        );
    }

    #[test]
    fn test_query_embedding_usage_stats_unknown_group_by_defaults_to_provider() {
        let conn = setup_usage_db();
        insert_embedding_usage_sync(&conn, &make_embedding_record(Some(1))).unwrap();
        insert_embedding_usage_sync(&conn, &make_embedding_record(Some(1))).unwrap();

        let stats_unknown = query_embedding_usage_stats(&conn, "invalid_group", None, None)
            .expect("query should succeed with unknown group_by");
        let stats_provider = query_embedding_usage_stats(&conn, "provider", None, None)
            .expect("query with explicit provider group_by should succeed");

        // Unknown group_by should default to provider -- same grouping, same results
        assert_eq!(
            stats_unknown.len(),
            stats_provider.len(),
            "unknown group_by should default to provider grouping"
        );
        assert!(
            !stats_unknown.is_empty(),
            "inserted records should appear in results"
        );
    }

    // ========================================================================
    // Empty results
    // ========================================================================

    #[test]
    fn test_query_llm_usage_stats_empty_table() {
        let conn = setup_usage_db();
        for group in ["role", "provider", "model", "provider_model"] {
            let stats =
                query_llm_usage_stats(&conn, group, None, None).expect("query should succeed");
            assert!(stats.is_empty(), "empty table for group_by={}", group);
        }
    }

    #[test]
    fn test_query_embedding_usage_stats_empty_table() {
        let conn = setup_usage_db();
        for group in ["provider", "model", "provider_model"] {
            let stats = query_embedding_usage_stats(&conn, group, None, None)
                .expect("query should succeed");
            assert!(stats.is_empty(), "empty table for group_by={}", group);
        }
    }

    #[test]
    fn test_get_llm_usage_summary_empty_table() {
        let conn = setup_usage_db();
        let summary = get_llm_usage_summary(&conn, None, None)
            .expect("summary should succeed on empty table");
        assert_eq!(summary.total_requests, 0);
        assert_eq!(summary.total_tokens, 0);
        assert_eq!(summary.total_cost, 0.0);
        assert!(summary.avg_duration_ms.is_none());
    }

    #[test]
    fn test_get_embedding_usage_summary_empty_table() {
        let conn = setup_usage_db();
        let summary = get_embedding_usage_summary(&conn, None, None)
            .expect("summary should succeed on empty table");
        assert_eq!(summary.total_requests, 0);
        assert_eq!(summary.total_tokens, 0);
        assert_eq!(summary.total_cost, 0.0);
    }

    // ========================================================================
    // since_days edge values: 0 and very large
    // ========================================================================

    #[test]
    fn test_query_llm_usage_stats_since_days_zero() {
        let conn = setup_usage_db();
        let stats = query_llm_usage_stats(&conn, "role", None, Some(0))
            .expect("query with since_days=0 should succeed");
        assert!(stats.is_empty());
    }

    #[test]
    fn test_query_llm_usage_stats_since_days_very_large() {
        let conn = setup_usage_db();
        let stats = query_llm_usage_stats(&conn, "role", None, Some(u32::MAX))
            .expect("query with very large since_days should succeed");
        assert!(stats.is_empty());
    }

    #[test]
    fn test_get_llm_usage_summary_since_days_zero() {
        let conn = setup_usage_db();
        let summary = get_llm_usage_summary(&conn, None, Some(0))
            .expect("summary with since_days=0 should succeed");
        assert_eq!(summary.total_requests, 0);
    }

    #[test]
    fn test_get_embedding_usage_summary_since_days_zero() {
        let conn = setup_usage_db();
        let summary = get_embedding_usage_summary(&conn, None, Some(0))
            .expect("summary with since_days=0 should succeed");
        assert_eq!(summary.total_requests, 0);
    }

    // ========================================================================
    // Filtering by nonexistent project_id
    // ========================================================================

    #[test]
    fn test_query_llm_usage_stats_nonexistent_project() {
        let conn = setup_usage_db();
        let record = LlmUsageRecord {
            provider: "deepseek".to_string(),
            model: "deepseek-chat".to_string(),
            role: "pondering".to_string(),
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            cache_hit_tokens: None,
            cache_miss_tokens: None,
            cost_estimate: Some(0.001),
            duration_ms: Some(500),
            project_id: Some(1),
            session_id: None,
        };
        insert_llm_usage_sync(&conn, &record).expect("insert should succeed");

        let stats =
            query_llm_usage_stats(&conn, "role", Some(99999), None).expect("query should succeed");
        assert!(stats.is_empty(), "nonexistent project should return empty");
    }
}
