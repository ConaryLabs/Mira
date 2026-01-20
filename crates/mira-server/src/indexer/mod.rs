// crates/mira-server/src/indexer/mod.rs
// Code indexing for symbol extraction and semantic search

pub mod parsers;

use crate::config::ignore;
use crate::db::Database;
use crate::embeddings::Embeddings;
use crate::search::embedding_to_bytes;
use anyhow::{Context, Result};
use rusqlite::params;
use std::path::Path;
use std::sync::Arc;
use tree_sitter::Parser;
use walkdir::WalkDir;

pub use parsers::{Import, Symbol};

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

/// Create semantic chunks based on symbol boundaries
/// Each chunk is a complete function/struct/etc with context metadata
fn create_semantic_chunks(content: &str, symbols: &[ParsedSymbol]) -> Vec<CodeChunk> {
    let lines: Vec<&str> = content.lines().collect();
    let mut chunks: Vec<CodeChunk> = Vec::new();
    let mut covered_lines: std::collections::HashSet<u32> = std::collections::HashSet::new();

    // Sort symbols by start line
    let mut sorted_symbols: Vec<&ParsedSymbol> = symbols.iter().collect();
    sorted_symbols.sort_by_key(|s| s.start_line);

    // Create a chunk for each symbol
    for sym in &sorted_symbols {
        let start = sym.start_line.saturating_sub(1) as usize; // 1-indexed to 0-indexed
        let end = std::cmp::min(sym.end_line as usize, lines.len());

        if start >= lines.len() {
            continue;
        }

        // Mark lines as covered
        for line in sym.start_line..=sym.end_line {
            covered_lines.insert(line);
        }

        // Extract symbol code
        let symbol_code: String = lines[start..end].join("\n");

        // Skip empty symbols
        if symbol_code.trim().is_empty() {
            continue;
        }

        // Add context header for better semantic matching
        let context = match sym.signature.as_ref() {
            Some(sig) => format!("// {} {}: {}\n{}", sym.kind, sym.name, sig, symbol_code),
            None => format!("// {} {}\n{}", sym.kind, sym.name, symbol_code),
        };

        // If symbol is very large (>2000 chars), split at logical boundaries
        if context.len() > 2000 {
            // Split into ~1000 char chunks at line boundaries
            let mut current_chunk = String::new();
            for line in context.lines() {
                if current_chunk.len() + line.len() > 1000 && !current_chunk.is_empty() {
                    chunks.push(CodeChunk { content: current_chunk, start_line: sym.start_line });
                    current_chunk = format!("// {} {} (continued)\n", sym.kind, sym.name);
                }
                current_chunk.push_str(line);
                current_chunk.push('\n');
            }
            if !current_chunk.trim().is_empty() {
                chunks.push(CodeChunk { content: current_chunk, start_line: sym.start_line });
            }
        } else {
            chunks.push(CodeChunk { content: context, start_line: sym.start_line });
        }
    }

    // Handle orphan code (not part of any symbol) - typically module-level items
    let total_lines = lines.len() as u32;
    let mut orphan_start: Option<u32> = None;

    for line_num in 1..=total_lines {
        if !covered_lines.contains(&line_num) {
            if orphan_start.is_none() {
                orphan_start = Some(line_num);
            }
        } else if let Some(start) = orphan_start {
            // End of orphan region - create chunk if substantial
            let orphan_code: String = lines[(start - 1) as usize..(line_num - 1) as usize].join("\n");
            if orphan_code.trim().len() > 50 {
                chunks.push(CodeChunk {
                    content: format!("// module-level code\n{}", orphan_code),
                    start_line: start,
                });
            }
            orphan_start = None;
        }
    }

    // Handle trailing orphan code
    if let Some(start) = orphan_start {
        let orphan_code: String = lines[(start - 1) as usize..].join("\n");
        if orphan_code.trim().len() > 50 {
            chunks.push(CodeChunk {
                content: format!("// module-level code\n{}", orphan_code),
                start_line: start,
            });
        }
    }

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

/// Index an entire project
pub async fn index_project(
    path: &Path,
    db: Arc<Database>,
    embeddings: Option<Arc<Embeddings>>,
    project_id: Option<i64>,
) -> Result<IndexStats> {
    tracing::info!("Starting index_project for {:?}", path);

    let mut stats = IndexStats {
        files: 0,
        symbols: 0,
        chunks: 0,
        errors: 0,
    };
    const CHUNK_FLUSH_THRESHOLD: usize = 1000;

    // Helper to flush accumulated chunks to database
    async fn flush_chunks(
        pending_chunks: &mut Vec<PendingChunk>,
        db: Arc<Database>,
        embeddings: Option<Arc<Embeddings>>,
        project_id: Option<i64>,
        stats: &mut IndexStats,
    ) -> Result<()> {
        if pending_chunks.is_empty() {
            return Ok(());
        }

        if let Some(ref emb) = embeddings {
            let chunk_count = pending_chunks.len();
            tracing::info!("Flushing {} chunks...", chunk_count);

            // Extract texts for embedding
            let texts: Vec<String> = pending_chunks.iter().map(|c| c.content.clone()).collect();

            // Embed all at once (client handles batching internally)
            match emb.embed_batch(&texts).await {
                Ok(vectors) => {
                    tracing::info!("Embedded {} chunks", vectors.len());

                    // Store all embeddings (runs on blocking thread pool with transaction)
                    let chunk_data: Vec<_> = pending_chunks
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
                        .collect();

                    let error_count = Database::run_blocking(db.clone(), move |conn| {
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
                        Ok::<_, rusqlite::Error>(errors)
                    }).await.unwrap_or(chunk_count);

                    stats.chunks += chunk_count - error_count;
                    stats.errors += error_count;
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

    tracing::info!("Collecting files...");

    // Collect files to index, tracking any walk errors
    let mut files = Vec::new();
    for entry in WalkDir::new(path)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Skip hidden, build outputs, dependencies, assets
            !ignore::should_skip(&name)
        })
    {
        match entry {
            Ok(e) if e.file_type().is_file() => {
                if matches!(
                    e.path().extension().and_then(|ext| ext.to_str()),
                    Some("rs" | "py" | "ts" | "tsx" | "js" | "jsx" | "go")
                ) {
                    files.push(e);
                }
            }
            Ok(_) => {} // Directory, skip
            Err(e) => {
                tracing::warn!("Failed to access path during indexing: {}", e);
                stats.errors += 1;
            }
        }
    }

    tracing::info!("Found {} files to index", files.len());

    // Clear existing data for this project (runs on blocking thread pool)
    tracing::info!("Clearing existing data...");
    Database::run_blocking(db.clone(), move |conn| {
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
    }).await?;

    tracing::info!("Processing files...");

    // Collect chunks for batch embedding
    let mut pending_chunks: Vec<PendingChunk> = Vec::new();

    // Index each file (parse and store symbols, collect chunks)
    for (i, entry) in files.iter().enumerate() {
        let file_path = entry.path();
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
                stats.symbols += symbols.len();

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

                // Store symbols, imports, and calls in database (runs on blocking thread pool)
                let db_start = std::time::Instant::now();
                let rel_path = relative_path.clone();
                let call_count = calls.len();
                Database::run_blocking(db.clone(), move |conn| {
                    let tx = conn.unchecked_transaction()?;

                    // Store symbols and capture IDs immediately (keyed by name+start_line for uniqueness)
                    // This avoids the issue where duplicate names (nested functions, impl methods) overwrite each other
                    let mut symbol_ranges: Vec<(String, u32, u32, i64)> = Vec::new(); // (name, start, end, id)

                    for sym in &symbols {
                        tx.execute(
                            "INSERT INTO code_symbols (project_id, file_path, name, symbol_type, start_line, end_line, signature)
                             VALUES (?, ?, ?, ?, ?, ?, ?)",
                            params![
                                project_id,
                                &rel_path,
                                sym.name,
                                sym.symbol_type,
                                sym.start_line,
                                sym.end_line,
                                sym.signature
                            ],
                        )?;
                        let id = tx.last_insert_rowid();
                        symbol_ranges.push((sym.name.clone(), sym.start_line, sym.end_line, id));
                    }

                    // Store imports
                    for import in &imports {
                        tx.execute(
                            "INSERT OR IGNORE INTO imports (project_id, file_path, import_path, is_external)
                             VALUES (?, ?, ?, ?)",
                            params![
                                project_id,
                                &rel_path,
                                import.import_path,
                                import.is_external as i32
                            ],
                        )?;
                    }

                    // Store function calls for call graph
                    // Find caller by matching call_line to symbol's line range (handles nested functions correctly)
                    for call in &calls {
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

                            tx.execute(
                                "INSERT INTO call_graph (caller_id, callee_name, callee_id)
                                 VALUES (?, ?, ?)",
                                params![cid, call.callee_name, callee_id],
                            )?;
                        }
                    }

                    tx.commit()?;
                    Ok::<_, rusqlite::Error>(())
                }).await?;
                tracing::debug!("  DB inserts in {:?} ({} calls)", db_start.elapsed(), call_count);

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
                        flush_chunks(
                            &mut pending_chunks,
                            db.clone(),
                            embeddings.clone(),
                            project_id,
                            &mut stats,
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

    // Flush any remaining chunks
    flush_chunks(
        &mut pending_chunks,
        db.clone(),
        embeddings.clone(),
        project_id,
        &mut stats,
    ).await?;

    // Rebuild FTS5 full-text search index for this project
    if let Some(pid) = project_id {
        tracing::info!("Rebuilding FTS5 search index for project {}", pid);
        if let Err(e) = db.rebuild_fts_for_project(pid) {
            tracing::warn!("Failed to rebuild FTS5 index: {}", e);
        }
    }

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
