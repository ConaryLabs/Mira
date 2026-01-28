// crates/mira-server/src/indexer/batch.rs
// Batch processing for symbols, imports, calls, and embeddings

use crate::db::{
    ImportInsert, SymbolInsert, insert_call_sync, insert_chunk_embedding_sync, insert_import_sync,
    insert_symbol_sync, pool::DatabasePool,
};
use crate::embeddings::EmbeddingClient;
use crate::indexer::parsing::{FunctionCall, Import, Symbol};
use crate::indexer::types::IndexStats;
use crate::search::embedding_to_bytes;
use anyhow::Result;
use std::sync::Arc;

/// Maximum symbols to accumulate before flushing to database
pub const SYMBOL_FLUSH_THRESHOLD: usize = 1000;
/// Maximum files to accumulate before flushing to database
pub const FILE_FLUSH_THRESHOLD: usize = 100;
/// Maximum chunks to accumulate before flushing to database
pub const CHUNK_FLUSH_THRESHOLD: usize = 1000;

/// Pending chunk for batch embedding
pub struct PendingChunk {
    pub file_path: String,
    pub start_line: usize,
    pub content: String,
}

/// Pending file data for batch database insertion
pub struct PendingFileBatch {
    pub file_path: String,
    pub symbols: Vec<Symbol>,
    pub imports: Vec<Import>,
    pub calls: Vec<FunctionCall>,
}

/// Helper to embed code chunks and return vectors
/// Uses RETRIEVAL_DOCUMENT task type for storage (pairs with CODE_RETRIEVAL_QUERY for search)
pub async fn embed_chunks(
    embeddings: &EmbeddingClient,
    pending_chunks: &[PendingChunk],
) -> Result<Vec<Vec<f32>>, String> {
    let texts: Vec<String> = pending_chunks.iter().map(|c| c.content.clone()).collect();
    embeddings
        .embed_batch_for_storage(&texts)
        .await
        .map_err(|e| e.to_string())
}

/// Helper to prepare chunk data for database storage
pub fn prepare_chunk_data(
    pending_chunks: &[PendingChunk],
    vectors: &[Vec<f32>],
) -> Vec<(String, String, usize, Vec<u8>)> {
    pending_chunks
        .iter()
        .zip(vectors.iter())
        .map(|(chunk, embedding)| {
            (
                chunk.file_path.clone(),
                chunk.content.clone(),
                chunk.start_line,
                embedding_to_bytes(embedding),
            )
        })
        .collect()
}

/// Helper to store chunk embeddings in database
pub async fn store_chunk_embeddings(
    pool: Arc<DatabasePool>,
    chunk_data: Vec<(String, String, usize, Vec<u8>)>,
    project_id: Option<i64>,
) -> Result<usize> {
    pool.interact(move |conn| {
        let tx = conn.unchecked_transaction()?;
        let mut errors = 0usize;

        for (file_path, content, start_line, embedding_bytes) in &chunk_data {
            if let Err(e) = insert_chunk_embedding_sync(
                &tx,
                embedding_bytes,
                file_path,
                content,
                project_id,
                *start_line,
            ) {
                tracing::warn!(
                    "Failed to store embedding ({}:{}): {}",
                    file_path,
                    start_line,
                    e
                );
                errors += 1;
            }
        }

        tx.commit()?;
        Ok(errors)
    })
    .await
}

/// Flush accumulated chunks to database and generate embeddings
pub async fn flush_chunks(
    mut pending_chunks: Vec<PendingChunk>,
    pool: Arc<DatabasePool>,
    embeddings: Option<Arc<EmbeddingClient>>,
    project_id: Option<i64>,
    stats: &mut IndexStats,
) -> Result<()> {
    if pending_chunks.is_empty() {
        return Ok(());
    }

    if let Some(ref emb) = embeddings {
        let chunk_count = pending_chunks.len();
        tracing::info!("Flushing {} chunks...", chunk_count);

        match embed_chunks(emb, &pending_chunks).await {
            Ok(vectors) => {
                tracing::info!("Embedded {} chunks", vectors.len());

                let chunk_data = prepare_chunk_data(&pending_chunks, &vectors);

                match store_chunk_embeddings(pool.clone(), chunk_data, project_id).await {
                    Ok(error_count) => {
                        stats.chunks += chunk_count - error_count;
                        stats.errors += error_count;
                    }
                    Err(e) => {
                        tracing::error!("Failed to store embeddings: {}", e);
                        stats.errors += chunk_count;
                    }
                }
            }
            Err(e) => {
                tracing::error!("Batch embedding failed: {}", e);
                stats.errors += pending_chunks.len();
            }
        }
    }

    // Clear the pending chunks after flush
    pending_chunks.clear();
    Ok(())
}

/// Helper to store symbols and capture their IDs
pub fn store_symbols_and_capture_ids(
    tx: &rusqlite::Transaction,
    project_id: Option<i64>,
    file_path: &str,
    symbols: &[Symbol],
) -> rusqlite::Result<(Vec<(String, u32, u32, i64)>, usize)> {
    let mut symbol_ranges = Vec::new();
    let mut errors = 0usize;

    for sym in symbols {
        let sym_insert = SymbolInsert {
            name: &sym.name,
            symbol_type: &sym.symbol_type,
            start_line: sym.start_line,
            end_line: sym.end_line,
            signature: sym.signature.as_deref(),
        };

        match insert_symbol_sync(tx, project_id, file_path, &sym_insert) {
            Ok(id) => {
                symbol_ranges.push((sym.name.clone(), sym.start_line, sym.end_line, id));
            }
            Err(e) => {
                tracing::warn!("Failed to store symbol {} ({}): {}", sym.name, file_path, e);
                errors += 1;
            }
        }
    }

    Ok((symbol_ranges, errors))
}

/// Helper to store imports
pub fn store_imports(
    tx: &rusqlite::Transaction,
    project_id: Option<i64>,
    file_path: &str,
    imports: &[Import],
) -> rusqlite::Result<usize> {
    let mut errors = 0usize;

    for import in imports {
        let import_insert = ImportInsert {
            import_path: &import.import_path,
            is_external: import.is_external,
        };

        if let Err(e) = insert_import_sync(tx, project_id, file_path, &import_insert) {
            tracing::warn!(
                "Failed to store import {} ({}): {}",
                import.import_path,
                file_path,
                e
            );
            errors += 1;
        }
    }

    Ok(errors)
}

/// Helper to store function calls
pub fn store_function_calls(
    tx: &rusqlite::Transaction,
    file_path: &str,
    calls: &[FunctionCall],
    symbol_ranges: &[(String, u32, u32, i64)],
) -> rusqlite::Result<usize> {
    let mut errors = 0usize;

    for call in calls {
        // Find the caller symbol whose line range contains this call
        let caller_id = symbol_ranges
            .iter()
            .find(|(name, start, end, _)| {
                name == &call.caller_name && call.call_line >= *start && call.call_line <= *end
            })
            .map(|(_, _, _, id)| *id);

        if let Some(cid) = caller_id {
            // Try to find callee ID (may be in same file)
            let callee_id = symbol_ranges
                .iter()
                .find(|(name, _, _, _)| name == &call.callee_name)
                .map(|(_, _, _, id)| *id);

            if let Err(e) = insert_call_sync(tx, cid, &call.callee_name, callee_id) {
                tracing::warn!(
                    "Failed to store call {} -> {} ({}): {}",
                    call.caller_name,
                    call.callee_name,
                    file_path,
                    e
                );
                errors += 1;
            }
        } else {
            // Caller not found (could be module-level call)
            tracing::debug!(
                "Skipping call {} -> {} (caller not found in {})",
                call.caller_name,
                call.callee_name,
                file_path
            );
        }
    }

    Ok(errors)
}

/// Flush accumulated file data (symbols, imports, calls) to database
pub async fn flush_code_batch(
    pending_batches: &mut Vec<PendingFileBatch>,
    pool: Arc<DatabasePool>,
    project_id: Option<i64>,
    stats: &mut IndexStats,
) -> Result<()> {
    if pending_batches.is_empty() {
        return Ok(());
    }

    let batches = std::mem::take(pending_batches);
    let total_symbols: usize = batches.iter().map(|b| b.symbols.len()).sum();
    let total_calls: usize = batches.iter().map(|b| b.calls.len()).sum();
    tracing::info!(
        "Flushing {} files ({} symbols, {} calls)...",
        batches.len(),
        total_symbols,
        total_calls
    );

    // Process all batches in a single transaction
    let error_count = pool
        .interact(move |conn| {
            let tx = conn.unchecked_transaction()?;
            let mut total_errors = 0usize;

            // Process each file batch
            for batch in batches {
                // Store symbols and capture IDs
                let (symbol_ranges, symbol_errors) = store_symbols_and_capture_ids(
                    &tx,
                    project_id,
                    &batch.file_path,
                    &batch.symbols,
                )?;
                total_errors += symbol_errors;

                // Store imports
                let import_errors =
                    store_imports(&tx, project_id, &batch.file_path, &batch.imports)?;
                total_errors += import_errors;

                // Store function calls for call graph
                let call_errors =
                    store_function_calls(&tx, &batch.file_path, &batch.calls, &symbol_ranges)?;
                total_errors += call_errors;
            }

            tx.commit()?;
            Ok(total_errors)
        })
        .await?;

    stats.symbols += total_symbols - error_count;
    stats.errors += error_count;

    // pending_batches already cleared by std::mem::take
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flush_thresholds() {
        assert_eq!(SYMBOL_FLUSH_THRESHOLD, 1000);
        assert_eq!(FILE_FLUSH_THRESHOLD, 100);
        assert_eq!(CHUNK_FLUSH_THRESHOLD, 1000);
    }
}
