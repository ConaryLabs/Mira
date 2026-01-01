// src/mcp/tools/code.rs
// Code intelligence tools

use crate::indexer;
use crate::mcp::MiraServer;
use std::path::Path;

/// Get symbols from a file
pub async fn get_symbols(
    _server: &MiraServer,
    file_path: String,
    symbol_type: Option<String>,
) -> Result<String, String> {
    let path = Path::new(&file_path);

    if !path.exists() {
        return Err(format!("File not found: {}", file_path));
    }

    // Parse file for symbols
    let symbols = indexer::extract_symbols(path).map_err(|e| e.to_string())?;

    if symbols.is_empty() {
        return Ok("No symbols found.".to_string());
    }

    // Filter by type if specified
    let symbols: Vec<_> = if let Some(ref filter) = symbol_type {
        symbols
            .into_iter()
            .filter(|s| s.symbol_type.eq_ignore_ascii_case(filter))
            .collect()
    } else {
        symbols
    };

    let total = symbols.len();
    let display: Vec<_> = symbols.into_iter().take(10).collect();

    let mut response = format!("{} symbols:\n", total);
    for sym in &display {
        let lines = if sym.start_line == sym.end_line {
            format!("line {}", sym.start_line)
        } else {
            format!("lines {}-{}", sym.start_line, sym.end_line)
        };
        response.push_str(&format!("  {} ({}) {}\n", sym.name, sym.symbol_type, lines));
    }

    if total > 10 {
        response.push_str(&format!("  ... and {} more\n", total - 10));
    }

    Ok(response)
}

/// Semantic code search
pub async fn semantic_code_search(
    server: &MiraServer,
    query: String,
    _language: Option<String>,
    limit: Option<i64>,
) -> Result<String, String> {
    let limit = limit.unwrap_or(10) as usize;

    // Check if embeddings available
    let embeddings = server
        .embeddings
        .as_ref()
        .ok_or("Semantic search requires GEMINI_API_KEY")?;

    // Get query embedding
    let query_embedding = embeddings
        .embed(&query)
        .await
        .map_err(|e| format!("Embedding failed: {}", e))?;

    let conn = server.db.conn();

    // Search vec_code
    use rusqlite::params;
    use zerocopy::AsBytes;

    let mut stmt = conn
        .prepare(
            "SELECT file_path, chunk_content, distance
             FROM vec_code
             WHERE embedding MATCH ?
             ORDER BY distance
             LIMIT ?",
        )
        .map_err(|e| e.to_string())?;

    let results: Vec<(String, String, f32)> = stmt
        .query_map(
            params![query_embedding.as_bytes(), limit as i64],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    if results.is_empty() {
        return Ok("No code matches found. Have you run 'index' yet?".to_string());
    }

    let mut response = format!("{} results:\n", results.len());
    for (file_path, content, distance) in results {
        let score = 1.0 - distance;
        let preview = if content.len() > 80 {
            format!("{}...", &content[..80].replace('\n', " "))
        } else {
            content.replace('\n', " ")
        };
        response.push_str(&format!("  {} (score: {:.2})\n    {}\n", file_path, score, preview));
    }

    Ok(response)
}

/// Index project
pub async fn index(
    server: &MiraServer,
    action: String,
    path: Option<String>,
) -> Result<String, String> {
    match action.as_str() {
        "project" | "file" => {
            let project = server.project.read().await;
            let project_path = path
                .or_else(|| project.as_ref().map(|p| p.path.clone()))
                .ok_or("No project path specified")?;

            let project_id = project.as_ref().map(|p| p.id);
            drop(project);

            let path = Path::new(&project_path);
            if !path.exists() {
                return Err(format!("Path not found: {}", project_path));
            }

            // Index code
            let stats = indexer::index_project(path, server.db.clone(), server.embeddings.clone(), project_id)
                .await
                .map_err(|e| e.to_string())?;

            Ok(format!(
                "Indexed {} files, {} symbols, {} chunks",
                stats.files, stats.symbols, stats.chunks
            ))
        }
        "status" => {
            let conn = server.db.conn();
            let symbols: i64 = conn
                .query_row("SELECT COUNT(*) FROM code_symbols", [], |r| r.get(0))
                .unwrap_or(0);
            let chunks: i64 = conn
                .query_row("SELECT COUNT(*) FROM vec_code", [], |r| r.get(0))
                .unwrap_or(0);

            Ok(format!("Index status: {} symbols, {} code chunks", symbols, chunks))
        }
        _ => Err(format!("Unknown action: {}. Use: project, file, status", action)),
    }
}
