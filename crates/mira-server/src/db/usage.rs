// db/usage.rs
// Database operations for usage tracking (embeddings)

use anyhow::Result;
use rusqlite::{params, Connection};

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
