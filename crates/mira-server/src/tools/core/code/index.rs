// crates/mira-server/src/tools/core/code/index.rs
// Index, summarize, and health scan operations

use std::path::Path;

use crate::cartographer;
use crate::error::MiraError;
use crate::indexer;
use crate::mcp::requests::IndexAction;
use crate::mcp::responses::Json;
use crate::mcp::responses::{
    IndexCompactData, IndexData, IndexHealthData, IndexOutput, IndexProjectData, IndexStatusData,
    IndexSummarizeData,
};
use crate::tools::core::{NO_ACTIVE_PROJECT_ERROR, ToolContext};

/// Index project
pub async fn index<C: ToolContext>(
    ctx: &C,
    action: IndexAction,
    path: Option<String>,
    skip_embed: bool,
) -> Result<Json<IndexOutput>, MiraError> {
    match action {
        IndexAction::Project | IndexAction::File => {
            #[cfg(not(feature = "parsers"))]
            {
                return Err(MiraError::Other("Code indexing requires the 'parsers' feature. Reinstall with: cargo install --git https://github.com/ConaryLabs/Mira.git --features parsers".to_string()));
            }
            #[cfg(feature = "parsers")]
            {
                let project = ctx.get_project().await;
                let project_path = path
                    .or_else(|| project.as_ref().map(|p| p.path.clone()))
                    .ok_or_else(|| {
                        MiraError::InvalidInput(format!(
                            "{} Provide a path directly.",
                            NO_ACTIVE_PROJECT_ERROR
                        ))
                    })?;

                let project_id = project.as_ref().map(|p| p.id);

                let path = Path::new(&project_path);
                if !path.exists() {
                    return Err(MiraError::InvalidInput(format!(
                        "Path not found: {}. Ensure project_path is an absolute path to an existing directory.",
                        project_path
                    )));
                }

                // Index code (skip embeddings if requested for faster indexing)
                let embeddings = if skip_embed {
                    None
                } else {
                    ctx.embeddings().cloned()
                };
                let stats =
                    indexer::index_project(path, ctx.code_pool().clone(), embeddings, project_id)
                        .await?;
                if let Some(cache) = ctx.fuzzy_cache() {
                    cache.invalidate_code(project_id).await;
                }

                let response = format!(
                    "Indexed {} files, {} symbols, {} chunks",
                    stats.files, stats.symbols, stats.chunks
                );

                // Auto-queue health scan after project indexing
                if let Some(pid) = project_id {
                    let pool_clone = ctx.pool().clone();
                    if let Err(e) = pool_clone
                        .run(move |conn| {
                            crate::background::code_health::mark_health_scan_needed_sync(conn, pid)
                        })
                        .await
                    {
                        tracing::warn!("Failed to queue health scan after indexing: {}", e);
                    }
                }

                Ok(Json(IndexOutput {
                    action: "project".into(),
                    message: response,
                    data: Some(IndexData::Project(IndexProjectData {
                        files: stats.files,
                        symbols: stats.symbols,
                        chunks: stats.chunks,
                        modules_summarized: None,
                    })),
                }))
            } // #[cfg(feature = "parsers")]
        }
        IndexAction::Compact => {
            let stats = ctx.code_pool().compact_code_db().await?;

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
                    Ok::<_, MiraError>((symbols, embedded))
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

/// Summarize codebase modules using heuristic analysis
pub async fn summarize_codebase<C: ToolContext>(ctx: &C) -> Result<Json<IndexOutput>, MiraError> {
    use crate::background::summaries::generate_heuristic_summaries;

    // Get project context
    let project = ctx.get_project().await;
    let (project_id, project_path) = match project.as_ref() {
        Some(p) => (p.id, p.path.clone()),
        None => {
            return Err(MiraError::ProjectNotSet);
        }
    };

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

    // Heuristic summaries (no external LLM dependency)
    let summaries: std::collections::HashMap<String, String> = {
        let heuristic = generate_heuristic_summaries(&modules);
        if heuristic.is_empty() {
            return Err(MiraError::Other(
                "No modules could be summarized heuristically.".to_string(),
            ));
        }
        heuristic.into_iter().collect()
    };

    // Update database (code DB)
    let summaries_clone = summaries.clone();
    let updated: usize = ctx
        .code_pool()
        .run(move |conn| {
            let count = cartographer::update_module_purposes(conn, project_id, &summaries_clone)?;
            Ok::<_, MiraError>(count)
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
pub async fn run_health_scan<C: ToolContext>(ctx: &C) -> Result<Json<IndexOutput>, MiraError> {
    use crate::db::count_symbols_sync;

    let project = ctx.get_project().await;
    let (project_id, project_path) = match project.as_ref() {
        Some(p) => (p.id, p.path.clone()),
        None => {
            return Err(MiraError::ProjectNotSet);
        }
    };

    // Guard: ensure the project has been indexed
    let has_symbols = ctx
        .code_pool()
        .run(move |conn| Ok::<_, MiraError>(count_symbols_sync(conn, Some(project_id))))
        .await?;

    if has_symbols == 0 {
        return Err(MiraError::InvalidInput(
            "No code indexed yet. Run index(action=\"project\") first.".to_string(),
        ));
    }

    // Run the full health scan (same as background worker, but forced)
    let issues = crate::background::code_health::scan_project_health_full(
        ctx.pool(),
        ctx.code_pool(),
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
