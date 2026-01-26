// crates/mira-server/src/background/diff_analysis.rs
// Core logic for semantic diff analysis

use crate::db::{map_files_to_symbols_sync, get_cached_diff_analysis_sync, store_diff_analysis_sync};
use crate::db::pool::DatabasePool;
use crate::llm::{DeepSeekClient, PromptBuilder};
use crate::search::find_callers;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

/// Maximum diff size to send to LLM (in bytes)
const MAX_DIFF_SIZE: usize = 50_000;

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
    map_files_to_symbols_sync(conn, project_id, changed_files)
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
    pool: &Arc<DatabasePool>,
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
    let from_for_cache = from_commit.clone();
    let to_for_cache = to_commit.clone();
    let cached = pool.interact(move |conn| {
        get_cached_diff_analysis_sync(conn, project_id, &from_for_cache, &to_for_cache)
            .map_err(|e| anyhow::anyhow!("{}", e))
    }).await.map_err(|e| e.to_string())?;

    if let Some(cached) = cached {
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
        let files = stats.files.clone();
        let changes_clone = changes.clone();
        let impact_result = pool.interact(move |conn| -> Result<ImpactAnalysis, anyhow::Error> {
            let symbols = map_to_symbols(conn, project_id, &files);
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
                Ok(build_impact_graph(conn, project_id, &pseudo_symbols, 2))
            } else {
                Ok(build_impact_graph(conn, project_id, &symbols, 2))
            }
        })
        .await
        .map_err(|e| e.to_string())?;
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

    // Clone values for the closure
    let from_for_store = from_commit.clone();
    let to_for_store = to_commit.clone();
    let summary_for_store = summary.clone();
    let files_changed = stats.files_changed;
    let lines_added = stats.lines_added;
    let lines_removed = stats.lines_removed;

    if let Err(e) = pool.interact(move |conn| {
        store_diff_analysis_sync(
            conn,
            project_id,
            &from_for_store,
            &to_for_store,
            "commit",
            changes_json.as_deref(),
            impact_json.as_deref(),
            risk_json.as_deref(),
            Some(&summary_for_store),
            Some(files_changed),
            Some(lines_added),
            Some(lines_removed),
        ).map_err(|e| anyhow::anyhow!("{}", e))
    }).await {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_stats_default() {
        let stats = DiffStats::default();
        assert_eq!(stats.files_changed, 0);
        assert_eq!(stats.lines_added, 0);
        assert_eq!(stats.lines_removed, 0);
        assert!(stats.files.is_empty());
    }

    #[test]
    fn test_calculate_risk_level_low() {
        let flags: Vec<String> = vec![];
        let changes: Vec<SemanticChange> = vec![];
        assert_eq!(calculate_risk_level(&flags, &changes), "Low");
    }

    #[test]
    fn test_calculate_risk_level_medium_with_flags() {
        let flags: Vec<String> = vec![
            "api_change".to_string(),
            "dependency_update".to_string(),
            "new_feature".to_string(),
            "config_change".to_string(),
        ];
        let changes: Vec<SemanticChange> = vec![];
        assert_eq!(calculate_risk_level(&flags, &changes), "Medium");
    }

    #[test]
    fn test_calculate_risk_level_high_with_breaking() {
        let flags: Vec<String> = vec!["breaking_change".to_string(), "breaking".to_string()];
        let changes: Vec<SemanticChange> = vec![];
        assert_eq!(calculate_risk_level(&flags, &changes), "High");
    }

    #[test]
    fn test_calculate_risk_level_high_with_breaking_change() {
        let flags: Vec<String> = vec![];
        let changes = vec![SemanticChange {
            change_type: "ModifiedFunction".to_string(),
            file_path: "src/lib.rs".to_string(),
            symbol_name: Some("parse".to_string()),
            description: "Changed function signature".to_string(),
            breaking: true,
            security_relevant: false,
        }];
        assert_eq!(calculate_risk_level(&flags, &changes), "High");
    }

    #[test]
    fn test_calculate_risk_level_critical_with_security() {
        let flags: Vec<String> = vec!["security_issue".to_string()];
        let changes = vec![SemanticChange {
            change_type: "ModifiedFunction".to_string(),
            file_path: "src/auth.rs".to_string(),
            symbol_name: Some("validate_token".to_string()),
            description: "Changed auth logic".to_string(),
            breaking: true,
            security_relevant: true,
        }];
        assert_eq!(calculate_risk_level(&flags, &changes), "Critical");
    }

    #[test]
    fn test_parse_llm_response_valid_json() {
        let content = r#"Here is the analysis:
{
    "changes": [
        {
            "change_type": "NewFunction",
            "file_path": "src/main.rs",
            "symbol_name": "process",
            "description": "Added new processing function",
            "breaking": false,
            "security_relevant": false
        }
    ],
    "summary": "Added a new function for processing",
    "risk_flags": ["new_feature"]
}
Some trailing text"#;

        let result = parse_llm_response(content);
        assert!(result.is_ok());
        let (changes, summary, flags) = result.unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].change_type, "NewFunction");
        assert_eq!(summary, "Added a new function for processing");
        assert_eq!(flags, vec!["new_feature"]);
    }

    #[test]
    fn test_parse_llm_response_no_json_fallback() {
        let content = "This is just plain text analysis without JSON.\nThe changes look good.";
        let result = parse_llm_response(content);
        assert!(result.is_ok());
        let (changes, summary, flags) = result.unwrap();
        assert!(changes.is_empty());
        assert_eq!(summary, "This is just plain text analysis without JSON.");
        assert!(flags.is_empty());
    }

    #[test]
    fn test_parse_llm_response_invalid_json_fallback() {
        let content = "Analysis: { broken json here }";
        let result = parse_llm_response(content);
        assert!(result.is_ok());
        let (changes, summary, _) = result.unwrap();
        assert!(changes.is_empty());
        assert_eq!(summary, "Analysis: { broken json here }");
    }

    #[test]
    fn test_format_change_markers_none() {
        let change = SemanticChange {
            change_type: "NewFunction".to_string(),
            file_path: "src/lib.rs".to_string(),
            symbol_name: None,
            description: "Added function".to_string(),
            breaking: false,
            security_relevant: false,
        };
        assert_eq!(format_change_markers(&change), "");
    }

    #[test]
    fn test_format_change_markers_breaking_only() {
        let change = SemanticChange {
            change_type: "SignatureChange".to_string(),
            file_path: "src/lib.rs".to_string(),
            symbol_name: Some("parse".to_string()),
            description: "Changed signature".to_string(),
            breaking: true,
            security_relevant: false,
        };
        assert_eq!(format_change_markers(&change), " [BREAKING]");
    }

    #[test]
    fn test_format_change_markers_security_only() {
        let change = SemanticChange {
            change_type: "ModifiedFunction".to_string(),
            file_path: "src/auth.rs".to_string(),
            symbol_name: Some("validate".to_string()),
            description: "Modified auth".to_string(),
            breaking: false,
            security_relevant: true,
        };
        assert_eq!(format_change_markers(&change), " [SECURITY]");
    }

    #[test]
    fn test_format_change_markers_both() {
        let change = SemanticChange {
            change_type: "ModifiedFunction".to_string(),
            file_path: "src/auth.rs".to_string(),
            symbol_name: Some("validate".to_string()),
            description: "Changed auth signature".to_string(),
            breaking: true,
            security_relevant: true,
        };
        assert_eq!(format_change_markers(&change), " [BREAKING] [SECURITY]");
    }

    #[test]
    fn test_format_diff_analysis_empty_changes() {
        let result = DiffAnalysisResult {
            from_ref: "abc123".to_string(),
            to_ref: "def456".to_string(),
            changes: vec![],
            impact: None,
            risk: RiskAssessment {
                overall: "Low".to_string(),
                flags: vec![],
            },
            summary: "No significant changes".to_string(),
            files_changed: 1,
            lines_added: 10,
            lines_removed: 5,
        };

        let output = format_diff_analysis(&result);
        assert!(output.contains("## Semantic Diff Analysis: abc123..def456"));
        assert!(output.contains("No significant changes"));
        assert!(output.contains("1 files changed, +10 -5"));
        assert!(output.contains("### Risk: Low"));
    }

    #[test]
    fn test_format_diff_analysis_with_changes() {
        let result = DiffAnalysisResult {
            from_ref: "abc123".to_string(),
            to_ref: "def456".to_string(),
            changes: vec![
                SemanticChange {
                    change_type: "NewFunction".to_string(),
                    file_path: "src/main.rs".to_string(),
                    symbol_name: Some("init".to_string()),
                    description: "Added init function".to_string(),
                    breaking: false,
                    security_relevant: false,
                },
                SemanticChange {
                    change_type: "ModifiedFunction".to_string(),
                    file_path: "src/lib.rs".to_string(),
                    symbol_name: Some("parse".to_string()),
                    description: "Changed parse logic".to_string(),
                    breaking: true,
                    security_relevant: false,
                },
            ],
            impact: Some(ImpactAnalysis {
                affected_functions: vec![
                    ("caller1".to_string(), "src/caller.rs".to_string(), 1),
                    ("caller2".to_string(), "src/other.rs".to_string(), 2),
                ],
                affected_files: vec!["src/caller.rs".to_string(), "src/other.rs".to_string()],
            }),
            risk: RiskAssessment {
                overall: "High".to_string(),
                flags: vec!["breaking_change".to_string()],
            },
            summary: "Added new feature and modified existing function".to_string(),
            files_changed: 2,
            lines_added: 50,
            lines_removed: 10,
        };

        let output = format_diff_analysis(&result);
        assert!(output.contains("### Changes (2)"));
        assert!(output.contains("**New Features**"));
        assert!(output.contains("Added init function"));
        assert!(output.contains("**Modifications**"));
        assert!(output.contains("[BREAKING]"));
        assert!(output.contains("### Impact"));
        assert!(output.contains("Directly affected: 1 functions"));
        assert!(output.contains("Transitively affected: 1 functions"));
        assert!(output.contains("### Risk: High"));
        assert!(output.contains("breaking_change"));
    }

    #[test]
    fn test_semantic_change_serialization() {
        let change = SemanticChange {
            change_type: "NewFunction".to_string(),
            file_path: "src/main.rs".to_string(),
            symbol_name: Some("test_fn".to_string()),
            description: "Added test function".to_string(),
            breaking: false,
            security_relevant: true,
        };

        let json = serde_json::to_string(&change).unwrap();
        let deserialized: SemanticChange = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.change_type, "NewFunction");
        assert_eq!(deserialized.file_path, "src/main.rs");
        assert_eq!(deserialized.symbol_name, Some("test_fn".to_string()));
        assert!(deserialized.security_relevant);
    }

    #[test]
    fn test_risk_assessment_serialization() {
        let risk = RiskAssessment {
            overall: "Medium".to_string(),
            flags: vec!["api_change".to_string(), "new_dependency".to_string()],
        };

        let json = serde_json::to_string(&risk).unwrap();
        let deserialized: RiskAssessment = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.overall, "Medium");
        assert_eq!(deserialized.flags.len(), 2);
    }

    #[test]
    fn test_impact_analysis_serialization() {
        let impact = ImpactAnalysis {
            affected_functions: vec![
                ("fn1".to_string(), "file1.rs".to_string(), 1),
                ("fn2".to_string(), "file2.rs".to_string(), 2),
            ],
            affected_files: vec!["file1.rs".to_string(), "file2.rs".to_string()],
        };

        let json = serde_json::to_string(&impact).unwrap();
        let deserialized: ImpactAnalysis = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.affected_functions.len(), 2);
        assert_eq!(deserialized.affected_files.len(), 2);
    }
}
