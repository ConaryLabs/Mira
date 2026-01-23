// crates/mira-server/src/indexer/mod.rs
// Code indexing for symbol extraction and semantic search

pub mod parsers;

use crate::db::Database;
use crate::embeddings::EmbeddingClient;
use crate::project_files::walker::FileWalker;
use crate::search::embedding_to_bytes;
use anyhow::{Context, Result};
use rusqlite::params;
use std::path::Path;
use std::sync::Arc;
use tree_sitter::Parser;

pub use parsers::{Import, Symbol, FunctionCall};

/// Index statistics
pub struct IndexStats {
    pub files: usize,
    pub symbols: usize,
    pub chunks: usize,
    pub errors: usize,
}

/// Pending chunk for batch embedding
struct PendingChunk {
    file_path: String,
    start_line: usize,
    content: String,
}

/// Pending file data for batch database insertion
struct PendingFileBatch {
    file_path: String,
    symbols: Vec<Symbol>,
    imports: Vec<Import>,
    calls: Vec<FunctionCall>,
}

/// Maximum symbols to accumulate before flushing to database
const SYMBOL_FLUSH_THRESHOLD: usize = 1000;
/// Maximum files to accumulate before flushing to database
const FILE_FLUSH_THRESHOLD: usize = 100;
/// Maximum chunks to accumulate before flushing to database
const CHUNK_FLUSH_THRESHOLD: usize = 1000;

/// Helper to embed chunks and return vectors
async fn embed_chunks(
    embeddings: &EmbeddingClient,
    pending_chunks: &[PendingChunk],
) -> Result<Vec<Vec<f32>>, String> {
    let texts: Vec<String> = pending_chunks.iter().map(|c| c.content.clone()).collect();
    embeddings.embed_batch(&texts).await.map_err(|e| e.to_string())
}

/// Helper to prepare chunk data for database storage
fn prepare_chunk_data(
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
async fn store_chunk_embeddings(
    db: Arc<Database>,
    chunk_data: Vec<(String, String, usize, Vec<u8>)>,
    project_id: Option<i64>,
) -> Result<usize, rusqlite::Error> {
    tokio::task::spawn_blocking(move || {
        let conn = db.conn();
        let tx = conn.unchecked_transaction()?;
        let mut errors = 0usize;

        for (file_path, content, start_line, embedding_bytes) in &chunk_data {
            if let Err(e) = tx.execute(
                "INSERT INTO vec_code (embedding, file_path, chunk_content, project_id, start_line)
                 VALUES (?, ?, ?, ?, ?)",
                params![embedding_bytes, file_path, content, project_id, start_line],
            ) {
                tracing::warn!("Failed to store embedding ({}:{}): {}", file_path, start_line, e);
                errors += 1;
            }
        }

        tx.commit()?;
        Ok(errors)
    }).await.expect("store_chunk_embeddings spawn_blocking panicked")
}

/// Flush accumulated chunks to database and generate embeddings
async fn flush_chunks(
    mut pending_chunks: Vec<PendingChunk>,
    db: Arc<Database>,
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

                match store_chunk_embeddings(db.clone(), chunk_data, project_id).await {
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
fn store_symbols_and_capture_ids(
    tx: &rusqlite::Transaction,
    project_id: Option<i64>,
    file_path: &str,
    symbols: &[Symbol],
) -> rusqlite::Result<(Vec<(String, u32, u32, i64)>, usize)> {
    let mut symbol_ranges = Vec::new();
    let mut errors = 0usize;

    for sym in symbols {
        if let Err(e) = tx.execute(
            "INSERT INTO code_symbols (project_id, file_path, name, symbol_type, start_line, end_line, signature)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                project_id,
                file_path,
                sym.name,
                sym.symbol_type,
                sym.start_line,
                sym.end_line,
                sym.signature
            ],
        ) {
            tracing::warn!("Failed to store symbol {} ({}): {}", sym.name, file_path, e);
            errors += 1;
            continue;
        }
        let id = tx.last_insert_rowid();
        symbol_ranges.push((sym.name.clone(), sym.start_line, sym.end_line, id));
    }

    Ok((symbol_ranges, errors))
}

/// Helper to store imports
fn store_imports(
    tx: &rusqlite::Transaction,
    project_id: Option<i64>,
    file_path: &str,
    imports: &[Import],
) -> rusqlite::Result<usize> {
    let mut errors = 0usize;

    for import in imports {
        if let Err(e) = tx.execute(
            "INSERT OR IGNORE INTO imports (project_id, file_path, import_path, is_external)
             VALUES (?, ?, ?, ?)",
            params![
                project_id,
                file_path,
                import.import_path,
                import.is_external as i32
            ],
        ) {
            tracing::warn!("Failed to store import {} ({}): {}", import.import_path, file_path, e);
            errors += 1;
        }
    }

    Ok(errors)
}

/// Helper to store function calls
fn store_function_calls(
    tx: &rusqlite::Transaction,
    file_path: &str,
    calls: &[FunctionCall],
    symbol_ranges: &[(String, u32, u32, i64)],
) -> rusqlite::Result<usize> {
    let mut errors = 0usize;

    for call in calls {
        // Find the caller symbol whose line range contains this call
        let caller_id = symbol_ranges.iter()
            .find(|(name, start, end, _)| {
                name == &call.caller_name && call.call_line >= *start && call.call_line <= *end
            })
            .map(|(_, _, _, id)| *id);

        if let Some(cid) = caller_id {
            // Try to find callee ID (may be in same file)
            let callee_id = symbol_ranges.iter()
                .find(|(name, _, _, _)| name == &call.callee_name)
                .map(|(_, _, _, id)| *id);

            if let Err(e) = tx.execute(
                "INSERT INTO call_graph (caller_id, callee_name, callee_id)
                 VALUES (?, ?, ?)",
                params![cid, call.callee_name, callee_id],
            ) {
                tracing::warn!("Failed to store call {} -> {} ({}): {}", call.caller_name, call.callee_name, file_path, e);
                errors += 1;
            }
        } else {
            // Caller not found (could be module-level call)
            tracing::debug!("Skipping call {} -> {} (caller not found in {})", call.caller_name, call.callee_name, file_path);
        }
    }

    Ok(errors)
}

/// Flush accumulated file data (symbols, imports, calls) to database
async fn flush_code_batch(
    pending_batches: &mut Vec<PendingFileBatch>,
    db: Arc<Database>,
    project_id: Option<i64>,
    stats: &mut IndexStats,
) -> Result<()> {
    if pending_batches.is_empty() {
        return Ok(());
    }

    let batches = std::mem::take(pending_batches);
    let total_symbols: usize = batches.iter().map(|b| b.symbols.len()).sum();
    let total_calls: usize = batches.iter().map(|b| b.calls.len()).sum();
    tracing::info!("Flushing {} files ({} symbols, {} calls)...", batches.len(), total_symbols, total_calls);

    // Process all batches in a single transaction
    let error_count = tokio::task::spawn_blocking(move || {
        let conn = db.conn();
        let tx = conn.unchecked_transaction()?;
        let mut total_errors = 0usize;

        // Process each file batch
        for batch in batches {
            // Store symbols and capture IDs
            let (symbol_ranges, symbol_errors) = store_symbols_and_capture_ids(
                &tx, project_id, &batch.file_path, &batch.symbols
            )?;
            total_errors += symbol_errors;

            // Store imports
            let import_errors = store_imports(
                &tx, project_id, &batch.file_path, &batch.imports
            )?;
            total_errors += import_errors;

            // Store function calls for call graph
            let call_errors = store_function_calls(
                &tx, &batch.file_path, &batch.calls, &symbol_ranges
            )?;
            total_errors += call_errors;
        }

        tx.commit()?;
        Ok::<_, rusqlite::Error>(total_errors)
    }).await.expect("flush_code_batch spawn_blocking panicked")?;

    stats.symbols += total_symbols - error_count;
    stats.errors += error_count;

    // pending_batches already cleared by std::mem::take
    Ok(())
}

/// Create a parser for a given file extension
fn create_parser(ext: &str) -> Option<Parser> {
    let mut parser = Parser::new();
    let language = match ext {
        "rs" => tree_sitter_rust::LANGUAGE,
        "py" => tree_sitter_python::LANGUAGE,
        "ts" | "tsx" | "js" | "jsx" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        "go" => tree_sitter_go::LANGUAGE,
        _ => return None,
    };
    parser.set_language(&language.into()).ok()?;
    Some(parser)
}

/// Extract symbols from a single file
pub fn extract_symbols(path: &Path) -> Result<Vec<Symbol>> {
    let (symbols, _, _, _) = extract_all(path)?;
    Ok(symbols)
}

/// Extract symbols, imports, calls, and file content from a single file
pub fn extract_all(path: &Path) -> Result<(Vec<Symbol>, Vec<Import>, Vec<parsers::FunctionCall>, String)> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    let mut parser = create_parser(ext).ok_or_else(|| anyhow::anyhow!("Unsupported file type"))?;

    let (symbols, imports, calls) = match ext {
        "rs" => parsers::rust::parse(&mut parser, &content)?,
        "py" => parsers::python::parse(&mut parser, &content)?,
        "ts" | "tsx" | "js" | "jsx" => parsers::typescript::parse(&mut parser, &content)?,
        "go" => parsers::go::parse(&mut parser, &content)?,
        _ => return Ok((vec![], vec![], vec![], content)),
    };

    Ok((symbols, imports, calls, content))
}

/// A code chunk with content and location info
pub struct CodeChunk {
    pub content: String,
    pub start_line: u32,
}

/// Result of parsing file content for incremental updates
pub struct FileParseResult {
    pub symbols: Vec<ParsedSymbol>,
    pub imports: Vec<ParsedImport>,
    pub chunks: Vec<CodeChunk>,
}

/// Simplified symbol for incremental indexing
pub struct ParsedSymbol {
    pub name: String,
    pub kind: String,
    pub start_line: u32,
    pub end_line: u32,
    pub signature: Option<String>,
}

/// Simplified import for incremental indexing
pub struct ParsedImport {
    pub path: String,
    pub is_external: bool,
}

/// Create chunks for a single symbol, handling large symbol splitting
fn create_chunks_for_symbol(sym: &ParsedSymbol, lines: &[&str]) -> Vec<CodeChunk> {
    let start = sym.start_line.saturating_sub(1) as usize; // 1-indexed to 0-indexed
    let end = std::cmp::min(sym.end_line as usize, lines.len());

    if start >= lines.len() {
        return Vec::new();
    }

    // Build context directly from lines to avoid intermediate allocation
    let mut context = String::with_capacity((end - start) * 20); // Estimate average line length
    match sym.signature.as_ref() {
        Some(sig) => context.push_str(&format!("// {} {}: {}\n", sym.kind, sym.name, sig)),
        None => context.push_str(&format!("// {} {}\n", sym.kind, sym.name)),
    }

    // Append symbol lines
    for line in &lines[start..end] {
        context.push_str(line);
        context.push('\n');
    }

    // Skip empty symbols
    if context.trim().is_empty() {
        return Vec::new();
    }

    // If symbol is very large (>2000 chars), split at logical boundaries
    if context.len() > 2000 {
        split_large_chunk(context, sym.start_line, &sym.kind, &sym.name)
    } else {
        vec![CodeChunk { content: context, start_line: sym.start_line }]
    }
}

/// Split a large chunk into smaller chunks at line boundaries
fn split_large_chunk(chunk: String, start_line: u32, kind: &str, name: &str) -> Vec<CodeChunk> {
    let mut result = Vec::new();
    let mut current_chunk = String::with_capacity(1000);
    let lines = chunk.lines().collect::<Vec<_>>();

    for line in lines {
        if current_chunk.len() + line.len() > 1000 && !current_chunk.is_empty() {
            result.push(CodeChunk { content: current_chunk, start_line });
            current_chunk = String::with_capacity(1000);
            current_chunk.push_str(&format!("// {} {} (continued)\n", kind, name));
        }
        current_chunk.push_str(line);
        current_chunk.push('\n');
    }

    if !current_chunk.trim().is_empty() {
        result.push(CodeChunk { content: current_chunk, start_line });
    }

    result
}

/// Create chunks for orphan code (lines not covered by any symbol)
fn create_chunks_for_orphan_code(lines: &[&str], covered_lines: &std::collections::HashSet<u32>) -> Vec<CodeChunk> {
    let total_lines = lines.len() as u32;
    let mut chunks = Vec::new();
    let mut orphan_start: Option<u32> = None;

    for line_num in 1..=total_lines {
        if !covered_lines.contains(&line_num) {
            if orphan_start.is_none() {
                orphan_start = Some(line_num);
            }
        } else if let Some(start) = orphan_start {
            // End of orphan region - create chunk if substantial
            let start_idx = (start - 1) as usize;
            let end_idx = (line_num - 1) as usize;

            // Check if region has substantial non-whitespace content
            let has_substantial_content = lines[start_idx..end_idx].iter()
                .any(|line| line.trim().len() > 10);

            if has_substantial_content {
                let mut content = String::with_capacity((end_idx - start_idx) * 20);
                content.push_str("// module-level code\n");
                for line in &lines[start_idx..end_idx] {
                    content.push_str(line);
                    content.push('\n');
                }
                chunks.push(CodeChunk {
                    content,
                    start_line: start,
                });
            }
            orphan_start = None;
        }
    }

    // Handle trailing orphan code
    if let Some(start) = orphan_start {
        let start_idx = (start - 1) as usize;

        // Check if region has substantial non-whitespace content
        let has_substantial_content = lines[start_idx..].iter()
            .any(|line| line.trim().len() > 10);

        if has_substantial_content {
            let mut content = String::with_capacity((lines.len() - start_idx) * 20);
            content.push_str("// module-level code\n");
            for line in &lines[start_idx..] {
                content.push_str(line);
                content.push('\n');
            }
            chunks.push(CodeChunk {
                content,
                start_line: start,
            });
        }
    }

    chunks
}

/// Create semantic chunks based on symbol boundaries
/// Each chunk is a complete function/struct/etc with context metadata
fn create_semantic_chunks(content: &str, symbols: &[ParsedSymbol]) -> Vec<CodeChunk> {
    let lines: Vec<&str> = content.lines().collect();
    let mut chunks: Vec<CodeChunk> = Vec::with_capacity(symbols.len());
    let mut covered_lines: std::collections::HashSet<u32> = std::collections::HashSet::with_capacity(lines.len());

    // Sort symbols by start line
    let mut sorted_symbols: Vec<&ParsedSymbol> = symbols.iter().collect();
    sorted_symbols.sort_by_key(|s| s.start_line);

    // Create chunks for each symbol
    for sym in &sorted_symbols {
        // Mark lines as covered
        for line in sym.start_line..=sym.end_line {
            covered_lines.insert(line);
        }

        // Create chunks for this symbol
        let mut symbol_chunks = create_chunks_for_symbol(sym, &lines);
        chunks.append(&mut symbol_chunks);
    }

    // Create chunks for orphan code (lines not covered by any symbol)
    let mut orphan_chunks = create_chunks_for_orphan_code(&lines, &covered_lines);
    chunks.append(&mut orphan_chunks);

    chunks
}

/// Parse file content directly (for incremental updates)
/// Returns symbols, imports, and content chunks for embedding
pub fn parse_file(content: &str, language: &str) -> Result<FileParseResult> {
    let ext = match language {
        "rust" => "rs",
        "python" => "py",
        "typescript" | "javascript" => "ts",
        "go" => "go",
        _ => return Err(anyhow::anyhow!("Unsupported language: {}", language)),
    };

    let mut parser = create_parser(ext).ok_or_else(|| anyhow::anyhow!("Failed to create parser"))?;

    let (symbols, imports, _) = match ext {
        "rs" => parsers::rust::parse(&mut parser, content)?,
        "py" => parsers::python::parse(&mut parser, content)?,
        "ts" => parsers::typescript::parse(&mut parser, content)?,
        "go" => parsers::go::parse(&mut parser, content)?,
        _ => (vec![], vec![], vec![]),
    };

    // Convert to simplified types
    let parsed_symbols: Vec<ParsedSymbol> = symbols
        .into_iter()
        .map(|s| ParsedSymbol {
            name: s.name,
            kind: s.symbol_type,
            start_line: s.start_line,
            end_line: s.end_line,
            signature: s.signature,
        })
        .collect();

    let parsed_imports: Vec<ParsedImport> = imports
        .into_iter()
        .map(|i| ParsedImport {
            path: i.import_path,
            is_external: i.is_external,
        })
        .collect();

    // AST-aware chunking: chunk at symbol boundaries
    let chunks = create_semantic_chunks(content, &parsed_symbols);

    Ok(FileParseResult {
        symbols: parsed_symbols,
        imports: parsed_imports,
        chunks,
    })
}

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
async fn clear_existing_project_data(db: Arc<Database>, project_id: Option<i64>) -> Result<()> {
    tracing::info!("Clearing existing data...");
    tokio::task::spawn_blocking(move || {
        let conn = db.conn();
        // Delete call_graph first (references code_symbols)
        conn.execute(
            "DELETE FROM call_graph WHERE caller_id IN (SELECT id FROM code_symbols WHERE project_id = ?)",
            params![project_id],
        )?;
        conn.execute(
            "DELETE FROM code_symbols WHERE project_id = ?",
            params![project_id],
        )?;
        conn.execute(
            "DELETE FROM vec_code WHERE project_id = ?",
            params![project_id],
        )?;
        conn.execute(
            "DELETE FROM imports WHERE project_id = ?",
            params![project_id],
        )?;
        conn.execute(
            "DELETE FROM codebase_modules WHERE project_id = ?",
            params![project_id],
        )?;
        Ok::<_, rusqlite::Error>(())
    }).await.expect("clear_existing_project_data spawn_blocking panicked")?;
    Ok(())
}

/// Process files in a loop, accumulating batches and chunks, flushing when thresholds reached
async fn process_files_loop(
    files: Vec<std::path::PathBuf>,
    path: &Path,
    pending_batches: &mut Vec<PendingFileBatch>,
    pending_chunks: &mut Vec<PendingChunk>,
    db: Arc<Database>,
    embeddings: Option<Arc<EmbeddingClient>>,
    project_id: Option<i64>,
    stats: &mut IndexStats,
) -> Result<()> {
    // Index each file (parse and store symbols, collect chunks)
    for (i, file_path) in files.iter().enumerate() {
        let relative_path = file_path
            .strip_prefix(path)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        let start = std::time::Instant::now();
        tracing::info!("[{}/{}] Parsing {}", i+1, files.len(), relative_path);

        match extract_all(file_path) {
            Ok((symbols, imports, calls, content)) => {
                let parse_time = start.elapsed();
                stats.files += 1;

                tracing::info!("  Parsed in {:?} ({} symbols, {} imports)", parse_time, symbols.len(), imports.len());

                // Convert symbols to ParsedSymbol before moving into closure
                // (needed for semantic chunking later)
                let parsed_symbols: Vec<ParsedSymbol> = symbols
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
                    file_path: relative_path.clone(),
                    symbols,
                    imports,
                    calls,
                });

                // Check if we should flush accumulated batches
                let total_batched_symbols: usize = pending_batches.iter().map(|b| b.symbols.len()).sum();
                if total_batched_symbols >= SYMBOL_FLUSH_THRESHOLD || pending_batches.len() >= FILE_FLUSH_THRESHOLD {
                    let flush_start = std::time::Instant::now();
                    flush_code_batch(pending_batches, db.clone(), project_id, stats).await?;
                    tracing::debug!("  Batch flush in {:?}", flush_start.elapsed());
                }

                // Collect chunks for batch embedding (if embeddings enabled)
                // Note: content was already read by extract_all, no need to re-read
                if embeddings.is_some() {
                    let chunks = create_semantic_chunks(&content, &parsed_symbols);
                    for chunk in chunks {
                        if !chunk.content.trim().is_empty() {
                            pending_chunks.push(PendingChunk {
                                file_path: relative_path.clone(),
                                start_line: chunk.start_line as usize,
                                content: chunk.content,
                            });
                        }
                    }

                    // Flush if we've accumulated enough chunks
                    if pending_chunks.len() >= CHUNK_FLUSH_THRESHOLD {
                        let chunks_to_flush = std::mem::replace(pending_chunks, Vec::new());
                        flush_chunks(
                            chunks_to_flush,
                            db.clone(),
                            embeddings.clone(),
                            project_id,
                            stats,
                        ).await?;
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to parse {}: {}", file_path.display(), e);
                stats.errors += 1;
            }
        }
    }
    Ok(())
}

/// Flush any remaining batches and chunks after processing all files
async fn flush_remaining_data(
    pending_batches: &mut Vec<PendingFileBatch>,
    pending_chunks: Vec<PendingChunk>,
    db: Arc<Database>,
    embeddings: Option<Arc<EmbeddingClient>>,
    project_id: Option<i64>,
    stats: &mut IndexStats,
) -> Result<()> {
    // Flush any remaining file batches
    if !pending_batches.is_empty() {
        flush_code_batch(pending_batches, db.clone(), project_id, stats).await?;
    }

    // Flush any remaining chunks
    flush_chunks(
        pending_chunks,
        db.clone(),
        embeddings.clone(),
        project_id,
        stats,
    ).await?;

    Ok(())
}

/// Rebuild FTS5 full-text search index for a project if project_id is Some
fn rebuild_fts_index_if_needed(db: Arc<Database>, project_id: Option<i64>) {
    if let Some(pid) = project_id {
        tracing::info!("Rebuilding FTS5 search index for project {}", pid);
        if let Err(e) = db.rebuild_fts_for_project(pid) {
            tracing::warn!("Failed to rebuild FTS5 index: {}", e);
        }
    }
}

/// Index an entire project
pub async fn index_project(
    path: &Path,
    db: Arc<Database>,
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
    clear_existing_project_data(db.clone(), project_id).await?;

    tracing::info!("Processing files...");

    // Collect chunks for batch embedding
    let mut pending_chunks: Vec<PendingChunk> = Vec::new();
    // Collect file data for batch database insertion
    let mut pending_batches: Vec<PendingFileBatch> = Vec::new();

    // Index each file (parse and store symbols, collect chunks)
    process_files_loop(
        files,
        path,
        &mut pending_batches,
        &mut pending_chunks,
        db.clone(),
        embeddings.clone(),
        project_id,
        &mut stats,
    ).await?;

    // Flush any remaining batches and chunks
    flush_remaining_data(
        &mut pending_batches,
        pending_chunks,
        db.clone(),
        embeddings.clone(),
        project_id,
        &mut stats,
    ).await?;

    // Rebuild FTS5 full-text search index for this project
    rebuild_fts_index_if_needed(db.clone(), project_id);

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
