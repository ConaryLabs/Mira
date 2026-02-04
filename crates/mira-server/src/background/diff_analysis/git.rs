// background/diff_analysis/git.rs
// Git operations for diff analysis

use super::types::DiffStats;
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

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

/// Parse git numstat output into DiffStats
///
/// Shared parser for all git diff --numstat variants (commit ranges, staged, working).
pub fn parse_numstat_output(stdout: &str) -> DiffStats {
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
    stats
}

/// Parse diff statistics using git diff --numstat between two refs.
///
/// Prefer `derive_stats_from_unified_diff` when a unified diff is already available
/// to avoid spawning an extra git process.
pub fn parse_diff_stats(
    project_path: &Path,
    from_ref: &str,
    to_ref: &str,
) -> Result<DiffStats, String> {
    let output = Command::new("git")
        .args(["diff", "--numstat", from_ref, to_ref])
        .current_dir(project_path)
        .output()
        .map_err(|e| format!("Failed to run git diff --numstat: {}", e))?;

    if !output.status.success() {
        return Ok(DiffStats::default());
    }

    Ok(parse_numstat_output(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

/// Derive diff statistics directly from a unified diff string.
///
/// Avoids spawning a second `git diff --numstat` process when the unified diff
/// is already available. Counts added/removed lines from `+`/`-` prefixed lines
/// and extracts file paths from `diff --git` headers.
pub fn derive_stats_from_unified_diff(diff: &str) -> DiffStats {
    let mut stats = DiffStats::default();
    let mut seen_files = HashSet::new();

    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            if let Some(b_part) = line.split(" b/").last() {
                if seen_files.insert(b_part.to_string()) {
                    stats.files.push(b_part.to_string());
                }
            }
        } else if line.starts_with('+') && !line.starts_with("+++") {
            stats.lines_added += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            stats.lines_removed += 1;
        }
    }

    stats.files_changed = stats.files.len() as i64;
    stats
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
