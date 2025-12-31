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
use zerocopy::AsBytes;

pub use parsers::Symbol;

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
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    let mut parser = create_parser(ext).ok_or_else(|| anyhow::anyhow!("Unsupported file type"))?;

    let (symbols, _, _) = match ext {
        "rs" => parsers::rust::parse(&mut parser, &content)?,
        "py" => parsers::python::parse(&mut parser, &content)?,
        "ts" | "tsx" | "js" | "jsx" => parsers::typescript::parse(&mut parser, &content)?,
        "go" => parsers::go::parse(&mut parser, &content)?,
        _ => return Ok(vec![]),
    };

    Ok(symbols)
}

/// Index an entire project
pub async fn index_project(
    path: &Path,
    db: Arc<Database>,
    embeddings: Option<Arc<Embeddings>>,
    project_id: Option<i64>,
) -> Result<IndexStats> {
    let mut stats = IndexStats {
        files: 0,
        symbols: 0,
        chunks: 0,
    };

    // Collect files to index
    let files: Vec<_> = WalkDir::new(path)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Skip hidden, node_modules, target, .git
            !name.starts_with('.') && name != "node_modules" && name != "target"
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

    // Clear existing symbols for this project
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
    }

    // Index each file
    for entry in files {
        let file_path = entry.path();
        let relative_path = file_path
            .strip_prefix(path)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        match extract_symbols(file_path) {
            Ok(symbols) => {
                stats.files += 1;
                stats.symbols += symbols.len();

                let conn = db.conn();
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
            }
            Err(e) => {
                tracing::warn!("Failed to parse {}: {}", file_path.display(), e);
            }
        }

        // Index code chunks for semantic search
        if let Some(ref emb) = embeddings {
            if let Ok(content) = std::fs::read_to_string(file_path) {
                // Simple chunking: split into ~500 char chunks
                for chunk in content.chars().collect::<Vec<_>>().chunks(500) {
                    let chunk_text: String = chunk.iter().collect();
                    if chunk_text.trim().is_empty() {
                        continue;
                    }

                    match emb.embed(&chunk_text).await {
                        Ok(embedding) => {
                            let conn = db.conn();
                            conn.execute(
                                "INSERT INTO vec_code (embedding, file_path, chunk_content, project_id)
                                 VALUES (?, ?, ?, ?)",
                                params![
                                    embedding.as_bytes(),
                                    relative_path,
                                    chunk_text,
                                    project_id
                                ],
                            )?;
                            stats.chunks += 1;
                        }
                        Err(e) => {
                            tracing::debug!("Failed to embed chunk: {}", e);
                        }
                    }
                }
            }
        }
    }

    Ok(stats)
}
