// crates/mira-server/src/background/embeddings.rs
// Background processing of pending embeddings queue

use crate::db::{Database, insert_chunk_embedding_sync, delete_pending_embedding_sync};
use crate::embeddings::EmbeddingClient;
use crate::search::embedding_to_bytes;
use std::sync::Arc;

/// Maximum embeddings to process per batch
const BATCH_SIZE: usize = 100;

/// Process pending embeddings from the queue
pub async fn process_pending_embeddings(
    db: &Arc<Database>,
    embeddings: Option<&Arc<EmbeddingClient>>,
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
    let count = tokio::task::spawn_blocking(move || {
        let conn = db_clone.conn();
        let tx = conn.unchecked_transaction()?;
        let mut stored = 0;

        for (chunk, embedding) in pending_clone.iter().zip(embeddings_result.iter()) {
            let embedding_bytes = embedding_to_bytes(embedding);

            // Insert into vec_code
            insert_chunk_embedding_sync(
                &tx,
                &embedding_bytes,
                &chunk.file_path,
                &chunk.chunk_content,
                chunk.project_id,
                chunk.start_line as usize,
            )?;

            // Remove from pending queue
            delete_pending_embedding_sync(&tx, chunk.id)?;

            stored += 1;
        }

        tx.commit()?;
        Ok::<_, rusqlite::Error>(stored)
    }).await.map_err(|e| format!("spawn_blocking panicked: {}", e))?.map_err(|e| e.to_string())?;

    tracing::info!("Stored {} embeddings from pending queue", count);
    Ok(count)
}
