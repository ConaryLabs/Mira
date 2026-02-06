// crates/mira-server/src/background/embeddings.rs
// Background processing of pending embeddings queue

use crate::db::pool::DatabasePool;
use crate::db::{
    delete_pending_embedding_sync, get_pending_embeddings_sync, insert_chunk_embedding_sync,
};
use crate::embeddings::EmbeddingClient;
use crate::search::embedding_to_bytes;
use crate::utils::ResultExt;
use std::sync::Arc;

/// Maximum embeddings to process per batch
const BATCH_SIZE: usize = 100;

/// Process pending embeddings from the queue
pub async fn process_pending_embeddings(
    pool: &Arc<DatabasePool>,
    embeddings: Option<&Arc<EmbeddingClient>>,
) -> Result<usize, String> {
    let emb = match embeddings {
        Some(e) => e,
        None => return Ok(0), // No embeddings client configured
    };

    // Fetch pending chunks
    let pending = pool
        .interact(move |conn| {
            get_pending_embeddings_sync(conn, BATCH_SIZE)
                .map_err(|e| anyhow::anyhow!("Failed to get pending embeddings: {}", e))
        })
        .await
        .str_err()?;

    if pending.is_empty() {
        return Ok(0);
    }

    tracing::info!("Processing {} pending embeddings", pending.len());

    // Extract texts for batch embedding
    let texts: Vec<String> = pending.iter().map(|p| p.chunk_content.clone()).collect();

    // Generate embeddings in batch
    let embeddings_result = emb
        .embed_batch(&texts)
        .await
        .map_err(|e| format!("Embedding generation failed: {}", e))?;

    // Store embeddings and cleanup pending queue
    let count = pool
        .interact(move |conn| {
            let tx = conn.unchecked_transaction()?;
            let mut stored = 0;

            for (chunk, embedding) in pending.iter().zip(embeddings_result.iter()) {
                let embedding_bytes = embedding_to_bytes(embedding);

                // Insert into vec_code
                insert_chunk_embedding_sync(
                    &tx,
                    &embedding_bytes,
                    &chunk.file_path,
                    &chunk.chunk_content,
                    chunk.project_id,
                    chunk.start_line as usize,
                )
                .map_err(|e| anyhow::anyhow!("Insert failed: {}", e))?;

                // Remove from pending queue
                delete_pending_embedding_sync(&tx, chunk.id)
                    .map_err(|e| anyhow::anyhow!("Delete failed: {}", e))?;

                stored += 1;
            }

            tx.commit()
                .map_err(|e| anyhow::anyhow!("Commit failed: {}", e))?;
            Ok(stored)
        })
        .await
        .str_err()?;

    tracing::info!("Stored {} embeddings from pending queue", count);
    Ok(count)
}
