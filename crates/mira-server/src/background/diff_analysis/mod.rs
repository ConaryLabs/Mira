// background/diff_analysis/
// Core logic for semantic diff analysis, split into focused sub-modules.

mod format;
mod heuristic;
mod impact;
mod llm;
#[cfg(test)]
mod tests;
mod types;

// Re-export public API (preserves all existing import paths)
pub use format::format_diff_analysis;
// Git operations re-exported from centralized crate::git module
pub use crate::git::{
    derive_stats_from_unified_diff, get_head_commit, get_staged_diff, get_unified_diff,
    get_working_diff, parse_diff_stats, parse_numstat_output, parse_staged_stats,
    parse_working_stats, resolve_ref,
};
pub use heuristic::{analyze_diff_heuristic, calculate_risk_level};
pub use impact::{build_impact_graph, compute_historical_risk, map_to_symbols};
pub use llm::analyze_diff_semantic;
pub use types::{
    DiffAnalysisResult, DiffStats, HistoricalRisk, ImpactAnalysis, MatchedPattern, RiskAssessment,
    SemanticChange,
};

use crate::db::pool::DatabasePool;
use crate::db::{
    DiffAnalysis, StoreDiffAnalysisParams, get_cached_diff_analysis_sync, store_diff_analysis_sync,
};
use crate::llm::LlmClient;
use std::sync::Arc;

/// Reconstruct a DiffAnalysisResult from cached database row
fn result_from_cache(cached: DiffAnalysis, from_ref: String, to_ref: String) -> DiffAnalysisResult {
    let files: Vec<String> = cached
        .files_json
        .as_deref()
        .and_then(|j| serde_json::from_str(j).ok())
        .unwrap_or_default();

    DiffAnalysisResult {
        from_ref,
        to_ref,
        changes: serde_json::from_str(&cached.changes_json.unwrap_or_default()).unwrap_or_default(),
        impact: cached
            .impact_json
            .and_then(|j| serde_json::from_str(&j).ok()),
        risk: cached
            .risk_json
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or(RiskAssessment {
                overall: "Unknown".to_string(),
                flags: vec![],
            }),
        summary: cached.summary.unwrap_or_default(),
        files_changed: cached.files_changed.unwrap_or(0),
        lines_added: cached.lines_added.unwrap_or(0),
        lines_removed: cached.lines_removed.unwrap_or(0),
        files,
    }
}

/// Store analysis result in cache
async fn cache_result(
    pool: &Arc<DatabasePool>,
    project_id: Option<i64>,
    result: &DiffAnalysisResult,
    analysis_type: &str,
) {
    let changes_json = match serde_json::to_string(&result.changes) {
        Ok(json) => Some(json),
        Err(e) => {
            tracing::warn!("Failed to serialize diff changes: {e}");
            None
        }
    };
    let impact_json = result
        .impact
        .as_ref()
        .and_then(|i| match serde_json::to_string(i) {
            Ok(json) => Some(json),
            Err(e) => {
                tracing::warn!("Failed to serialize diff impact: {e}");
                None
            }
        });
    let risk_json = match serde_json::to_string(&result.risk) {
        Ok(json) => Some(json),
        Err(e) => {
            tracing::warn!("Failed to serialize diff risk: {e}");
            None
        }
    };
    let from = result.from_ref.clone();
    let to = result.to_ref.clone();
    let summary = result.summary.clone();
    let files_changed = result.files_changed;
    let lines_added = result.lines_added;
    let lines_removed = result.lines_removed;
    let analysis_type = analysis_type.to_string();

    // Use the full file list from git numstat for outcome tracking
    let files_json = if result.files.is_empty() {
        None
    } else {
        match serde_json::to_string(&result.files) {
            Ok(json) => Some(json),
            Err(e) => {
                tracing::warn!("Failed to serialize diff files: {e}");
                None
            }
        }
    };

    pool.try_interact_warn("cache diff analysis", move |conn| {
        store_diff_analysis_sync(
            conn,
            &StoreDiffAnalysisParams {
                project_id,
                from_commit: &from,
                to_commit: &to,
                analysis_type: &analysis_type,
                changes_json: changes_json.as_deref(),
                impact_json: impact_json.as_deref(),
                risk_json: risk_json.as_deref(),
                summary: Some(&summary),
                files_changed: Some(files_changed),
                lines_added: Some(lines_added),
                lines_removed: Some(lines_removed),
                files_json: files_json.as_deref(),
            },
        )
        .map_err(|e| anyhow::anyhow!("{}", e))
    })
    .await;
}

/// Perform complete diff analysis (LLM optional — falls back to heuristic)
pub async fn analyze_diff(
    pool: &Arc<DatabasePool>,
    llm_client: Option<&Arc<dyn LlmClient>>,
    project_path: &std::path::Path,
    project_id: Option<i64>,
    from_ref: &str,
    to_ref: &str,
    include_impact: bool,
) -> Result<DiffAnalysisResult, String> {
    // Resolve refs
    let from_commit = resolve_ref(project_path, from_ref)?;
    let to_commit = resolve_ref(project_path, to_ref)?;

    // Check cache first (skip heuristic-cached results so LLM can re-analyze when available)
    let from_for_cache = from_commit.clone();
    let to_for_cache = to_commit.clone();
    let cached = pool
        .run(move |conn| {
            get_cached_diff_analysis_sync(conn, project_id, &from_for_cache, &to_for_cache)
        })
        .await?;

    if let Some(cached) = cached {
        // If LLM is available and cached result is heuristic, skip cache to re-analyze
        let is_heuristic_cache = cached.analysis_type == "heuristic";
        if !is_heuristic_cache || llm_client.is_none() {
            tracing::info!(
                "Using cached diff analysis for {}..{}",
                from_commit,
                to_commit
            );
            return Ok(result_from_cache(cached, from_commit, to_commit));
        }
    }

    // Get diff content and derive stats from it (avoids a second git process)
    let diff_content = get_unified_diff(project_path, &from_commit, &to_commit)?;
    let stats = derive_stats_from_unified_diff(&diff_content);

    if diff_content.is_empty() {
        return Ok(DiffAnalysisResult {
            from_ref: from_commit,
            to_ref: to_commit,
            changes: vec![],
            impact: None,
            risk: RiskAssessment {
                overall: "Low".to_string(),
                flags: vec![],
            },
            summary: "No changes between the specified commits.".to_string(),
            files: vec![],
            files_changed: 0,
            lines_added: 0,
            lines_removed: 0,
        });
    }

    // Semantic analysis via LLM or heuristic fallback
    let (changes, summary, risk_flags, analysis_type) = if let Some(client) = llm_client {
        let (c, s, f) = analyze_diff_semantic(&diff_content, client, pool, project_id).await?;
        (c, s, f, "commit")
    } else {
        let (c, s, f) = analyze_diff_heuristic(&diff_content, &stats);
        (c, s, f, "heuristic")
    };

    // Build impact analysis if requested (DB-based, works without LLM)
    let impact = if include_impact && !changes.is_empty() {
        let files = stats.files.clone();
        let changes_clone = changes.clone();
        let impact_result = pool
            .run(move |conn| -> Result<ImpactAnalysis, String> {
                let symbols = map_to_symbols(conn, project_id, &files);
                if symbols.is_empty() {
                    let pseudo_symbols: Vec<(String, String, String)> = changes_clone
                        .iter()
                        .filter_map(|c| {
                            c.symbol_name.as_ref().map(|name| {
                                (name.clone(), "function".to_string(), c.file_path.clone())
                            })
                        })
                        .collect();
                    Ok(build_impact_graph(conn, project_id, &pseudo_symbols, 2))
                } else {
                    Ok(build_impact_graph(conn, project_id, &symbols, 2))
                }
            })
            .await?;
        Some(impact_result)
    } else {
        None
    };

    let risk = RiskAssessment {
        overall: calculate_risk_level(&risk_flags, &changes),
        flags: risk_flags,
    };

    let result = DiffAnalysisResult {
        from_ref: from_commit,
        to_ref: to_commit,
        changes,
        impact,
        risk,
        summary,
        files: stats.files.clone(),
        files_changed: stats.files_changed,
        lines_added: stats.lines_added,
        lines_removed: stats.lines_removed,
    };

    cache_result(pool, project_id, &result, analysis_type).await;

    Ok(result)
}
