// crates/mira-server/src/tools/core/diff.rs
// MCP tool handler for semantic diff analysis

use std::path::Path;

use crate::background::diff_analysis::{
    DiffAnalysisResult, DiffStats, RiskAssessment, analyze_diff, build_impact_graph,
    format_diff_analysis, get_head_commit, get_staged_diff, get_working_diff, map_to_symbols,
    resolve_ref,
};
use crate::db::get_recent_diff_analyses_sync;
use crate::mcp::responses::{DiffAnalysisData, DiffData, DiffOutput};
use crate::search::format_project_header;
use crate::tools::core::ToolContext;
use crate::mcp::responses::Json;

/// Analyze git diff semantically
///
/// Identifies change types, calculates impact, and assesses risk.
pub async fn analyze_diff_tool<C: ToolContext>(
    ctx: &C,
    from_ref: Option<String>,
    to_ref: Option<String>,
    include_impact: Option<bool>,
) -> Result<Json<DiffOutput>, String> {
    let project = ctx.get_project().await;
    let (project_id, project_path) = match project.as_ref() {
        Some(p) => (Some(p.id), p.path.clone()),
        None => return Err("No active project. Call session_start first.".to_string()),
    };

    let context_header = format_project_header(project.as_ref());
    let path = Path::new(&project_path);

    // Get LLM client for semantic analysis (optional — falls back to heuristic)
    let llm_client = ctx.llm_factory().client_for_background();

    let include_impact = include_impact.unwrap_or(true);

    // Determine what to analyze
    let (from, to, _analysis_type) = match (from_ref.as_deref(), to_ref.as_deref()) {
        // Explicit refs provided
        (Some(from), Some(to)) => (from.to_string(), to.to_string(), "commit"),
        // Only from_ref: compare from_ref to HEAD
        (Some(from), None) => {
            let head = get_head_commit(path)?;
            (from.to_string(), head, "commit")
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
            (parent, head, "commit")
        }
        // Only to_ref is unusual, treat as HEAD..to_ref
        (None, Some(to)) => {
            let head = get_head_commit(path)?;
            (head, to.to_string(), "commit")
        }
    };

    // Perform full analysis
    let result = analyze_diff(
        ctx.pool(),
        llm_client.as_ref(),
        path,
        project_id,
        &from,
        &to,
        include_impact,
    )
    .await?;

    let formatted = format_diff_analysis(&result);
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
) -> Result<Json<DiffOutput>, String> {
    use crate::background::diff_analysis::{
        analyze_diff_heuristic, analyze_diff_semantic, calculate_risk_level,
    };

    let llm_client = ctx.llm_factory().client_for_background();

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

    // Semantic analysis via LLM or heuristic fallback
    let (changes, summary, risk_flags) = if let Some(ref client) = llm_client {
        analyze_diff_semantic(diff_content, client, ctx.pool(), project_id).await?
    } else {
        analyze_diff_heuristic(diff_content, &stats)
    };

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
            Ok::<_, String>(result)
        })
        .await
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
        files_changed: stats.files_changed,
        lines_added: stats.lines_added,
        lines_removed: stats.lines_removed,
    };

    let formatted = format_diff_analysis(&result);
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
        })),
    }))
}

/// Parse stats for staged changes
fn parse_staged_stats(path: &Path) -> Result<DiffStats, String> {
    use std::process::Command;

    let output = Command::new("git")
        .args(["diff", "--cached", "--numstat"])
        .current_dir(path)
        .output()
        .map_err(|e| format!("Failed to run git diff --cached: {}", e))?;

    parse_numstat_output(&String::from_utf8_lossy(&output.stdout))
}

/// Parse stats for working directory changes
fn parse_working_stats(path: &Path) -> Result<DiffStats, String> {
    use std::process::Command;

    let output = Command::new("git")
        .args(["diff", "--numstat"])
        .current_dir(path)
        .output()
        .map_err(|e| format!("Failed to run git diff: {}", e))?;

    parse_numstat_output(&String::from_utf8_lossy(&output.stdout))
}

/// Parse git numstat output
fn parse_numstat_output(stdout: &str) -> Result<DiffStats, String> {
    let mut stats = DiffStats::default();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 {
            if let (Ok(added), Ok(removed)) = (parts[0].parse::<i64>(), parts[1].parse::<i64>()) {
                stats.lines_added += added;
                stats.lines_removed += removed;
                stats.files.push(parts[2].to_string());
            }
        }
    }

    stats.files_changed = stats.files.len() as i64;
    Ok(stats)
}

/// List recent diff analyses for the project
pub async fn list_diff_analyses<C: ToolContext>(
    ctx: &C,
    limit: Option<i64>,
) -> Result<Json<DiffOutput>, String> {
    let project = ctx.get_project().await;
    let project_id = project.as_ref().map(|p| p.id);
    let context_header = format_project_header(project.as_ref());

    let limit = limit.unwrap_or(10) as usize;

    let analyses = ctx
        .pool()
        .run(move |conn| get_recent_diff_analyses_sync(conn, project_id, limit))
        .await?;

    if analyses.is_empty() {
        return Ok(Json(DiffOutput {
            action: "list".into(),
            message: format!("{}No diff analyses found.", context_header),
            data: None,
        }));
    }

    let mut output = format!("{}## Recent Diff Analyses\n\n", context_header);

    for analysis in analyses {
        let summary = analysis.summary.as_deref().unwrap_or("No summary");
        let truncated = if summary.len() > 100 {
            format!("{}...", &summary[..100])
        } else {
            summary.to_string()
        };

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
    use super::*;

    #[test]
    fn test_parse_numstat_output_empty() {
        let result = parse_numstat_output("").unwrap();
        assert_eq!(result.files_changed, 0);
        assert_eq!(result.lines_added, 0);
        assert_eq!(result.lines_removed, 0);
        assert!(result.files.is_empty());
    }

    #[test]
    fn test_parse_numstat_output_single_file() {
        let output = "10\t5\tsrc/main.rs";
        let result = parse_numstat_output(output).unwrap();
        assert_eq!(result.files_changed, 1);
        assert_eq!(result.lines_added, 10);
        assert_eq!(result.lines_removed, 5);
        assert_eq!(result.files, vec!["src/main.rs"]);
    }

    #[test]
    fn test_parse_numstat_output_multiple_files() {
        let output = "10\t5\tsrc/main.rs\n20\t3\tsrc/lib.rs\n5\t0\tREADME.md";
        let result = parse_numstat_output(output).unwrap();
        assert_eq!(result.files_changed, 3);
        assert_eq!(result.lines_added, 35); // 10 + 20 + 5
        assert_eq!(result.lines_removed, 8); // 5 + 3 + 0
        assert_eq!(result.files.len(), 3);
        assert!(result.files.contains(&"src/main.rs".to_string()));
        assert!(result.files.contains(&"src/lib.rs".to_string()));
        assert!(result.files.contains(&"README.md".to_string()));
    }

    #[test]
    fn test_parse_numstat_output_binary_files() {
        // Binary files show as - for lines
        let output = "-\t-\timage.png\n10\t5\tsrc/main.rs";
        let result = parse_numstat_output(output).unwrap();
        // Binary file is skipped (parse fails), only the text file counts
        assert_eq!(result.files_changed, 1);
        assert_eq!(result.lines_added, 10);
        assert_eq!(result.lines_removed, 5);
    }

    #[test]
    fn test_parse_numstat_output_file_with_spaces() {
        let output = "10\t5\tpath/to/file with spaces.rs";
        let result = parse_numstat_output(output).unwrap();
        assert_eq!(result.files_changed, 1);
        assert_eq!(result.files[0], "path/to/file with spaces.rs");
    }

    #[test]
    fn test_parse_numstat_output_malformed_line() {
        // Lines with wrong format are skipped
        let output = "malformed line\n10\t5\tvalid.rs";
        let result = parse_numstat_output(output).unwrap();
        assert_eq!(result.files_changed, 1);
        assert_eq!(result.files[0], "valid.rs");
    }
}
