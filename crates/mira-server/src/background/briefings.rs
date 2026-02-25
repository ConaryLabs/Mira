// crates/mira-server/src/background/briefings.rs
// Background worker for generating "What's New" project briefings

use super::HEURISTIC_PREFIX;
use crate::db::pool::DatabasePool;
use crate::db::{get_projects_for_briefing_check_sync, update_project_briefing_sync};
use crate::utils::{truncate, truncate_at_boundary};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

/// Max commits to display in heuristic fallback
const FALLBACK_MAX_COMMITS: usize = 5;
/// Max total output length for heuristic briefing
const FALLBACK_MAX_LENGTH: usize = 200;

/// Check all projects for git changes and generate briefings
pub async fn process_briefings(
    pool: &Arc<DatabasePool>,
) -> Result<usize, String> {
    let projects = pool.run(get_projects_for_briefing_check_sync).await?;

    let mut processed = 0;

    for (project_id, project_path, last_known_commit) in projects {
        // Check if project path exists
        if !Path::new(&project_path).exists() {
            continue;
        }

        // Get current git HEAD
        let current_commit = match get_git_head(&project_path) {
            Some(commit) => commit,
            None => continue, // Not a git repo or error
        };

        // Check if we need to generate a briefing
        let needs_briefing = match &last_known_commit {
            Some(known) => known != &current_commit,
            None => true, // First time seeing this project
        };

        if !needs_briefing {
            continue;
        }

        // Generate the briefing
        let briefing = generate_briefing(
            &project_path,
            last_known_commit.as_deref(),
            &current_commit,
        )
        .await;

        // Only update commit marker if briefing was successfully generated
        // This ensures failed briefings will be retried on next run
        if let Some(ref text) = briefing {
            let commit = current_commit.clone();
            let text_clone = text.clone();
            pool.run(move |conn| {
                update_project_briefing_sync(conn, project_id, &commit, Some(&text_clone))
            })
            .await?;
            tracing::info!(
                "Generated briefing for project {} ({})",
                project_id,
                project_path
            );
            processed += 1;
        } else {
            tracing::debug!(
                "Briefing generation failed for project {}, will retry next run",
                project_id
            );
        }
    }

    Ok(processed)
}

use crate::git::{get_git_head, is_ancestor, validate_ref};

/// Max commits to include in briefing to prevent context overflow
const MAX_COMMITS: usize = 50;

/// Get git log between two commits (or recent commits if no base)
fn get_git_changes(project_path: &str, from_commit: Option<&str>) -> Option<String> {
    if let Some(from) = from_commit {
        validate_ref(from).ok()?;
    }
    let args = match from_commit {
        Some(from) if is_ancestor(project_path, from) => {
            // Verified ancestor - safe to use range, but limit to MAX_COMMITS
            vec![
                "log".to_string(),
                "--oneline".to_string(),
                "--no-decorate".to_string(),
                format!("-{}", MAX_COMMITS),
                format!("{}..HEAD", from),
            ]
        }
        Some(_) => {
            // Not an ancestor (rebase/force push) - fall back to recent commits
            tracing::debug!("from_commit not an ancestor of HEAD, using recent commits");
            vec![
                "log".to_string(),
                "--oneline".to_string(),
                "--no-decorate".to_string(),
                format!("-{}", MAX_COMMITS),
            ]
        }
        None => vec![
            "log".to_string(),
            "--oneline".to_string(),
            "--no-decorate".to_string(),
            "-10".to_string(), // Last 10 commits for first briefing
        ],
    };

    let output = Command::new("git")
        .args(&args)
        .current_dir(project_path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let log = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if log.is_empty() {
        return None;
    }

    Some(log)
}

/// Get a summary of files changed
fn get_files_changed(project_path: &str, from_commit: Option<&str>) -> Option<String> {
    if let Some(from) = from_commit {
        validate_ref(from).ok()?;
    }
    let args = match from_commit {
        Some(from) if is_ancestor(project_path, from) => vec![
            "diff".to_string(),
            "--stat".to_string(),
            "--stat-width=80".to_string(),
            format!("{}..HEAD", from),
        ],
        _ => return None, // No diff for first briefing or invalid ancestor
    };

    let output = Command::new("git")
        .args(&args)
        .current_dir(project_path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stat = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stat.is_empty() {
        return None;
    }

    // Truncate if too long
    Some(truncate(&stat, 1000))
}

/// Generate a heuristic briefing from git log
async fn generate_briefing(
    project_path: &str,
    from_commit: Option<&str>,
    _to_commit: &str,
) -> Option<String> {
    // Run blocking git calls on the blocking threadpool so they don't stall the
    // async runtime and can be cancelled by the slow-lane task timeout.
    let path = project_path.to_string();
    let from = from_commit.map(|s| s.to_string());
    let (git_log, file_stats) = tokio::task::spawn_blocking(move || {
        let log = get_git_changes(&path, from.as_deref());
        let stats = get_files_changed(&path, from.as_deref());
        (log, stats)
    })
    .await
    .ok()?;

    Some(generate_briefing_fallback(
        git_log.as_deref(),
        file_stats.as_deref(),
    ))
}

/// Generate a heuristic briefing from git log (no LLM required)
fn generate_briefing_fallback(git_log: Option<&str>, file_stats: Option<&str>) -> String {
    let git_log = match git_log {
        Some(log) if !log.is_empty() => log,
        _ => return format!("{}No new git activity", HEURISTIC_PREFIX),
    };

    let lines: Vec<&str> = git_log.lines().collect();
    let total_commits = lines.len();

    // Extract first commit message -- strip hash prefix, truncate at 72 chars
    let latest_msg = lines
        .first()
        .map(|line| {
            // Oneline format: "abc1234 commit message"
            let msg = line
                .find(' ')
                .map(|i| &line[i + 1..])
                .unwrap_or(line)
                .trim();
            // Strip newlines and truncate
            let msg = msg.replace('\n', " ");
            truncate(&msg, 69)
        })
        .unwrap_or_default();

    // Parse diff --stat summary line for insertions/deletions
    let stat_summary = file_stats.and_then(|stats| {
        // Last line of diff --stat looks like: " 22 files changed, 500 insertions(+), 100 deletions(-)"
        stats.lines().last().and_then(|last_line| {
            if last_line.contains("changed") {
                Some(last_line.trim().to_string())
            } else {
                None
            }
        })
    });

    let mut result = format!(
        "{}{} commit{}. Latest: {}",
        HEURISTIC_PREFIX,
        total_commits,
        if total_commits == 1 { "" } else { "s" },
        latest_msg,
    );

    if total_commits > FALLBACK_MAX_COMMITS {
        result.push_str(&format!(
            " (+{} more)",
            total_commits - FALLBACK_MAX_COMMITS
        ));
    }

    if let Some(stats) = stat_summary {
        result.push_str(&format!(". {}", stats));
    }

    // Cap total output length (UTF-8 safe)
    if result.len() > FALLBACK_MAX_LENGTH {
        result = format!(
            "{}...",
            truncate_at_boundary(&result, FALLBACK_MAX_LENGTH - 3)
        );
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_briefing_fallback_with_commits() {
        let log = "abc1234 feat: add nucleo fuzzy fallback\ndef5678 fix: handle empty results";
        let result = generate_briefing_fallback(Some(log), None);
        assert!(result.starts_with(HEURISTIC_PREFIX));
        assert!(result.contains("2 commits"));
        assert!(result.contains("feat: add nucleo fuzzy fallback"));
    }

    #[test]
    fn test_briefing_fallback_single_commit() {
        let log = "abc1234 fix: typo";
        let result = generate_briefing_fallback(Some(log), None);
        assert!(result.contains("1 commit."));
        assert!(!result.contains("commits"));
    }

    #[test]
    fn test_briefing_fallback_no_git() {
        let result = generate_briefing_fallback(None, None);
        assert!(result.starts_with(HEURISTIC_PREFIX));
        assert!(result.contains("No new git activity"));
    }

    #[test]
    fn test_briefing_fallback_empty_log() {
        let result = generate_briefing_fallback(Some(""), None);
        assert!(result.contains("No new git activity"));
    }

    #[test]
    fn test_briefing_fallback_with_stats() {
        let log = "abc1234 feat: add feature";
        let stats = " src/main.rs | 10 +++\n 1 file changed, 10 insertions(+)";
        let result = generate_briefing_fallback(Some(log), Some(stats));
        assert!(result.contains("1 file changed"));
    }

    #[test]
    fn test_briefing_fallback_many_commits() {
        let mut log = String::new();
        for i in 0..10 {
            if i > 0 {
                log.push('\n');
            }
            log.push_str(&format!("abc{:04} commit {}", i, i));
        }
        let result = generate_briefing_fallback(Some(&log), None);
        assert!(result.contains("10 commits"));
        assert!(result.contains("(+5 more)"));
    }

    #[test]
    fn test_briefing_fallback_long_message_truncated() {
        let long_msg = format!("abc1234 {}", "a".repeat(100));
        let result = generate_briefing_fallback(Some(&long_msg), None);
        assert!(result.len() <= FALLBACK_MAX_LENGTH);
    }
}
