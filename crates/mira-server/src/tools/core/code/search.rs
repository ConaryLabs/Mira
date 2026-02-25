// crates/mira-server/src/tools/core/code/search.rs
// Search, callers, callees, and symbol extraction

use std::path::Path;

use crate::error::MiraError;
use crate::indexer;
use crate::mcp::responses::Json;
use crate::mcp::responses::{
    CallGraphData, CallGraphEntry, CodeData, CodeOutput, CodeSearchResult, SearchResultsData,
    SymbolInfo, SymbolsData,
};
use crate::search::{
    CrossRefType, QueryIntent, crossref_search, expand_context_with_conn, find_callers,
    format_crossref_results, skeletonize_content,
};
use crate::tools::core::{ToolContext, get_project_info};
use crate::utils::truncate;

use super::{query_callees, query_callers, query_search_code};

/// Search code using semantic similarity or keyword fallback
pub async fn search_code<C: ToolContext>(
    ctx: &C,
    query: String,
    limit: Option<i64>,
) -> Result<Json<CodeOutput>, MiraError> {
    let limit = limit.unwrap_or(10).clamp(1, 100) as usize;
    let pi = get_project_info(ctx).await;
    let project_id = pi.id;
    let project_path = pi.path.clone();
    let context_header = pi.header;

    // Check for cross-reference query patterns first ("who calls X", "callers of X", etc.)
    let query_clone = query.clone();
    let crossref_result = ctx
        .code_pool()
        .run(move |conn| {
            crossref_search(conn, &query_clone, project_id, limit)
                .map_err(|e| MiraError::Other(format!("Failed to search code cross-references: {}", e)))
        })
        .await
        .map_err(|e| MiraError::Other(format!("Failed to search code cross-references: {}. Try re-indexing with index(action=\"project\").", e)))?;

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
    let detected_intent = result.intent;

    if result.results.is_empty() {
        // Check whether the project has any indexed data at all
        let has_index = ctx
            .code_pool()
            .run(move |conn| {
                let symbols: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM code_symbols WHERE project_id IS ?1",
                        rusqlite::params![project_id],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);
                if symbols > 0 {
                    return Ok::<bool, MiraError>(true);
                }
                let chunks: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM code_chunks WHERE project_id IS ?1",
                        rusqlite::params![project_id],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);
                Ok(chunks > 0)
            })
            .await
            .unwrap_or(false);

        let empty_message = if has_index {
            format!(
                "{}No code matches found for '{}'. Try different search terms or check spelling.",
                context_header, query
            )
        } else {
            format!(
                "{}No code index found for this project. The index builds automatically as you work, or run index(action='project') to index now.",
                context_header
            )
        };

        return Ok(Json(CodeOutput {
            action: "search".into(),
            message: empty_message,
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
        .run(move |conn| -> Result<Vec<ExpandedResult>, MiraError> {
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
        .await
        .map_err(|e| MiraError::Other(format!("Failed to expand code search results: {}. Try re-indexing with index(action=\"project\").", e)))?;

    /// Number of top-ranked results that get full content; rest are skeletonized
    const TIERED_FULL_COUNT: usize = 2;

    let items: Vec<CodeSearchResult> = expanded_results
        .iter()
        .enumerate()
        .map(|(rank, (file_path, content, score, expanded))| {
            let use_full = rank < TIERED_FULL_COUNT;
            CodeSearchResult {
                file_path: file_path.clone(),
                score: *score,
                symbol_info: if use_full {
                    expanded.as_ref().and_then(|(info, _)| info.clone())
                } else {
                    None
                },
                content: if use_full {
                    expanded
                        .as_ref()
                        .map(|(_, code)| truncate(code, 1500))
                        .unwrap_or_else(|| truncate(content, 500))
                } else {
                    skeletonize_content(
                        expanded
                            .as_ref()
                            .map(|(_, code)| code.as_str())
                            .unwrap_or(content),
                    )
                },
            }
        })
        .collect();

    for (rank, (file_path, content, score, expanded)) in expanded_results.into_iter().enumerate() {
        response.push_str(&format!("━━━ {} (score: {:.2}) ━━━\n", file_path, score));

        if rank < TIERED_FULL_COUNT {
            // Top-ranked results get full content
            if let Some((symbol_info, full_code)) = expanded {
                if let Some(info) = symbol_info {
                    response.push_str(&format!("{}\n", info));
                }
                let code_display = if full_code.len() > 1500 {
                    format!("{}\n[truncated]", truncate(&full_code, 1500))
                } else {
                    full_code
                };
                response.push_str(&format!("```\n{}\n```\n\n", code_display));
            } else {
                let display = truncate(&content, 500);
                response.push_str(&format!("```\n{}\n```\n\n", display));
            }
        } else {
            // Lower-ranked results get skeletonized
            let source = expanded
                .as_ref()
                .map(|(_, code)| code.as_str())
                .unwrap_or(&content);
            let skeleton = skeletonize_content(source);
            response.push_str(&format!("```\n{}\n```\n\n", skeleton));
        }
    }

    // Refactor intent: enrich top results with caller info ("blast radius")
    if detected_intent == QueryIntent::Refactor && !items.is_empty() {
        // Extract public function names from top results for caller lookup
        let mut pub_fns: Vec<String> = Vec::new();
        for item in items.iter().take(3) {
            for line in item.content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("pub fn ") || trimmed.starts_with("pub async fn ") {
                    // Extract function name from signature
                    let after_fn = if let Some(rest) = trimmed.strip_prefix("pub async fn ") {
                        rest
                    } else if let Some(rest) = trimmed.strip_prefix("pub fn ") {
                        rest
                    } else {
                        continue;
                    };
                    if let Some(name) = after_fn.split('(').next() {
                        let name = name.split('<').next().unwrap_or(name).trim();
                        if !name.is_empty() {
                            pub_fns.push(name.to_string());
                        }
                    }
                }
            }
        }

        if !pub_fns.is_empty() {
            // Look up callers for discovered public functions
            let caller_results: Vec<(String, Vec<crate::search::CrossRefResult>)> = {
                let fns = pub_fns.clone();
                ctx.code_pool()
                    .run(
                        move |conn| -> Result<
                            Vec<(String, Vec<crate::search::CrossRefResult>)>,
                            MiraError,
                        > {
                            let mut out = Vec::new();
                            for fn_name in &fns {
                                match find_callers(conn, project_id, fn_name, 5) {
                                    Ok(callers) if !callers.is_empty() => {
                                        out.push((fn_name.clone(), callers));
                                    }
                                    _ => {}
                                }
                            }
                            Ok(out)
                        },
                    )
                    .await
                    .unwrap_or_default()
            };

            if !caller_results.is_empty() {
                response.push_str("━━━ Blast Radius (callers of top results) ━━━\n");
                for (fn_name, callers) in &caller_results {
                    response.push_str(&format!("  {} ({} callers):", fn_name, callers.len()));
                    for caller in callers {
                        response.push_str(&format!(" {}", caller.symbol_name));
                    }
                    response.push('\n');
                }
                response.push('\n');
            }
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
) -> Result<Json<CodeOutput>, MiraError> {
    if function_name.is_empty() {
        return Err(MiraError::InvalidInput(
            "function_name is required for code(action=callers)".to_string(),
        ));
    }

    let limit = limit.unwrap_or(20).clamp(1, 100) as usize;
    let context_header = get_project_info(ctx).await.header;

    let results = query_callers(ctx, &function_name, limit).await?;

    if results.is_empty() {
        return Ok(Json(CodeOutput {
            action: "callers".into(),
            message: format!(
                "{}No callers found for `{}`. The function may have no callers, or try re-indexing with index(action=\"project\").",
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
) -> Result<Json<CodeOutput>, MiraError> {
    if function_name.is_empty() {
        return Err(MiraError::InvalidInput(
            "function_name is required for code(action=callees)".to_string(),
        ));
    }

    let limit = limit.unwrap_or(20).clamp(1, 100) as usize;
    let context_header = get_project_info(ctx).await.header;

    let results = query_callees(ctx, &function_name, limit).await?;

    if results.is_empty() {
        return Ok(Json(CodeOutput {
            action: "callees".into(),
            message: format!(
                "{}No callees found for `{}`. The function may have no callees, or try re-indexing with index(action=\"project\").",
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
) -> Result<Json<CodeOutput>, MiraError> {
    #[cfg(not(feature = "parsers"))]
    {
        let _ = (file_path, symbol_type);
        return Err(MiraError::Other("Symbol extraction requires the 'parsers' feature. Reinstall with: cargo install --git https://github.com/ConaryLabs/Mira.git --features parsers".to_string()));
    }
    #[cfg(feature = "parsers")]
    {
        let path = Path::new(&file_path);

        if !path.exists() {
            return Err(MiraError::InvalidInput(format!(
                "File not found: {}. Check the path exists and is within the project directory.",
                file_path
            )));
        }

        if path.is_dir() {
            return Err(MiraError::InvalidInput(format!(
                "'{}' is a directory, not a file. Provide a path to a specific source file.",
                file_path
            )));
        }

        // Guard against reading very large files (e.g. generated code).
        // The indexer skips files > 1MB; apply the same limit at query time.
        const MAX_SYMBOLS_FILE_BYTES: u64 = 1_024 * 1_024;
        if let Ok(meta) = std::fs::metadata(path)
            && meta.len() > MAX_SYMBOLS_FILE_BYTES
        {
            return Err(MiraError::InvalidInput(format!(
                "File is too large ({:.1} MB) for symbol extraction. Max: 1 MB.",
                meta.len() as f64 / (1_024.0 * 1_024.0)
            )));
        }

        // Parse file for symbols
        let symbols = indexer::extract_symbols(path)?;

        if symbols.is_empty() {
            return Ok(Json(CodeOutput {
                action: "symbols".into(),
                message: "No symbols found. The file may not contain recognizable definitions, or the language may not be supported. Supported: Rust (.rs), Python (.py), TypeScript (.ts/.tsx), JavaScript (.js/.jsx), Go (.go).".to_string(),
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

        // Cap structured data at 500 symbols to prevent oversized MCP responses
        let symbol_items: Vec<SymbolInfo> = symbols
            .iter()
            .take(500)
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
    } // #[cfg(feature = "parsers")]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::core::test_utils::MockToolContext;

    // ========================================================================
    // search_code
    // ========================================================================

    #[tokio::test]
    async fn test_search_code_empty_query_returns_no_results_message() {
        let ctx = MockToolContext::with_project().await;
        // Empty query succeeds at the search_code level (validation is in handle_code).
        // With an empty index it should return the "No code index found" message.
        let result = search_code(&ctx, String::new(), None).await;
        assert!(
            result.is_ok(),
            "search_code with empty query should not error"
        );
        let output = result.unwrap();
        assert!(
            output.0.message.contains("No code index")
                || output.0.message.contains("No code matches"),
            "Expected 'no index' message, got: {}",
            output.0.message
        );
    }

    #[tokio::test]
    async fn test_search_code_valid_query_empty_index_returns_no_index_message() {
        let ctx = MockToolContext::with_project().await;
        let result = search_code(&ctx, "authentication".to_string(), None).await;
        assert!(
            result.is_ok(),
            "search_code should succeed with empty index"
        );
        let output = result.unwrap();
        // No indexed data -> should mention indexing
        assert!(
            output.0.message.contains("No code index")
                || output.0.message.contains("No code matches"),
            "Expected no-index message, got: {}",
            output.0.message
        );
    }

    // ========================================================================
    // find_function_callers / find_function_callees: empty function_name
    // ========================================================================

    #[tokio::test]
    async fn test_find_function_callers_empty_name_errors() {
        let ctx = MockToolContext::with_project().await;
        match find_function_callers(&ctx, String::new(), None).await {
            Err(e) => assert!(
                e.to_string().contains("function_name"),
                "Error should mention 'function_name', got: {e}"
            ),
            Ok(_) => panic!("Empty function_name should fail"),
        }
    }

    #[tokio::test]
    async fn test_find_function_callees_empty_name_errors() {
        let ctx = MockToolContext::with_project().await;
        match find_function_callees(&ctx, String::new(), None).await {
            Err(e) => assert!(
                e.to_string().contains("function_name"),
                "Error should mention 'function_name', got: {e}"
            ),
            Ok(_) => panic!("Empty function_name should fail"),
        }
    }

    // ========================================================================
    // get_symbols: nonexistent file and directory path
    // ========================================================================

    #[cfg(feature = "parsers")]
    #[test]
    fn test_get_symbols_nonexistent_file_errors() {
        match get_symbols("/nonexistent/path/that/does/not/exist.rs".to_string(), None) {
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("File not found") || msg.contains("not found"),
                    "Error should mention file not found, got: {msg}"
                );
            }
            Ok(_) => panic!("Nonexistent file should fail"),
        }
    }

    #[cfg(feature = "parsers")]
    #[test]
    fn test_get_symbols_directory_path_errors() {
        match get_symbols("/tmp".to_string(), None) {
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("is a directory") || msg.contains("directory"),
                    "Error should mention directory, got: {msg}"
                );
            }
            Ok(_) => panic!("Directory path should fail"),
        }
    }

    #[cfg(not(feature = "parsers"))]
    #[test]
    fn test_get_symbols_requires_parsers_feature() {
        match get_symbols("/tmp/any_file.rs".to_string(), None) {
            Err(e) => assert!(
                e.to_string().contains("parsers"),
                "Error should mention 'parsers' feature, got: {}",
                e
            ),
            Ok(_) => panic!("Without parsers feature, get_symbols should fail"),
        }
    }
}
