// crates/mira-server/src/tools/core/code.rs
// Unified code tools (search, callers, callees, symbols, index)

use std::path::Path;

use crate::cartographer;

use crate::indexer;
use crate::llm::{LlmClient, Message, record_llm_usage};
use crate::mcp::requests::IndexAction;
use crate::mcp::responses::{
    CallGraphData, CallGraphEntry, CodeData, CodeOutput, CodeSearchResult, DependenciesData,
    DependencyEdge, DebtFactor, IndexCompactData, IndexData, IndexOutput, IndexProjectData,
    IndexStatusData, IndexSummarizeData, ModulePatterns, PatternEntry, PatternsData,
    SearchResultsData, SymbolInfo, SymbolsData, TechDebtData, TechDebtModule, TechDebtTier,
};
use crate::search::{
    CrossRefType, crossref_search, expand_context_with_conn, find_callees, find_callers,
    format_crossref_results, format_project_header, hybrid_search,
};
use crate::tools::core::ToolContext;
use crate::utils::ResultExt;
use crate::mcp::responses::Json;

/// Unified code tool dispatcher
pub async fn handle_code<C: ToolContext>(
    ctx: &C,
    req: crate::mcp::requests::CodeRequest,
) -> Result<Json<CodeOutput>, String> {
    use crate::mcp::requests::CodeAction;
    match req.action {
        CodeAction::Search => {
            let query = req.query.ok_or("query is required for action 'search'")?;
            search_code(ctx, query, req.language, req.limit).await
        }
        CodeAction::Symbols => {
            let file_path = req
                .file_path
                .ok_or("file_path is required for action 'symbols'")?;
            get_symbols(file_path, req.symbol_type)
        }
        CodeAction::Callers => {
            let function_name = req
                .function_name
                .ok_or("function_name is required for action 'callers'")?;
            find_function_callers(ctx, function_name, req.limit).await
        }
        CodeAction::Callees => {
            let function_name = req
                .function_name
                .ok_or("function_name is required for action 'callees'")?;
            find_function_callees(ctx, function_name, req.limit).await
        }
        CodeAction::Dependencies => get_dependencies(ctx).await,
        CodeAction::Patterns => get_patterns(ctx).await,
        CodeAction::TechDebt => get_tech_debt(ctx).await,
    }
}

/// Search code using semantic similarity or keyword fallback
pub async fn search_code<C: ToolContext>(
    ctx: &C,
    query: String,
    _language: Option<String>,
    limit: Option<i64>,
) -> Result<Json<CodeOutput>, String> {
    let limit = limit.unwrap_or(10) as usize;
    let project_id = ctx.project_id().await;
    let project = ctx.get_project().await;
    let project_path = project.as_ref().map(|p| p.path.clone());
    let context_header = format_project_header(project.as_ref());

    // Check for cross-reference query patterns first ("who calls X", "callers of X", etc.)
    let query_clone = query.clone();
    let crossref_result = ctx
        .code_pool()
        .run(move |conn| Ok::<_, String>(crossref_search(conn, &query_clone, project_id, limit)))
        .await?;

    if let Some((target, ref_type, results)) = crossref_result {
        let direction = match ref_type {
            CrossRefType::Caller => "callers",
            CrossRefType::Callee => "callees",
        };
        let functions: Vec<CallGraphEntry> = results
            .iter()
            .map(|r| CallGraphEntry {
                function_name: r.symbol_name.clone(),
                file_path: r.file_path.clone(),
                line: None,
            })
            .collect();
        let total = functions.len();
        return Ok(Json(CodeOutput {
            action: "search".into(),
            message: format!(
                "{}{}",
                context_header,
                format_crossref_results(&target, ref_type, &results)
            ),
            data: Some(CodeData::CallGraph(CallGraphData {
                target,
                direction: direction.into(),
                functions,
                total,
            })),
        }));
    }

    // Use shared hybrid search
    let result = hybrid_search(
        ctx.code_pool(),
        ctx.embeddings(),
        ctx.fuzzy_cache(),
        &query,
        project_id,
        project_path.as_deref(),
        limit,
    )
    .await
    .str_err()?;

    if result.results.is_empty() {
        return Ok(Json(CodeOutput {
            action: "search".into(),
            message: format!("{}No code matches found.", context_header),
            data: Some(CodeData::Search(SearchResultsData {
                results: vec![],
                search_type: result.search_type.to_string(),
                total: 0,
            })),
        }));
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
        .code_pool()
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

    let items: Vec<CodeSearchResult> = expanded_results
        .iter()
        .map(|(file_path, content, score, expanded)| CodeSearchResult {
            file_path: file_path.clone(),
            score: *score,
            symbol_info: expanded.as_ref().and_then(|(info, _)| info.clone()),
            content: expanded
                .as_ref()
                .map(|(_, code)| {
                    if code.len() > 1500 {
                        format!("{}...", &code[..1500])
                    } else {
                        code.clone()
                    }
                })
                .unwrap_or_else(|| {
                    if content.len() > 500 {
                        format!("{}...", &content[..500])
                    } else {
                        content.clone()
                    }
                }),
        })
        .collect();

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

    let total = items.len();
    Ok(Json(CodeOutput {
        action: "search".into(),
        message: response,
        data: Some(CodeData::Search(SearchResultsData {
            results: items,
            search_type: result.search_type.to_string(),
            total,
        })),
    }))
}

/// Find functions that call a specific function
pub async fn find_function_callers<C: ToolContext>(
    ctx: &C,
    function_name: String,
    limit: Option<i64>,
) -> Result<Json<CodeOutput>, String> {
    if function_name.is_empty() {
        return Err("function_name is required".to_string());
    }

    let limit = limit.unwrap_or(20) as usize;
    let project_id = ctx.project_id().await;
    let project = ctx.get_project().await;
    let context_header = format_project_header(project.as_ref());

    let fn_name = function_name.clone();
    let results = ctx
        .code_pool()
        .run(move |conn| Ok::<_, String>(find_callers(conn, project_id, &fn_name, limit)))
        .await?;

    if results.is_empty() {
        return Ok(Json(CodeOutput {
            action: "callers".into(),
            message: format!("{}No callers found for `{}`.", context_header, function_name),
            data: Some(CodeData::CallGraph(CallGraphData {
                target: function_name,
                direction: "callers".into(),
                functions: vec![],
                total: 0,
            })),
        }));
    }

    let total = results.len();
    Ok(Json(CodeOutput {
        action: "callers".into(),
        message: format!(
            "{}{}",
            context_header,
            format_crossref_results(&function_name, CrossRefType::Caller, &results)
        ),
        data: Some(CodeData::CallGraph(CallGraphData {
            target: function_name,
            direction: "callers".into(),
            functions: results
                .iter()
                .map(|r| CallGraphEntry {
                    function_name: r.symbol_name.clone(),
                    file_path: r.file_path.clone(),
                    line: None,
                })
                .collect(),
            total,
        })),
    }))
}

/// Find functions called by a specific function
pub async fn find_function_callees<C: ToolContext>(
    ctx: &C,
    function_name: String,
    limit: Option<i64>,
) -> Result<Json<CodeOutput>, String> {
    if function_name.is_empty() {
        return Err("function_name is required".to_string());
    }

    let limit = limit.unwrap_or(20) as usize;
    let project_id = ctx.project_id().await;
    let project = ctx.get_project().await;
    let context_header = format_project_header(project.as_ref());

    let fn_name = function_name.clone();
    let results = ctx
        .code_pool()
        .run(move |conn| Ok::<_, String>(find_callees(conn, project_id, &fn_name, limit)))
        .await?;

    if results.is_empty() {
        return Ok(Json(CodeOutput {
            action: "callees".into(),
            message: format!("{}No callees found for `{}`.", context_header, function_name),
            data: Some(CodeData::CallGraph(CallGraphData {
                target: function_name,
                direction: "callees".into(),
                functions: vec![],
                total: 0,
            })),
        }));
    }

    let total = results.len();
    Ok(Json(CodeOutput {
        action: "callees".into(),
        message: format!(
            "{}{}",
            context_header,
            format_crossref_results(&function_name, CrossRefType::Callee, &results)
        ),
        data: Some(CodeData::CallGraph(CallGraphData {
            target: function_name,
            direction: "callees".into(),
            functions: results
                .iter()
                .map(|r| CallGraphEntry {
                    function_name: r.symbol_name.clone(),
                    file_path: r.file_path.clone(),
                    line: None,
                })
                .collect(),
            total,
        })),
    }))
}

/// Get symbols from a file
pub fn get_symbols(
    file_path: String,
    symbol_type: Option<String>,
) -> Result<Json<CodeOutput>, String> {
    let path = Path::new(&file_path);

    if !path.exists() {
        return Err(format!("File not found: {}", file_path));
    }

    // Parse file for symbols
    let symbols = indexer::extract_symbols(path).str_err()?;

    if symbols.is_empty() {
        return Ok(Json(CodeOutput {
            action: "symbols".into(),
            message: "No symbols found.".to_string(),
            data: Some(CodeData::Symbols(SymbolsData {
                symbols: vec![],
                total: 0,
            })),
        }));
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
    let display: Vec<_> = symbols.iter().take(10).collect();

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

    let symbol_items: Vec<SymbolInfo> = symbols
        .iter()
        .map(|sym| SymbolInfo {
            name: sym.name.clone(),
            symbol_type: sym.symbol_type.clone(),
            start_line: sym.start_line as usize,
            end_line: sym.end_line as usize,
        })
        .collect();

    Ok(Json(CodeOutput {
        action: "symbols".into(),
        message: response,
        data: Some(CodeData::Symbols(SymbolsData {
            symbols: symbol_items,
            total,
        })),
    }))
}

/// Index project
pub async fn index<C: ToolContext>(
    ctx: &C,
    action: IndexAction,
    path: Option<String>,
    skip_embed: bool,
) -> Result<Json<IndexOutput>, String> {
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
            let stats =
                indexer::index_project(path, ctx.code_pool().clone(), embeddings, project_id)
                    .await
                    .str_err()?;
            if let Some(cache) = ctx.fuzzy_cache() {
                cache.invalidate_code(project_id).await;
            }

            let mut response = format!(
                "Indexed {} files, {} symbols, {} chunks",
                stats.files, stats.symbols, stats.chunks
            );

            // Auto-summarize modules that don't have descriptions yet
            let mut modules_summarized = None;
            if let Some(pid) = project_id {
                if let Some(llm_client) = ctx.llm_factory().client_for_background() {
                    match auto_summarize_modules(
                        ctx.code_pool(),
                        ctx.pool(),
                        pid,
                        &project_path,
                        &*llm_client,
                    )
                    .await
                    {
                        Ok(count) if count > 0 => {
                            response.push_str(&format!(", summarized {} modules", count));
                            modules_summarized = Some(count);
                        }
                        Ok(_) => {} // No modules needed summarization
                        Err(e) => {
                            tracing::warn!("Auto-summarize failed: {}", e);
                        }
                    }
                }
            }

            Ok(Json(IndexOutput {
                action: "project".into(),
                message: response,
                data: Some(IndexData::Project(IndexProjectData {
                    files: stats.files,
                    symbols: stats.symbols,
                    chunks: stats.chunks,
                    modules_summarized,
                })),
            }))
        }
        IndexAction::Compact => {
            let stats = ctx
                .code_pool()
                .compact_code_db()
                .await
                .str_err()?;

            let message = format!(
                "Compacted vec_code: {} rows preserved, ~{:.1} MB estimated savings.\n\
                 VACUUM complete — database file should now reflect reduced size.",
                stats.rows_preserved, stats.estimated_savings_mb
            );

            Ok(Json(IndexOutput {
                action: "compact".into(),
                message,
                data: Some(IndexData::Compact(IndexCompactData {
                    rows_preserved: stats.rows_preserved,
                    estimated_savings_mb: stats.estimated_savings_mb,
                })),
            }))
        }
        IndexAction::Summarize => {
            return summarize_codebase(ctx).await;
        }
        IndexAction::Status => {
            use crate::db::{count_embedded_chunks_sync, count_symbols_sync};

            let project = ctx.get_project().await;
            let project_id = project.as_ref().map(|p| p.id);

            let (symbols, embedded) = ctx
                .code_pool()
                .run(move |conn| {
                    let symbols = count_symbols_sync(conn, project_id);
                    let embedded = count_embedded_chunks_sync(conn, project_id);
                    Ok::<_, String>((symbols, embedded))
                })
                .await?;

            Ok(Json(IndexOutput {
                action: "status".into(),
                message: format!(
                    "Index status: {} symbols, {} embedded chunks",
                    symbols, embedded
                ),
                data: Some(IndexData::Status(IndexStatusData {
                    symbols: symbols as usize,
                    embedded_chunks: embedded as usize,
                })),
            }))
        }
    }
}

/// Auto-summarize modules that don't have descriptions (called after indexing)
async fn auto_summarize_modules(
    code_pool: &std::sync::Arc<crate::db::pool::DatabasePool>,
    main_pool: &std::sync::Arc<crate::db::pool::DatabasePool>,
    project_id: i64,
    project_path: &str,
    llm_client: &dyn LlmClient,
) -> Result<usize, String> {
    // Get modules needing summaries (from code DB)
    let mut modules = code_pool
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

    // Record usage (main DB)
    record_llm_usage(
        main_pool,
        llm_client.provider_type(),
        &llm_client.model_name(),
        "tool:auto_summarize",
        &result,
        Some(project_id),
        None,
    )
    .await;

    let content = result.content.ok_or("No content in LLM response")?;

    // Parse and update (code DB)
    let summaries = cartographer::parse_summary_response(&content);
    if summaries.is_empty() {
        return Err("Failed to parse summaries".to_string());
    }

    let updated = code_pool
        .run(move |conn| cartographer::update_module_purposes(conn, project_id, &summaries))
        .await?;

    Ok(updated)
}

/// Summarize codebase modules using LLM (or heuristic fallback)
pub async fn summarize_codebase<C: ToolContext>(ctx: &C) -> Result<Json<IndexOutput>, String> {
    use crate::background::summaries::generate_heuristic_summaries;

    // Get project context
    let project = ctx.get_project().await;
    let (project_id, project_path) = match project.as_ref() {
        Some(p) => (p.id, p.path.clone()),
        None => return Err("No active project. Call session_start first.".to_string()),
    };

    // Get LLM client (optional — falls back to heuristic)
    let llm_client = ctx.llm_factory().client_for_background();

    // Get modules needing summaries (from code DB)
    let mut modules = ctx
        .code_pool()
        .run(move |conn| cartographer::get_modules_needing_summaries(conn, project_id))
        .await?;

    if modules.is_empty() {
        return Ok(Json(IndexOutput {
            action: "summarize".into(),
            message: "All modules already have summaries.".to_string(),
            data: Some(IndexData::Summarize(IndexSummarizeData {
                modules_summarized: 0,
            })),
        }));
    }

    // Fill in code previews
    let project_path_ref = Path::new(&project_path);
    for module in &mut modules {
        module.code_preview = cartographer::get_module_code_preview(project_path_ref, &module.path);
    }

    let summaries = if let Some(llm_client) = llm_client {
        // LLM path
        let prompt = cartographer::build_summary_prompt(&modules);
        let messages = vec![Message::user(prompt)];
        let result = llm_client
            .chat(messages, None)
            .await
            .map_err(|e| format!("LLM request failed: {}", e))?;

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
        let parsed = cartographer::parse_summary_response(&content);
        if parsed.is_empty() {
            return Err(format!(
                "Failed to parse summaries from LLM response:\n{}",
                content
            ));
        }
        parsed
    } else {
        // Heuristic fallback — no LLM available
        let heuristic = generate_heuristic_summaries(&modules);
        if heuristic.is_empty() {
            return Err("No modules could be summarized heuristically.".to_string());
        }
        heuristic.into_iter().collect()
    };

    // Update database (code DB)
    // Only clear cached modules when using LLM summaries (heuristic ones are upgradeable)
    let has_llm = ctx.llm_factory().has_providers();
    let summaries_clone = summaries.clone();
    let updated: usize = ctx
        .code_pool()
        .run(move |conn| {
            let count = cartographer::update_module_purposes(conn, project_id, &summaries_clone)
                .str_err()?;

            if has_llm {
                use crate::db::clear_modules_without_purpose_sync;
                clear_modules_without_purpose_sync(conn, project_id).str_err()?;
            }

            Ok::<_, String>(count)
        })
        .await?;

    let message = format!(
        "Summarized {} modules:\n{}",
        updated,
        summaries
            .iter()
            .map(|(id, summary)| format!("  {}: {}", id, summary))
            .collect::<Vec<_>>()
            .join("\n")
    );

    Ok(Json(IndexOutput {
        action: "summarize".into(),
        message,
        data: Some(IndexData::Summarize(IndexSummarizeData {
            modules_summarized: updated,
        })),
    }))
}

/// Get module dependencies and circular dependency warnings
async fn get_dependencies<C: ToolContext>(ctx: &C) -> Result<Json<CodeOutput>, String> {
    let project_id = ctx
        .project_id()
        .await
        .ok_or("No active project. Call project(action='start') first.")?;

    let deps = ctx
        .code_pool()
        .run(move |conn| {
            crate::db::dependencies::get_module_deps_sync(conn, project_id)
                .str_err()
        })
        .await?;

    if deps.is_empty() {
        return Ok(Json(CodeOutput {
            action: "dependencies".into(),
            message: "No module dependencies found. Run a health scan or index the project first.".to_string(),
            data: Some(CodeData::Dependencies(DependenciesData {
                edges: vec![],
                circular_count: 0,
                total: 0,
            })),
        }));
    }

    let circular: Vec<_> = deps.iter().filter(|d| d.is_circular).collect();
    let circular_count = circular.len();

    let mut response = format!("Module dependencies ({} edges):\n\n", deps.len());

    // Show circular warnings first
    if !circular.is_empty() {
        response.push_str(&format!("⚠ {} circular dependencies detected:\n", circular.len()));
        for dep in &circular {
            response.push_str(&format!(
                "  {} <-> {} ({} calls, {} imports)\n",
                dep.source_module_id, dep.target_module_id, dep.call_count, dep.import_count
            ));
        }
        response.push('\n');
    }

    // Show top dependencies by weight
    response.push_str("Top dependencies (by call+import count):\n");
    for dep in deps.iter().take(30) {
        let circular_marker = if dep.is_circular { " ⚠" } else { "" };
        response.push_str(&format!(
            "  {} -> {} [{}] calls:{} imports:{}{}\n",
            dep.source_module_id,
            dep.target_module_id,
            dep.dependency_type,
            dep.call_count,
            dep.import_count,
            circular_marker,
        ));
    }

    if deps.len() > 30 {
        response.push_str(&format!("  ... and {} more\n", deps.len() - 30));
    }

    let total = deps.len();
    let edges: Vec<DependencyEdge> = deps
        .iter()
        .map(|d| DependencyEdge {
            source: d.source_module_id.clone(),
            target: d.target_module_id.clone(),
            dependency_type: d.dependency_type.clone(),
            call_count: d.call_count,
            import_count: d.import_count,
            is_circular: d.is_circular,
        })
        .collect();

    Ok(Json(CodeOutput {
        action: "dependencies".into(),
        message: response,
        data: Some(CodeData::Dependencies(DependenciesData {
            edges,
            circular_count,
            total,
        })),
    }))
}

/// Get detected architectural patterns
async fn get_patterns<C: ToolContext>(ctx: &C) -> Result<Json<CodeOutput>, String> {
    let project_id = ctx
        .project_id()
        .await
        .ok_or("No active project. Call project(action='start') first.")?;

    let patterns = ctx
        .code_pool()
        .run(move |conn| {
            crate::background::code_health::patterns::get_all_module_patterns(conn, project_id)
        })
        .await?;

    if patterns.is_empty() {
        return Ok(Json(CodeOutput {
            action: "patterns".into(),
            message: "No architectural patterns detected yet. Run a health scan first.".to_string(),
            data: Some(CodeData::Patterns(PatternsData {
                modules: vec![],
                total: 0,
            })),
        }));
    }

    let mut response = format!("Architectural patterns ({} modules):\n\n", patterns.len());

    let mut module_patterns_list: Vec<ModulePatterns> = Vec::new();

    for (module_id, name, patterns_json) in &patterns {
        response.push_str(&format!("━━━ {} ({}) ━━━\n", module_id, name));

        let mut pattern_entries: Vec<PatternEntry> = Vec::new();

        if let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(patterns_json) {
            for p in &parsed {
                let pattern = p.get("pattern").and_then(|v| v.as_str()).unwrap_or("?");
                let confidence = p.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let evidence_list = p
                    .get("evidence")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                    });
                let evidence_str = evidence_list
                    .as_ref()
                    .map(|v| v.join(", "))
                    .unwrap_or_default();

                response.push_str(&format!(
                    "  [{}] {:.0}% — {}\n",
                    pattern,
                    confidence * 100.0,
                    evidence_str
                ));

                pattern_entries.push(PatternEntry {
                    pattern: pattern.to_string(),
                    confidence,
                    evidence: evidence_list,
                });
            }
        }
        response.push('\n');

        module_patterns_list.push(ModulePatterns {
            module_id: module_id.clone(),
            module_name: name.clone(),
            patterns: pattern_entries,
        });
    }

    let total = module_patterns_list.len();
    Ok(Json(CodeOutput {
        action: "patterns".into(),
        message: response,
        data: Some(CodeData::Patterns(PatternsData {
            modules: module_patterns_list,
            total,
        })),
    }))
}

/// Get tech debt scores for all modules
async fn get_tech_debt<C: ToolContext>(ctx: &C) -> Result<Json<CodeOutput>, String> {
    use crate::background::code_health::scoring::tier_label;

    let project_id = ctx
        .project_id()
        .await
        .ok_or("No active project. Call project(action='start') first.")?;

    let scores = ctx
        .pool()
        .run(move |conn| {
            crate::db::tech_debt::get_debt_scores_sync(conn, project_id)
                .str_err()
        })
        .await?;

    if scores.is_empty() {
        return Ok(Json(CodeOutput {
            action: "tech_debt".into(),
            message: "No tech debt scores computed yet. Run a health scan first.".to_string(),
            data: Some(CodeData::TechDebt(TechDebtData {
                modules: vec![],
                summary: vec![],
                total: 0,
            })),
        }));
    }

    // Summary
    let summary = ctx
        .pool()
        .run(move |conn| {
            crate::db::tech_debt::get_debt_summary_sync(conn, project_id)
                .str_err()
        })
        .await?;

    let mut response = format!("Tech Debt Report ({} modules):\n\n", scores.len());

    // Tier summary
    response.push_str("Summary by tier:\n");
    for (tier, count) in &summary {
        response.push_str(&format!("  {} ({}): {} modules\n", tier, tier_label(tier), count));
    }
    response.push('\n');

    // Per-module scores (worst first)
    response.push_str("Modules (worst first):\n\n");

    let mut module_items: Vec<TechDebtModule> = Vec::new();

    for score in &scores {
        let line_info = score
            .line_count
            .map(|l| format!(" {}L", l))
            .unwrap_or_default();
        let finding_info = score
            .finding_count
            .map(|f| format!(" {}findings", f))
            .unwrap_or_default();

        response.push_str(&format!(
            "  {} [{} {}] score:{:.0}{}{}\n",
            score.module_path,
            score.tier,
            tier_label(&score.tier),
            score.overall_score,
            line_info,
            finding_info,
        ));

        // Show top factors for D/F tier
        let mut top_factors: Option<Vec<DebtFactor>> = None;
        if score.tier == "D" || score.tier == "F" {
            if let Ok(factors) = serde_json::from_str::<serde_json::Value>(&score.factor_scores) {
                let mut factor_list: Vec<(String, f64)> = Vec::new();
                if let Some(obj) = factors.as_object() {
                    for (name, val) in obj {
                        if let Some(s) = val.get("score").and_then(|v| v.as_f64()) {
                            if s > 20.0 {
                                factor_list.push((name.clone(), s));
                            }
                        }
                    }
                }
                factor_list.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                for (name, s) in factor_list.iter().take(3) {
                    response.push_str(&format!("    ↳ {}: {:.0}\n", name, s));
                }
                if !factor_list.is_empty() {
                    top_factors = Some(
                        factor_list
                            .into_iter()
                            .take(3)
                            .map(|(name, s)| DebtFactor { name, score: s })
                            .collect(),
                    );
                }
            }
        }

        module_items.push(TechDebtModule {
            module_path: score.module_path.clone(),
            tier: score.tier.clone(),
            overall_score: score.overall_score,
            line_count: score.line_count,
            finding_count: score.finding_count,
            top_factors,
        });
    }

    let summary_items: Vec<TechDebtTier> = summary
        .iter()
        .map(|(tier, count)| TechDebtTier {
            tier: tier.clone(),
            label: tier_label(tier).to_string(),
            count: *count as usize,
        })
        .collect();

    let total = module_items.len();
    Ok(Json(CodeOutput {
        action: "tech_debt".into(),
        message: response,
        data: Some(CodeData::TechDebt(TechDebtData {
            modules: module_items,
            summary: summary_items,
            total,
        })),
    }))
}
