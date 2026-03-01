// crates/mira-server/src/tools/core/code/mod.rs
// Unified code tools (search, callers, callees, symbols, index, analysis)

mod analysis;
mod bundle;
mod index;
mod search;

// Re-export everything for backward compatibility with `pub use code::*;`
pub use analysis::*;
pub use bundle::*;
pub use index::*;
pub use search::*;

use crate::error::MiraError;
use crate::mcp::responses::CodeOutput;
use crate::mcp::responses::Json;
use crate::search::{
    CrossRefResult, HybridSearchResult, find_callees, find_callers, hybrid_search,
};
use crate::tools::core::{ToolContext, get_project_info};

// ═══════════════════════════════════════════════════════════════════════════════
// Query Core — shared by MCP handlers
// ═══════════════════════════════════════════════════════════════════════════════

/// Search code semantically. Returns raw search results without MCP formatting.
pub async fn query_search_code<C: ToolContext>(
    ctx: &C,
    query: &str,
    limit: usize,
) -> Result<HybridSearchResult, MiraError> {
    let pi = get_project_info(ctx).await;
    hybrid_search(
        ctx.code_pool().inner(),
        ctx.embeddings(),
        ctx.fuzzy_cache(),
        query,
        pi.id,
        pi.path.as_deref(),
        limit,
    )
    .await
}

/// Find callers of a function. Returns raw crossref results.
pub async fn query_callers<C: ToolContext>(
    ctx: &C,
    fn_name: &str,
    limit: usize,
) -> Result<Vec<CrossRefResult>, MiraError> {
    let project_id = ctx.project_id().await;
    let fn_name = fn_name.to_string();
    ctx.code_pool()
        .run(move |conn| {
            find_callers(conn, project_id, &fn_name, limit).map_err(|e| {
                tracing::debug!("callers query error: {}", e);
                MiraError::Other(format!(
                    "Failed to query callers: {}. Try re-indexing with index(action=\"project\").",
                    e
                ))
            })
        })
        .await
        .map_err(|e| {
            tracing::debug!("callers pool error: {}", e);
            MiraError::Other(format!(
                "Failed to query callers: {}. Try re-indexing with index(action=\"project\").",
                e
            ))
        })
}

/// Find callees of a function. Returns raw crossref results.
pub async fn query_callees<C: ToolContext>(
    ctx: &C,
    fn_name: &str,
    limit: usize,
) -> Result<Vec<CrossRefResult>, MiraError> {
    let project_id = ctx.project_id().await;
    let fn_name = fn_name.to_string();
    ctx.code_pool()
        .run(move |conn| {
            find_callees(conn, project_id, &fn_name, limit).map_err(|e| {
                tracing::debug!("callees query error: {}", e);
                MiraError::Other(format!(
                    "Failed to query callees: {}. Try re-indexing with index(action=\"project\").",
                    e
                ))
            })
        })
        .await
        .map_err(|e| {
            tracing::debug!("callees pool error: {}", e);
            MiraError::Other(format!(
                "Failed to query callees: {}. Try re-indexing with index(action=\"project\").",
                e
            ))
        })
}

// ═══════════════════════════════════════════════════════════════════════════════
// MCP Handler — dispatches to sub-modules
// ═══════════════════════════════════════════════════════════════════════════════

/// Unified code tool dispatcher
pub async fn handle_code<C: ToolContext>(
    ctx: &C,
    req: crate::mcp::requests::CodeRequest,
) -> Result<Json<CodeOutput>, MiraError> {
    use crate::mcp::requests::CodeAction;
    match req.action {
        CodeAction::Search => {
            let query = req.query.ok_or_else(|| {
                MiraError::InvalidInput("query is required for code(action=search)".to_string())
            })?;
            search_code(ctx, query, req.limit).await
        }
        CodeAction::Symbols => {
            let file_path = req.file_path.ok_or_else(|| {
                MiraError::InvalidInput(
                    "file_path is required for code(action=symbols)".to_string(),
                )
            })?;
            // Validate file_path is within the project directory.
            // Both paths must canonicalize successfully — falling back to raw
            // strings would bypass traversal detection (e.g. "../../etc/passwd").
            if let Some(project) = ctx.get_project().await {
                let project_path =
                    std::path::Path::new(&project.path)
                        .canonicalize()
                        .map_err(|e| {
                            MiraError::InvalidInput(format!(
                                "Cannot resolve project path '{}': {}",
                                project.path, e
                            ))
                        })?;
                let target_path = std::path::Path::new(&file_path)
                    .canonicalize()
                    .map_err(|e| {
                        MiraError::InvalidInput(format!(
                            "Cannot resolve file path '{}': {}. Does the file exist?",
                            file_path, e
                        ))
                    })?;
                if !target_path.starts_with(&project_path) {
                    return Err(MiraError::InvalidInput(format!(
                        "File path must be within the project directory: {}",
                        project.path
                    )));
                }
            }
            get_symbols(file_path, req.symbol_type)
        }
        CodeAction::Callers => {
            let function_name = req.function_name.ok_or_else(|| {
                MiraError::InvalidInput(
                    "function_name is required for code(action=callers)".to_string(),
                )
            })?;
            find_function_callers(ctx, function_name, req.limit).await
        }
        CodeAction::Callees => {
            let function_name = req.function_name.ok_or_else(|| {
                MiraError::InvalidInput(
                    "function_name is required for code(action=callees)".to_string(),
                )
            })?;
            find_function_callees(ctx, function_name, req.limit).await
        }
        CodeAction::Dependencies => get_dependencies(ctx).await,
        CodeAction::Diff => {
            // Defensive guard: router intercepts Tasks/Diff actions before reaching this handler
            Err(MiraError::Other(
                "Internal routing error for code(action=diff) — please report this as a bug."
                    .to_string(),
            ))
        }
        CodeAction::DeadCode => get_dead_code(ctx, req.limit).await,
        CodeAction::Bundle => {
            let scope = req.scope.ok_or_else(|| {
                MiraError::InvalidInput("scope is required for code(action=bundle)".to_string())
            })?;
            generate_bundle(ctx, scope, req.budget, req.depth).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::requests::{CodeAction, CodeRequest};
    use crate::tools::core::test_utils::MockToolContext;

    fn make_code_request(action: CodeAction) -> CodeRequest {
        CodeRequest {
            action,
            query: None,
            file_path: None,
            function_name: None,
            symbol_type: None,
            limit: None,
            from_ref: None,
            to_ref: None,
            include_impact: None,
            scope: None,
            budget: None,
            depth: None,
        }
    }

    // ========================================================================
    // handle_code dispatch: missing required fields
    // ========================================================================

    #[tokio::test]
    async fn test_handle_code_search_missing_query() {
        let ctx = MockToolContext::with_project().await;
        let req = make_code_request(CodeAction::Search);
        match handle_code(&ctx, req).await {
            Err(e) => assert!(
                e.to_string().contains("query"),
                "Error should mention 'query', got: {e}"
            ),
            Ok(_) => panic!("Search without query should fail"),
        }
    }

    #[tokio::test]
    async fn test_handle_code_symbols_missing_file_path() {
        let ctx = MockToolContext::with_project().await;
        let req = make_code_request(CodeAction::Symbols);
        match handle_code(&ctx, req).await {
            Err(e) => assert!(
                e.to_string().contains("file_path"),
                "Error should mention 'file_path', got: {e}"
            ),
            Ok(_) => panic!("Symbols without file_path should fail"),
        }
    }

    #[tokio::test]
    async fn test_handle_code_callers_missing_function_name() {
        let ctx = MockToolContext::with_project().await;
        let req = make_code_request(CodeAction::Callers);
        match handle_code(&ctx, req).await {
            Err(e) => assert!(
                e.to_string().contains("function_name"),
                "Error should mention 'function_name', got: {e}"
            ),
            Ok(_) => panic!("Callers without function_name should fail"),
        }
    }

    #[tokio::test]
    async fn test_handle_code_callees_missing_function_name() {
        let ctx = MockToolContext::with_project().await;
        let req = make_code_request(CodeAction::Callees);
        match handle_code(&ctx, req).await {
            Err(e) => assert!(
                e.to_string().contains("function_name"),
                "Error should mention 'function_name', got: {e}"
            ),
            Ok(_) => panic!("Callees without function_name should fail"),
        }
    }

    // ========================================================================
    // query_callers / query_callees: empty index returns empty vec
    // ========================================================================

    #[tokio::test]
    async fn test_query_callers_empty_index() {
        let ctx = MockToolContext::with_project().await;
        let result = query_callers(&ctx, "some_function", 10).await;
        assert!(
            result.is_ok(),
            "query_callers should succeed with empty index"
        );
        assert!(
            result.unwrap().is_empty(),
            "No callers expected in empty index"
        );
    }

    #[tokio::test]
    async fn test_query_callees_empty_index() {
        let ctx = MockToolContext::with_project().await;
        let result = query_callees(&ctx, "some_function", 10).await;
        assert!(
            result.is_ok(),
            "query_callees should succeed with empty index"
        );
        assert!(
            result.unwrap().is_empty(),
            "No callees expected in empty index"
        );
    }
}
