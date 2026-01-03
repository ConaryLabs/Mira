// crates/mira-server/src/mcp/tools/code.rs
// Code intelligence tools

use crate::cartographer;
use crate::db::Database;
use crate::embeddings::Embeddings;
use crate::indexer;
use crate::mcp::MiraServer;
use crate::search::{expand_context_with_db, hybrid_search};
use crate::web::deepseek::{DeepSeekClient, Message};
use rusqlite::params;
use std::path::Path;
use std::sync::Arc;
use zerocopy::AsBytes;

/// Process pending embeddings for a project inline (real-time fallback)
/// Only called if no embeddings exist yet - otherwise batch API handles it
async fn process_pending_embeddings_inline(
    db: &Arc<Database>,
    embeddings: &Arc<Embeddings>,
    project_id: i64,
) -> Result<usize, String> {
    // First check if we already have some embeddings - if so, skip inline processing
    // and let the batch API handle the rest
    {
        let conn = db.conn();
        let existing: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM vec_code WHERE project_id = ?",
                params![project_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if existing > 0 {
            // Already have embeddings, don't block on more
            return Ok(0);
        }
    }

    // No embeddings exist yet - process a small batch to bootstrap search
    // Note: Must drop conn before await to satisfy Send requirement
    let pending: Vec<(i64, String, String)> = {
        let conn = db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, file_path, chunk_content
                 FROM pending_embeddings
                 WHERE project_id = ? AND status = 'pending'
                 ORDER BY id ASC
                 LIMIT 10",
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

    tracing::info!("Bootstrapping {} embeddings inline for project {}", pending.len(), project_id);

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
        tracing::info!("Bootstrapped {} embeddings inline", processed);
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

/// Semantic code search with hybrid fallback
pub async fn semantic_code_search(
    server: &MiraServer,
    query: String,
    _language: Option<String>,
    limit: Option<i64>,
) -> Result<String, String> {
    let limit = limit.unwrap_or(10) as usize;

    // Get project context
    let (project_path, project_id) = {
        let proj = server.project.read().await;
        (
            proj.as_ref().map(|p| p.path.clone()),
            proj.as_ref().map(|p| p.id),
        )
    };

    // Process any pending embeddings for the active project (real-time fallback)
    if let Some(ref embeddings) = server.embeddings {
        if let Some(ref project) = *server.project.read().await {
            if let Err(e) = process_pending_embeddings_inline(&server.db, embeddings, project.id).await {
                tracing::debug!("Failed to process pending embeddings: {}", e);
            }
        }
    }

    // Use shared hybrid search
    let result = hybrid_search(
        &server.db,
        server.embeddings.as_ref(),
        &query,
        project_id,
        project_path.as_deref(),
        limit,
    )
    .await?;

    // Format with context expansion (MCP style with box drawing)
    if result.results.is_empty() {
        return Ok("No code matches found. Have you run 'index' yet?".to_string());
    }

    let mut response = format!(
        "{} results ({} search):\n\n",
        result.results.len(),
        result.search_type
    );

    for r in &result.results {
        // Use shared context expansion with DB for full symbol bounds
        let expanded = expand_context_with_db(
            &r.file_path,
            &r.content,
            project_path.as_deref(),
            Some(&server.db),
            project_id,
        );

        response.push_str(&format!("━━━ {} (score: {:.2}) ━━━\n", r.file_path, r.score));

        if let Some((symbol_info, full_code)) = expanded {
            if let Some(info) = symbol_info {
                response.push_str(&format!("{}\n", info));
            }
            let code_display = if full_code.len() > 1500 {
                format!("{}...\n[truncated]", &full_code[..1500])
            } else {
                full_code
            };
            response.push_str(&format!("```\n{}\n```\n\n", code_display));
        } else {
            let display = if r.content.len() > 500 {
                format!("{}...", &r.content[..500])
            } else {
                r.content.clone()
            };
            response.push_str(&format!("```\n{}\n```\n\n", display));
        }
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
