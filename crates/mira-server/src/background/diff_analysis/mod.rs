// background/diff_analysis/
// Core logic for diff analysis, split into focused sub-modules.

mod format;
mod heuristic;
mod impact;
mod types;

// Re-export public API (preserves all existing import paths)
pub use format::format_diff_analysis;
// Git operations re-exported from centralized crate::git module
pub use crate::git::{
    derive_stats_from_unified_diff, get_head_commit, get_staged_diff, get_unified_diff,
    get_working_diff, parse_diff_stats, parse_numstat_output, parse_staged_stats,
    parse_working_stats, resolve_ref,
};
pub use heuristic::analyze_diff_heuristic;
pub use impact::{build_impact_graph, map_to_symbols};
pub use types::{DiffAnalysisResult, DiffStats, ImpactAnalysis};

use crate::db::pool::DatabasePool;
use crate::db::{
    DiffAnalysis, StoreDiffAnalysisParams, get_cached_diff_analysis_sync, store_diff_analysis_sync,
};
use std::sync::Arc;

/// Determine whether a cached DiffAnalysis exists for the resolved commit pair.
/// Extracted for unit-testability of the cache-hit path.
#[cfg(test)]
pub(crate) async fn get_cache_hit(
    pool: &Arc<DatabasePool>,
    project_id: Option<i64>,
    from_commit: &str,
    to_commit: &str,
) -> Result<Option<DiffAnalysis>, String> {
    let from = from_commit.to_string();
    let to = to_commit.to_string();
    pool.run(move |conn| get_cached_diff_analysis_sync(conn, project_id, &from, &to))
        .await
        .map_err(|e| e.to_string())
}

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
        impact: cached
            .impact_json
            .and_then(|j| serde_json::from_str(&j).ok()),
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
                changes_json: None,
                impact_json: impact_json.as_deref(),
                risk_json: None,
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

/// Perform complete diff analysis using heuristic analysis
pub async fn analyze_diff(
    pool: &Arc<DatabasePool>,
    project_path: &std::path::Path,
    project_id: Option<i64>,
    from_ref: &str,
    to_ref: &str,
    include_impact: bool,
) -> Result<DiffAnalysisResult, String> {
    // Resolve refs
    let from_commit = resolve_ref(project_path, from_ref)?;
    let to_commit = resolve_ref(project_path, to_ref)?;

    // Check cache first
    let from_for_cache = from_commit.clone();
    let to_for_cache = to_commit.clone();
    let cached = pool
        .run(move |conn| {
            get_cached_diff_analysis_sync(conn, project_id, &from_for_cache, &to_for_cache)
        })
        .await?;

    if let Some(cached) = cached {
        tracing::info!(
            "Using cached diff analysis for {}..{}",
            from_commit,
            to_commit
        );
        return Ok(result_from_cache(cached, from_commit, to_commit));
    }

    // Get diff content and derive stats from it (avoids a second git process)
    let diff_content = get_unified_diff(project_path, &from_commit, &to_commit)?;
    let stats = derive_stats_from_unified_diff(&diff_content);

    if diff_content.is_empty() {
        return Ok(DiffAnalysisResult {
            from_ref: from_commit,
            to_ref: to_commit,
            impact: None,
            summary: "No changes between the specified commits.".to_string(),
            files: vec![],
            files_changed: 0,
            lines_added: 0,
            lines_removed: 0,
        });
    }

    // Heuristic summary
    let summary = analyze_diff_heuristic(&diff_content, &stats);
    let analysis_type = "heuristic";

    // Build impact analysis if requested (DB-based, works without LLM)
    let impact = if include_impact {
        let files = stats.files.clone();
        let impact_result = pool
            .run(move |conn| -> Result<ImpactAnalysis, String> {
                let symbols = map_to_symbols(conn, project_id, &files);
                Ok(build_impact_graph(conn, project_id, &symbols, 2))
            })
            .await?;
        Some(impact_result)
    } else {
        None
    };

    let result = DiffAnalysisResult {
        from_ref: from_commit,
        to_ref: to_commit,
        impact,
        summary,
        files: stats.files.clone(),
        files_changed: stats.files_changed,
        lines_added: stats.lines_added,
        lines_removed: stats.lines_removed,
    };

    cache_result(pool, project_id, &result, analysis_type).await;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::setup_test_pool;

    // =========================================================================
    // result_from_cache: verify round-trip reconstruction from DiffAnalysis row
    // =========================================================================

    fn make_cached_analysis(project_id: Option<i64>) -> DiffAnalysis {
        DiffAnalysis {
            id: 1,
            project_id,
            from_commit: "aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000".to_string(),
            to_commit: "bbbb1111bbbb1111bbbb1111bbbb1111bbbb1111".to_string(),
            analysis_type: "heuristic".to_string(),
            changes_json: None,
            impact_json: None,
            risk_json: None,
            summary: Some("1 file changed (+5 -0)".to_string()),
            files_changed: Some(1),
            lines_added: Some(5),
            lines_removed: Some(0),
            status: "completed".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            files_json: Some(r#"["src/lib.rs"]"#.to_string()),
        }
    }

    #[test]
    fn result_from_cache_reconstructs_fields() {
        let cached = make_cached_analysis(Some(1));
        let result = result_from_cache(
            cached,
            "aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000".to_string(),
            "bbbb1111bbbb1111bbbb1111bbbb1111bbbb1111".to_string(),
        );

        assert_eq!(result.from_ref, "aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000");
        assert_eq!(result.to_ref, "bbbb1111bbbb1111bbbb1111bbbb1111bbbb1111");
        assert_eq!(result.summary, "1 file changed (+5 -0)");
        assert_eq!(result.files_changed, 1);
        assert_eq!(result.lines_added, 5);
        assert_eq!(result.lines_removed, 0);
        assert!(result.impact.is_none());
        assert_eq!(result.files, vec!["src/lib.rs"]);
    }

    #[test]
    fn result_from_cache_handles_missing_optional_fields() {
        let cached = DiffAnalysis {
            id: 2,
            project_id: None,
            from_commit: "from".to_string(),
            to_commit: "to".to_string(),
            analysis_type: "heuristic".to_string(),
            changes_json: None,
            impact_json: None,
            risk_json: None,
            summary: None,
            files_changed: None,
            lines_added: None,
            lines_removed: None,
            status: "completed".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            files_json: None,
        };

        let result = result_from_cache(cached, "from".to_string(), "to".to_string());

        assert!(result.impact.is_none(), "null impact_json -> None");
        assert!(result.summary.is_empty(), "null summary -> empty string");
        assert_eq!(result.files_changed, 0);
        assert_eq!(result.lines_added, 0);
        assert_eq!(result.lines_removed, 0);
        assert!(result.files.is_empty());
    }

    // =========================================================================
    // get_cache_hit: DB-backed cache lookup
    // =========================================================================

    #[tokio::test]
    async fn get_cache_hit_returns_none_when_empty() {
        let pool = setup_test_pool().await;

        let hit = get_cache_hit(&pool, None, "abc123", "def456")
            .await
            .unwrap();

        assert!(hit.is_none(), "cache should be empty before any insert");
    }

    #[tokio::test]
    async fn get_cache_hit_returns_some_after_store() {
        let pool = setup_test_pool().await;
        let project_id: Option<i64> = None;

        // Seed a cache entry directly via the DB layer
        pool.run(move |conn| {
            crate::db::store_diff_analysis_sync(
                conn,
                &StoreDiffAnalysisParams {
                    project_id,
                    from_commit: "from_sha",
                    to_commit: "to_sha",
                    analysis_type: "heuristic",
                    changes_json: None,
                    impact_json: None,
                    risk_json: None,
                    summary: Some("cached summary"),
                    files_changed: Some(2),
                    lines_added: Some(10),
                    lines_removed: Some(3),
                    files_json: None,
                },
            )
            .map_err(|e| anyhow::anyhow!("{e}"))
        })
        .await
        .unwrap();

        let hit = get_cache_hit(&pool, project_id, "from_sha", "to_sha")
            .await
            .unwrap();

        assert!(hit.is_some(), "should find the stored entry");
        let entry = hit.unwrap();
        assert_eq!(entry.from_commit, "from_sha");
        assert_eq!(entry.to_commit, "to_sha");
        assert_eq!(entry.summary.as_deref(), Some("cached summary"));
        assert_eq!(entry.files_changed, Some(2));
    }

    #[tokio::test]
    async fn get_cache_hit_misses_on_different_commits() {
        let pool = setup_test_pool().await;

        pool.run(move |conn| {
            crate::db::store_diff_analysis_sync(
                conn,
                &StoreDiffAnalysisParams {
                    project_id: None,
                    from_commit: "aaa",
                    to_commit: "bbb",
                    analysis_type: "heuristic",
                    changes_json: None,
                    impact_json: None,
                    risk_json: None,
                    summary: None,
                    files_changed: None,
                    lines_added: None,
                    lines_removed: None,
                    files_json: None,
                },
            )
            .map_err(|e| anyhow::anyhow!("{e}"))
        })
        .await
        .unwrap();

        let hit = get_cache_hit(&pool, None, "aaa", "ccc").await.unwrap();
        assert!(hit.is_none(), "different to_commit should not match");

        let hit = get_cache_hit(&pool, None, "zzz", "bbb").await.unwrap();
        assert!(hit.is_none(), "different from_commit should not match");
    }

    // =========================================================================
    // cache_result + get_cache_hit: end-to-end cache write/read
    // =========================================================================

    #[tokio::test]
    async fn cache_result_then_get_cache_hit_round_trip() {
        let pool = setup_test_pool().await;

        let result = DiffAnalysisResult {
            from_ref: "commit_a".to_string(),
            to_ref: "commit_b".to_string(),
            impact: None,
            summary: "round-trip test".to_string(),
            files: vec!["src/main.rs".to_string()],
            files_changed: 1,
            lines_added: 7,
            lines_removed: 2,
        };

        cache_result(&pool, None, &result, "heuristic").await;

        let hit = get_cache_hit(&pool, None, "commit_a", "commit_b")
            .await
            .unwrap();

        assert!(hit.is_some(), "cache_result should have stored the entry");
        let entry = hit.unwrap();
        assert_eq!(entry.summary.as_deref(), Some("round-trip test"));
        assert_eq!(entry.files_changed, Some(1));
        assert_eq!(entry.lines_added, Some(7));
        assert_eq!(entry.lines_removed, Some(2));
    }
}
