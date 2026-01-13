// src/indexer/mod.rs
// Code indexing for symbol extraction and semantic search

pub mod parsers;

use crate::db::Database;
use crate::embeddings::Embeddings;
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
    let (symbols, _, _) = extract_all(path)?;
    Ok(symbols)
}

/// Extract symbols, imports, and calls from a single file
pub fn extract_all(path: &Path) -> Result<(Vec<Symbol>, Vec<Import>, Vec<parsers::FunctionCall>)> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    let mut parser = create_parser(ext).ok_or_else(|| anyhow::anyhow!("Unsupported file type"))?;

    let result = match ext {
        "rs" => parsers::rust::parse(&mut parser, &content)?,
        "py" => parsers::python::parse(&mut parser, &content)?,
        "ts" | "tsx" | "js" | "jsx" => parsers::typescript::parse(&mut parser, &content)?,
        "go" => parsers::go::parse(&mut parser, &content)?,
        _ => return Ok((vec![], vec![], vec![])),
    };

    Ok(result)
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

    tracing::info!("Collecting files...");

    // Collect files to index, tracking any walk errors
    let mut files = Vec::new();
    for entry in WalkDir::new(path)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Skip hidden, build outputs, dependencies, assets
            !name.starts_with('.')
                && name != "node_modules"
                && name != "target"
                && name != "assets"
                && name != "pkg"      // wasm-pack output
                && name != "dist"     // common build output
                && name != "build"    // common build output
                && name != "vendor"   // vendored deps
                && name != "__pycache__"
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

    // Clear existing data for this project
    tracing::info!("Clearing existing data...");
    {
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
    }

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
            Ok((symbols, imports, calls)) => {
                let parse_time = start.elapsed();
                stats.files += 1;
                stats.symbols += symbols.len();

                tracing::info!("  Parsed in {:?} ({} symbols, {} imports)", parse_time, symbols.len(), imports.len());

                // Store symbols, imports, and calls in database
                {
                    let db_start = std::time::Instant::now();
                    let conn = db.conn();

                    // Store symbols
                    for sym in &symbols {
                        conn.execute(
                            "INSERT INTO code_symbols (project_id, file_path, name, symbol_type, start_line, end_line, signature)
                             VALUES (?, ?, ?, ?, ?, ?, ?)",
                            params![
                                project_id,
                                relative_path,
                                sym.name,
                                sym.symbol_type,
                                sym.start_line,
                                sym.end_line,
                                sym.signature
                            ],
                        )?;
                    }

                    // Store imports
                    for import in &imports {
                        conn.execute(
                            "INSERT OR IGNORE INTO imports (project_id, file_path, import_path, is_external)
                             VALUES (?, ?, ?, ?)",
                            params![
                                project_id,
                                relative_path,
                                import.import_path,
                                import.is_external as i32
                            ],
                        )?;
                    }

                    // Store function calls for call graph
                    let mut symbol_ids: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
                    {
                        let mut stmt = conn.prepare(
                            "SELECT id, name FROM code_symbols WHERE project_id = ? AND file_path = ?"
                        )?;
                        let rows = stmt.query_map(params![project_id, &relative_path], |row| {
                            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                        })?;
                        for row in rows {
                            if let Ok((id, name)) = row {
                                symbol_ids.insert(name, id);
                            }
                        }
                    }

                    for call in &calls {
                        if let Some(&caller_id) = symbol_ids.get(&call.caller_name) {
                            let callee_id = symbol_ids.get(&call.callee_name).copied();
                            conn.execute(
                                "INSERT INTO call_graph (caller_id, callee_name, callee_id)
                                 VALUES (?, ?, ?)",
                                params![caller_id, call.callee_name, callee_id],
                            )?;
                        }
                    }

                    tracing::debug!("  DB inserts in {:?} ({} calls)", db_start.elapsed(), calls.len());
                }

                // Collect chunks for batch embedding (if embeddings enabled)
                if embeddings.is_some() {
                    if let Ok(content) = std::fs::read_to_string(file_path) {
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
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to parse {}: {}", file_path.display(), e);
                stats.errors += 1;
            }
        }
    }

    // Batch embed all collected chunks
    if let Some(ref emb) = embeddings {
        if !pending_chunks.is_empty() {
            tracing::info!("Embedding {} chunks in parallel batches...", pending_chunks.len());
            let embed_start = std::time::Instant::now();

            // Extract texts for embedding
            let texts: Vec<String> = pending_chunks.iter().map(|c| c.content.clone()).collect();

            // Embed all at once (client handles batching internally)
            match emb.embed_batch(&texts).await {
                Ok(vectors) => {
                    tracing::info!("Embedded {} chunks in {:?}", vectors.len(), embed_start.elapsed());

                    // Store all embeddings
                    let conn = db.conn();
                    for (chunk, embedding) in pending_chunks.iter().zip(vectors.iter()) {
                        let embedding_bytes: Vec<u8> = embedding
                            .iter()
                            .flat_map(|f| f.to_le_bytes())
                            .collect();

                        if let Err(e) = conn.execute(
                            "INSERT INTO vec_code (embedding, file_path, chunk_content, project_id, start_line)
                             VALUES (?, ?, ?, ?, ?)",
                            params![embedding_bytes, chunk.file_path, chunk.content, project_id, chunk.start_line],
                        ) {
                            tracing::warn!("Failed to store embedding ({}:{}): {}", chunk.file_path, chunk.start_line, e);
                            stats.errors += 1;
                        } else {
                            stats.chunks += 1;
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Batch embedding failed: {}", e);
                    stats.errors += pending_chunks.len();
                }
            }
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
