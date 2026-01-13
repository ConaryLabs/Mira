// crates/mira-server/src/background/briefings.rs
// Background worker for generating "What's New" project briefings

use crate::db::Database;
use crate::llm::DeepSeekClient;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

/// Check all projects for git changes and generate briefings
pub async fn process_briefings(
    db: &Arc<Database>,
    deepseek: &Arc<DeepSeekClient>,
) -> Result<usize, String> {
    let projects = db
        .get_projects_for_briefing_check()
        .map_err(|e| e.to_string())?;

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
            deepseek,
        )
        .await;

        // Store the briefing (even if None, we update the commit)
        db.update_project_briefing(project_id, &current_commit, briefing.as_deref())
            .map_err(|e| e.to_string())?;

        if briefing.is_some() {
            tracing::info!(
                "Generated briefing for project {} ({})",
                project_id,
                project_path
            );
            processed += 1;
        }
    }

    Ok(processed)
}

/// Get the current git HEAD commit hash
fn get_git_head(project_path: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(project_path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Get git log between two commits (or recent commits if no base)
fn get_git_changes(project_path: &str, from_commit: Option<&str>) -> Option<String> {
    let args = match from_commit {
        Some(from) => vec![
            "log".to_string(),
            "--oneline".to_string(),
            "--no-decorate".to_string(),
            format!("{}..HEAD", from),
        ],
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
    let args = match from_commit {
        Some(from) => vec![
            "diff".to_string(),
            "--stat".to_string(),
            "--stat-width=80".to_string(),
            format!("{}..HEAD", from),
        ],
        None => return None, // No diff for first briefing
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
    if stat.len() > 1000 {
        Some(format!("{}...", &stat[..1000]))
    } else {
        Some(stat)
    }
}

/// Generate a briefing using DeepSeek Reasoner
async fn generate_briefing(
    project_path: &str,
    from_commit: Option<&str>,
    _to_commit: &str,
    deepseek: &Arc<DeepSeekClient>,
) -> Option<String> {
    use crate::llm::Message;

    // Get git log
    let git_log = get_git_changes(project_path, from_commit)?;

    // Get file stats
    let file_stats = get_files_changed(project_path, from_commit);

    // Build context for DeepSeek
    let mut context = format!("Git commits:\n{}", git_log);
    if let Some(stats) = file_stats {
        context.push_str(&format!("\n\nFiles changed:\n{}", stats));
    }

    // Ask DeepSeek Reasoner to summarize
    let prompt = format!(
        r#"Summarize these git changes in 1-2 concise sentences for a developer returning to the project.
Focus on: what was done, which areas of code changed.
Be specific but brief. No preamble, just the summary.

{}

Summary:"#,
        context
    );

    let messages = vec![Message::user(prompt)];

    // Use reasoner (chat method uses deepseek-reasoner model)
    match deepseek.chat(messages, None).await {
        Ok(result) => {
            let summary = result.content.as_deref().unwrap_or("").trim().to_string();
            if summary.is_empty() {
                None
            } else {
                Some(summary)
            }
        }
        Err(e) => {
            tracing::warn!("Failed to generate briefing: {}", e);
            None
        }
    }
}
