// crates/mira-server/src/tools/core/code/search.rs
// Search, callers, callees, and symbol extraction

use std::path::Path;

use crate::indexer;
use crate::mcp::responses::Json;
use crate::mcp::responses::{
    CallGraphData, CallGraphEntry, CodeData, CodeOutput, CodeSearchResult, SearchResultsData,
    SymbolInfo, SymbolsData,
};
use crate::search::{
    CrossRefType, crossref_search, expand_context_with_conn, format_crossref_results,
    format_project_header,
};
use crate::tools::core::ToolContext;
use crate::utils::ResultExt;

use super::{query_callees, query_callers, query_search_code};

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

    // Use shared query core for hybrid search
    let result = query_search_code(ctx, &query, limit).await?;

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
    let project = ctx.get_project().await;
    let context_header = format_project_header(project.as_ref());

    let results = query_callers(ctx, &function_name, limit).await;

    if results.is_empty() {
        return Ok(Json(CodeOutput {
            action: "callers".into(),
            message: format!(
                "{}No callers found for `{}`.",
                context_header, function_name
            ),
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
    let project = ctx.get_project().await;
    let context_header = format_project_header(project.as_ref());

    let results = query_callees(ctx, &function_name, limit).await;

    if results.is_empty() {
        return Ok(Json(CodeOutput {
            action: "callees".into(),
            message: format!(
                "{}No callees found for `{}`.",
                context_header, function_name
            ),
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
