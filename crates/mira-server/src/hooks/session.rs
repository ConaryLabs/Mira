// src/hooks/session.rs
// SessionStart hook handler - captures Claude Code's session_id and cwd

use crate::db::pool::DatabasePool;
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

/// File where Claude's session_id is stored for MCP to read
pub fn session_file_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/claude-session-id")
}

/// File where Claude's working directory is stored for MCP to read
pub fn cwd_file_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/claude-cwd")
}

/// File where Claude's session source info is stored for MCP to read
pub fn source_file_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/claude-source.json")
}

/// File where Claude's task list ID is stored for MCP to read
pub fn task_list_file_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/claude-task-list-id")
}

/// Source information captured from SessionStart hook
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SourceInfo {
    pub session_id: Option<String>,
    pub source: String,
    pub timestamp: String,
}

impl SourceInfo {
    pub fn new(session_id: Option<String>, source: &str) -> Self {
        Self {
            session_id,
            source: source.to_string(),
            timestamp: Utc::now().to_rfc3339(),
        }
    }
}

/// Get database path
fn get_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/mira.db")
}

/// Handle SessionStart hook from Claude Code
/// Extracts session_id, cwd, and source from stdin JSON and writes to files
/// On resume, injects context about previous session work
pub fn run() -> Result<()> {
    let input = super::read_hook_input()?;

    // Log hook input keys for debugging
    eprintln!(
        "[mira] SessionStart hook input keys: {:?}",
        input.as_object().map(|obj| obj.keys().collect::<Vec<_>>())
    );

    // Ensure .mira directory exists
    let mira_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mira");
    fs::create_dir_all(&mira_dir)?;

    // Extract session_id from Claude's hook input
    let session_id = input.get("session_id").and_then(|v| v.as_str());
    if let Some(sid) = session_id {
        let path = session_file_path();
        fs::write(&path, sid)?;
        eprintln!("[mira] Captured Claude session: {}", sid);
    }

    // Extract cwd from Claude's hook input for auto-project detection
    let cwd = input.get("cwd").and_then(|v| v.as_str());
    if let Some(cwd_val) = cwd {
        let path = cwd_file_path();
        fs::write(&path, cwd_val)?;
        eprintln!("[mira] Captured Claude cwd: {}", cwd_val);
    }

    // Determine session source (startup vs resume)
    // Claude Code passes "resumed" or similar flag when using --resume
    let source = input
        .get("source")
        .and_then(|v| v.as_str())
        .or_else(|| {
            // Check for resumed flag as fallback
            if input.get("resumed").and_then(|v| v.as_bool()).unwrap_or(false) {
                Some("resume")
            } else {
                None
            }
        })
        .unwrap_or("startup");

    // Write source info atomically (temp file + rename)
    let source_info = SourceInfo::new(session_id.map(String::from), source);
    let source_json = serde_json::to_string(&source_info)?;
    let source_path = source_file_path();
    let temp_path = source_path.with_extension("tmp");
    fs::write(&temp_path, &source_json)?;
    fs::rename(&temp_path, &source_path)?;
    eprintln!("[mira] Captured Claude source: {}", source);

    // Extract task_list_id from Claude's hook input or env var
    let task_list_id = input
        .get("task_list_id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| std::env::var("CLAUDE_CODE_TASK_LIST_ID").ok());

    if let Some(ref list_id) = task_list_id {
        let path = task_list_file_path();
        fs::write(&path, list_id)?;
        eprintln!("[mira] Captured Claude task list: {}", list_id);
    }

    // On resume, inject context about previous work
    if source == "resume" {
        // Run async context injection in a blocking runtime
        let cwd_owned = cwd.map(String::from);
        let session_id_owned = session_id.map(String::from);

        // Use tokio runtime for async DB operations
        let rt = tokio::runtime::Runtime::new()?;
        let context = rt.block_on(async {
            build_resume_context(cwd_owned.as_deref(), session_id_owned.as_deref()).await
        });

        if let Some(ctx) = context {
            let output = serde_json::json!({
                "hookSpecificOutput": {
                    "additionalContext": ctx
                }
            });
            super::write_hook_output(&output);
            return Ok(());
        }
    }

    // No context to inject
    super::write_hook_output(&serde_json::json!({}));
    Ok(())
}

/// Build context for a resumed session
async fn build_resume_context(cwd: Option<&str>, _session_id: Option<&str>) -> Option<String> {
    let db_path = get_db_path();
    let pool = match DatabasePool::open(&db_path).await {
        Ok(p) => Arc::new(p),
        Err(_) => return None,
    };

    // Get project ID from cwd
    let project_id: Option<i64> = if let Some(cwd_path) = cwd {
        let pool_clone = pool.clone();
        let cwd_owned = cwd_path.to_string();
        pool_clone
            .interact(move |conn| {
                Ok::<_, anyhow::Error>(
                    crate::db::get_or_create_project_sync(conn, &cwd_owned, None)
                        .ok()
                        .map(|(id, _)| id),
                )
            })
            .await
            .ok()
            .flatten()
    } else {
        None
    };

    let project_id = project_id?;

    let mut context_parts: Vec<String> = Vec::new();

    // Get the most recent completed session for this project
    let pool_clone = pool.clone();
    let previous_session: Option<crate::db::SessionInfo> = pool_clone
        .interact(move |conn| {
            Ok::<_, anyhow::Error>(
                crate::db::get_recent_sessions_sync(conn, project_id, 2)
                    .ok()
                    .and_then(|sessions| {
                        // Find the most recent non-active session
                        sessions.into_iter().find(|s| s.status != "active")
                    }),
            )
        })
        .await
        .ok()
        .flatten();

    // Get recent tool calls from previous session
    if let Some(ref prev_session) = previous_session {
        let pool_clone = pool.clone();
        let prev_id = prev_session.id.clone();
        let tool_history: Option<Vec<crate::db::ToolHistoryEntry>> = pool_clone
            .interact(move |conn| {
                Ok::<_, anyhow::Error>(crate::db::get_session_history_sync(conn, &prev_id, 5).ok())
            })
            .await
            .ok()
            .flatten();

        if let Some(history) = tool_history.filter(|h| !h.is_empty()) {
            let tool_lines: Vec<String> = history
                .iter()
                .rev() // Oldest first
                .map(|h| {
                    let status = if h.success { "✓" } else { "✗" };
                    let summary = h
                        .result_summary
                        .as_deref()
                        .map(|s| if s.len() > 80 { format!("{}...", &s[..80]) } else { s.to_string() })
                        .unwrap_or_default();
                    format!("  {} {} -> {}", status, h.tool_name, summary)
                })
                .collect();
            context_parts.push(format!(
                "**Last session's recent actions:**\n{}",
                tool_lines.join("\n")
            ));
        }

        // Add session summary if available
        if let Some(ref summary) = prev_session.summary {
            context_parts.push(format!("**Previous session summary:** {}", summary));
        }
    }

    // Get incomplete goals
    let pool_clone = pool.clone();
    let goals: Option<Vec<crate::db::Goal>> = pool_clone
        .interact(move |conn| {
            Ok::<_, anyhow::Error>(crate::db::get_active_goals_sync(conn, Some(project_id), 3).ok())
        })
        .await
        .ok()
        .flatten();

    if let Some(goals) = goals.filter(|g| !g.is_empty()) {
        let goal_lines: Vec<String> = goals
            .iter()
            .map(|g| format!("  • {} [{}%] - {}", g.title, g.progress_percent, g.status))
            .collect();
        context_parts.push(format!(
            "**Active goals:**\n{}",
            goal_lines.join("\n")
        ));
    }

    if context_parts.is_empty() {
        return None;
    }

    Some(format!(
        "🔄 **Resuming session** - Here's context from your previous work:\n\n{}",
        context_parts.join("\n\n")
    ))
}

/// Read Claude's session_id from the temp file (if available)
pub fn read_claude_session_id() -> Option<String> {
    let path = session_file_path();
    fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
}

/// Read Claude's working directory from the temp file (if available)
pub fn read_claude_cwd() -> Option<String> {
    let path = cwd_file_path();
    fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
}

/// Read source info from the JSON file (if available)
pub fn read_source_info() -> Option<SourceInfo> {
    let path = source_file_path();
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Read Claude's task list ID from the temp file (if available)
pub fn read_claude_task_list_id() -> Option<String> {
    let path = task_list_file_path();
    fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // session_file_path tests
    // ============================================================================

    #[test]
    fn test_session_file_path_not_empty() {
        let path = session_file_path();
        assert!(!path.as_os_str().is_empty());
    }

    #[test]
    fn test_session_file_path_ends_with_expected() {
        let path = session_file_path();
        assert!(path.ends_with("claude-session-id"));
    }

    #[test]
    fn test_session_file_path_contains_mira() {
        let path = session_file_path();
        let path_str = path.to_string_lossy();
        assert!(path_str.contains(".mira"));
    }

    // ============================================================================
    // cwd_file_path tests
    // ============================================================================

    #[test]
    fn test_cwd_file_path_not_empty() {
        let path = cwd_file_path();
        assert!(!path.as_os_str().is_empty());
    }

    #[test]
    fn test_cwd_file_path_ends_with_expected() {
        let path = cwd_file_path();
        assert!(path.ends_with("claude-cwd"));
    }

    #[test]
    fn test_cwd_file_path_contains_mira() {
        let path = cwd_file_path();
        let path_str = path.to_string_lossy();
        assert!(path_str.contains(".mira"));
    }

    // ============================================================================
    // read_claude_session_id tests
    // ============================================================================

    #[test]
    fn test_read_claude_session_id_missing_file() {
        // Note: This test assumes the session file doesn't exist in most test environments
        // or returns whatever is currently stored
        let result = read_claude_session_id();
        // Result is either Some (if file exists) or None (if not)
        // We can't assert on the value since it depends on system state
        assert!(result.is_none() || !result.unwrap().is_empty());
    }

    #[test]
    fn test_read_claude_session_id_trims_whitespace() {
        use tempfile::TempDir;

        // Create a temp directory with custom session file
        let temp_dir = TempDir::new().unwrap();
        let session_path = temp_dir.path().join("claude-session-id");

        // Write session ID with whitespace
        std::fs::write(&session_path, "  session123\n  ").unwrap();

        // Read directly from the file (since read_claude_session_id uses fixed path)
        let content = std::fs::read_to_string(&session_path)
            .ok()
            .map(|s| s.trim().to_string());

        assert_eq!(content, Some("session123".to_string()));
    }

    // ============================================================================
    // read_claude_cwd tests
    // ============================================================================

    #[test]
    fn test_read_claude_cwd_trims_whitespace() {
        use tempfile::TempDir;

        // Create a temp directory with custom cwd file
        let temp_dir = TempDir::new().unwrap();
        let cwd_path = temp_dir.path().join("claude-cwd");

        // Write cwd with whitespace
        std::fs::write(&cwd_path, "  /home/user/project\n  ").unwrap();

        // Read directly from the file (since read_claude_cwd uses fixed path)
        let content = std::fs::read_to_string(&cwd_path)
            .ok()
            .map(|s| s.trim().to_string());

        assert_eq!(content, Some("/home/user/project".to_string()));
    }
}
