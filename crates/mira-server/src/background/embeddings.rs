// crates/mira-server/src/background/embeddings.rs
// Background processing of pending embeddings queue

use crate::db::Database;
use crate::embeddings::Embeddings;
use crate::search::embedding_to_bytes;
use rusqlite::params;
use std::sync::Arc;

/// Maximum embeddings to process per batch
const BATCH_SIZE: usize = 100;

/// Process pending embeddings from the queue
pub async fn process_pending_embeddings(
    db: &Arc<Database>,
    embeddings: Option<&Arc<Embeddings>>,
) -> Result<usize, String> {
    let emb = match embeddings {
        Some(e) => e,
        None => return Ok(0), // No embeddings client configured
    };

    // Fetch pending chunks
    let pending = db.get_pending_embeddings(BATCH_SIZE)
        .map_err(|e| format!("Failed to get pending embeddings: {}", e))?;

    if pending.is_empty() {
        return Ok(0);
    }

    tracing::info!("Processing {} pending embeddings", pending.len());

    // Extract texts for batch embedding
    let texts: Vec<String> = pending.iter().map(|p| p.chunk_content.clone()).collect();

    // Generate embeddings in batch
    let embeddings_result = emb.embed_batch(&texts).await
        .map_err(|e| format!("Embedding generation failed: {}", e))?;

    // Store embeddings and cleanup pending queue
    let db_clone = db.clone();
    let pending_clone = pending.clone();
    let count = Database::run_blocking(db_clone, move |conn| {
        let tx = conn.unchecked_transaction()?;
        let mut stored = 0;

        for (chunk, embedding) in pending_clone.iter().zip(embeddings_result.iter()) {
            let embedding_bytes = embedding_to_bytes(embedding);

            // Insert into vec_code
            tx.execute(
                "INSERT INTO vec_code (embedding, file_path, chunk_content, project_id, start_line)
                 VALUES (?, ?, ?, ?, ?)",
                params![
                    embedding_bytes,
                    chunk.file_path,
                    chunk.chunk_content,
                    chunk.project_id,
                    chunk.start_line
                ],
            )?;

            // Remove from pending queue
            tx.execute(
                "DELETE FROM pending_embeddings WHERE id = ?",
                params![chunk.id],
            )?;

            stored += 1;
        }

        tx.commit()?;
        Ok::<_, rusqlite::Error>(stored)
    }).await.map_err(|e| e.to_string())?;

    tracing::info!("Stored {} embeddings from pending queue", count);
    Ok(count)
}
