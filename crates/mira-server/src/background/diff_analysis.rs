// crates/mira-server/src/background/diff_analysis.rs
// Core logic for semantic diff analysis

use crate::db::pool::DatabasePool;
use crate::db::{
    DiffAnalysis, get_cached_diff_analysis_sync, map_files_to_symbols_sync,
    store_diff_analysis_sync,
};
use crate::llm::{LlmClient, PromptBuilder, record_llm_usage};
use crate::proactive::PatternType;
use crate::proactive::patterns::{PatternData, get_patterns_by_type};
use crate::search::find_callers;
use crate::utils::ResultExt;
use crate::utils::json::parse_json_hardened;
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
    /// Full list of changed file paths (from git numstat)
    #[serde(default)]
    pub files: Vec<String>,
}

/// Diff statistics from git
#[derive(Debug, Default)]
pub struct DiffStats {
    pub files_changed: i64,
    pub lines_added: i64,
    pub lines_removed: i64,
    pub files: Vec<String>,
}

/// Historical risk assessment computed from mined change patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalRisk {
    /// Overall risk adjustment: "elevated" or "normal"
    pub risk_delta: String,
    /// Patterns that matched the current diff
    pub matching_patterns: Vec<MatchedPattern>,
    /// Weighted average confidence across matched patterns
    pub overall_confidence: f64,
}

/// A single pattern that matched the current diff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchedPattern {
    /// Pattern subtype: "module_hotspot", "co_change_gap", "size_risk"
    pub pattern_subtype: String,
    /// Human-readable description of the match
    pub description: String,
    /// Pattern confidence (0.0-1.0)
    pub confidence: f64,
    /// Bad outcome rate from historical data
    pub bad_rate: f64,
}

/// Compute historical risk by matching current diff files against mined ChangePattern patterns.
///
/// This is computed LIVE at query time (never cached) so it always reflects
/// the latest mined patterns.
pub fn compute_historical_risk(
    conn: &rusqlite::Connection,
    project_id: i64,
    files: &[String],
    files_changed: i64,
) -> Option<HistoricalRisk> {
    let patterns = get_patterns_by_type(conn, project_id, &PatternType::ChangePattern, 50).ok()?;

    if patterns.is_empty() {
        return None;
    }

    let file_set: HashSet<&str> = files.iter().map(|f| f.as_str()).collect();

    // Extract top-level module for each file (same logic as mining)
    let file_modules: HashSet<&str> = files
        .iter()
        .map(|f| match f.find('/') {
            Some(idx) => &f[..idx],
            None => f.as_str(),
        })
        .collect();

    // Determine size bucket (same buckets as mining)
    let size_bucket = if files_changed <= 3 {
        "small"
    } else if files_changed <= 10 {
        "medium"
    } else {
        "large"
    };

    let mut matches = Vec::new();

    for pattern in &patterns {
        if let PatternData::ChangePattern {
            ref files,
            ref module,
            ref pattern_subtype,
            ref outcome_stats,
            ..
        } = pattern.pattern_data
        {
            let bad_rate = if outcome_stats.total > 0 {
                (outcome_stats.reverted + outcome_stats.follow_up_fix) as f64
                    / outcome_stats.total as f64
            } else {
                0.0
            };

            match pattern_subtype.as_str() {
                "module_hotspot" => {
                    if let Some(m) = module {
                        if file_modules.contains(m.as_str()) {
                            matches.push(MatchedPattern {
                                pattern_subtype: pattern_subtype.clone(),
                                description: format!(
                                    "Module '{}' has {:.0}% bad outcome rate ({} of {} changes)",
                                    m,
                                    bad_rate * 100.0,
                                    outcome_stats.reverted + outcome_stats.follow_up_fix,
                                    outcome_stats.total
                                ),
                                confidence: pattern.confidence,
                                bad_rate,
                            });
                        }
                    }
                }
                "co_change_gap" => {
                    if files.len() >= 2 {
                        let file_a = &files[0];
                        let file_b = &files[1];
                        // Flag if file_a is in diff but file_b is NOT
                        if file_set.contains(file_a.as_str())
                            && !file_set.contains(file_b.as_str())
                        {
                            matches.push(MatchedPattern {
                                pattern_subtype: pattern_subtype.clone(),
                                description: format!(
                                    "'{}' changed without '{}' — historically {:.0}% bad outcome rate",
                                    file_a,
                                    file_b,
                                    bad_rate * 100.0
                                ),
                                confidence: pattern.confidence,
                                bad_rate,
                            });
                        }
                        // Also check the reverse: file_b without file_a
                        if file_set.contains(file_b.as_str())
                            && !file_set.contains(file_a.as_str())
                        {
                            matches.push(MatchedPattern {
                                pattern_subtype: pattern_subtype.clone(),
                                description: format!(
                                    "'{}' changed without '{}' — historically {:.0}% bad outcome rate",
                                    file_b,
                                    file_a,
                                    bad_rate * 100.0
                                ),
                                confidence: pattern.confidence,
                                bad_rate,
                            });
                        }
                    }
                }
                "size_risk" => {
                    // Extract bucket from pattern_key: "size_risk:small"
                    let pattern_bucket = pattern
                        .pattern_key
                        .strip_prefix("size_risk:")
                        .unwrap_or("");
                    if pattern_bucket == size_bucket {
                        matches.push(MatchedPattern {
                            pattern_subtype: pattern_subtype.clone(),
                            description: format!(
                                "{} changes ({} files) have {:.0}% bad outcome rate historically",
                                size_bucket.to_uppercase(),
                                files_changed,
                                bad_rate * 100.0
                            ),
                            confidence: pattern.confidence,
                            bad_rate,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    if matches.is_empty() {
        return None;
    }

    let total_confidence: f64 = matches.iter().map(|m| m.confidence).sum();
    let overall_confidence = total_confidence / matches.len() as f64;

    let risk_delta = if matches.iter().any(|m| m.confidence > 0.5) {
        "elevated".to_string()
    } else {
        "normal".to_string()
    };

    Some(HistoricalRisk {
        risk_delta,
        matching_patterns: matches,
        overall_confidence,
    })
}

/// Get unified diff between two refs
pub fn get_unified_diff(
    project_path: &Path,
    from_ref: &str,
    to_ref: &str,
) -> Result<String, String> {
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
pub fn parse_diff_stats(
    project_path: &Path,
    from_ref: &str,
    to_ref: &str,
) -> Result<DiffStats, String> {
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
                        affected_functions.push((
                            caller.symbol_name.clone(),
                            caller.file_path.clone(),
                            depth,
                        ));
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

/// Security-relevant keywords for heuristic scanning
const SECURITY_KEYWORDS: &[&str] = &[
    "password",
    "token",
    "secret",
    "auth",
    "sql",
    "unsafe",
    "exec",
    "eval",
    "credential",
    "private_key",
    "api_key",
    "encrypt",
    "decrypt",
    "hash",
    "permission",
    "privilege",
    "sanitize",
    "injection",
];

/// Function definition patterns for heuristic detection
const FUNCTION_PATTERNS: &[&str] = &["fn ", "def ", "function ", "class ", "impl "];

/// Analyze diff heuristically without LLM
pub fn analyze_diff_heuristic(
    diff_content: &str,
    stats: &DiffStats,
) -> (Vec<SemanticChange>, String, Vec<String>) {
    if diff_content.is_empty() {
        return (Vec::new(), "[heuristic] No changes".to_string(), Vec::new());
    }

    let mut changes = Vec::new();
    let mut risk_flags = Vec::new();
    let mut current_file: Option<String> = None;
    let mut security_hits: Vec<String> = Vec::new();

    for line in diff_content.lines() {
        // Parse file headers: "diff --git a/path b/path"
        if line.starts_with("diff --git ") {
            // Extract file path from "diff --git a/foo b/foo"
            if let Some(b_part) = line.split(" b/").last() {
                current_file = Some(b_part.to_string());
            }
            continue;
        }

        // Handle rename lines: "rename from ..." / "rename to ..."
        if line.starts_with("rename to ") {
            if let Some(path) = line.strip_prefix("rename to ") {
                current_file = Some(path.to_string());
            }
            continue;
        }

        // Skip binary diffs
        if line.starts_with("Binary files") {
            continue;
        }

        // Only scan added/removed lines within hunks
        let is_added = line.starts_with('+') && !line.starts_with("+++");
        let is_removed = line.starts_with('-') && !line.starts_with("---");

        if !is_added && !is_removed {
            continue;
        }

        let content = if is_added { &line[1..] } else { &line[1..] };
        let file_path = current_file.clone().unwrap_or_default();

        // Detect function definitions in changed lines
        for pattern in FUNCTION_PATTERNS {
            if content.contains(pattern) {
                let symbol_name = extract_symbol_name(content, pattern);
                let change_type = if is_added {
                    "NewFunction"
                } else {
                    "DeletedFunction"
                };
                // Avoid duplicates for the same symbol in the same file
                let already_exists = changes.iter().any(|c: &SemanticChange| {
                    c.file_path == file_path
                        && c.symbol_name.as_deref() == Some(symbol_name.as_str())
                        && c.change_type == change_type
                });
                if !already_exists {
                    changes.push(SemanticChange {
                        change_type: change_type.to_string(),
                        file_path: file_path.clone(),
                        symbol_name: Some(symbol_name),
                        description: format!(
                            "{} {}",
                            if is_added { "Added" } else { "Removed" },
                            pattern.trim()
                        ),
                        breaking: is_removed,
                        security_relevant: false,
                    });
                }
                break;
            }
        }

        // Scan for security-relevant keywords
        let lower = content.to_lowercase();
        for keyword in SECURITY_KEYWORDS {
            if lower.contains(keyword) {
                security_hits.push(format!("{}:{}", file_path, keyword));
                break;
            }
        }
    }

    // Mark security-relevant changes
    if !security_hits.is_empty() {
        risk_flags.push("security_relevant_change".to_string());
        // Mark changes in files with security hits as security_relevant
        let security_files: std::collections::HashSet<String> = security_hits
            .iter()
            .filter_map(|h| h.split(':').next().map(|s| s.to_string()))
            .collect();
        for change in &mut changes {
            if security_files.contains(&change.file_path) {
                change.security_relevant = true;
            }
        }
    }

    // Risk flag: large change (>500 total lines)
    if stats.lines_added + stats.lines_removed > 500 {
        risk_flags.push("large_change".to_string());
    }

    // Risk flag: wide change (>10 files)
    if stats.files_changed > 10 {
        risk_flags.push("wide_change".to_string());
    }

    // Risk flag: breaking API change (removed functions)
    let removed_count = changes
        .iter()
        .filter(|c| c.change_type == "DeletedFunction")
        .count();
    if removed_count > 0 {
        risk_flags.push("breaking_api_change".to_string());
    }

    // Build summary
    let added_fns = changes
        .iter()
        .filter(|c| c.change_type == "NewFunction")
        .count();
    let summary = format!(
        "[heuristic] {} files changed (+{} -{}), {} functions added, {} removed{}",
        stats.files_changed,
        stats.lines_added,
        stats.lines_removed,
        added_fns,
        removed_count,
        if !security_hits.is_empty() {
            format!("; {} security-relevant change(s)", security_hits.len())
        } else {
            String::new()
        }
    );

    (changes, summary, risk_flags)
}

/// Extract a symbol name from a line containing a function/class pattern
fn extract_symbol_name(line: &str, pattern: &str) -> String {
    if let Some(after) = line.split(pattern).nth(1) {
        // Take everything up to first ( or { or : or < or whitespace
        let name: String = after
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            return name;
        }
    }
    "unknown".to_string()
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
    llm_client: &Arc<dyn LlmClient>,
    pool: &Arc<DatabasePool>,
    project_id: Option<i64>,
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

    let result = llm_client
        .chat(messages, None)
        .await
        .map_err(|e| format!("LLM request failed: {}", e))?;

    // Record usage
    record_llm_usage(
        pool,
        llm_client.provider_type(),
        &llm_client.model_name(),
        "background:diff_analysis",
        &result,
        project_id,
        None,
    )
    .await;

    let content = result.content.ok_or("No content in LLM response")?;

    // Try to parse JSON from response
    parse_llm_response(&content)
}

/// Parse the LLM response to extract structured data
fn parse_llm_response(content: &str) -> Result<(Vec<SemanticChange>, String, Vec<String>), String> {
    // Try hardened JSON parsing first
    if let Ok(response) = parse_json_hardened::<LlmDiffResponse>(content) {
        return Ok((response.changes, response.summary, response.risk_flags));
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
    let changes_json = serde_json::to_string(&result.changes).ok();
    let impact_json = result
        .impact
        .as_ref()
        .and_then(|i| serde_json::to_string(i).ok());
    let risk_json = serde_json::to_string(&result.risk).ok();
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
        serde_json::to_string(&result.files).ok()
    };

    if let Err(e) = pool
        .interact(move |conn| {
            store_diff_analysis_sync(
                conn,
                project_id,
                &from,
                &to,
                &analysis_type,
                changes_json.as_deref(),
                impact_json.as_deref(),
                risk_json.as_deref(),
                Some(&summary),
                Some(files_changed),
                Some(lines_added),
                Some(lines_removed),
                files_json.as_deref(),
            )
            .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
    {
        tracing::warn!("Failed to cache diff analysis: {}", e);
    }
}

/// Perform complete diff analysis (LLM optional — falls back to heuristic)
pub async fn analyze_diff(
    pool: &Arc<DatabasePool>,
    llm_client: Option<&Arc<dyn LlmClient>>,
    project_path: &Path,
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
        .interact(move |conn| {
            get_cached_diff_analysis_sync(conn, project_id, &from_for_cache, &to_for_cache)
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .str_err()?;

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
            .interact(move |conn| -> Result<ImpactAnalysis, anyhow::Error> {
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
            .await
            .str_err()?;
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

/// Format diff analysis result for display
pub fn format_diff_analysis(
    result: &DiffAnalysisResult,
    historical_risk: Option<&HistoricalRisk>,
) -> String {
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
        output.push_str(&format_changes_section(&result.changes));
    }

    // Impact
    if let Some(ref impact) = result.impact {
        output.push_str(&format_impact_section(impact));
    }

    // Risk
    output.push_str(&format!("### Risk: {}\n", result.risk.overall));
    for flag in &result.risk.flags {
        output.push_str(&format!("- {}\n", flag));
    }

    // Historical Risk (from mined change patterns)
    if let Some(hr) = historical_risk {
        output.push_str(&format!(
            "\n### Historical Risk: {}\n",
            hr.risk_delta.to_uppercase()
        ));
        output.push_str(&format!(
            "Based on {} matching pattern(s) (confidence: {:.0}%)\n",
            hr.matching_patterns.len(),
            hr.overall_confidence * 100.0
        ));
        for mp in &hr.matching_patterns {
            output.push_str(&format!("- **{}**: {}\n", mp.pattern_subtype, mp.description));
        }
    }

    output
}

/// Format grouped changes section
fn format_changes_section(changes: &[SemanticChange]) -> String {
    let mut output = format!("### Changes ({})\n", changes.len());

    let groups: &[(&str, &[&str])] = &[
        ("New Features", &["NewFunction", "NewFeature"]),
        (
            "Modifications",
            &["ModifiedFunction", "SignatureChange", "Refactoring"],
        ),
        ("Deletions", &["DeletedFunction"]),
    ];

    let mut classified = HashSet::new();

    for (title, types) in groups {
        let matching: Vec<_> = changes
            .iter()
            .filter(|c| types.contains(&c.change_type.as_str()))
            .collect();

        if !matching.is_empty() {
            output.push_str(&format!("**{}**\n", title));
            for c in &matching {
                let markers = format_change_markers(c);
                output.push_str(&format!(
                    "- {}: {}{}\n",
                    c.file_path, c.description, markers
                ));
                classified.insert(&c.description);
            }
            output.push('\n');
        }
    }

    // Other (unclassified)
    let other: Vec<_> = changes
        .iter()
        .filter(|c| !classified.contains(&c.description))
        .collect();

    if !other.is_empty() {
        output.push_str("**Other Changes**\n");
        for c in other {
            let markers = format_change_markers(c);
            output.push_str(&format!(
                "- {}: {}{}\n",
                c.file_path, c.description, markers
            ));
        }
        output.push('\n');
    }

    output
}

/// Format impact analysis section
fn format_impact_section(impact: &ImpactAnalysis) -> String {
    if impact.affected_functions.is_empty() {
        return String::new();
    }

    let direct = impact
        .affected_functions
        .iter()
        .filter(|(_, _, d)| *d == 1)
        .count();
    let transitive = impact
        .affected_functions
        .iter()
        .filter(|(_, _, d)| *d > 1)
        .count();

    format!(
        "### Impact\n- Directly affected: {} functions\n- Transitively affected: {} functions\n- Affected files: {}\n\n",
        direct,
        transitive,
        impact.affected_files.len()
    )
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
            files: vec!["src/main.rs".to_string()],
            files_changed: 1,
            lines_added: 10,
            lines_removed: 5,
        };

        let output = format_diff_analysis(&result, None);
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
            files: vec!["src/main.rs".to_string(), "src/lib.rs".to_string()],
            files_changed: 2,
            lines_added: 50,
            lines_removed: 10,
        };

        let output = format_diff_analysis(&result, None);
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

    // =========================================================================
    // Historical Risk Tests
    // =========================================================================

    fn setup_patterns_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE behavior_patterns (
                id INTEGER PRIMARY KEY,
                project_id INTEGER,
                pattern_type TEXT NOT NULL,
                pattern_key TEXT NOT NULL,
                pattern_data TEXT NOT NULL,
                confidence REAL DEFAULT 0.5,
                occurrence_count INTEGER DEFAULT 1,
                last_triggered_at TEXT,
                first_seen_at TEXT DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(project_id, pattern_type, pattern_key)
            );",
        )
        .unwrap();
        conn
    }

    fn seed_pattern(conn: &rusqlite::Connection, project_id: i64, key: &str, data: &str, confidence: f64, count: i64) {
        conn.execute(
            "INSERT INTO behavior_patterns (project_id, pattern_type, pattern_key, pattern_data, confidence, occurrence_count)
             VALUES (?, 'change_pattern', ?, ?, ?, ?)",
            rusqlite::params![project_id, key, data, confidence, count],
        )
        .unwrap();
    }

    #[test]
    fn test_historical_risk_no_patterns() {
        let conn = setup_patterns_db();
        let result = compute_historical_risk(&conn, 1, &["src/main.rs".into()], 1);
        assert!(result.is_none(), "Should return None when no patterns exist");
    }

    #[test]
    fn test_historical_risk_module_hotspot_match() {
        let conn = setup_patterns_db();
        let data = serde_json::json!({
            "type": "change_pattern",
            "files": [],
            "module": "src",
            "pattern_subtype": "module_hotspot",
            "outcome_stats": { "total": 10, "clean": 5, "reverted": 3, "follow_up_fix": 2 },
            "sample_commits": []
        });
        seed_pattern(&conn, 1, "module_hotspot:src", &data.to_string(), 0.7, 10);

        let result = compute_historical_risk(
            &conn, 1, &["src/lib.rs".into(), "src/main.rs".into()], 2,
        );
        assert!(result.is_some());
        let hr = result.unwrap();
        assert_eq!(hr.risk_delta, "elevated");
        assert_eq!(hr.matching_patterns.len(), 1);
        assert_eq!(hr.matching_patterns[0].pattern_subtype, "module_hotspot");
        assert!((hr.matching_patterns[0].bad_rate - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_historical_risk_module_hotspot_no_match() {
        let conn = setup_patterns_db();
        let data = serde_json::json!({
            "type": "change_pattern",
            "files": [],
            "module": "tests",
            "pattern_subtype": "module_hotspot",
            "outcome_stats": { "total": 10, "clean": 3, "reverted": 5, "follow_up_fix": 2 },
            "sample_commits": []
        });
        seed_pattern(&conn, 1, "module_hotspot:tests", &data.to_string(), 0.8, 10);

        // Files are in "src", not "tests"
        let result = compute_historical_risk(&conn, 1, &["src/main.rs".into()], 1);
        assert!(result.is_none());
    }

    #[test]
    fn test_historical_risk_co_change_gap_match() {
        let conn = setup_patterns_db();
        let data = serde_json::json!({
            "type": "change_pattern",
            "files": ["src/schema.rs", "src/migrations.rs"],
            "module": null,
            "pattern_subtype": "co_change_gap",
            "outcome_stats": { "total": 5, "clean": 1, "reverted": 2, "follow_up_fix": 2 },
            "sample_commits": []
        });
        seed_pattern(&conn, 1, "co_change_gap:src/schema.rs|src/migrations.rs", &data.to_string(), 0.65, 5);

        // schema.rs changed WITHOUT migrations.rs
        let result = compute_historical_risk(
            &conn, 1, &["src/schema.rs".into(), "src/lib.rs".into()], 2,
        );
        assert!(result.is_some());
        let hr = result.unwrap();
        assert_eq!(hr.matching_patterns.len(), 1);
        assert_eq!(hr.matching_patterns[0].pattern_subtype, "co_change_gap");
        assert!(hr.matching_patterns[0].description.contains("schema.rs"));
        assert!(hr.matching_patterns[0].description.contains("migrations.rs"));
    }

    #[test]
    fn test_historical_risk_co_change_gap_both_present() {
        let conn = setup_patterns_db();
        let data = serde_json::json!({
            "type": "change_pattern",
            "files": ["src/schema.rs", "src/migrations.rs"],
            "module": null,
            "pattern_subtype": "co_change_gap",
            "outcome_stats": { "total": 5, "clean": 1, "reverted": 2, "follow_up_fix": 2 },
            "sample_commits": []
        });
        seed_pattern(&conn, 1, "co_change_gap:src/schema.rs|src/migrations.rs", &data.to_string(), 0.65, 5);

        // Both files present — no gap, no match
        let result = compute_historical_risk(
            &conn, 1, &["src/schema.rs".into(), "src/migrations.rs".into()], 2,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_historical_risk_size_risk_match() {
        let conn = setup_patterns_db();
        let data = serde_json::json!({
            "type": "change_pattern",
            "files": [],
            "module": null,
            "pattern_subtype": "size_risk",
            "outcome_stats": { "total": 8, "clean": 3, "reverted": 3, "follow_up_fix": 2 },
            "sample_commits": []
        });
        seed_pattern(&conn, 1, "size_risk:large", &data.to_string(), 0.6, 8);

        // 15 files = "large" bucket
        let files: Vec<String> = (0..15).map(|i| format!("src/file{}.rs", i)).collect();
        let result = compute_historical_risk(&conn, 1, &files, 15);
        assert!(result.is_some());
        let hr = result.unwrap();
        assert_eq!(hr.matching_patterns.len(), 1);
        assert_eq!(hr.matching_patterns[0].pattern_subtype, "size_risk");
        assert!(hr.matching_patterns[0].description.contains("LARGE"));
    }

    #[test]
    fn test_historical_risk_size_bucket_no_match() {
        let conn = setup_patterns_db();
        let data = serde_json::json!({
            "type": "change_pattern",
            "files": [],
            "module": null,
            "pattern_subtype": "size_risk",
            "outcome_stats": { "total": 8, "clean": 3, "reverted": 3, "follow_up_fix": 2 },
            "sample_commits": []
        });
        seed_pattern(&conn, 1, "size_risk:large", &data.to_string(), 0.6, 8);

        // 2 files = "small" bucket, pattern is for "large"
        let result = compute_historical_risk(
            &conn, 1, &["src/a.rs".into(), "src/b.rs".into()], 2,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_historical_risk_multiple_matches() {
        let conn = setup_patterns_db();

        // Module hotspot
        let hotspot = serde_json::json!({
            "type": "change_pattern",
            "files": [],
            "module": "src",
            "pattern_subtype": "module_hotspot",
            "outcome_stats": { "total": 10, "clean": 5, "reverted": 3, "follow_up_fix": 2 },
            "sample_commits": []
        });
        seed_pattern(&conn, 1, "module_hotspot:src", &hotspot.to_string(), 0.7, 10);

        // Size risk for medium
        let size = serde_json::json!({
            "type": "change_pattern",
            "files": [],
            "module": null,
            "pattern_subtype": "size_risk",
            "outcome_stats": { "total": 6, "clean": 2, "reverted": 2, "follow_up_fix": 2 },
            "sample_commits": []
        });
        seed_pattern(&conn, 1, "size_risk:medium", &size.to_string(), 0.55, 6);

        // 5 files in src = matches both module_hotspot:src AND size_risk:medium
        let files: Vec<String> = (0..5).map(|i| format!("src/file{}.rs", i)).collect();
        let result = compute_historical_risk(&conn, 1, &files, 5);
        assert!(result.is_some());
        let hr = result.unwrap();
        assert_eq!(hr.matching_patterns.len(), 2);
        assert_eq!(hr.risk_delta, "elevated");
        // Confidence should be average of 0.7 and 0.55
        assert!((hr.overall_confidence - 0.625).abs() < 0.01);
    }

    #[test]
    fn test_historical_risk_low_confidence_normal() {
        let conn = setup_patterns_db();
        let data = serde_json::json!({
            "type": "change_pattern",
            "files": [],
            "module": "src",
            "pattern_subtype": "module_hotspot",
            "outcome_stats": { "total": 3, "clean": 1, "reverted": 1, "follow_up_fix": 1 },
            "sample_commits": []
        });
        // Low confidence (0.4) — should match but risk_delta = "normal"
        seed_pattern(&conn, 1, "module_hotspot:src", &data.to_string(), 0.4, 3);

        let result = compute_historical_risk(&conn, 1, &["src/main.rs".into()], 1);
        assert!(result.is_some());
        let hr = result.unwrap();
        assert_eq!(hr.risk_delta, "normal");
    }

    #[test]
    fn test_historical_risk_wrong_project() {
        let conn = setup_patterns_db();
        let data = serde_json::json!({
            "type": "change_pattern",
            "files": [],
            "module": "src",
            "pattern_subtype": "module_hotspot",
            "outcome_stats": { "total": 10, "clean": 5, "reverted": 3, "follow_up_fix": 2 },
            "sample_commits": []
        });
        seed_pattern(&conn, 1, "module_hotspot:src", &data.to_string(), 0.7, 10);

        // Query for project 2 — shouldn't match project 1's patterns
        let result = compute_historical_risk(&conn, 2, &["src/main.rs".into()], 1);
        assert!(result.is_none());
    }

    #[test]
    fn test_historical_risk_serialization_roundtrip() {
        let hr = HistoricalRisk {
            risk_delta: "elevated".to_string(),
            matching_patterns: vec![
                MatchedPattern {
                    pattern_subtype: "module_hotspot".to_string(),
                    description: "Module 'src' has 50% bad outcome rate".to_string(),
                    confidence: 0.7,
                    bad_rate: 0.5,
                },
            ],
            overall_confidence: 0.7,
        };

        let json = serde_json::to_string(&hr).unwrap();
        let deserialized: HistoricalRisk = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.risk_delta, "elevated");
        assert_eq!(deserialized.matching_patterns.len(), 1);
        assert!((deserialized.overall_confidence - 0.7).abs() < 0.001);
    }

    #[test]
    fn test_format_diff_analysis_with_historical_risk() {
        let result = DiffAnalysisResult {
            from_ref: "abc123".to_string(),
            to_ref: "def456".to_string(),
            changes: vec![],
            impact: None,
            risk: RiskAssessment {
                overall: "Medium".to_string(),
                flags: vec![],
            },
            summary: "Test".to_string(),
            files: vec!["src/main.rs".to_string()],
            files_changed: 1,
            lines_added: 5,
            lines_removed: 2,
        };

        let hr = HistoricalRisk {
            risk_delta: "elevated".to_string(),
            matching_patterns: vec![
                MatchedPattern {
                    pattern_subtype: "module_hotspot".to_string(),
                    description: "Module 'src' has 50% bad outcome rate".to_string(),
                    confidence: 0.7,
                    bad_rate: 0.5,
                },
            ],
            overall_confidence: 0.7,
        };

        let output = format_diff_analysis(&result, Some(&hr));
        assert!(output.contains("### Historical Risk: ELEVATED"));
        assert!(output.contains("1 matching pattern(s)"));
        assert!(output.contains("module_hotspot"));
        assert!(output.contains("50% bad outcome rate"));
    }

    #[test]
    fn test_format_diff_analysis_without_historical_risk() {
        let result = DiffAnalysisResult {
            from_ref: "abc123".to_string(),
            to_ref: "def456".to_string(),
            changes: vec![],
            impact: None,
            risk: RiskAssessment {
                overall: "Low".to_string(),
                flags: vec![],
            },
            summary: "Test".to_string(),
            files: vec![],
            files_changed: 0,
            lines_added: 0,
            lines_removed: 0,
        };

        let output = format_diff_analysis(&result, None);
        assert!(!output.contains("Historical Risk"));
    }
}
