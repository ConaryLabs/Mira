// crates/mira-server/src/db/embeddings.rs
// Pending embeddings queue operations

use anyhow::Result;
use rusqlite::params;

use super::Database;
use crate::search::embedding_to_bytes;

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

impl Database {
    /// Fetch pending embeddings from the queue
    pub fn get_pending_embeddings(&self, limit: usize) -> Result<Vec<PendingEmbedding>> {
        let conn = self.conn();
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

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Store a code embedding in vec_code
    pub fn store_code_embedding(
        &self,
        project_id: Option<i64>,
        file_path: &str,
        chunk_content: &str,
        start_line: i64,
        embedding: &[f32],
    ) -> Result<()> {
        let conn = self.conn();
        let embedding_bytes = embedding_to_bytes(embedding);

        conn.execute(
            "INSERT INTO vec_code (embedding, file_path, chunk_content, project_id, start_line)
             VALUES (?, ?, ?, ?, ?)",
            params![embedding_bytes, file_path, chunk_content, project_id, start_line],
        )?;

        Ok(())
    }

    /// Delete a pending embedding by ID (after processing)
    pub fn delete_pending_embedding(&self, id: i64) -> Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM pending_embeddings WHERE id = ?", params![id])?;
        Ok(())
    }

    /// Queue a chunk for embedding (used by file watcher)
    pub fn queue_pending_embedding(
        &self,
        project_id: i64,
        file_path: &str,
        chunk_content: &str,
        start_line: u32,
    ) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO pending_embeddings (project_id, file_path, chunk_content, start_line, status)
             VALUES (?, ?, ?, ?, 'pending')",
            params![project_id, file_path, chunk_content, start_line as i64],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get count of pending embeddings
    pub fn count_pending_embeddings(&self) -> Result<i64> {
        let conn = self.conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pending_embeddings WHERE status = 'pending'",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }
}
