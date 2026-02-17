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
        params_vec.push(Box::new(-(days as i32)));
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
        params_vec.push(Box::new(-(days as i32)));
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
        params_vec.push(Box::new(-(days as i32)));
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
        params_vec.push(Box::new(-(days as i32)));
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
