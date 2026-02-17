// crates/mira-server/src/tools/core/code/index.rs
// Index, summarize, and health scan operations

use std::path::Path;

use crate::cartographer;
use crate::indexer;
use crate::llm::{LlmClient, Message, record_llm_usage};
use crate::mcp::requests::IndexAction;
use crate::mcp::responses::Json;
use crate::mcp::responses::{
    IndexCompactData, IndexData, IndexHealthData, IndexOutput, IndexProjectData, IndexStatusData,
    IndexSummarizeData,
};
use crate::tools::core::{NO_ACTIVE_PROJECT_ERROR, ToolContext};
use crate::utils::ResultExt;

/// Index project
pub async fn index<C: ToolContext>(
    ctx: &C,
    action: IndexAction,
    path: Option<String>,
    skip_embed: bool,
) -> Result<Json<IndexOutput>, String> {
    match action {
        IndexAction::Project | IndexAction::File => {
            #[cfg(not(feature = "parsers"))]
            {
                return Err("Code indexing requires the 'parsers' feature. Reinstall with: cargo install --git https://github.com/ConaryLabs/Mira.git --features parsers".to_string());
            }
            #[cfg(feature = "parsers")]
            {
                let project = ctx.get_project().await;
                let project_path = path
                    .or_else(|| project.as_ref().map(|p| p.path.clone()))
                    .ok_or_else(|| {
                        format!("{} Provide a path directly.", NO_ACTIVE_PROJECT_ERROR)
                    })?;

                let project_id = project.as_ref().map(|p| p.id);

                let path = Path::new(&project_path);
                if !path.exists() {
                    return Err(format!(
                        "Path not found: {}. Ensure project_path is an absolute path to an existing directory.",
                        project_path
                    ));
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
                if let Some(pid) = project_id
                    && let Some(llm_client) = ctx.llm_factory().client_for_background()
                {
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

                // Auto-queue health scan after project indexing
                if let Some(pid) = project_id {
                    let pool_clone = ctx.pool().clone();
                    let _ = pool_clone
                        .run(move |conn| {
                            crate::background::code_health::mark_health_scan_needed_sync(conn, pid)
                        })
                        .await;
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
            } // #[cfg(feature = "parsers")]
        }
        IndexAction::Compact => {
            let stats = ctx.code_pool().compact_code_db().await.str_err()?;

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
        IndexAction::Health => {
            return run_health_scan(ctx).await;
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

/// System prompt for code summarization (stable prefix for KV cache optimization)
const SUMMARIZE_SYSTEM_PROMPT: &str = "You are a code analysis assistant. Your task is to generate concise, accurate summaries of code modules. Focus on the primary purpose and functionality of each module. Be direct and technical.";

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

    // Build prompt and call LLM (system prompt first for KV cache optimization)
    let prompt = cartographer::build_summary_prompt(&modules);
    let messages = vec![
        Message::system(SUMMARIZE_SYSTEM_PROMPT.to_string()),
        Message::user(prompt),
    ];
    let result = llm_client
        .chat(messages, None)
        .await
        .map_err(|e| format!("LLM request failed: {}. Check API key configuration in ~/.mira/.env or run mira setup.", e))?;

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
        return Err("Failed to parse summaries. The LLM response was malformed. Try re-indexing or check LLM provider configuration.".to_string());
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
        None => {
            return Err(NO_ACTIVE_PROJECT_ERROR.to_string());
        }
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

    let (summaries, used_llm) = if let Some(llm_client) = llm_client {
        // LLM path (system prompt first for KV cache optimization)
        let prompt = cartographer::build_summary_prompt(&modules);
        let messages = vec![
            Message::system(SUMMARIZE_SYSTEM_PROMPT.to_string()),
            Message::user(prompt),
        ];
        let result = llm_client
            .chat(messages, None)
            .await
            .map_err(|e| format!("LLM request failed: {}. Check API key configuration in ~/.mira/.env or run mira setup.", e))?;

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
                "Failed to parse summaries from LLM response. The LLM response was malformed. Try re-indexing or check LLM provider configuration.\n{}",
                content
            ));
        }
        (parsed, true)
    } else {
        // Heuristic fallback — no LLM available
        let heuristic = generate_heuristic_summaries(&modules);
        if heuristic.is_empty() {
            return Err("No modules could be summarized heuristically.".to_string());
        }
        (heuristic.into_iter().collect(), false)
    };

    // Update database (code DB)
    // Only clear cached modules when LLM actually generated summaries (heuristic ones are upgradeable)
    let has_llm = used_llm;
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

/// Run a full code health scan (dependencies, patterns, tech debt, etc.)
pub async fn run_health_scan<C: ToolContext>(ctx: &C) -> Result<Json<IndexOutput>, String> {
    use crate::db::count_symbols_sync;

    let project = ctx.get_project().await;
    let (project_id, project_path) = match project.as_ref() {
        Some(p) => (p.id, p.path.clone()),
        None => {
            return Err(NO_ACTIVE_PROJECT_ERROR.to_string());
        }
    };

    // Guard: ensure the project has been indexed
    let has_symbols = ctx
        .code_pool()
        .run(move |conn| Ok::<_, String>(count_symbols_sync(conn, Some(project_id))))
        .await?;

    if has_symbols == 0 {
        return Err("No code indexed yet. Run index(action=\"project\") first.".to_string());
    }

    // Get LLM client for complexity/error-quality analysis (optional)
    let llm_client = ctx.llm_factory().client_for_background();

    // Run the full health scan (same as background worker, but forced)
    let issues = crate::background::code_health::scan_project_health_full(
        ctx.pool(),
        ctx.code_pool(),
        llm_client.as_ref(),
        project_id,
        &project_path,
    )
    .await?;

    // Mark as scanned so the background worker doesn't re-run immediately
    let pid = project_id;
    ctx.pool()
        .run(move |conn| crate::background::code_health::mark_health_scanned(conn, pid))
        .await?;

    Ok(Json(IndexOutput {
        action: "health".into(),
        message: format!(
            "Health scan complete: {} issues found for project {}",
            issues, project_path
        ),
        data: Some(IndexData::Health(IndexHealthData {
            issues_found: issues,
        })),
    }))
}
