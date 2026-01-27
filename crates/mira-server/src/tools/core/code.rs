// crates/mira-server/src/tools/core/code.rs
// Unified code tools (search, callers, callees, symbols, index)

use std::path::Path;

use crate::cartographer;
use crate::db::search_capabilities_sync;
use crate::indexer;
use crate::llm::{record_llm_usage, LlmClient, Message};
use crate::mcp::requests::IndexAction;
use crate::search::{
    crossref_search, embedding_to_bytes, expand_context_with_conn, find_callers, find_callees,
    format_crossref_results, format_project_header, hybrid_search, CrossRefType,
};
use crate::tools::core::ToolContext;

/// Search code using semantic similarity or keyword fallback
pub async fn search_code<C: ToolContext>(
    ctx: &C,
    query: String,
    _language: Option<String>,
    limit: Option<i64>,
) -> Result<String, String> {
    let limit = limit.unwrap_or(10) as usize;
    let project_id = ctx.project_id().await;
    let project = ctx.get_project().await;
    let project_path = project.as_ref().map(|p| p.path.clone());
    let context_header = format_project_header(project.as_ref());

    // Check for cross-reference query patterns first ("who calls X", "callers of X", etc.)
    let query_clone = query.clone();
    let crossref_result = ctx
        .pool()
        .run(move |conn| Ok::<_, String>(crossref_search(conn, &query_clone, project_id, limit)))
        .await?;

    if let Some((target, ref_type, results)) = crossref_result {
        return Ok(format!(
            "{}{}",
            context_header,
            format_crossref_results(&target, ref_type, &results)
        ));
    }

    // Use shared hybrid search
    let result = hybrid_search(
        ctx.pool(),
        ctx.embeddings(),
        &query,
        project_id,
        project_path.as_deref(),
        limit,
    )
    .await
    .map_err(|e| e.to_string())?;

    if result.results.is_empty() {
        return Ok(format!("{}No code matches found.", context_header));
    }

    // Format results (MCP-style with box drawing characters)
    let mut response = format!(
        "{}Found {} results ({} search):\n\n",
        context_header,
        result.results.len(),
        result.search_type
    );

    // Batch expand results with DB access for symbol bounds
    let results_data: Vec<_> = result.results.iter()
        .map(|r| (r.file_path.clone(), r.content.clone(), r.score))
        .collect();

    let project_path_clone = project_path.clone();
    type ExpandedResult = (String, String, f32, Option<(Option<String>, String)>);
    let expanded_results: Vec<ExpandedResult> = ctx
        .pool()
        .run(move |conn| -> Result<Vec<ExpandedResult>, String> {
            Ok(results_data.iter()
                .map(|(file_path, content, score)| {
                    let expanded = expand_context_with_conn(
                        file_path,
                        content,
                        project_path_clone.as_deref(),
                        Some(conn),
                        project_id,
                    );
                    (file_path.clone(), content.clone(), *score, expanded)
                })
                .collect())
        })
        .await?;

    for (file_path, content, score, expanded) in expanded_results {
        response.push_str(&format!("━━━ {} (score: {:.2}) ━━━\n", file_path, score));

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
            let display = if content.len() > 500 {
                format!("{}...", &content[..500])
            } else {
                content
            };
            response.push_str(&format!("```\n{}\n```\n\n", display));
        }
    }

    Ok(response)
}

/// Find functions that call a specific function
pub async fn find_function_callers<C: ToolContext>(
    ctx: &C,
    function_name: String,
    limit: Option<i64>,
) -> Result<String, String> {
    if function_name.is_empty() {
        return Err("function_name is required".to_string());
    }

    let limit = limit.unwrap_or(20) as usize;
    let project_id = ctx.project_id().await;
    let project = ctx.get_project().await;
    let context_header = format_project_header(project.as_ref());

    let fn_name = function_name.clone();
    let results = ctx
        .pool()
        .run(move |conn| Ok::<_, String>(find_callers(conn, project_id, &fn_name, limit)))
        .await?;

    if results.is_empty() {
        return Ok(format!(
            "{}No callers found for `{}`.",
            context_header, function_name
        ));
    }

    Ok(format!(
        "{}{}",
        context_header,
        format_crossref_results(&function_name, CrossRefType::Caller, &results)
    ))
}

/// Find functions called by a specific function
pub async fn find_function_callees<C: ToolContext>(
    ctx: &C,
    function_name: String,
    limit: Option<i64>,
) -> Result<String, String> {
    if function_name.is_empty() {
        return Err("function_name is required".to_string());
    }

    let limit = limit.unwrap_or(20) as usize;
    let project_id = ctx.project_id().await;
    let project = ctx.get_project().await;
    let context_header = format_project_header(project.as_ref());

    let fn_name = function_name.clone();
    let results = ctx
        .pool()
        .run(move |conn| Ok::<_, String>(find_callees(conn, project_id, &fn_name, limit)))
        .await?;

    if results.is_empty() {
        return Ok(format!(
            "{}No callees found for `{}`.",
            context_header, function_name
        ));
    }

    Ok(format!(
        "{}{}",
        context_header,
        format_crossref_results(&function_name, CrossRefType::Callee, &results)
    ))
}

/// Check if a capability/feature exists in the codebase.
/// First searches cached capability memories, then falls back to live code search.
pub async fn check_capability<C: ToolContext>(
    ctx: &C,
    description: String,
) -> Result<String, String> {
    if description.is_empty() {
        return Err("description is required".to_string());
    }

    let project_id = ctx.project_id().await;
    let project = ctx.get_project().await;
    let context_header = format_project_header(project.as_ref());
    let project_path = project.as_ref().map(|p| p.path.clone());

    // Step 1: Search capability memories (using RETRIEVAL_QUERY for optimal search)
    if let Some(embeddings) = ctx.embeddings() {
        if let Ok(query_embedding) = embeddings.embed_for_query(&description).await {
            let embedding_bytes = embedding_to_bytes(&query_embedding);

            // Run vector search via connection pool
            let capability_results: Result<Vec<(i64, String, String, f32)>, String> = ctx
                .pool()
                .run(move |conn| search_capabilities_sync(conn, &embedding_bytes, project_id, 5))
                .await;

            if let Ok(capability_results) = capability_results {
                // Check if we have good matches (similarity > 0.6)
                let good_matches: Vec<_> = capability_results
                    .iter()
                    .filter(|(_, _, _, dist)| (1.0 - dist) > 0.6)
                    .collect();

                if !good_matches.is_empty() {
                    let mut response = format!(
                        "{}Found {} matching capabilities:\n\n",
                        context_header,
                        good_matches.len()
                    );

                    for (id, content, fact_type, distance) in &good_matches {
                        let score = 1.0 - distance;
                        let type_label = if *fact_type == "issue" {
                            "[ISSUE]"
                        } else {
                            "[CAPABILITY]"
                        };
                        response.push_str(&format!(
                            "  {} (score: {:.2}, id: {}) {}\n",
                            type_label, score, id, content
                        ));
                    }

                    return Ok(response);
                }
            }
        }
    }

    // Step 2: Fall back to live code search
    let search_result = hybrid_search(
        ctx.pool(),
        ctx.embeddings(),
        &format!("feature capability {}", description),
        project_id,
        project_path.as_deref(),
        5,
    )
    .await
    .map_err(|e| e.to_string())?;

    if search_result.results.is_empty() {
        return Ok(format!(
            "{}No capability found matching: \"{}\"\n\nThis feature may not exist in the codebase.",
            context_header, description
        ));
    }

    // Format the search results
    let mut response = format!(
        "{}No cached capability found, but code search found {} potentially related locations:\n\n",
        context_header,
        search_result.results.len()
    );

    for r in &search_result.results {
        let preview = if r.content.len() > 200 {
            format!("{}...", &r.content[..200])
        } else {
            r.content.clone()
        };
        response.push_str(&format!(
            "  - {} (score: {:.2})\n    {}\n\n",
            r.file_path, r.score, preview.replace('\n', "\n    ")
        ));
    }

    response.push_str("Note: Run `recall(\"capabilities\")` to see the full capabilities inventory, or wait for the next background scan.");

    Ok(response)
}

/// Get symbols from a file
pub fn get_symbols(
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

/// Index project
pub async fn index<C: ToolContext>(
    ctx: &C,
    action: IndexAction,
    path: Option<String>,
    skip_embed: bool,
) -> Result<String, String> {
    match action {
        IndexAction::Project | IndexAction::File => {
            let project = ctx.get_project().await;
            let project_path = path
                .or_else(|| project.as_ref().map(|p| p.path.clone()))
                .ok_or("No project path specified")?;

            let project_id = project.as_ref().map(|p| p.id);

            let path = Path::new(&project_path);
            if !path.exists() {
                return Err(format!("Path not found: {}", project_path));
            }

            // Index code (skip embeddings if requested for faster indexing)
            let embeddings = if skip_embed { None } else { ctx.embeddings().cloned() };
            let stats = indexer::index_project(path, ctx.pool().clone(), embeddings, project_id)
                .await
                .map_err(|e| e.to_string())?;

            let mut response = format!(
                "Indexed {} files, {} symbols, {} chunks",
                stats.files, stats.symbols, stats.chunks
            );

            // Auto-summarize modules that don't have descriptions yet
            if let Some(pid) = project_id {
                if let Some(deepseek) = ctx.deepseek() {
                    match auto_summarize_modules(ctx.pool(), pid, &project_path, deepseek).await {
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
        IndexAction::Status => {
            use crate::db::{count_symbols_sync, count_embedded_chunks_sync};

            let project = ctx.get_project().await;
            let project_id = project.as_ref().map(|p| p.id);

            let (symbols, embedded) = ctx
                .pool()
                .run(move |conn| {
                    let symbols = count_symbols_sync(conn, project_id);
                    let embedded = count_embedded_chunks_sync(conn, project_id);
                    Ok::<_, String>((symbols, embedded))
                })
                .await?;

            Ok(format!("Index status: {} symbols, {} embedded chunks", symbols, embedded))
        }
    }
}

/// Auto-summarize modules that don't have descriptions (called after indexing)
async fn auto_summarize_modules(
    pool: &std::sync::Arc<crate::db::pool::DatabasePool>,
    project_id: i64,
    project_path: &str,
    deepseek: &crate::llm::DeepSeekClient,
) -> Result<usize, String> {
    // Get modules needing summaries
    let mut modules = pool
        .run(move |conn| cartographer::get_modules_needing_summaries(conn, project_id))
        .await?;

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

    // Record usage
    record_llm_usage(
        pool,
        deepseek.provider_type(),
        &deepseek.model_name(),
        "tool:auto_summarize",
        &result,
        Some(project_id),
        None,
    ).await;

    let content = result.content.ok_or("No content in DeepSeek response")?;

    // Parse and update
    let summaries = cartographer::parse_summary_response(&content);
    if summaries.is_empty() {
        return Err("Failed to parse summaries".to_string());
    }

    let updated = pool
        .run(move |conn| cartographer::update_module_purposes(conn, project_id, &summaries))
        .await?;

    Ok(updated)
}

/// Summarize codebase modules using DeepSeek
pub async fn summarize_codebase<C: ToolContext>(ctx: &C) -> Result<String, String> {
    // Get project context
    let project = ctx.get_project().await;
    let (project_id, project_path) = match project.as_ref() {
        Some(p) => (p.id, p.path.clone()),
        None => return Err("No active project. Call session_start first.".to_string()),
    };

    // Get DeepSeek client
    let deepseek = ctx
        .deepseek()
        .ok_or("DeepSeek not configured. Set DEEPSEEK_API_KEY.")?;

    // Get modules needing summaries
    let mut modules = ctx
        .pool()
        .run(move |conn| cartographer::get_modules_needing_summaries(conn, project_id))
        .await?;

    if modules.is_empty() {
        return Ok("All modules already have summaries.".to_string());
    }

    // Fill in code previews
    let project_path_ref = Path::new(&project_path);
    for module in &mut modules {
        module.code_preview = cartographer::get_module_code_preview(project_path_ref, &module.path);
    }

    // Build prompt
    let prompt = cartographer::build_summary_prompt(&modules);

    // Call DeepSeek using shared client (no tools needed for summarization)
    let messages = vec![Message::user(prompt)];
    let result = deepseek
        .chat(messages, None)
        .await
        .map_err(|e| format!("DeepSeek request failed: {}", e))?;

    // Record usage
    record_llm_usage(
        ctx.pool(),
        deepseek.provider_type(),
        &deepseek.model_name(),
        "tool:summarize_codebase",
        &result,
        Some(project_id),
        None,
    ).await;

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

    // Update database and clear cached modules
    use crate::db::clear_modules_without_purpose_sync;

    let summaries_clone = summaries.clone();
    let updated: usize = ctx
        .pool()
        .run(move |conn| {
            let count = cartographer::update_module_purposes(conn, project_id, &summaries_clone)
                .map_err(|e| e.to_string())?;

            // Clear cached modules to force regeneration
            clear_modules_without_purpose_sync(conn, project_id)
                .map_err(|e| e.to_string())?;

            Ok::<_, String>(count)
        })
        .await?;

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
