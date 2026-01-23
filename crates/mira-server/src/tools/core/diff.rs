// crates/mira-server/src/tools/core/diff.rs
// MCP tool handler for semantic diff analysis

use std::path::Path;

use crate::background::diff_analysis::{
    analyze_diff, format_diff_analysis, get_head_commit, get_staged_diff, get_working_diff,
    resolve_ref, DiffAnalysisResult, DiffStats, RiskAssessment,
};
use crate::search::format_project_header;
use crate::tools::core::ToolContext;

/// Analyze git diff semantically
///
/// Identifies change types, calculates impact, and assesses risk.
pub async fn analyze_diff_tool<C: ToolContext>(
    ctx: &C,
    from_ref: Option<String>,
    to_ref: Option<String>,
    include_impact: Option<bool>,
) -> Result<String, String> {
    let project = ctx.get_project().await;
    let (project_id, project_path) = match project.as_ref() {
        Some(p) => (Some(p.id), p.path.clone()),
        None => return Err("No active project. Call session_start first.".to_string()),
    };

    let context_header = format_project_header(project.as_ref());
    let path = Path::new(&project_path);

    // Get DeepSeek client for semantic analysis
    let deepseek = ctx
        .deepseek()
        .ok_or("DeepSeek not configured. Set DEEPSEEK_API_KEY for semantic analysis.")?;

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
                return analyze_staged_or_working(ctx, path, project_id, &context_header, "staged", &staged, include_impact).await;
            }

            // Check working directory changes
            let working = get_working_diff(path)?;
            if !working.is_empty() {
                return analyze_staged_or_working(ctx, path, project_id, &context_header, "working", &working, include_impact).await;
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
        ctx.db(),
        deepseek,
        path,
        project_id,
        &from,
        &to,
        include_impact,
    )
    .await?;

    Ok(format!("{}{}", context_header, format_diff_analysis(&result)))
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
) -> Result<String, String> {
    use crate::background::diff_analysis::{
        analyze_diff_semantic, build_impact_graph, calculate_risk_level, map_to_symbols,
    };

    let deepseek = ctx
        .deepseek()
        .ok_or("DeepSeek not configured. Set DEEPSEEK_API_KEY for semantic analysis.")?;

    // Get stats
    let stats = if analysis_type == "staged" {
        parse_staged_stats(path)?
    } else {
        parse_working_stats(path)?
    };

    if diff_content.is_empty() {
        return Ok(format!(
            "{}No {} changes to analyze.",
            context_header, analysis_type
        ));
    }

    // Semantic analysis
    let (changes, summary, risk_flags) = analyze_diff_semantic(diff_content, deepseek).await?;

    // Build impact if requested
    let impact = if include_impact && !changes.is_empty() {
        let db = ctx.db().clone();
        let files = stats.files.clone();
        let changes_clone = changes.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.conn();
            let symbols = map_to_symbols(&conn, project_id, &files);
            if symbols.is_empty() {
                let pseudo_symbols: Vec<(String, String, String)> = changes_clone
                    .iter()
                    .filter_map(|c| {
                        c.symbol_name.as_ref().map(|name| {
                            (name.clone(), "function".to_string(), c.file_path.clone())
                        })
                    })
                    .collect();
                build_impact_graph(&conn, project_id, &pseudo_symbols, 2)
            } else {
                build_impact_graph(&conn, project_id, &symbols, 2)
            }
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

    Ok(format!("{}{}", context_header, format_diff_analysis(&result)))
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
) -> Result<String, String> {
    let project = ctx.get_project().await;
    let project_id = project.as_ref().map(|p| p.id);
    let context_header = format_project_header(project.as_ref());

    let limit = limit.unwrap_or(10) as usize;

    let analyses = ctx
        .db()
        .get_recent_diff_analyses(project_id, limit)
        .map_err(|e| e.to_string())?;

    if analyses.is_empty() {
        return Ok(format!("{}No diff analyses found.", context_header));
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

    Ok(output)
}
