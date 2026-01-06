//! Unified code tools (search, callers, callees, symbols, index)

use crate::search::{
    crossref_search, expand_context_with_db, find_callers, find_callees, 
    format_crossref_results, format_project_header, hybrid_search, CrossRefType,
};
use crate::tools::core::ToolContext;

/// Search code using semantic similarity or keyword fallback
pub async fn search_code<C: ToolContext>(
    ctx: &C,
    query: String,
    language: Option<String>,
    limit: Option<i64>,
) -> Result<String, String> {
    let limit = limit.unwrap_or(10) as usize;
    let project_id = ctx.project_id().await;
    let project = ctx.get_project().await;
    let project_path = project.as_ref().map(|p| p.path.clone());
    let context_header = format_project_header(project.as_ref());

    // Check for cross-reference query patterns first ("who calls X", "callers of X", etc.)
    if let Some((target, ref_type, results)) = crossref_search(ctx.db(), &query, project_id, limit) {
        return Ok(format!("{}{}", context_header, format_crossref_results(&target, ref_type, &results)));
    }

    // Use shared hybrid search
    let result = hybrid_search(
        ctx.db(),
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

    for r in &result.results {
        // Use shared context expansion with DB for full symbol bounds
        let expanded = expand_context_with_db(
            &r.file_path,
            &r.content,
            project_path.as_deref(),
            Some(ctx.db()),
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

    let results = find_callers(ctx.db(), project_id, &function_name, limit);
    
    if results.is_empty() {
        return Ok(format!("{}No callers found for `{}`.", context_header, function_name));
    }

    Ok(format!("{}{}", context_header, format_crossref_results(&function_name, CrossRefType::Caller, &results)))
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

    let results = find_callees(ctx.db(), project_id, &function_name, limit);
    
    if results.is_empty() {
        return Ok(format!("{}No callees found for `{}`.", context_header, function_name));
    }

    Ok(format!("{}{}", context_header, format_crossref_results(&function_name, CrossRefType::Callee, &results)))
}
