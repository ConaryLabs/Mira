// crates/mira-server/src/background/outcome_scanner.rs
// Background worker for tracking diff outcomes (reverts, follow-up fixes, clean aging)
//
// Scans git history to determine whether previously-analyzed diffs caused problems.
// This data feeds into change pattern mining (Milestone 2) and predictive risk scoring (Milestone 3).

use crate::db::diff_outcomes::get_unscanned_diffs_sync;
use crate::db::diff_outcomes::mark_clean_outcomes_sync;
use crate::db::diff_outcomes::{StoreDiffOutcomeParams, store_diff_outcome_sync};
use crate::db::pool::DatabasePool;
use crate::db::{get_indexed_projects_sync, set_server_state_sync};
use crate::git::{CommitWithFiles, get_commits_with_files, get_git_head};
use crate::utils::{ResultExt, truncate_at_boundary};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Maximum number of unscanned diffs to process per project per cycle
const MAX_DIFFS_PER_CYCLE: usize = 50;

/// Days before marking a diff with no issues as "clean"
const CLEAN_AGING_DAYS: i64 = 7;

/// Hours window for detecting follow-up fixes
const FOLLOWUP_WINDOW_HOURS: i64 = 72;

/// Server state key prefix for tracking last scan commit per project
const STATE_KEY_PREFIX: &str = "last_outcome_scan_";

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

    // Batch-fetch recent commits with files (single git call replaces N+1)
    let recent = get_commits_with_files(project_path, 200);
    let commit_map: HashMap<&str, &CommitWithFiles> =
        recent.iter().map(|c| (c.hash.as_str(), c)).collect();

    // Detect reverts in recent git history
    let reverts = detect_reverts(&commit_hashes, &recent, &commit_map);

    // Detect follow-up fixes
    let followup_fixes = detect_followup_fixes(&unscanned, &commit_map);

    // Collect all detected outcomes, then persist in a single transaction
    struct Outcome {
        diff_analysis_id: i64,
        outcome_type: &'static str,
        evidence_commit: String,
        evidence_message: String,
        time_to_outcome_seconds: i64,
    }

    let mut outcomes = Vec::new();

    for (diff_id, to_commit, _from_commit, _files_json) in &unscanned {
        // Check for reverts
        if let Some((evidence_commit, evidence_msg, time_delta)) = reverts.get(to_commit.as_str()) {
            outcomes.push(Outcome {
                diff_analysis_id: *diff_id,
                outcome_type: "reverted",
                evidence_commit: evidence_commit.clone(),
                evidence_message: evidence_msg.clone(),
                time_to_outcome_seconds: *time_delta,
            });
        }

        // Check for follow-up fixes
        if let Some(fixes) = followup_fixes.get(to_commit.as_str()) {
            for (evidence_commit, evidence_msg, time_delta) in fixes {
                outcomes.push(Outcome {
                    diff_analysis_id: *diff_id,
                    outcome_type: "follow_up_fix",
                    evidence_commit: evidence_commit.clone(),
                    evidence_message: evidence_msg.clone(),
                    time_to_outcome_seconds: *time_delta,
                });
            }
        }
    }

    // Batch-write all outcomes in one pool.interact call
    if !outcomes.is_empty() {
        let pid = project_id;
        let count = outcomes.len();
        pool.interact(move |conn| {
            let tx = conn
                .unchecked_transaction()
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            for o in &outcomes {
                store_diff_outcome_sync(
                    &tx,
                    &StoreDiffOutcomeParams {
                        diff_analysis_id: o.diff_analysis_id,
                        project_id: Some(pid),
                        outcome_type: o.outcome_type,
                        evidence_commit: Some(&o.evidence_commit),
                        evidence_message: Some(&o.evidence_message),
                        time_to_outcome_seconds: Some(o.time_to_outcome_seconds),
                        detected_by: "git_scan",
                    },
                )
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            }
            tx.commit().map_err(|e| anyhow::anyhow!("{}", e))?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .str_err()?;
        processed += count;
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
/// Uses pre-fetched commit data to avoid per-commit git subprocess calls.
/// Returns a map of original_commit -> (revert_commit, message, time_delta_seconds).
fn detect_reverts<'a>(
    commit_hashes: &'a [String],
    recent: &[CommitWithFiles],
    commit_map: &HashMap<&str, &CommitWithFiles>,
) -> HashMap<&'a str, (String, String, i64)> {
    let mut result = HashMap::new();

    if commit_hashes.is_empty() {
        return result;
    }

    for commit in recent {
        let msg_lower = commit.message.to_lowercase();

        // Match "revert" commits
        if !msg_lower.starts_with("revert") {
            continue;
        }

        // Check if the revert message references any of our tracked commits
        for hash in commit_hashes {
            // Check for full SHA or short SHA (first 7+ chars) in the revert message
            let short_hash = truncate_at_boundary(hash, 8);
            if commit.message.contains(hash.as_str()) || commit.message.contains(short_hash) {
                // Look up original commit timestamp from pre-fetched data
                if let Some(original) = commit_map.get(hash.as_str()) {
                    let time_delta = commit.timestamp - original.timestamp;
                    result.insert(
                        hash.as_str(),
                        (commit.hash.clone(), commit.message.clone(), time_delta),
                    );
                }
                break;
            }

            // Also check for "Revert \"<message>\"" pattern
            if let Some(original) = commit_map.get(hash.as_str()) {
                let quoted = format!("\"{}\"", original.message.trim());
                if commit.message.contains(&quoted) {
                    let time_delta = commit.timestamp - original.timestamp;
                    result.insert(
                        hash.as_str(),
                        (commit.hash.clone(), commit.message.clone(), time_delta),
                    );
                    break;
                }
            }
        }
    }

    result
}

/// Detect follow-up fixes: commits within FOLLOWUP_WINDOW_HOURS that touch the same files
/// and have fix-like messages.
/// Uses pre-fetched commit data for in-memory lookups instead of per-commit git calls.
/// Returns a map of original_commit -> vec of (fix_commit, message, time_delta_seconds).
fn detect_followup_fixes<'a>(
    diffs: &'a [(i64, String, String, Option<String>)],
    commit_map: &HashMap<&str, &CommitWithFiles>,
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

        // Look up original commit timestamp from pre-fetched data
        let commit_ts = match commit_map.get(to_commit.as_str()) {
            Some(c) => c.timestamp,
            None => continue,
        };

        let window_end = commit_ts + (FOLLOWUP_WINDOW_HOURS * 3600);

        // Scan all pre-fetched commits for follow-up fixes
        for candidate in commit_map.values() {
            // Must be after the original commit and within the time window
            if candidate.timestamp <= commit_ts || candidate.timestamp > window_end {
                continue;
            }
            if candidate.hash == *to_commit {
                continue;
            }
            if !is_fix_message(&candidate.message) {
                continue;
            }

            // Check if the fix touches any of the same files (using pre-fetched file list)
            let has_overlap = candidate.files.iter().any(|f| diff_files.contains(f));

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
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
