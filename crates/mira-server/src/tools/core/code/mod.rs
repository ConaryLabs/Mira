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
        ctx.code_pool(),
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
            find_callers(conn, project_id, &fn_name, limit)
                .map_err(|e| MiraError::Other(format!("Failed to query callers: {}", e)))
        })
        .await
        .map_err(|e| {
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
            find_callees(conn, project_id, &fn_name, limit)
                .map_err(|e| MiraError::Other(format!("Failed to query callees: {}", e)))
        })
        .await
        .map_err(|e| {
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
            // Validate file_path is within the project directory
            if let Some(project) = ctx.get_project().await {
                let project_path = std::path::Path::new(&project.path)
                    .canonicalize()
                    .unwrap_or_else(|_| std::path::PathBuf::from(&project.path));
                let target_path = std::path::Path::new(&file_path)
                    .canonicalize()
                    .unwrap_or_else(|_| std::path::PathBuf::from(&file_path));
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
        CodeAction::Patterns => get_patterns(ctx).await,
        CodeAction::TechDebt => get_tech_debt(ctx).await,
        CodeAction::Diff => {
            // Defensive guard: router intercepts Tasks/Diff actions before reaching this handler
            Err(MiraError::Other(
                "Internal routing error for code(action=diff) — please report this as a bug."
                    .to_string(),
            ))
        }
        CodeAction::DeadCode => get_dead_code(ctx, req.limit).await,
        CodeAction::Conventions => {
            let file_path = req.file_path.ok_or_else(|| {
                MiraError::InvalidInput(
                    "file_path is required for code(action=conventions)".to_string(),
                )
            })?;
            get_conventions(ctx, file_path).await
        }
        CodeAction::DebtDelta => get_debt_delta(ctx).await,
        CodeAction::Bundle => {
            let scope = req.scope.ok_or_else(|| {
                MiraError::InvalidInput("scope is required for code(action=bundle)".to_string())
            })?;
            generate_bundle(ctx, scope, req.budget, req.depth).await
        }
    }
}
