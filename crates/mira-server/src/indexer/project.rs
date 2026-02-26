// crates/mira-server/src/indexer/project.rs
// Project-level indexing operations

use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use crate::indexer::batch::{
    CHUNK_FLUSH_THRESHOLD, FILE_FLUSH_THRESHOLD, PendingChunk, PendingFileBatch,
    SYMBOL_FLUSH_THRESHOLD, flush_chunks, flush_code_batch,
};
use crate::indexer::chunking::create_semantic_chunks;
use crate::indexer::parsing::{FunctionCall, Import, Symbol, extract_all};
use crate::indexer::types::{IndexStats, ParsedSymbol};
use crate::project_files::FileWalker;
use anyhow::Result;
#[cfg(feature = "parallel")]
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Maximum file size for indexing (1 MB). Files larger than this are
/// typically generated code, minified bundles, or data files that
/// provide poor symbol/semantic value and risk OOM.
const MAX_INDEX_FILE_BYTES: u64 = 1_024 * 1_024;

/// File extensions supported for indexing
const SUPPORTED_EXTENSIONS: &[&str] = &["rs", "py", "ts", "tsx", "js", "jsx", "go"];

/// Collect files to index, filtering by supported extensions and ignoring patterns.
///
/// Also tracks files with unsupported extensions in `stats.skipped_by_extension`
/// so the user gets visibility into what was not indexed.
fn collect_files_to_index(path: &Path, stats: &mut IndexStats) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();

    // Walk all files (no extension filter) so we can count skipped extensions
    let walker = FileWalker::new(path)
        .follow_links(true)
        .use_gitignore(true)
        .skip_hidden(true);

    for result in walker.walk_paths() {
        match result {
            Ok(file_path) => {
                let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");

                if !SUPPORTED_EXTENSIONS.contains(&ext) {
                    // Count unsupported files by extension (skip extensionless files)
                    if !ext.is_empty() {
                        let key = format!(".{}", ext);
                        *stats.skipped_by_extension.entry(key).or_insert(0) += 1;
                    }
                    continue;
                }

                // Skip files that are too large (generated code, minified bundles, etc.)
                match file_path.metadata() {
                    Ok(meta) if meta.len() > MAX_INDEX_FILE_BYTES => {
                        tracing::debug!(
                            "Skipping large file ({} bytes): {}",
                            meta.len(),
                            file_path.display()
                        );
                        stats.skipped += 1;
                        continue;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to stat {}: {}", file_path.display(), e);
                        stats.errors += 1;
                        continue;
                    }
                    _ => {}
                }
                files.push(file_path);
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
async fn clear_existing_project_data(
    pool: Arc<DatabasePool>,
    project_id: Option<i64>,
    embedding_dims: usize,
) -> Result<()> {
    use crate::db::clear_project_index_sync;

    tracing::info!("Clearing existing data...");
    if let Some(pid) = project_id {
        pool.interact(move |conn| {
            clear_project_index_sync(conn, pid, embedding_dims).map_err(|e| anyhow::anyhow!(e))
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
fn parse_files_parallel(
    files: &[std::path::PathBuf],
    base_path: &Path,
) -> (Vec<ParsedFile>, usize) {
    #[cfg(feature = "parallel")]
    let iter = files.par_iter();
    #[cfg(not(feature = "parallel"))]
    let iter = files.iter();

    let results: Vec<_> = iter
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
    let mut batched_symbol_count: usize = 0;

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

        let symbol_count = parsed.symbols.len();

        // Accumulate file data for batch insertion
        pending_batches.push(PendingFileBatch {
            file_path: parsed.relative_path.clone(),
            symbols: parsed.symbols,
            imports: parsed.imports,
            calls: parsed.calls,
        });
        batched_symbol_count += symbol_count;

        // Check if we should flush accumulated batches
        if batched_symbol_count >= SYMBOL_FLUSH_THRESHOLD
            || pending_batches.len() >= FILE_FLUSH_THRESHOLD
        {
            let flush_start = std::time::Instant::now();
            flush_code_batch(pending_batches, pool.clone(), project_id, stats).await?;
            batched_symbol_count = 0;
            tracing::debug!("  Batch flush in {:?}", flush_start.elapsed());
        }

        // Always collect chunks (stored to code_chunks; optionally embedded to vec_code)
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
            )
            .await?;
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
    )
    .await?;

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
        skipped: 0,
        skipped_by_extension: HashMap::new(),
    };

    tracing::info!("Collecting files...");
    let files = collect_files_to_index(path, &mut stats);

    tracing::info!("Found {} files to index", files.len());

    // Clear existing data for this project, using the configured embedding dims so
    // that vec_code is recreated with the correct dimension (not the legacy 1536 default).
    let embedding_dims = embeddings.as_ref().map(|e| e.dimensions()).unwrap_or(1536);
    clear_existing_project_data(pool.clone(), project_id, embedding_dims).await?;

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
    )
    .await?;

    // Flush any remaining batches and chunks
    flush_remaining_data(
        &mut pending_batches,
        pending_chunks,
        pool.clone(),
        embeddings.clone(),
        project_id,
        &mut stats,
    )
    .await?;

    // Rebuild FTS5 full-text search index for this project
    rebuild_fts_index_if_needed(pool.clone(), project_id).await;

    // Build skipped-by-extension summary for logging
    let skipped_ext_summary = if stats.skipped_by_extension.is_empty() {
        String::new()
    } else {
        let mut pairs: Vec<_> = stats.skipped_by_extension.iter().collect();
        pairs.sort_by(|a, b| b.1.cmp(a.1));
        let parts: Vec<String> = pairs.iter().map(|(k, v)| format!("{}: {}", k, v)).collect();
        format!(", skipped by extension: {}", parts.join(", "))
    };

    if stats.errors > 0 {
        tracing::warn!(
            "Indexing complete with errors: {} files, {} symbols, {} chunks, {} errors, {} skipped{}",
            stats.files,
            stats.symbols,
            stats.chunks,
            stats.errors,
            stats.skipped,
            skipped_ext_summary
        );
    } else {
        tracing::info!(
            "Indexing complete: {} files, {} symbols, {} chunks, {} skipped{}",
            stats.files,
            stats.symbols,
            stats.chunks,
            stats.skipped,
            skipped_ext_summary
        );
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_files_skips_large_files() {
        let dir = tempfile::tempdir().expect("Failed to create temp dir");

        // Create a small .rs file (should be included)
        let small_path = dir.path().join("small.rs");
        std::fs::write(&small_path, "fn main() {}").expect("Failed to write small file");

        // Create a large .rs file (> 1MB, should be skipped)
        let large_path = dir.path().join("large.rs");
        let large_content = "x".repeat(MAX_INDEX_FILE_BYTES as usize + 1);
        std::fs::write(&large_path, large_content).expect("Failed to write large file");

        let mut stats = IndexStats {
            files: 0,
            symbols: 0,
            chunks: 0,
            errors: 0,
            skipped: 0,
            skipped_by_extension: HashMap::new(),
        };

        let files = collect_files_to_index(dir.path(), &mut stats);

        // Only the small file should be collected
        assert_eq!(files.len(), 1, "Only small file should be collected");
        assert!(
            files[0].ends_with("small.rs"),
            "Collected file should be small.rs, got: {:?}",
            files[0]
        );
        assert_eq!(stats.skipped, 1, "Large file should be counted as skipped");
    }

    #[test]
    fn test_max_index_file_bytes_is_one_mb() {
        assert_eq!(MAX_INDEX_FILE_BYTES, 1_024 * 1_024);
    }

    #[test]
    fn test_collect_files_tracks_skipped_by_extension() {
        let dir = tempfile::tempdir().expect("Failed to create temp dir");

        // Supported files — should be collected
        std::fs::write(dir.path().join("lib.rs"), "pub fn foo() {}").unwrap();
        std::fs::write(dir.path().join("script.py"), "def foo(): pass").unwrap();

        // Unsupported file — should be skipped and tracked by extension
        std::fs::write(dir.path().join("Main.java"), "public class Main {}").unwrap();

        let mut stats = IndexStats {
            files: 0,
            symbols: 0,
            chunks: 0,
            errors: 0,
            skipped: 0,
            skipped_by_extension: HashMap::new(),
        };

        let files = collect_files_to_index(dir.path(), &mut stats);

        // .rs and .py files should be collected
        let collected: Vec<_> = files
            .iter()
            .filter_map(|p| p.extension().and_then(|e| e.to_str()))
            .collect();
        assert!(collected.contains(&"rs"), "expected .rs to be collected");
        assert!(collected.contains(&"py"), "expected .py to be collected");

        // .java should appear in skipped_by_extension
        assert_eq!(
            stats
                .skipped_by_extension
                .get(".java")
                .copied()
                .unwrap_or(0),
            1,
            "expected 1 java file in skipped_by_extension"
        );
        assert!(
            !stats.skipped_by_extension.contains_key(".rs"),
            ".rs should not appear in skipped_by_extension"
        );
        assert!(
            !stats.skipped_by_extension.contains_key(".py"),
            ".py should not appear in skipped_by_extension"
        );
    }
}
