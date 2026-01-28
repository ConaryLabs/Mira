// crates/mira-server/src/background/briefings.rs
// Background worker for generating "What's New" project briefings

use crate::db::pool::DatabasePool;
use crate::db::{get_projects_for_briefing_check_sync, update_project_briefing_sync};
use crate::llm::{LlmClient, PromptBuilder, record_llm_usage};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

/// Check all projects for git changes and generate briefings
pub async fn process_briefings(
    pool: &Arc<DatabasePool>,
    client: &Arc<dyn LlmClient>,
) -> Result<usize, String> {
    let projects = pool
        .interact(move |conn| {
            get_projects_for_briefing_check_sync(conn)
                .map_err(|e| anyhow::anyhow!("Failed to get projects: {}", e))
        })
        .await
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
            client,
            pool,
            project_id,
        )
        .await;

        // Only update commit marker if briefing was successfully generated
        // This ensures failed briefings will be retried on next run
        if let Some(ref text) = briefing {
            let commit = current_commit.clone();
            let text_clone = text.clone();
            pool.interact(move |conn| {
                update_project_briefing_sync(conn, project_id, &commit, Some(&text_clone))
                    .map_err(|e| anyhow::anyhow!("Failed to update: {}", e))
            })
            .await
            .map_err(|e| e.to_string())?;
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

/// Check if a commit is an ancestor of HEAD (handles rebases, force pushes)
fn is_ancestor(project_path: &str, commit: &str) -> bool {
    Command::new("git")
        .args(["merge-base", "--is-ancestor", commit, "HEAD"])
        .current_dir(project_path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Max commits to include in briefing to prevent context overflow
const MAX_COMMITS: usize = 50;

/// Get git log between two commits (or recent commits if no base)
fn get_git_changes(project_path: &str, from_commit: Option<&str>) -> Option<String> {
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
    if stat.len() > 1000 {
        Some(format!("{}...", &stat[..1000]))
    } else {
        Some(stat)
    }
}

/// Generate a briefing using configured LLM provider
async fn generate_briefing(
    project_path: &str,
    from_commit: Option<&str>,
    _to_commit: &str,
    client: &Arc<dyn LlmClient>,
    pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Option<String> {
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

    let messages = PromptBuilder::for_briefings().build_messages(prompt);

    // Use configured background provider
    match client.chat(messages, None).await {
        Ok(result) => {
            // Record usage
            record_llm_usage(
                pool,
                client.provider_type(),
                &client.model_name(),
                "background:briefing",
                &result,
                Some(project_id),
                None,
            )
            .await;

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
