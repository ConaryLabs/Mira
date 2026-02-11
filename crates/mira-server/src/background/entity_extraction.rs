// crates/mira-server/src/background/entity_extraction.rs
// Background backfill: extract entities from existing memories that lack them

use crate::db::entities::{
    find_facts_without_entities_sync, link_entity_to_fact_sync, mark_fact_has_entities_sync,
    upsert_entity_sync,
};
use crate::db::pool::DatabasePool;
use crate::entities::extract_entities_heuristic;
use std::sync::Arc;

/// Batch size for backfill processing
const BACKFILL_BATCH_SIZE: usize = 50;

/// Process a batch of memories that haven't had entity extraction run.
///
/// Uses heuristic extraction only (no LLM). Runs every slow lane cycle
/// until all existing memories are caught up.
///
/// Returns the number of facts processed.
pub async fn process_entity_backfill(pool: &Arc<DatabasePool>) -> Result<usize, String> {
    // Find facts without entities
    let facts: Vec<(i64, Option<i64>, String)> = pool
        .interact(move |conn| {
            find_facts_without_entities_sync(conn, BACKFILL_BATCH_SIZE)
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .map_err(|e| e.to_string())?;

    if facts.is_empty() {
        return Ok(0);
    }

    let count = facts.len();

    pool.interact(move |conn| -> anyhow::Result<()> {
        let mut tx = conn.unchecked_transaction()?;
        for (fact_id, project_id, content) in &facts {
            // Use a savepoint so a failed fact rolls back its partial writes.
            // Block scope ensures savepoint is dropped before we borrow tx again.
            {
                let mut sp = tx.savepoint()?;
                match (|| -> anyhow::Result<()> {
                    let entities = extract_entities_heuristic(content);
                    for entity in &entities {
                        let entity_id = upsert_entity_sync(
                            &sp,
                            *project_id,
                            &entity.canonical_name,
                            entity.entity_type.as_str(),
                            &entity.name,
                        )?;
                        link_entity_to_fact_sync(&sp, *fact_id, entity_id)?;
                    }
                    Ok(())
                })() {
                    Ok(()) => {
                        sp.commit()?;
                    }
                    Err(e) => {
                        tracing::warn!("Entity backfill: skipping fact {}: {}", fact_id, e);
                        sp.rollback()?;
                    }
                }
            }
            // Always mark as processed, even on error, to avoid re-processing
            mark_fact_has_entities_sync(&tx, *fact_id)?;
        }
        tx.commit()?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?;

    if count > 0 {
        tracing::info!("Entity backfill: processed {} facts", count);
    }

    Ok(count)
}
