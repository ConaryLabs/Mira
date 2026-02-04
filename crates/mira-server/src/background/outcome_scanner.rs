// crates/mira-server/src/background/outcome_scanner.rs
// Background worker for tracking diff outcomes (reverts, follow-up fixes, clean aging)
//
// Scans git history to determine whether previously-analyzed diffs caused problems.
// This data feeds into change pattern mining (Milestone 2) and predictive risk scoring (Milestone 3).

use crate::db::diff_outcomes::get_unscanned_diffs_sync;
use crate::db::diff_outcomes::mark_clean_outcomes_sync;
use crate::db::diff_outcomes::store_diff_outcome_sync;
use crate::db::pool::DatabasePool;
use crate::db::{get_indexed_projects_sync, set_server_state_sync};
use crate::utils::ResultExt;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

/// Maximum number of unscanned diffs to process per project per cycle
const MAX_DIFFS_PER_CYCLE: usize = 50;

/// Days before marking a diff with no issues as "clean"
const CLEAN_AGING_DAYS: i64 = 7;

/// Hours window for detecting follow-up fixes
const FOLLOWUP_WINDOW_HOURS: i64 = 72;

/// Server state key prefix for tracking last scan commit per project
const STATE_KEY_PREFIX: &str = "last_outcome_scan_";

/// A commit from git log with metadata
#[derive(Debug)]
struct GitCommit {
    hash: String,
    timestamp: i64,
    message: String,
}

/// Scan all indexed projects for diff outcomes
pub async fn process_outcome_scanning(pool: &Arc<DatabasePool>) -> Result<usize, String> {
    let projects = pool
        .interact(|conn| {
            get_indexed_projects_sync(conn)
                .map_err(|e| anyhow::anyhow!("Failed to get projects: {}", e))
        })
        .await
        .str_err()?;

    let mut processed = 0;

    for (project_id, project_path) in projects {
        if !Path::new(&project_path).exists() {
            continue;
        }

        match scan_project(pool, project_id, &project_path).await {
            Ok(count) => processed += count,
            Err(e) => {
                tracing::warn!(
                    "Outcome scan failed for project {} ({}): {}",
                    project_id,
                    project_path,
                    e
                );
            }
        }
    }

    Ok(processed)
}

/// Scan a single project for diff outcomes
async fn scan_project(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    let mut processed = 0;

    // Get unscanned diffs for this project
    let pid = project_id;
    let unscanned = pool
        .interact(move |conn| {
            get_unscanned_diffs_sync(conn, pid, MAX_DIFFS_PER_CYCLE)
                .map_err(|e| anyhow::anyhow!("Failed to get unscanned diffs: {}", e))
        })
        .await
        .str_err()?;

    if unscanned.is_empty() {
        // Still mark clean outcomes for aged diffs
        let pid = project_id;
        let aged = pool
            .interact(move |conn| {
                mark_clean_outcomes_sync(conn, pid, CLEAN_AGING_DAYS)
                    .map_err(|e| anyhow::anyhow!("Failed to mark clean: {}", e))
            })
            .await
            .str_err()?;
        return Ok(aged);
    }

    // Collect all commit hashes we need to check
    let commit_hashes: Vec<String> = unscanned
        .iter()
        .map(|(_, hash, _, _)| hash.clone())
        .collect();

    // Detect reverts in recent git history
    let reverts = detect_reverts(project_path, &commit_hashes);

    // Detect follow-up fixes
    let followup_fixes = detect_followup_fixes(project_path, &unscanned);

    // Store detected outcomes
    for (diff_id, to_commit, _from_commit, _files_json) in &unscanned {
        let diff_id = *diff_id;

        // Check for reverts
        if let Some((evidence_commit, evidence_msg, time_delta)) = reverts.get(to_commit.as_str()) {
            let ev_commit = evidence_commit.clone();
            let ev_msg = evidence_msg.clone();
            let td = *time_delta;
            let pid = project_id;
            pool.interact(move |conn| {
                store_diff_outcome_sync(
                    conn,
                    diff_id,
                    Some(pid),
                    "reverted",
                    Some(&ev_commit),
                    Some(&ev_msg),
                    Some(td),
                    "git_scan",
                )
                .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
            .str_err()?;
            processed += 1;
        }

        // Check for follow-up fixes
        if let Some(fixes) = followup_fixes.get(to_commit.as_str()) {
            for (evidence_commit, evidence_msg, time_delta) in fixes {
                let ev_commit = evidence_commit.clone();
                let ev_msg = evidence_msg.clone();
                let td = *time_delta;
                let pid = project_id;
                pool.interact(move |conn| {
                    store_diff_outcome_sync(
                        conn,
                        diff_id,
                        Some(pid),
                        "follow_up_fix",
                        Some(&ev_commit),
                        Some(&ev_msg),
                        Some(td),
                        "git_scan",
                    )
                    .map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .str_err()?;
                processed += 1;
            }
        }
    }

    // Mark aged diffs as clean
    let pid = project_id;
    let aged = pool
        .interact(move |conn| {
            mark_clean_outcomes_sync(conn, pid, CLEAN_AGING_DAYS)
                .map_err(|e| anyhow::anyhow!("Failed to mark clean: {}", e))
        })
        .await
        .str_err()?;
    processed += aged;

    // Update scan state
    if let Some(latest_commit) = get_git_head(project_path) {
        let key = format!("{}{}", STATE_KEY_PREFIX, project_id);
        pool.interact(move |conn| {
            set_server_state_sync(conn, &key, &latest_commit).map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .str_err()?;
    }

    Ok(processed)
}

/// Detect reverts in git history that reference any of the given commit hashes.
/// Returns a map of original_commit -> (revert_commit, message, time_delta_seconds).
fn detect_reverts<'a>(
    project_path: &str,
    commit_hashes: &'a [String],
) -> HashMap<&'a str, (String, String, i64)> {
    let mut result = HashMap::new();

    if commit_hashes.is_empty() {
        return result;
    }

    // Get recent commits with full hash, timestamp, and subject
    let commits = get_recent_commits(project_path, 200);

    for commit in &commits {
        let msg_lower = commit.message.to_lowercase();

        // Match "revert" commits
        if !msg_lower.starts_with("revert") {
            continue;
        }

        // Check if the revert message references any of our tracked commits
        for hash in commit_hashes {
            // Check for full SHA or short SHA (first 7+ chars) in the revert message
            let short_hash = &hash[..std::cmp::min(hash.len(), 8)];
            if commit.message.contains(hash.as_str()) || commit.message.contains(short_hash) {
                // Get timestamp of original commit
                if let Some(original_ts) = get_commit_timestamp(project_path, hash) {
                    let time_delta = commit.timestamp - original_ts;
                    result.insert(
                        hash.as_str(),
                        (commit.hash.clone(), commit.message.clone(), time_delta),
                    );
                }
                break;
            }

            // Also check for "Revert \"<message>\"" pattern — match the original commit's message
            if let Some(original_msg) = get_commit_message(project_path, hash) {
                let quoted = format!("\"{}\"", original_msg.trim());
                if commit.message.contains(&quoted) {
                    if let Some(original_ts) = get_commit_timestamp(project_path, hash) {
                        let time_delta = commit.timestamp - original_ts;
                        result.insert(
                            hash.as_str(),
                            (commit.hash.clone(), commit.message.clone(), time_delta),
                        );
                    }
                    break;
                }
            }
        }
    }

    result
}

/// Detect follow-up fixes: commits within FOLLOWUP_WINDOW_HOURS that touch the same files
/// and have fix-like messages.
/// Returns a map of original_commit -> vec of (fix_commit, message, time_delta_seconds).
fn detect_followup_fixes<'a>(
    project_path: &str,
    diffs: &'a [(i64, String, String, Option<String>)],
) -> HashMap<&'a str, Vec<(String, String, i64)>> {
    let mut result: HashMap<&str, Vec<(String, String, i64)>> = HashMap::new();

    for (_diff_id, to_commit, _from_commit, files_json) in diffs {
        // Parse the file list for this diff
        let diff_files: Vec<String> = files_json
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok())
            .unwrap_or_default();

        if diff_files.is_empty() {
            continue;
        }

        // Get the timestamp of the analyzed commit
        let commit_ts = match get_commit_timestamp(project_path, to_commit) {
            Some(ts) => ts,
            None => continue,
        };

        // Find commits after this one within the window
        let window_end = commit_ts + (FOLLOWUP_WINDOW_HOURS * 3600);
        let candidates = get_commits_in_range(project_path, to_commit, window_end);

        for candidate in candidates {
            if !is_fix_message(&candidate.message) {
                continue;
            }

            // Check if the fix touches any of the same files
            let fix_files = get_files_for_commit(project_path, &candidate.hash);
            let has_overlap = fix_files.iter().any(|f| diff_files.contains(f));

            if has_overlap {
                let time_delta = candidate.timestamp - commit_ts;
                result.entry(to_commit.as_str()).or_default().push((
                    candidate.hash.clone(),
                    candidate.message.clone(),
                    time_delta,
                ));
            }
        }
    }

    result
}

/// Check if a commit message indicates a fix
fn is_fix_message(message: &str) -> bool {
    let lower = message.to_lowercase();
    // Common fix indicators
    lower.starts_with("fix")
        || lower.starts_with("hotfix")
        || lower.starts_with("bugfix")
        || lower.contains("fix:")
        || lower.contains("fix(")
        || lower.contains("hotfix:")
        || lower.starts_with("revert")
        || lower.contains("broken")
        || lower.contains("oops")
        || lower.contains("typo")
        || lower.contains("patch:")
}

// ============================================================================
// Git helpers
// ============================================================================

/// Get recent commits with hash, unix timestamp, and subject
fn get_recent_commits(project_path: &str, limit: usize) -> Vec<GitCommit> {
    let output = Command::new("git")
        .args(["log", &format!("-{}", limit), "--format=%H %ct %s"])
        .current_dir(project_path)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            parse_commit_lines(&String::from_utf8_lossy(&out.stdout))
        }
        _ => vec![],
    }
}

/// Get commits after a given commit up to a timestamp
fn get_commits_in_range(
    project_path: &str,
    after_commit: &str,
    before_timestamp: i64,
) -> Vec<GitCommit> {
    // Get commits after the given commit, up to the time window
    let output = Command::new("git")
        .args([
            "log",
            "--format=%H %ct %s",
            &format!("--before={}", before_timestamp),
            &format!("{}..HEAD", after_commit),
        ])
        .current_dir(project_path)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            parse_commit_lines(&String::from_utf8_lossy(&out.stdout))
        }
        _ => vec![],
    }
}

/// Parse "HASH TIMESTAMP SUBJECT" lines into GitCommit structs
fn parse_commit_lines(output: &str) -> Vec<GitCommit> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            // Format: "HASH TIMESTAMP SUBJECT..."
            let mut parts = line.splitn(3, ' ');
            let hash = parts.next()?.to_string();
            let timestamp: i64 = parts.next()?.parse().ok()?;
            let message = parts.next().unwrap_or("").to_string();

            // Validate hash is 40 hex chars
            if hash.len() != 40 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }

            Some(GitCommit {
                hash,
                timestamp,
                message,
            })
        })
        .collect()
}

/// Get the unix timestamp for a specific commit
fn get_commit_timestamp(project_path: &str, commit_hash: &str) -> Option<i64> {
    let output = Command::new("git")
        .args(["log", "-1", "--format=%ct", commit_hash])
        .current_dir(project_path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

/// Get the commit message (subject line) for a specific commit
fn get_commit_message(project_path: &str, commit_hash: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["log", "-1", "--format=%s", commit_hash])
        .current_dir(project_path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let msg = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if msg.is_empty() { None } else { Some(msg) }
}

/// Get the list of files changed in a specific commit
fn get_files_for_commit(project_path: &str, commit_hash: &str) -> Vec<String> {
    let output = Command::new("git")
        .args([
            "diff-tree",
            "--no-commit-id",
            "-r",
            "--name-only",
            commit_hash,
        ])
        .current_dir(project_path)
        .output();

    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.trim().to_string())
            .collect(),
        _ => vec![],
    }
}

/// Get git HEAD full SHA
fn get_git_head(project_path: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(project_path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if hash.len() == 40 { Some(hash) } else { None }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_commit_lines_valid() {
        let hash1 = "a".repeat(40);
        let hash2 = "b".repeat(40);
        let input = format!(
            "{} 1706000000 feat: add feature\n{} 1706003600 fix: broken thing\n",
            hash1, hash2
        );
        let commits = parse_commit_lines(&input);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, hash1);
        assert_eq!(commits[0].timestamp, 1706000000);
        assert_eq!(commits[0].message, "feat: add feature");
        assert_eq!(commits[1].hash, hash2);
        assert_eq!(commits[1].message, "fix: broken thing");
    }

    #[test]
    fn test_parse_commit_lines_empty() {
        assert!(parse_commit_lines("").is_empty());
        assert!(parse_commit_lines("   \n  \n").is_empty());
    }

    #[test]
    fn test_parse_commit_lines_invalid_hash() {
        // Short hash should be rejected
        let input = "abc123 1706000000 some commit\n";
        assert!(parse_commit_lines(input).is_empty());
    }

    #[test]
    fn test_parse_commit_lines_invalid_timestamp() {
        let hash = "a".repeat(40);
        let input = format!("{} notanumber some commit\n", hash);
        assert!(parse_commit_lines(&input).is_empty());
    }

    #[test]
    fn test_is_fix_message() {
        assert!(is_fix_message("fix: resolve null pointer"));
        assert!(is_fix_message("Fix typo in readme"));
        assert!(is_fix_message("hotfix: urgent production issue"));
        assert!(is_fix_message("bugfix: handle edge case"));
        assert!(is_fix_message("fix(auth): token expiry"));
        assert!(is_fix_message("revert: undo bad change"));
        assert!(is_fix_message("oops, forgot to save"));
        assert!(is_fix_message("Fixed broken import"));
        assert!(is_fix_message("patch: security update"));

        assert!(!is_fix_message("feat: add new feature"));
        assert!(!is_fix_message("refactor: clean up code"));
        assert!(!is_fix_message("docs: update readme"));
        assert!(!is_fix_message("chore: bump version"));
    }

    #[test]
    fn test_is_fix_message_case_insensitive() {
        assert!(is_fix_message("FIX: something"));
        assert!(is_fix_message("HOTFIX: critical"));
        assert!(is_fix_message("Contains BROKEN reference"));
    }
}
