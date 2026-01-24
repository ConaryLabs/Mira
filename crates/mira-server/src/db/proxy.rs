// db/proxy.rs
// Database operations for proxy usage tracking

use crate::proxy::UsageRecord;
use anyhow::Result;
use rusqlite::{params, Connection};

// ═══════════════════════════════════════════════════════════════════════════════
// Sync functions for pool.interact() usage
// ═══════════════════════════════════════════════════════════════════════════════

/// Insert a proxy usage record - sync version for pool.interact()
pub fn insert_proxy_usage_sync(conn: &Connection, record: &UsageRecord) -> Result<i64> {
    conn.execute(
        "INSERT INTO proxy_usage (
            backend_name, model, input_tokens, output_tokens,
            cache_creation_tokens, cache_read_tokens, cost_estimate,
            request_id, session_id, project_id
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            record.backend_name,
            record.model,
            record.input_tokens as i64,
            record.output_tokens as i64,
            record.cache_creation_tokens as i64,
            record.cache_read_tokens as i64,
            record.cost_estimate,
            record.request_id,
            record.session_id,
            record.project_id,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Insert an embedding usage record - sync version for pool.interact()
pub fn insert_embedding_usage_sync(conn: &Connection, record: &EmbeddingUsageRecord) -> Result<i64> {
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
