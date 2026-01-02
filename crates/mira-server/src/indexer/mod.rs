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

/// Result of parsing file content for incremental updates
pub struct FileParseResult {
    pub symbols: Vec<ParsedSymbol>,
    pub imports: Vec<ParsedImport>,
    pub chunks: Vec<String>,
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

    // Create chunks (~500 chars each)
    let chunks: Vec<String> = content
        .chars()
        .collect::<Vec<_>>()
        .chunks(500)
        .map(|c| c.iter().collect::<String>())
        .filter(|c| !c.trim().is_empty())
        .collect();

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
    };

    tracing::info!("Collecting files...");

    // Collect files to index
    let files: Vec<_> = WalkDir::new(path)
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
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            matches!(
                e.path().extension().and_then(|e| e.to_str()),
                Some("rs" | "py" | "ts" | "tsx" | "js" | "jsx" | "go")
            )
        })
        .collect();

    tracing::info!("Found {} files to index", files.len());

    // Clear existing data for this project
    tracing::info!("Clearing existing data...");
    {
        let conn = db.conn();
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
        conn.execute(
            "DELETE FROM pending_embeddings WHERE project_id = ?",
            params![project_id],
        )?;
    }

    tracing::info!("Processing files...");

    // Index each file
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
            Ok((symbols, imports, _calls)) => {
                let parse_time = start.elapsed();
                stats.files += 1;
                stats.symbols += symbols.len();

                tracing::info!("  Parsed in {:?} ({} symbols, {} imports)", parse_time, symbols.len(), imports.len());

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

                tracing::debug!("  DB inserts in {:?}", db_start.elapsed());
            }
            Err(e) => {
                tracing::warn!("Failed to parse {}: {}", file_path.display(), e);
            }
        }

        // Queue code chunks for batch embedding (50% cheaper via OpenAI Batch API)
        // The background worker will process these
        if embeddings.is_some() {
            if let Ok(content) = std::fs::read_to_string(file_path) {
                let conn = db.conn();
                // Simple chunking: split into ~500 char chunks
                for chunk in content.chars().collect::<Vec<_>>().chunks(500) {
                    let chunk_text: String = chunk.iter().collect();
                    if chunk_text.trim().is_empty() {
                        continue;
                    }

                    // Queue for batch processing instead of embedding inline
                    if let Err(e) = conn.execute(
                        "INSERT INTO pending_embeddings (project_id, file_path, chunk_content, status)
                         VALUES (?, ?, ?, 'pending')",
                        params![project_id, relative_path, chunk_text],
                    ) {
                        tracing::debug!("Failed to queue chunk for embedding: {}", e);
                    } else {
                        stats.chunks += 1;
                    }
                }
            }
        }
    }

    tracing::info!("Indexing complete: {} files, {} symbols", stats.files, stats.symbols);
    Ok(stats)
}
