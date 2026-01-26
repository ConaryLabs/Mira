// crates/mira-server/src/indexer/project.rs
// Project-level indexing operations

use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use crate::indexer::batch::{
    flush_chunks, flush_code_batch,
    PendingChunk, PendingFileBatch,
    SYMBOL_FLUSH_THRESHOLD, FILE_FLUSH_THRESHOLD, CHUNK_FLUSH_THRESHOLD,
};
use crate::indexer::chunking::create_semantic_chunks;
use crate::indexer::parsing::{extract_all, Symbol, Import, FunctionCall};
use crate::indexer::types::{IndexStats, ParsedSymbol};
use crate::project_files::walker::FileWalker;
use anyhow::Result;
use rayon::prelude::*;
use std::path::Path;
use std::sync::Arc;

/// Collect files to index, filtering by extension and ignoring patterns
fn collect_files_to_index(path: &Path, stats: &mut IndexStats) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();

    // Create a FileWalker for all supported language extensions
    let walker = FileWalker::new(path)
        .follow_links(true)
        .use_gitignore(true)
        .skip_hidden(true)
        .with_extension("rs")
        .with_extension("py")
        .with_extension("ts")
        .with_extension("tsx")
        .with_extension("js")
        .with_extension("jsx")
        .with_extension("go");

    for result in walker.walk_paths() {
        match result {
            Ok(path) => {
                files.push(path);
            }
            Err(e) => {
                tracing::warn!("Failed to access path during indexing: {}", e);
                stats.errors += 1;
            }
        }
    }

    files
}

/// Clear existing data for a project from all relevant tables
async fn clear_existing_project_data(pool: Arc<DatabasePool>, project_id: Option<i64>) -> Result<()> {
    use crate::db::clear_project_index_sync;

    tracing::info!("Clearing existing data...");
    if let Some(pid) = project_id {
        pool.interact(move |conn| {
            clear_project_index_sync(conn, pid).map_err(|e| anyhow::anyhow!(e))
        })
        .await?;
    }
    Ok(())
}

/// Result of parsing a single file (used for parallel parsing)
struct ParsedFile {
    relative_path: String,
    symbols: Vec<Symbol>,
    imports: Vec<Import>,
    calls: Vec<FunctionCall>,
    content: String,
    parse_time_ms: u64,
}

/// Parse all files in parallel using rayon (CPU-bound work)
fn parse_files_parallel(files: &[std::path::PathBuf], base_path: &Path) -> (Vec<ParsedFile>, usize) {
    let results: Vec<_> = files
        .par_iter()
        .map(|file_path| {
            let relative_path = file_path
                .strip_prefix(base_path)
                .unwrap_or(file_path)
                .to_string_lossy()
                .to_string();

            let start = std::time::Instant::now();
            match extract_all(file_path) {
                Ok((symbols, imports, calls, content)) => {
                    let parse_time_ms = start.elapsed().as_millis() as u64;
                    Ok(ParsedFile {
                        relative_path,
                        symbols,
                        imports,
                        calls,
                        content,
                        parse_time_ms,
                    })
                }
                Err(e) => Err((relative_path, e)),
            }
        })
        .collect();

    let mut parsed_files = Vec::with_capacity(results.len());
    let mut error_count = 0;

    for result in results {
        match result {
            Ok(parsed) => parsed_files.push(parsed),
            Err((path, e)) => {
                tracing::warn!("Failed to parse {}: {}", path, e);
                error_count += 1;
            }
        }
    }

    (parsed_files, error_count)
}

/// Process parsed files, accumulating batches and chunks, flushing when thresholds reached
async fn process_parsed_files(
    parsed_files: Vec<ParsedFile>,
    pending_batches: &mut Vec<PendingFileBatch>,
    pending_chunks: &mut Vec<PendingChunk>,
    pool: Arc<DatabasePool>,
    embeddings: Option<Arc<EmbeddingClient>>,
    project_id: Option<i64>,
    stats: &mut IndexStats,
) -> Result<()> {
    let total_files = parsed_files.len();

    for (i, parsed) in parsed_files.into_iter().enumerate() {
        tracing::info!(
            "[{}/{}] Processing {} ({} symbols, {}ms parse)",
            i + 1,
            total_files,
            parsed.relative_path,
            parsed.symbols.len(),
            parsed.parse_time_ms
        );

        stats.files += 1;

        // Convert symbols to ParsedSymbol for semantic chunking
        let parsed_symbols: Vec<ParsedSymbol> = parsed
            .symbols
            .iter()
            .map(|s| ParsedSymbol {
                name: s.name.clone(),
                kind: s.symbol_type.clone(),
                start_line: s.start_line,
                end_line: s.end_line,
                signature: s.signature.clone(),
            })
            .collect();

        // Accumulate file data for batch insertion
        pending_batches.push(PendingFileBatch {
            file_path: parsed.relative_path.clone(),
            symbols: parsed.symbols,
            imports: parsed.imports,
            calls: parsed.calls,
        });

        // Check if we should flush accumulated batches
        let total_batched_symbols: usize = pending_batches.iter().map(|b| b.symbols.len()).sum();
        if total_batched_symbols >= SYMBOL_FLUSH_THRESHOLD || pending_batches.len() >= FILE_FLUSH_THRESHOLD {
            let flush_start = std::time::Instant::now();
            flush_code_batch(pending_batches, pool.clone(), project_id, stats).await?;
            tracing::debug!("  Batch flush in {:?}", flush_start.elapsed());
        }

        // Collect chunks for batch embedding (if embeddings enabled)
        if embeddings.is_some() {
            let chunks = create_semantic_chunks(&parsed.content, &parsed_symbols);
            for chunk in chunks {
                if !chunk.content.trim().is_empty() {
                    pending_chunks.push(PendingChunk {
                        file_path: parsed.relative_path.clone(),
                        start_line: chunk.start_line as usize,
                        content: chunk.content,
                    });
                }
            }

            // Flush if we've accumulated enough chunks
            if pending_chunks.len() >= CHUNK_FLUSH_THRESHOLD {
                let chunks_to_flush = std::mem::take(pending_chunks);
                flush_chunks(
                    chunks_to_flush,
                    pool.clone(),
                    embeddings.clone(),
                    project_id,
                    stats,
                ).await?;
            }
        }
    }
    Ok(())
}

/// Flush any remaining batches and chunks after processing all files
async fn flush_remaining_data(
    pending_batches: &mut Vec<PendingFileBatch>,
    pending_chunks: Vec<PendingChunk>,
    pool: Arc<DatabasePool>,
    embeddings: Option<Arc<EmbeddingClient>>,
    project_id: Option<i64>,
    stats: &mut IndexStats,
) -> Result<()> {
    // Flush any remaining file batches
    if !pending_batches.is_empty() {
        flush_code_batch(pending_batches, pool.clone(), project_id, stats).await?;
    }

    // Flush any remaining chunks
    flush_chunks(
        pending_chunks,
        pool.clone(),
        embeddings.clone(),
        project_id,
        stats,
    ).await?;

    Ok(())
}

/// Rebuild FTS5 full-text search index for a project if project_id is Some
async fn rebuild_fts_index_if_needed(pool: Arc<DatabasePool>, project_id: Option<i64>) {
    if let Some(pid) = project_id {
        tracing::info!("Rebuilding FTS5 search index for project {}", pid);
        if let Err(e) = pool.rebuild_fts_for_project(pid).await {
            tracing::warn!("Failed to rebuild FTS5 index: {}", e);
        }
    }
}

/// Index an entire project
pub async fn index_project(
    path: &Path,
    pool: Arc<DatabasePool>,
    embeddings: Option<Arc<EmbeddingClient>>,
    project_id: Option<i64>,
) -> Result<IndexStats> {
    tracing::info!("Starting index_project for {:?}", path);

    let mut stats = IndexStats {
        files: 0,
        symbols: 0,
        chunks: 0,
        errors: 0,
    };



    tracing::info!("Collecting files...");
    let files = collect_files_to_index(path, &mut stats);

    tracing::info!("Found {} files to index", files.len());

    // Clear existing data for this project
    clear_existing_project_data(pool.clone(), project_id).await?;

    // Phase 1: Parse all files in parallel (CPU-bound, uses all cores)
    tracing::info!("Parsing {} files in parallel...", files.len());
    let parse_start = std::time::Instant::now();
    let (parsed_files, parse_errors) = parse_files_parallel(&files, path);
    stats.errors += parse_errors;
    tracing::info!(
        "Parallel parsing complete in {:?} ({} files, {} errors)",
        parse_start.elapsed(),
        parsed_files.len(),
        parse_errors
    );

    // Phase 2: Process parsed files and batch insert to DB (IO-bound)
    tracing::info!("Processing parsed files...");
    let mut pending_chunks: Vec<PendingChunk> = Vec::new();
    let mut pending_batches: Vec<PendingFileBatch> = Vec::new();

    process_parsed_files(
        parsed_files,
        &mut pending_batches,
        &mut pending_chunks,
        pool.clone(),
        embeddings.clone(),
        project_id,
        &mut stats,
    ).await?;

    // Flush any remaining batches and chunks
    flush_remaining_data(
        &mut pending_batches,
        pending_chunks,
        pool.clone(),
        embeddings.clone(),
        project_id,
        &mut stats,
    ).await?;

    // Rebuild FTS5 full-text search index for this project
    rebuild_fts_index_if_needed(pool.clone(), project_id).await;

    if stats.errors > 0 {
        tracing::warn!(
            "Indexing complete with errors: {} files, {} symbols, {} chunks, {} errors",
            stats.files, stats.symbols, stats.chunks, stats.errors
        );
    } else {
        tracing::info!(
            "Indexing complete: {} files, {} symbols, {} chunks",
            stats.files, stats.symbols, stats.chunks
        );
    }
    Ok(stats)
}
