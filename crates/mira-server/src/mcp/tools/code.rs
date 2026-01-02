// crates/mira-server/src/mcp/tools/code.rs
// Code intelligence tools

use crate::cartographer;
use crate::db::Database;
use crate::embeddings::Embeddings;
use crate::indexer;
use crate::mcp::MiraServer;
use crate::web::deepseek::{DeepSeekClient, Message};
use rusqlite::params;
use std::path::Path;
use std::sync::Arc;
use zerocopy::AsBytes;

/// Process pending embeddings for a project inline (real-time fallback)
/// This is called when semantic search is requested but batch hasn't completed
async fn process_pending_embeddings_inline(
    db: &Arc<Database>,
    embeddings: &Arc<Embeddings>,
    project_id: i64,
) -> Result<usize, String> {
    // Get pending embeddings for this project (limit to avoid blocking too long)
    // Note: Must drop conn before await to satisfy Send requirement
    let pending: Vec<(i64, String, String)> = {
        let conn = db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, file_path, chunk_content
                 FROM pending_embeddings
                 WHERE project_id = ? AND status = 'pending'
                 ORDER BY id ASC
                 LIMIT 50",
            )
            .map_err(|e| e.to_string())?;

        stmt.query_map(params![project_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect()
    }; // conn dropped here

    if pending.is_empty() {
        return Ok(0);
    }

    tracing::info!("Processing {} pending embeddings inline for project {}", pending.len(), project_id);

    let mut processed = 0;
    for (id, file_path, chunk_content) in pending {
        // Embed the chunk
        match embeddings.embed(&chunk_content).await {
            Ok(embedding) => {
                // Re-acquire connection for DB operations
                let conn = db.conn();

                // Insert into vec_code
                if let Err(e) = conn.execute(
                    "INSERT INTO vec_code (embedding, file_path, chunk_content, project_id)
                     VALUES (?, ?, ?, ?)",
                    params![embedding.as_bytes(), file_path, chunk_content, project_id],
                ) {
                    tracing::debug!("Failed to insert embedding: {}", e);
                    continue;
                }

                // Mark as completed
                let _ = conn.execute(
                    "DELETE FROM pending_embeddings WHERE id = ?",
                    params![id],
                );

                processed += 1;
            }
            Err(e) => {
                tracing::debug!("Failed to embed chunk: {}", e);
            }
        }
    }

    if processed > 0 {
        tracing::info!("Processed {} embeddings inline", processed);
    }

    Ok(processed)
}

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
        .ok_or("Semantic search requires OPENAI_API_KEY")?;

    // Process any pending embeddings for the active project (real-time fallback)
    // This ensures we have up-to-date search results even if batch hasn't completed
    if let Some(ref project) = *server.project.read().await {
        if let Err(e) = process_pending_embeddings_inline(&server.db, embeddings, project.id).await {
            tracing::debug!("Failed to process pending embeddings: {}", e);
        }
    }

    // Get query embedding
    let query_embedding = embeddings
        .embed(&query)
        .await
        .map_err(|e| format!("Embedding failed: {}", e))?;

    let conn = server.db.conn();

    // Search vec_code
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

            let mut response = format!(
                "Indexed {} files, {} symbols, {} chunks",
                stats.files, stats.symbols, stats.chunks
            );

            // Auto-summarize modules that don't have descriptions yet
            if let Some(pid) = project_id {
                if let Some(ref deepseek) = server.deepseek {
                    match auto_summarize_modules(server, pid, &project_path, deepseek).await {
                        Ok(count) if count > 0 => {
                            response.push_str(&format!(", summarized {} modules", count));
                        }
                        Ok(_) => {} // No modules needed summarization
                        Err(e) => {
                            tracing::warn!("Auto-summarize failed: {}", e);
                        }
                    }
                }
            }

            Ok(response)
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

// ═══════════════════════════════════════
// LLM-POWERED SUMMARIES
// ═══════════════════════════════════════

/// Auto-summarize modules that don't have descriptions (called after indexing)
async fn auto_summarize_modules(
    server: &MiraServer,
    project_id: i64,
    project_path: &str,
    deepseek: &DeepSeekClient,
) -> Result<usize, String> {
    // Get modules needing summaries
    let mut modules = cartographer::get_modules_needing_summaries(&server.db, project_id)
        .map_err(|e| e.to_string())?;

    if modules.is_empty() {
        return Ok(0);
    }

    // Fill in code previews
    let path = Path::new(project_path);
    for module in &mut modules {
        module.code_preview = cartographer::get_module_code_preview(path, &module.path);
    }

    // Build prompt and call DeepSeek
    let prompt = cartographer::build_summary_prompt(&modules);
    let messages = vec![Message::user(prompt)];
    let result = deepseek
        .chat(messages, None)
        .await
        .map_err(|e| format!("DeepSeek request failed: {}", e))?;

    let content = result.content.ok_or("No content in DeepSeek response")?;

    // Parse and update
    let summaries = cartographer::parse_summary_response(&content);
    if summaries.is_empty() {
        return Err("Failed to parse summaries".to_string());
    }

    let updated = cartographer::update_module_purposes(&server.db, project_id, &summaries)
        .map_err(|e| e.to_string())?;

    Ok(updated)
}

/// Summarize codebase modules using DeepSeek
pub async fn summarize_codebase(server: &MiraServer) -> Result<String, String> {
    // Get project context
    let project = server.project.read().await;
    let (project_id, project_path) = match project.as_ref() {
        Some(p) => (p.id, p.path.clone()),
        None => return Err("No active project. Call session_start first.".to_string()),
    };
    drop(project);

    // Get DeepSeek client
    let deepseek = server
        .deepseek
        .as_ref()
        .ok_or("DeepSeek not configured. Set DEEPSEEK_API_KEY.")?;

    // Get modules needing summaries
    let mut modules = cartographer::get_modules_needing_summaries(&server.db, project_id)
        .map_err(|e| e.to_string())?;

    if modules.is_empty() {
        return Ok("All modules already have summaries.".to_string());
    }

    // Fill in code previews
    let project_path = Path::new(&project_path);
    for module in &mut modules {
        module.code_preview = cartographer::get_module_code_preview(project_path, &module.path);
    }

    // Build prompt
    let prompt = cartographer::build_summary_prompt(&modules);

    // Call DeepSeek using shared client (no tools needed for summarization)
    let messages = vec![Message::user(prompt)];
    let result = deepseek
        .chat(messages, None)
        .await
        .map_err(|e| format!("DeepSeek request failed: {}", e))?;

    let content = result
        .content
        .ok_or("No content in DeepSeek response")?;

    // Parse summaries from response
    let summaries = cartographer::parse_summary_response(&content);

    if summaries.is_empty() {
        return Err(format!(
            "Failed to parse summaries from LLM response:\n{}",
            content
        ));
    }

    // Update database
    let updated = cartographer::update_module_purposes(&server.db, project_id, &summaries)
        .map_err(|e| e.to_string())?;

    // Clear cached modules to force regeneration
    let conn = server.db.conn();
    let _ = conn.execute(
        "DELETE FROM codebase_modules WHERE project_id = ? AND purpose IS NULL",
        rusqlite::params![project_id],
    );

    Ok(format!(
        "Summarized {} modules:\n{}",
        updated,
        summaries
            .iter()
            .map(|(id, summary)| format!("  {}: {}", id, summary))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}
