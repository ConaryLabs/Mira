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

/// Keyword-based code search fallback
/// Searches chunk content and symbol names using LIKE matching
fn keyword_code_search(
    conn: &rusqlite::Connection,
    query: &str,
    project_id: Option<i64>,
    limit: usize,
    project_path: Option<&str>,
) -> Vec<(String, String, f32)> {
    let mut results = Vec::new();

    // Split query into terms for flexible matching
    let terms: Vec<&str> = query.split_whitespace().collect();
    if terms.is_empty() {
        return results;
    }

    // Build LIKE pattern - match any term
    let like_patterns: Vec<String> = terms
        .iter()
        .map(|t| format!("%{}%", t.to_lowercase()))
        .collect();

    // Search vec_code chunk_content
    if let Some(pid) = project_id {
        for pattern in &like_patterns {
            let query_sql = "SELECT file_path, chunk_content FROM vec_code
                             WHERE project_id = ? AND LOWER(chunk_content) LIKE ?
                             LIMIT ?";
            if let Ok(mut stmt) = conn.prepare(query_sql) {
                if let Ok(rows) = stmt.query_map(params![pid, pattern, limit as i64], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                }) {
                    for row in rows.flatten() {
                        // Avoid duplicates
                        if !results.iter().any(|(f, c, _)| f == &row.0 && c == &row.1) {
                            results.push((row.0, row.1, 0.5)); // Fixed score for keyword matches
                        }
                    }
                }
            }
            if results.len() >= limit {
                break;
            }
        }
    }

    // Also search symbol names for direct matches
    if let Some(pid) = project_id {
        for pattern in &like_patterns {
            let query_sql = "SELECT file_path, name, signature, start_line, end_line
                             FROM code_symbols
                             WHERE project_id = ? AND LOWER(name) LIKE ?
                             LIMIT ?";
            if let Ok(mut stmt) = conn.prepare(query_sql) {
                if let Ok(rows) = stmt.query_map(params![pid, pattern, limit as i64], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                    ))
                }) {
                    for row in rows.flatten() {
                        let (file_path, name, signature, start_line, end_line) = row;

                        // Try to read the actual code from file
                        let content = if let (Some(proj_path), Some(start), Some(end)) =
                            (project_path, start_line, end_line)
                        {
                            let full_path = Path::new(proj_path).join(&file_path);
                            if let Ok(file_content) = std::fs::read_to_string(&full_path) {
                                let lines: Vec<&str> = file_content.lines().collect();
                                let start_idx = (start as usize).saturating_sub(1);
                                let end_idx = (end as usize).min(lines.len());
                                lines[start_idx..end_idx].join("\n")
                            } else {
                                signature.unwrap_or_else(|| name.clone())
                            }
                        } else {
                            signature.unwrap_or_else(|| name.clone())
                        };

                        // Avoid duplicates
                        if !results.iter().any(|(f, _, _)| f == &file_path && content.contains(&name)) {
                            results.push((file_path, content, 0.6)); // Slightly higher score for symbol matches
                        }
                    }
                }
            }
            if results.len() >= limit {
                break;
            }
        }
    }

    results.truncate(limit);
    results
}

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

/// Semantic code search with context expansion
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

    // Get project context for file reading and hybrid fallback
    let (project_path, project_id) = {
        let proj = server.project.read().await;
        (
            proj.as_ref().map(|p| p.path.clone()),
            proj.as_ref().map(|p| p.id),
        )
    };

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

    let semantic_results: Vec<(String, String, f32)> = stmt
        .query_map(
            params![query_embedding.as_bytes(), limit as i64],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    // Convert distance to score and check quality
    let semantic_with_scores: Vec<(String, String, f32)> = semantic_results
        .into_iter()
        .map(|(f, c, d)| (f, c, 1.0 - d))
        .collect();

    // Determine if we need keyword fallback
    // Fallback if: no results, or best score is below threshold
    let best_score = semantic_with_scores
        .iter()
        .map(|(_, _, s)| *s)
        .fold(0.0f32, |a, b| a.max(b));

    let (results, used_fallback) = if semantic_with_scores.is_empty() || best_score < 0.25 {
        // Try keyword fallback
        let keyword_results = keyword_code_search(
            &conn,
            &query,
            project_id,
            limit,
            project_path.as_deref(),
        );

        if !keyword_results.is_empty() {
            tracing::debug!(
                "Semantic search poor (best_score={:.2}), using {} keyword results",
                best_score,
                keyword_results.len()
            );
            (keyword_results, true)
        } else if !semantic_with_scores.is_empty() {
            // Keep semantic results even if low quality
            (semantic_with_scores, false)
        } else {
            return Ok("No code matches found. Have you run 'index' yet?".to_string());
        }
    } else {
        (semantic_with_scores, false)
    };

    let search_type = if used_fallback { "keyword" } else { "semantic" };
    let mut response = format!("{} results ({} search):\n\n", results.len(), search_type);
    for (file_path, chunk_content, score) in results {

        // Try to expand context
        let expanded = expand_search_context(
            &conn,
            &file_path,
            &chunk_content,
            project_path.as_deref(),
        );

        response.push_str(&format!("━━━ {} (score: {:.2}) ━━━\n", file_path, score));

        if let Some((symbol_info, full_code)) = expanded {
            // Show symbol info if available
            if let Some(info) = symbol_info {
                response.push_str(&format!("{}\n", info));
            }
            // Show full code (up to reasonable limit)
            let code_display = if full_code.len() > 1500 {
                format!("{}...\n[truncated]", &full_code[..1500])
            } else {
                full_code
            };
            response.push_str(&format!("```\n{}\n```\n\n", code_display));
        } else {
            // Fallback to chunk content
            let display = if chunk_content.len() > 500 {
                format!("{}...", &chunk_content[..500])
            } else {
                chunk_content
            };
            response.push_str(&format!("```\n{}\n```\n\n", display));
        }
    }

    Ok(response)
}

/// Expand search result with containing symbol and surrounding context
fn expand_search_context(
    conn: &rusqlite::Connection,
    file_path: &str,
    chunk_content: &str,
    project_path: Option<&str>,
) -> Option<(Option<String>, String)> {
    // Try to find the containing symbol for this chunk
    // Look for a symbol whose line range contains part of the chunk

    // First, try to identify which symbol this chunk belongs to
    // by checking if the chunk starts with our context header (e.g., "// function foo:")
    let symbol_info = if chunk_content.starts_with("// ") {
        // Extract the first line as symbol info
        chunk_content.lines().next().map(|s| s.to_string())
    } else {
        None
    };

    // Try to read full file and find the matching section
    if let Some(proj_path) = project_path {
        let full_path = std::path::Path::new(proj_path).join(file_path);
        if let Ok(file_content) = std::fs::read_to_string(&full_path) {
            // The chunk content (minus our added header) should be in the file
            // Strip the header comment if present
            let search_content = if chunk_content.starts_with("// ") {
                chunk_content.lines().skip(1).collect::<Vec<_>>().join("\n")
            } else {
                chunk_content.to_string()
            };

            // Find the position in the file
            if let Some(pos) = file_content.find(&search_content) {
                // Count lines to this position
                let lines_before = file_content[..pos].matches('\n').count();

                // Get surrounding context (5 lines before, full match, 5 lines after)
                let all_lines: Vec<&str> = file_content.lines().collect();
                let match_lines = search_content.matches('\n').count() + 1;

                let start_line = lines_before.saturating_sub(5);
                let end_line = std::cmp::min(lines_before + match_lines + 5, all_lines.len());

                let context_code: String = all_lines[start_line..end_line].join("\n");
                return Some((symbol_info, context_code));
            }
        }
    }

    // If we couldn't expand, just return the chunk content with its header
    Some((symbol_info, chunk_content.to_string()))
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
