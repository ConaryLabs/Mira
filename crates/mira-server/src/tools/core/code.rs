// crates/mira-server/src/tools/core/code.rs
// Unified code tools (search, callers, callees, symbols, index)

use std::path::Path;

use crate::cartographer;

use crate::indexer;
use crate::llm::{LlmClient, Message, record_llm_usage};
use crate::mcp::requests::IndexAction;
use crate::search::{
    CrossRefType, crossref_search, expand_context_with_conn, find_callees,
    find_callers, format_crossref_results, format_project_header, hybrid_search,
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
    let results_data: Vec<_> = result
        .results
        .iter()
        .map(|r| (r.file_path.clone(), r.content.clone(), r.score))
        .collect();

    let project_path_clone = project_path.clone();
    type ExpandedResult = (String, String, f32, Option<(Option<String>, String)>);
    let expanded_results: Vec<ExpandedResult> = ctx
        .pool()
        .run(move |conn| -> Result<Vec<ExpandedResult>, String> {
            Ok(results_data
                .iter()
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

/// Get symbols from a file
pub fn get_symbols(file_path: String, symbol_type: Option<String>) -> Result<String, String> {
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
            let embeddings = if skip_embed {
                None
            } else {
                ctx.embeddings().cloned()
            };
            let stats = indexer::index_project(path, ctx.pool().clone(), embeddings, project_id)
                .await
                .map_err(|e| e.to_string())?;

            let mut response = format!(
                "Indexed {} files, {} symbols, {} chunks",
                stats.files, stats.symbols, stats.chunks
            );

            // Auto-summarize modules that don't have descriptions yet
            if let Some(pid) = project_id {
                if let Some(llm_client) = ctx.llm_factory().client_for_background() {
                    match auto_summarize_modules(ctx.pool(), pid, &project_path, &*llm_client).await {
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
            use crate::db::{count_embedded_chunks_sync, count_symbols_sync};

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

            Ok(format!(
                "Index status: {} symbols, {} embedded chunks",
                symbols, embedded
            ))
        }
    }
}

/// Auto-summarize modules that don't have descriptions (called after indexing)
async fn auto_summarize_modules(
    pool: &std::sync::Arc<crate::db::pool::DatabasePool>,
    project_id: i64,
    project_path: &str,
    llm_client: &dyn LlmClient,
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

    // Build prompt and call LLM
    let prompt = cartographer::build_summary_prompt(&modules);
    let messages = vec![Message::user(prompt)];
    let result = llm_client
        .chat(messages, None)
        .await
        .map_err(|e| format!("LLM request failed: {}", e))?;

    // Record usage
    record_llm_usage(
        pool,
        llm_client.provider_type(),
        &llm_client.model_name(),
        "tool:auto_summarize",
        &result,
        Some(project_id),
        None,
    )
    .await;

    let content = result.content.ok_or("No content in LLM response")?;

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

/// Summarize codebase modules using LLM
pub async fn summarize_codebase<C: ToolContext>(ctx: &C) -> Result<String, String> {
    // Get project context
    let project = ctx.get_project().await;
    let (project_id, project_path) = match project.as_ref() {
        Some(p) => (p.id, p.path.clone()),
        None => return Err("No active project. Call session_start first.".to_string()),
    };

    // Get LLM client
    let llm_client = ctx
        .llm_factory()
        .client_for_background()
        .ok_or("No LLM provider configured. Set DEEPSEEK_API_KEY or GEMINI_API_KEY.")?;

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

    // Call LLM (no tools needed for summarization)
    let messages = vec![Message::user(prompt)];
    let result = llm_client
        .chat(messages, None)
        .await
        .map_err(|e| format!("LLM request failed: {}", e))?;

    // Record usage
    record_llm_usage(
        ctx.pool(),
        llm_client.provider_type(),
        &llm_client.model_name(),
        "tool:summarize_codebase",
        &result,
        Some(project_id),
        None,
    )
    .await;

    let content = result.content.ok_or("No content in LLM response")?;

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
            clear_modules_without_purpose_sync(conn, project_id).map_err(|e| e.to_string())?;

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
