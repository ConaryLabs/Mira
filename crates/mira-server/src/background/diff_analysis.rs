// crates/mira-server/src/background/diff_analysis.rs
// Core logic for semantic diff analysis

use crate::db::Database;
use crate::llm::{DeepSeekClient, PromptBuilder};
use crate::search::find_callers;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

/// Maximum diff size to send to LLM (in bytes)
const MAX_DIFF_SIZE: usize = 50_000;

/// Maximum files per LLM chunk (reserved for future chunking logic)
#[allow(dead_code)]
const MAX_FILES_PER_CHUNK: usize = 10;

/// A semantic change identified in the diff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticChange {
    pub change_type: String,
    pub file_path: String,
    pub symbol_name: Option<String>,
    pub description: String,
    pub breaking: bool,
    pub security_relevant: bool,
}

/// Impact analysis results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactAnalysis {
    /// Functions affected (name, file, depth from changed code)
    pub affected_functions: Vec<(String, String, u32)>,
    /// Files that might be affected
    pub affected_files: Vec<String>,
}

/// Risk assessment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessment {
    pub overall: String, // Low, Medium, High, Critical
    pub flags: Vec<String>,
}

/// Complete diff analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffAnalysisResult {
    pub from_ref: String,
    pub to_ref: String,
    pub changes: Vec<SemanticChange>,
    pub impact: Option<ImpactAnalysis>,
    pub risk: RiskAssessment,
    pub summary: String,
    pub files_changed: i64,
    pub lines_added: i64,
    pub lines_removed: i64,
}

/// Diff statistics from git
#[derive(Debug, Default)]
pub struct DiffStats {
    pub files_changed: i64,
    pub lines_added: i64,
    pub lines_removed: i64,
    pub files: Vec<String>,
}

/// Get unified diff between two refs
pub fn get_unified_diff(project_path: &Path, from_ref: &str, to_ref: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["diff", "--unified=3", from_ref, to_ref])
        .current_dir(project_path)
        .output()
        .map_err(|e| format!("Failed to run git diff: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git diff failed: {}", stderr));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Get diff for staged changes
pub fn get_staged_diff(project_path: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["diff", "--unified=3", "--cached"])
        .current_dir(project_path)
        .output()
        .map_err(|e| format!("Failed to run git diff: {}", e))?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Get diff for working directory changes
pub fn get_working_diff(project_path: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["diff", "--unified=3"])
        .current_dir(project_path)
        .output()
        .map_err(|e| format!("Failed to run git diff: {}", e))?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Parse diff statistics using git diff --stat
pub fn parse_diff_stats(project_path: &Path, from_ref: &str, to_ref: &str) -> Result<DiffStats, String> {
    let output = Command::new("git")
        .args(["diff", "--stat", "--numstat", from_ref, to_ref])
        .current_dir(project_path)
        .output()
        .map_err(|e| format!("Failed to run git diff --stat: {}", e))?;

    if !output.status.success() {
        return Ok(DiffStats::default());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut stats = DiffStats::default();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 {
            // Format: additions\tdeletions\tfilename
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

/// Resolve a git ref to a commit hash
pub fn resolve_ref(project_path: &Path, ref_name: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short", ref_name])
        .current_dir(project_path)
        .output()
        .map_err(|e| format!("Failed to resolve ref: {}", e))?;

    if !output.status.success() {
        return Err(format!("Invalid ref: {}", ref_name));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get current HEAD commit
pub fn get_head_commit(project_path: &Path) -> Result<String, String> {
    resolve_ref(project_path, "HEAD")
}

/// Map changed files to affected symbols in the database
pub fn map_to_symbols(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    changed_files: &[String],
) -> Vec<(String, String, String)> {
    // (symbol_name, symbol_type, file_path)
    let mut symbols = Vec::new();

    for file in changed_files {
        let mut stmt = match conn.prepare(
            "SELECT name, symbol_type, file_path FROM code_symbols
             WHERE (project_id = ? OR project_id IS NULL) AND file_path LIKE ?
             ORDER BY start_line"
        ) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let file_pattern = format!("%{}", file);
        if let Ok(rows) = stmt.query_map(params![project_id, file_pattern], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        }) {
            for row in rows.flatten() {
                symbols.push(row);
            }
        }
    }

    symbols
}

/// Build impact analysis by traversing call graph
pub fn build_impact_graph(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    changed_symbols: &[(String, String, String)],
    max_depth: u32,
) -> ImpactAnalysis {
    let mut affected_functions: Vec<(String, String, u32)> = Vec::new();
    let mut affected_files: HashSet<String> = HashSet::new();
    let mut seen: HashSet<String> = HashSet::new();

    // Start with the changed functions
    let function_names: Vec<&str> = changed_symbols
        .iter()
        .filter(|(_, sym_type, _)| sym_type == "function" || sym_type == "method")
        .map(|(name, _, _)| name.as_str())
        .collect();

    for func_name in function_names {
        if seen.contains(func_name) {
            continue;
        }
        seen.insert(func_name.to_string());

        // Find callers at each depth level
        let mut current_level = vec![func_name.to_string()];

        for depth in 1..=max_depth {
            let mut next_level = Vec::new();

            for name in &current_level {
                let callers = find_callers(conn, project_id, name, 20);
                for caller in callers {
                    if !seen.contains(&caller.symbol_name) {
                        seen.insert(caller.symbol_name.clone());
                        affected_functions.push((caller.symbol_name.clone(), caller.file_path.clone(), depth));
                        affected_files.insert(caller.file_path);
                        next_level.push(caller.symbol_name);
                    }
                }
            }

            if next_level.is_empty() {
                break;
            }
            current_level = next_level;
        }
    }

    ImpactAnalysis {
        affected_functions,
        affected_files: affected_files.into_iter().collect(),
    }
}

/// LLM response structure for diff analysis
#[derive(Debug, Deserialize)]
struct LlmDiffResponse {
    changes: Vec<SemanticChange>,
    summary: String,
    risk_flags: Vec<String>,
}

/// Analyze diff semantically using LLM
pub async fn analyze_diff_semantic(
    diff_content: &str,
    deepseek: &Arc<DeepSeekClient>,
) -> Result<(Vec<SemanticChange>, String, Vec<String>), String> {
    if diff_content.is_empty() {
        return Ok((Vec::new(), "No changes".to_string(), Vec::new()));
    }

    // Truncate if too large
    let diff_to_analyze = if diff_content.len() > MAX_DIFF_SIZE {
        format!(
            "{}...\n\n[Diff truncated - {} more bytes]",
            &diff_content[..MAX_DIFF_SIZE],
            diff_content.len() - MAX_DIFF_SIZE
        )
    } else {
        diff_content.to_string()
    };

    let user_prompt = format!(
        "Analyze this git diff:\n\n```diff\n{}\n```",
        diff_to_analyze
    );

    let messages = PromptBuilder::for_diff_analysis().build_messages(user_prompt);

    let result = deepseek
        .chat(messages, None)
        .await
        .map_err(|e| format!("LLM request failed: {}", e))?;

    let content = result.content.ok_or("No content in LLM response")?;

    // Try to parse JSON from response
    parse_llm_response(&content)
}

/// Parse the LLM response to extract structured data
fn parse_llm_response(content: &str) -> Result<(Vec<SemanticChange>, String, Vec<String>), String> {
    // Try to find JSON in the response
    let json_start = content.find('{');
    let json_end = content.rfind('}');

    if let (Some(start), Some(end)) = (json_start, json_end) {
        let json_str = &content[start..=end];
        if let Ok(response) = serde_json::from_str::<LlmDiffResponse>(json_str) {
            return Ok((response.changes, response.summary, response.risk_flags));
        }
    }

    // Fallback: extract what we can from plain text
    let summary = content
        .lines()
        .find(|l| !l.trim().is_empty() && !l.starts_with('{'))
        .unwrap_or("Changes analyzed")
        .to_string();

    Ok((Vec::new(), summary, Vec::new()))
}

/// Calculate overall risk level from flags
pub fn calculate_risk_level(flags: &[String], changes: &[SemanticChange]) -> String {
    let has_breaking = changes.iter().any(|c| c.breaking);
    let has_security = changes.iter().any(|c| c.security_relevant);
    let breaking_count = flags.iter().filter(|f| f.contains("breaking")).count();
    let security_count = flags.iter().filter(|f| f.contains("security")).count();

    if has_security || security_count > 0 {
        if has_breaking || breaking_count > 0 {
            return "Critical".to_string();
        }
        return "High".to_string();
    }

    if has_breaking || breaking_count > 1 {
        return "High".to_string();
    }

    if breaking_count > 0 || flags.len() > 3 {
        return "Medium".to_string();
    }

    "Low".to_string()
}

/// Perform complete diff analysis
pub async fn analyze_diff(
    db: &Arc<Database>,
    deepseek: &Arc<DeepSeekClient>,
    project_path: &Path,
    project_id: Option<i64>,
    from_ref: &str,
    to_ref: &str,
    include_impact: bool,
) -> Result<DiffAnalysisResult, String> {
    // Resolve refs
    let from_commit = resolve_ref(project_path, from_ref)?;
    let to_commit = resolve_ref(project_path, to_ref)?;

    // Check cache first
    if let Ok(Some(cached)) = db.get_cached_diff_analysis(project_id, &from_commit, &to_commit) {
        tracing::info!("Using cached diff analysis for {}..{}", from_commit, to_commit);
        return Ok(DiffAnalysisResult {
            from_ref: from_commit,
            to_ref: to_commit,
            changes: serde_json::from_str(&cached.changes_json.unwrap_or_default()).unwrap_or_default(),
            impact: cached.impact_json.and_then(|j| serde_json::from_str(&j).ok()),
            risk: cached.risk_json.and_then(|j| serde_json::from_str(&j).ok())
                .unwrap_or(RiskAssessment { overall: "Unknown".to_string(), flags: vec![] }),
            summary: cached.summary.unwrap_or_default(),
            files_changed: cached.files_changed.unwrap_or(0),
            lines_added: cached.lines_added.unwrap_or(0),
            lines_removed: cached.lines_removed.unwrap_or(0),
        });
    }

    // Get diff content and stats
    let diff_content = get_unified_diff(project_path, &from_commit, &to_commit)?;
    let stats = parse_diff_stats(project_path, &from_commit, &to_commit)?;

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
            files_changed: 0,
            lines_added: 0,
            lines_removed: 0,
        });
    }

    // Semantic analysis via LLM
    let (changes, summary, risk_flags) = analyze_diff_semantic(&diff_content, deepseek).await?;

    // Build impact analysis if requested
    let impact = if include_impact && !changes.is_empty() {
        let db_clone = db.clone();
        let files = stats.files.clone();
        let changes_clone = changes.clone();
        let impact_result = tokio::task::spawn_blocking(move || {
            let conn = db_clone.conn();
            let symbols = map_to_symbols(&conn, project_id, &files);
            if symbols.is_empty() {
                // If no symbols found, use changes from LLM
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
        .map_err(|e| format!("spawn_blocking panicked: {}", e))?;
        Some(impact_result)
    } else {
        None
    };

    // Calculate risk
    let risk = RiskAssessment {
        overall: calculate_risk_level(&risk_flags, &changes),
        flags: risk_flags,
    };

    // Store in cache
    let changes_json = serde_json::to_string(&changes).ok();
    let impact_json = impact.as_ref().and_then(|i| serde_json::to_string(i).ok());
    let risk_json = serde_json::to_string(&risk).ok();

    if let Err(e) = db.store_diff_analysis(
        project_id,
        &from_commit,
        &to_commit,
        "commit",
        changes_json.as_deref(),
        impact_json.as_deref(),
        risk_json.as_deref(),
        Some(&summary),
        Some(stats.files_changed),
        Some(stats.lines_added),
        Some(stats.lines_removed),
    ) {
        tracing::warn!("Failed to cache diff analysis: {}", e);
    }

    Ok(DiffAnalysisResult {
        from_ref: from_commit,
        to_ref: to_commit,
        changes,
        impact,
        risk,
        summary,
        files_changed: stats.files_changed,
        lines_added: stats.lines_added,
        lines_removed: stats.lines_removed,
    })
}

/// Format diff analysis result for display
pub fn format_diff_analysis(result: &DiffAnalysisResult) -> String {
    let mut output = String::new();

    output.push_str(&format!(
        "## Semantic Diff Analysis: {}..{}\n\n",
        result.from_ref, result.to_ref
    ));

    // Summary
    output.push_str("### Summary\n");
    output.push_str(&result.summary);
    output.push_str("\n\n");

    // Stats
    output.push_str(&format!(
        "**Stats:** {} files changed, +{} -{}\n\n",
        result.files_changed, result.lines_added, result.lines_removed
    ));

    // Changes
    if !result.changes.is_empty() {
        output.push_str(&format!("### Changes ({})\n", result.changes.len()));

        // Group by type
        let mut new_features: Vec<&SemanticChange> = vec![];
        let mut modifications: Vec<&SemanticChange> = vec![];
        let mut deletions: Vec<&SemanticChange> = vec![];
        let mut other: Vec<&SemanticChange> = vec![];

        for change in &result.changes {
            match change.change_type.as_str() {
                "NewFunction" | "NewFeature" => new_features.push(change),
                "ModifiedFunction" | "SignatureChange" | "Refactoring" => modifications.push(change),
                "DeletedFunction" => deletions.push(change),
                _ => other.push(change),
            }
        }

        if !new_features.is_empty() {
            output.push_str("**New Features**\n");
            for c in new_features {
                let markers = format_change_markers(c);
                output.push_str(&format!("- {}: {}{}\n", c.file_path, c.description, markers));
            }
            output.push('\n');
        }

        if !modifications.is_empty() {
            output.push_str("**Modifications**\n");
            for c in modifications {
                let markers = format_change_markers(c);
                output.push_str(&format!("- {}: {}{}\n", c.file_path, c.description, markers));
            }
            output.push('\n');
        }

        if !deletions.is_empty() {
            output.push_str("**Deletions**\n");
            for c in deletions {
                output.push_str(&format!("- {}: {}\n", c.file_path, c.description));
            }
            output.push('\n');
        }

        if !other.is_empty() {
            output.push_str("**Other Changes**\n");
            for c in other {
                let markers = format_change_markers(c);
                output.push_str(&format!("- {}: {}{}\n", c.file_path, c.description, markers));
            }
            output.push('\n');
        }
    }

    // Impact
    if let Some(ref impact) = result.impact {
        if !impact.affected_functions.is_empty() {
            output.push_str("### Impact\n");
            output.push_str(&format!(
                "- Directly affected: {} functions\n",
                impact.affected_functions.iter().filter(|(_, _, d)| *d == 1).count()
            ));
            output.push_str(&format!(
                "- Transitively affected: {} functions\n",
                impact.affected_functions.iter().filter(|(_, _, d)| *d > 1).count()
            ));
            output.push_str(&format!("- Affected files: {}\n\n", impact.affected_files.len()));
        }
    }

    // Risk
    output.push_str(&format!("### Risk: {}\n", result.risk.overall));
    if !result.risk.flags.is_empty() {
        for flag in &result.risk.flags {
            output.push_str(&format!("- {}\n", flag));
        }
    }

    output
}

fn format_change_markers(change: &SemanticChange) -> String {
    let mut markers = String::new();
    if change.breaking {
        markers.push_str(" [BREAKING]");
    }
    if change.security_relevant {
        markers.push_str(" [SECURITY]");
    }
    markers
}
