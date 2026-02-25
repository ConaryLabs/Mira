// crates/mira-server/src/background/embeddings.rs
// Background processing of pending embeddings queue

use crate::db::pool::DatabasePool;
use crate::db::{
    delete_pending_embedding_sync, get_pending_embeddings_sync, insert_chunk_embedding_sync,
};
use crate::embeddings::EmbeddingClient;
use crate::search::embedding_to_bytes;
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
        .run(move |conn| get_pending_embeddings_sync(conn, BATCH_SIZE))
        .await?;

    if pending.is_empty() {
        return Ok(0);
    }

    tracing::info!("Processing {} pending embeddings", pending.len());

    // Extract texts for batch embedding
    let texts: Vec<String> = pending.iter().map(|p| p.chunk_content.clone()).collect();

    // Generate embeddings in batch.
    // On failure, log and skip this cycle rather than blocking all future embeddings.
    let embeddings_result = match emb.embed_batch(&texts).await {
        Ok(result) => result,
        Err(e) => {
            tracing::warn!(
                "Embedding batch failed for {} chunks, skipping cycle: {}",
                pending.len(),
                e
            );
            return Ok(0);
        }
    };

    // Store embeddings and cleanup pending queue
    let count = pool
        .run(move |conn| {
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
                )?;

                // Remove from pending queue
                delete_pending_embedding_sync(&tx, chunk.id)?;

                stored += 1;
            }

            tx.commit()?;
            Ok::<_, rusqlite::Error>(stored)
        })
        .await?;

    tracing::info!("Stored {} embeddings from pending queue", count);
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::setup_test_pool;

    // ========================================================================
    // None embeddings client: early return path
    // ========================================================================

    #[tokio::test]
    async fn test_process_pending_embeddings_none_client() {
        let pool = setup_test_pool().await;

        // Passing None for embeddings client should return Ok(0) immediately
        let result = process_pending_embeddings(&pool, None).await;
        assert!(result.is_ok(), "None client should succeed");
        assert_eq!(result.unwrap(), 0, "None client should return 0 processed");
    }

    // ========================================================================
    // Empty pending queue (sync test -- pending_embeddings is in code DB)
    // ========================================================================

    #[test]
    fn test_get_pending_embeddings_empty_queue() {
        // pending_embeddings lives in code DB schema, not main migrations.
        // Create it manually for this test.
        let conn = crate::db::test_support::setup_test_connection();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS pending_embeddings (
                id INTEGER PRIMARY KEY,
                project_id INTEGER,
                file_path TEXT NOT NULL,
                chunk_content TEXT NOT NULL,
                start_line INTEGER NOT NULL DEFAULT 1,
                status TEXT DEFAULT 'pending',
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            );",
        )
        .unwrap();

        let pending =
            crate::db::get_pending_embeddings_sync(&conn, 100).expect("query should succeed");
        assert!(
            pending.is_empty(),
            "fresh table should have no pending embeddings"
        );
    }
}
