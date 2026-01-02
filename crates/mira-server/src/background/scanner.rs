// crates/mira-server/src/background/scanner.rs
// Scan for work items that need processing

use crate::db::Database;
use rusqlite::params;
use std::sync::Arc;

/// Pending embedding work item
#[derive(Debug)]
pub struct PendingEmbedding {
    pub id: i64,
    pub chunk_content: String,
}

/// Find pending embeddings that need processing
pub fn find_pending_embeddings(db: &Arc<Database>, limit: usize) -> Result<Vec<PendingEmbedding>, String> {
    let conn = db.conn();

    let mut stmt = conn
        .prepare(
            "SELECT id, chunk_content
             FROM pending_embeddings
             WHERE status = 'pending'
             ORDER BY created_at ASC
             LIMIT ?",
        )
        .map_err(|e| e.to_string())?;

    let results: Vec<PendingEmbedding> = stmt
        .query_map(params![limit as i64], |row| {
            Ok(PendingEmbedding {
                id: row.get(0)?,
                chunk_content: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

/// Mark embeddings as processing (to avoid duplicate work)
pub fn mark_embeddings_processing(db: &Arc<Database>, ids: &[i64]) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }

    let conn = db.conn();
    let placeholders: Vec<&str> = ids.iter().map(|_| "?").collect();
    let sql = format!(
        "UPDATE pending_embeddings SET status = 'processing' WHERE id IN ({})",
        placeholders.join(",")
    );

    let params: Vec<&dyn rusqlite::ToSql> = ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
    conn.execute(&sql, params.as_slice()).map_err(|e| e.to_string())?;

    Ok(())
}

/// Mark embeddings as completed
pub fn mark_embeddings_completed(db: &Arc<Database>, ids: &[i64]) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }

    let conn = db.conn();
    let placeholders: Vec<&str> = ids.iter().map(|_| "?").collect();
    let sql = format!(
        "DELETE FROM pending_embeddings WHERE id IN ({})",
        placeholders.join(",")
    );

    let params: Vec<&dyn rusqlite::ToSql> = ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
    conn.execute(&sql, params.as_slice()).map_err(|e| e.to_string())?;

    Ok(())
}
