// crates/mira-server/src/db/embeddings.rs
// Pending embeddings queue operations

use rusqlite::params;


/// A pending embedding chunk from the queue
#[derive(Debug, Clone)]
pub struct PendingEmbedding {
    pub id: i64,
    pub project_id: Option<i64>,
    pub file_path: String,
    pub chunk_content: String,
    pub start_line: i64,
}

/// Fetch pending embeddings from the queue (sync version for pool.interact)
pub fn get_pending_embeddings_sync(
    conn: &rusqlite::Connection,
    limit: usize,
) -> rusqlite::Result<Vec<PendingEmbedding>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, file_path, chunk_content, start_line
         FROM pending_embeddings
         WHERE status = 'pending'
         ORDER BY created_at ASC
         LIMIT ?",
    )?;

    let rows = stmt.query_map(params![limit as i64], |row| {
        Ok(PendingEmbedding {
            id: row.get(0)?,
            project_id: row.get(1)?,
            file_path: row.get(2)?,
            chunk_content: row.get(3)?,
            start_line: row.get(4)?,
        })
    })?;

    rows.collect()
}

