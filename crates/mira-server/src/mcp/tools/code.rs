// crates/mira-server/src/mcp/tools/code.rs
// Code intelligence tools

use crate::cartographer;
use crate::indexer;
use crate::mcp::MiraServer;
use crate::llm::{DeepSeekClient, Message};
use crate::tools::core::code;
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

/// Semantic code search with hybrid fallback and cross-reference support
pub async fn semantic_code_search(
    server: &MiraServer,
    query: String,
    language: Option<String>,
    limit: Option<i64>,
) -> Result<String, String> {
    code::search_code(server, query, language, limit).await
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
            let project = server.project.read().await;
            let project_id = project.as_ref().map(|p| p.id);
            drop(project);

            let conn = server.db.conn();

            // Count symbols for current project (or all if no project)
            let symbols: i64 = if let Some(pid) = project_id {
                conn.query_row(
                    "SELECT COUNT(*) FROM code_symbols WHERE project_id = ?",
                    [pid],
                    |r| r.get(0),
                )
                .unwrap_or(0)
            } else {
                conn.query_row("SELECT COUNT(*) FROM code_symbols", [], |r| r.get(0))
                    .unwrap_or(0)
            };

            // Count embedded chunks for current project
            let embedded: i64 = if let Some(pid) = project_id {
                conn.query_row(
                    "SELECT COUNT(*) FROM vec_code WHERE project_id = ?",
                    [pid],
                    |r| r.get(0),
                )
                .unwrap_or(0)
            } else {
                conn.query_row("SELECT COUNT(*) FROM vec_code", [], |r| r.get(0))
                    .unwrap_or(0)
            };

            // Count pending embeddings
            let (pending, processing): (i64, i64) = if let Some(pid) = project_id {
                let pending = conn
                    .query_row(
                        "SELECT COUNT(*) FROM pending_embeddings WHERE project_id = ? AND status = 'pending'",
                        [pid],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                let processing = conn
                    .query_row(
                        "SELECT COUNT(*) FROM pending_embeddings WHERE project_id = ? AND status = 'processing'",
                        [pid],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                (pending, processing)
            } else {
                let pending = conn
                    .query_row(
                        "SELECT COUNT(*) FROM pending_embeddings WHERE status = 'pending'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                let processing = conn
                    .query_row(
                        "SELECT COUNT(*) FROM pending_embeddings WHERE status = 'processing'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                (pending, processing)
            };

            // Check for active batch
            let active_batch: Option<String> = conn
                .query_row(
                    "SELECT batch_id FROM background_batches WHERE status = 'active' LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .ok();

            let mut status = format!("Index status: {} symbols, {} embedded chunks", symbols, embedded);

            if pending > 0 || processing > 0 {
                status.push_str(&format!("\nPending embeddings: {} pending, {} processing", pending, processing));
            }

            if let Some(batch_id) = active_batch {
                status.push_str(&format!("\nActive batch: {}", batch_id));
            }

            Ok(status)
        }
        "check-batch" => {
            // Force check batch status
            if server.embeddings.is_none() {
                return Err("Embeddings not configured".to_string());
            }

            let result = crate::background::check_batch_now(
                &server.db,
                server.embeddings.as_ref().unwrap(),
            )
            .await;

            match result {
                Ok(processed) if processed > 0 => {
                    Ok(format!("Batch complete: {} embeddings processed", processed))
                }
                Ok(_) => Ok("Batch still processing or no active batch".to_string()),
                Err(e) => Err(format!("Batch check failed: {}", e)),
            }
        }
        "reset-stuck" => {
            // Reset stuck processing items back to pending
            let conn = server.db.conn();
            let reset = conn
                .execute(
                    "UPDATE pending_embeddings SET status = 'pending' WHERE status = 'processing'",
                    [],
                )
                .map_err(|e| e.to_string())?;

            // Also clear any stale active batches
            let _ = conn.execute(
                "UPDATE background_batches SET status = 'failed' WHERE status = 'active'",
                [],
            );

            Ok(format!("Reset {} stuck items to pending", reset))
        }
        "embed-now" => {
            // Process embeddings immediately using direct API (not batch file API)
            if server.embeddings.is_none() {
                return Err("Embeddings not configured".to_string());
            }

            // Process up to 100 at a time (direct API limit per request)
            let limit = 100;
            let mut total = 0;

            loop {
                let result = crate::background::embed_now(
                    &server.db,
                    server.embeddings.as_ref().unwrap(),
                    limit,
                )
                .await;

                match result {
                    Ok(processed) if processed > 0 => {
                        total += processed;
                        // Continue processing if we hit the limit
                        if processed < limit {
                            break;
                        }
                    }
                    Ok(_) => break, // No more pending
                    Err(e) => {
                        if total > 0 {
                            return Ok(format!("Embedded {} items before error: {}", total, e));
                        }
                        return Err(format!("Embedding failed: {}", e));
                    }
                }
            }

            if total > 0 {
                Ok(format!("Embedded {} items in real-time", total))
            } else {
                Ok("No pending embeddings to process".to_string())
            }
        }
        "cancel-batch" => {
            // Cancel active batch and reset items to pending
            crate::background::cancel_batch(&server.db)
        }
        _ => Err(format!("Unknown action: {}. Use: project, file, status, check-batch, reset-stuck, embed-now, cancel-batch", action)),
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

/// Find all functions that call a given function
pub async fn mcp_find_callers(
    server: &MiraServer,
    function_name: String,
    limit: Option<i64>,
) -> Result<String, String> {
    code::find_function_callers(server, function_name, limit).await
}

/// Find all functions called by a given function
pub async fn mcp_find_callees(
    server: &MiraServer,
    function_name: String,
    limit: Option<i64>,
) -> Result<String, String> {
    code::find_function_callees(server, function_name, limit).await
}

/// Check if a capability/feature exists in the codebase
pub async fn check_capability(
    server: &MiraServer,
    description: String,
) -> Result<String, String> {
    code::check_capability(server, description).await
}
