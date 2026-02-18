// crates/mira-server/src/background/memory_embeddings.rs
// Background worker for re-embedding memory facts after provider changes

use crate::db::pool::DatabasePool;
use crate::db::{find_facts_without_embeddings_sync, store_fact_embedding_sync};
use crate::embeddings::EmbeddingClient;
use crate::search::embedding_to_bytes;
use std::sync::Arc;

/// Batch size for memory re-embedding
const BATCH_SIZE: usize = 50;

/// Process memory facts that need embeddings.
///
/// Fetches facts with `has_embedding = 0`, embeds them in batches,
/// and stores the results. Returns the number of facts processed.
pub async fn process_memory_embeddings(
    pool: &Arc<DatabasePool>,
    embeddings: &Arc<EmbeddingClient>,
) -> Result<usize, String> {
    // Fetch facts needing embeddings
    let facts = pool
        .run(move |conn| find_facts_without_embeddings_sync(conn, BATCH_SIZE))
        .await?;

    if facts.is_empty() {
        return Ok(0);
    }

    tracing::info!("Re-embedding {} memory facts", facts.len());

    // Collect texts for batch embedding
    let texts: Vec<String> = facts.iter().map(|f| f.content.clone()).collect();

    // Embed in batch
    let vectors = embeddings
        .embed_batch(&texts)
        .await
        .map_err(|e| format!("Embedding generation failed: {}", e))?;

    // Store each embedding
    let facts_with_vectors: Vec<(i64, String, Vec<u8>)> = facts
        .iter()
        .zip(vectors.iter())
        .map(|(fact, vec)| (fact.id, fact.content.clone(), embedding_to_bytes(vec)))
        .collect();

    let stored = pool
        .run(move |conn| {
            let tx = conn.unchecked_transaction()?;
            let mut count = 0;
            for (id, content, embedding_bytes) in &facts_with_vectors {
                if let Err(e) = store_fact_embedding_sync(&tx, *id, content, embedding_bytes) {
                    tracing::warn!("Failed to store embedding for fact {}: {}", id, e);
                    continue;
                }
                count += 1;
            }
            tx.commit()?;
            Ok::<_, rusqlite::Error>(count)
        })
        .await?;

    if stored > 0 {
        tracing::info!("Re-embedded {} memory facts", stored);
    }

    Ok(stored)
}

#[cfg(test)]
mod tests {
    use crate::db::test_support::setup_test_pool;

    // ========================================================================
    // Empty facts list: early return path
    // ========================================================================

    #[tokio::test]
    async fn test_process_memory_embeddings_empty_facts_list() {
        let pool = setup_test_pool().await;

        // Fresh DB has no memory facts needing embeddings.
        // Verify the underlying query returns empty (the function itself
        // requires an Arc<EmbeddingClient>, but the empty-list path returns
        // Ok(0) before calling embed_batch).
        let count: usize = pool
            .run(|conn| {
                let facts = crate::db::find_facts_without_embeddings_sync(conn, 50)?;
                Ok::<_, rusqlite::Error>(facts.len())
            })
            .await
            .expect("query should succeed");
        assert_eq!(count, 0, "fresh DB should have no facts needing embeddings");
    }

    // ========================================================================
    // Facts exist but all have embeddings: also returns 0
    // ========================================================================

    #[tokio::test]
    async fn test_no_facts_without_embeddings_when_all_marked() {
        let pool = setup_test_pool().await;

        // Store a memory fact, then mark it as having an embedding
        pool.run(|conn| {
            let (pid, _) = crate::db::get_or_create_project_sync(conn, "/test/mem", Some("test"))?;
            let fact_id = crate::db::store_memory_sync(
                conn,
                crate::db::StoreMemoryParams {
                    project_id: Some(pid),
                    key: None,
                    content: "Test fact for embeddings",
                    fact_type: "general",
                    category: None,
                    confidence: 0.9,
                    session_id: None,
                    user_id: None,
                    scope: "project",
                    branch: None,
                    team_id: None,
                    suspicious: false,
                },
            )?;
            // Mark it as having an embedding
            crate::db::mark_fact_has_embedding_sync(conn, fact_id)?;
            Ok::<_, rusqlite::Error>(())
        })
        .await
        .expect("setup should succeed");

        // Now query should find 0 facts needing embeddings
        let count: usize = pool
            .run(|conn| {
                let facts = crate::db::find_facts_without_embeddings_sync(conn, 50)?;
                Ok::<_, rusqlite::Error>(facts.len())
            })
            .await
            .expect("query should succeed");
        assert_eq!(
            count, 0,
            "all facts have embeddings, so none should need processing"
        );
    }
}
