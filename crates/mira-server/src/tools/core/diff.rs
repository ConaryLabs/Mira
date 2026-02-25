// crates/mira-server/src/tools/core/diff.rs
// MCP tool handler for semantic diff analysis

use std::path::Path;

use crate::background::diff_analysis::{
    DiffAnalysisResult, HistoricalRisk, RiskAssessment, analyze_diff, build_impact_graph,
    compute_historical_risk, format_diff_analysis, map_to_symbols,
};
use crate::db::get_recent_diff_analyses_sync;
use crate::error::MiraError;
use crate::git::{
    get_head_commit, get_staged_diff, get_working_diff, parse_staged_stats, parse_working_stats,
    resolve_ref,
};
use crate::mcp::responses::Json;
use crate::mcp::responses::{
    DiffAnalysisData, DiffData, DiffOutput, HistoricalRiskData, PatternMatchInfo,
};
use crate::tools::core::{ToolContext, get_project_info};
use crate::utils::truncate;

/// Analyze git diff semantically
///
/// Identifies change types, calculates impact, and assesses risk.
pub async fn analyze_diff_tool<C: ToolContext>(
    ctx: &C,
    from_ref: Option<String>,
    to_ref: Option<String>,
    include_impact: Option<bool>,
) -> Result<Json<DiffOutput>, MiraError> {
    // Validate ref lengths before any git operations
    if let Some(ref r) = from_ref
        && r.len() > 256
    {
        return Err(MiraError::InvalidInput(
            "from_ref exceeds maximum length of 256 characters".to_string(),
        ));
    }
    if let Some(ref r) = to_ref
        && r.len() > 256
    {
        return Err(MiraError::InvalidInput(
            "to_ref exceeds maximum length of 256 characters".to_string(),
        ));
    }

    let pi = get_project_info(ctx).await;
    let project_path = match pi.path {
        Some(ref p) => p.clone(),
        None => {
            return Err(MiraError::ProjectNotSet);
        }
    };
    let project_id = pi.id;
    let context_header = pi.header;
    let path = Path::new(&project_path);

    let include_impact = include_impact.unwrap_or(true);

    // Determine what to analyze
    let (from, to) = match (from_ref.as_deref(), to_ref.as_deref()) {
        // Explicit refs provided
        (Some(from), Some(to)) => (from.to_string(), to.to_string()),
        // Only from_ref: compare from_ref to HEAD
        (Some(from), None) => {
            let head = get_head_commit(path)?;
            (from.to_string(), head)
        }
        // No refs: analyze last commit (HEAD~1..HEAD)
        (None, None) => {
            // Check if there are staged changes first
            let staged = get_staged_diff(path)?;
            if !staged.is_empty() {
                return analyze_staged_or_working(
                    ctx,
                    path,
                    project_id,
                    &context_header,
                    "staged",
                    &staged,
                    include_impact,
                )
                .await;
            }

            // Check working directory changes
            let working = get_working_diff(path)?;
            if !working.is_empty() {
                return analyze_staged_or_working(
                    ctx,
                    path,
                    project_id,
                    &context_header,
                    "working",
                    &working,
                    include_impact,
                )
                .await;
            }

            // Default to last commit
            let head = get_head_commit(path)?;
            let parent = resolve_ref(path, "HEAD~1").unwrap_or_else(|_| head.clone());
            (parent, head)
        }
        // Only to_ref is unusual, treat as HEAD..to_ref
        (None, Some(to)) => {
            let head = get_head_commit(path)?;
            (head, to.to_string())
        }
    };

    // Perform full analysis
    let result = analyze_diff(
        ctx.pool(),
        path,
        project_id,
        &from,
        &to,
        include_impact,
    )
    .await?;

    // Compute historical risk LIVE from mined patterns (never cached)
    let historical_risk =
        compute_historical_risk_live(ctx, project_id, &result.files, result.files_changed).await;

    let formatted = format_diff_analysis(&result, historical_risk.as_ref());
    Ok(Json(DiffOutput {
        action: "analyze".into(),
        message: format!("{}{}", context_header, formatted),
        data: Some(DiffData::Analysis(DiffAnalysisData {
            from_ref: from.clone(),
            to_ref: to.clone(),
            files_changed: result.files_changed,
            lines_added: result.lines_added,
            lines_removed: result.lines_removed,
            summary: Some(result.summary.clone()),
            risk_level: Some(result.risk.overall.clone()),
            historical_risk: historical_risk.map(to_historical_risk_data),
        })),
    }))
}

/// Analyze staged or working directory changes (no caching, simpler flow)
async fn analyze_staged_or_working<C: ToolContext>(
    ctx: &C,
    path: &Path,
    project_id: Option<i64>,
    context_header: &str,
    analysis_type: &str,
    diff_content: &str,
    include_impact: bool,
) -> Result<Json<DiffOutput>, MiraError> {
    use crate::background::diff_analysis::{
        analyze_diff_heuristic, calculate_risk_level,
    };

    // Get stats
    let stats = if analysis_type == "staged" {
        parse_staged_stats(path)?
    } else {
        parse_working_stats(path)?
    };

    if diff_content.is_empty() {
        return Ok(Json(DiffOutput {
            action: "analyze".into(),
            message: format!("{}No {} changes to analyze.", context_header, analysis_type),
            data: None,
        }));
    }

    // Heuristic analysis
    let (changes, summary, risk_flags) = analyze_diff_heuristic(diff_content, &stats);

    // Build impact if requested
    let impact = if include_impact && !changes.is_empty() {
        let pool = ctx.pool().clone();
        let files = stats.files.clone();
        let changes_clone = changes.clone();
        pool.run(move |conn| {
            let symbols = map_to_symbols(conn, project_id, &files);
            let result = if symbols.is_empty() {
                let pseudo_symbols: Vec<(String, String, String)> = changes_clone
                    .iter()
                    .filter_map(|c| {
                        c.symbol_name
                            .as_ref()
                            .map(|name| (name.clone(), "function".to_string(), c.file_path.clone()))
                    })
                    .collect();
                build_impact_graph(conn, project_id, &pseudo_symbols, 2)
            } else {
                build_impact_graph(conn, project_id, &symbols, 2)
            };
            Ok::<_, MiraError>(result)
        })
        .await
        .map_err(|e| {
            tracing::warn!("Impact graph lookup failed: {e}");
            e
        })
        .ok()
    } else {
        None
    };

    // Calculate risk
    let risk = RiskAssessment {
        overall: calculate_risk_level(&risk_flags, &changes),
        flags: risk_flags,
    };

    let result = DiffAnalysisResult {
        from_ref: if analysis_type == "staged" {
            "INDEX".to_string()
        } else {
            "HEAD".to_string()
        },
        to_ref: if analysis_type == "staged" {
            "staged".to_string()
        } else {
            "working".to_string()
        },
        changes,
        impact,
        risk,
        summary,
        files: stats.files.clone(),
        files_changed: stats.files_changed,
        lines_added: stats.lines_added,
        lines_removed: stats.lines_removed,
    };

    // Compute historical risk LIVE from mined patterns
    let historical_risk =
        compute_historical_risk_live(ctx, project_id, &result.files, result.files_changed).await;

    let formatted = format_diff_analysis(&result, historical_risk.as_ref());
    Ok(Json(DiffOutput {
        action: "analyze".into(),
        message: format!("{}{}", context_header, formatted),
        data: Some(DiffData::Analysis(DiffAnalysisData {
            from_ref: result.from_ref.clone(),
            to_ref: result.to_ref.clone(),
            files_changed: result.files_changed,
            lines_added: result.lines_added,
            lines_removed: result.lines_removed,
            summary: Some(result.summary.clone()),
            risk_level: Some(result.risk.overall.clone()),
            historical_risk: historical_risk.map(to_historical_risk_data),
        })),
    }))
}

/// Compute historical risk from mined change patterns (never cached).
async fn compute_historical_risk_live<C: ToolContext>(
    ctx: &C,
    project_id: Option<i64>,
    files: &[String],
    files_changed: i64,
) -> Option<HistoricalRisk> {
    let pid = project_id?;
    let files = files.to_vec();
    ctx.pool()
        .run(move |conn| {
            Ok::<_, MiraError>(compute_historical_risk(conn, pid, &files, files_changed))
        })
        .await
        .ok()
        .flatten()
}

/// Convert internal HistoricalRisk to the output HistoricalRiskData
fn to_historical_risk_data(hr: HistoricalRisk) -> HistoricalRiskData {
    HistoricalRiskData {
        risk_delta: hr.risk_delta,
        matching_patterns: hr
            .matching_patterns
            .into_iter()
            .map(|mp| PatternMatchInfo {
                pattern_type: mp.pattern_subtype,
                description: mp.description,
                confidence: mp.confidence,
            })
            .collect(),
        overall_confidence: hr.overall_confidence,
    }
}

/// List recent diff analyses for the project
pub async fn list_diff_analyses<C: ToolContext>(
    ctx: &C,
    limit: Option<i64>,
) -> Result<Json<DiffOutput>, MiraError> {
    let pi = get_project_info(ctx).await;
    let project_id = pi.id;
    let context_header = pi.header;

    let limit = limit.unwrap_or(10).max(0) as usize;

    let analyses = ctx
        .pool()
        .run(move |conn| get_recent_diff_analyses_sync(conn, project_id, limit))
        .await?;

    if analyses.is_empty() {
        return Ok(Json(DiffOutput {
            action: "list".into(),
            message: format!(
                "{}No diff analyses found. Run diff(from_ref=\"...\", to_ref=\"...\") to analyze changes first.",
                context_header
            ),
            data: None,
        }));
    }

    let mut output = format!("{}## Recent Diff Analyses\n\n", context_header);

    for analysis in analyses {
        let summary = analysis.summary.as_deref().unwrap_or("No summary");
        let truncated = truncate(summary, 100);

        output.push_str(&format!(
            "- **{}..{}** ({})\n  {} files, +{} -{}\n  {}\n\n",
            analysis.from_commit,
            analysis.to_commit,
            analysis.created_at,
            analysis.files_changed.unwrap_or(0),
            analysis.lines_added.unwrap_or(0),
            analysis.lines_removed.unwrap_or(0),
            truncated
        ));
    }

    Ok(Json(DiffOutput {
        action: "list".into(),
        message: output,
        data: None,
    }))
}

#[cfg(test)]
mod tests {
    use crate::git::parse_numstat_output;

    #[test]
    fn test_parse_numstat_output_empty() {
        let result = parse_numstat_output("");
        assert_eq!(result.files_changed, 0);
        assert_eq!(result.lines_added, 0);
        assert_eq!(result.lines_removed, 0);
        assert!(result.files.is_empty());
    }

    #[test]
    fn test_parse_numstat_output_single_file() {
        let result = parse_numstat_output("10\t5\tsrc/main.rs");
        assert_eq!(result.files_changed, 1);
        assert_eq!(result.lines_added, 10);
        assert_eq!(result.lines_removed, 5);
        assert_eq!(result.files, vec!["src/main.rs"]);
    }

    #[test]
    fn test_parse_numstat_output_multiple_files() {
        let result = parse_numstat_output("10\t5\tsrc/main.rs\n20\t3\tsrc/lib.rs\n5\t0\tREADME.md");
        assert_eq!(result.files_changed, 3);
        assert_eq!(result.lines_added, 35);
        assert_eq!(result.lines_removed, 8);
        assert_eq!(result.files.len(), 3);
    }

    #[test]
    fn test_parse_numstat_output_binary_files() {
        let result = parse_numstat_output("-\t-\timage.png\n10\t5\tsrc/main.rs");
        assert_eq!(result.files_changed, 1);
        assert_eq!(result.lines_added, 10);
        assert_eq!(result.lines_removed, 5);
    }

    #[test]
    fn test_parse_numstat_output_file_with_spaces() {
        let result = parse_numstat_output("10\t5\tpath/to/file with spaces.rs");
        assert_eq!(result.files_changed, 1);
        assert_eq!(result.files[0], "path/to/file with spaces.rs");
    }

    #[test]
    fn test_parse_numstat_output_malformed_line() {
        let result = parse_numstat_output("malformed line\n10\t5\tvalid.rs");
        assert_eq!(result.files_changed, 1);
        assert_eq!(result.files[0], "valid.rs");
    }
}
