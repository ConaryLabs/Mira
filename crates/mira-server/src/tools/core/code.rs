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

/// Check if a capability/feature exists in the codebase.
/// First searches cached capability memories, then falls back to live code search.
pub async fn check_capability<C: ToolContext>(
    ctx: &C,
    description: String,
) -> Result<String, String> {
    use crate::search::embedding_to_bytes;
    use rusqlite::params;

    if description.is_empty() {
        return Err("description is required".to_string());
    }

    let project_id = ctx.project_id().await;
    let project = ctx.get_project().await;
    let context_header = format_project_header(project.as_ref());
    let project_path = project.as_ref().map(|p| p.path.clone());

    // Step 1: Search capability memories
    if let Some(embeddings) = ctx.embeddings() {
        if let Ok(query_embedding) = embeddings.embed(&description).await {
            let conn = ctx.db().conn();

            // Search for capability and issue memories
            let mut stmt = conn
                .prepare(
                    "SELECT f.id, f.content, f.fact_type, vec_distance_cosine(v.embedding, ?1) as distance
                     FROM vec_memory v
                     JOIN memory_facts f ON v.fact_id = f.id
                     WHERE (f.project_id = ?2 OR f.project_id IS NULL OR ?2 IS NULL)
                       AND f.fact_type IN ('capability', 'issue')
                     ORDER BY distance
                     LIMIT 5",
                )
                .map_err(|e| e.to_string())?;

            let capability_results: Vec<(i64, String, String, f32)> = stmt
                .query_map(
                    params![embedding_to_bytes(&query_embedding), project_id, 5i64],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect();

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
                    let type_label = if *fact_type == "issue" { "[ISSUE]" } else { "[CAPABILITY]" };
                    response.push_str(&format!(
                        "  {} (score: {:.2}, id: {}) {}\n",
                        type_label, score, id, content
                    ));
                }

                return Ok(response);
            }
        }
    }

    // Step 2: Fall back to live code search
    let search_result = hybrid_search(
        ctx.db(),
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
