// crates/mira-server/src/hooks/session/mod.rs
//! SessionStart hook handler - captures Claude Code's session_id and cwd

mod context;
mod team;

// Re-export public API
#[cfg(test)]
pub(crate) use context::{
    build_compaction_summary, build_working_on_summary, get_session_snapshot_sync,
};
pub(crate) use context::{build_resume_context, build_startup_context};
pub use team::{
    TeamDetectionResult, TeamMembership, cleanup_team_file, detect_team_membership,
    read_team_membership, read_team_membership_from_db, team_file_path_for_session,
    write_team_membership,
};

use crate::ipc::client::HookClient;
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Get the home directory, logging a warning if unavailable.
pub(super) fn home_dir_or_fallback() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| {
        eprintln!("[Mira] WARNING: HOME directory not set, using '.' as fallback");
        PathBuf::from(".")
    })
}

/// File where Claude's session_id is stored for MCP to read
pub fn session_file_path() -> PathBuf {
    home_dir_or_fallback().join(".mira/claude-session-id")
}

/// File where Claude's working directory is stored for MCP to read
pub fn cwd_file_path() -> PathBuf {
    home_dir_or_fallback().join(".mira/claude-cwd")
}

/// File where Claude's session source info is stored for MCP to read
pub fn source_file_path() -> PathBuf {
    home_dir_or_fallback().join(".mira/claude-source.json")
}

/// File where Claude's task list ID is stored for MCP to read
pub fn task_list_file_path() -> PathBuf {
    home_dir_or_fallback().join(".mira/claude-task-list-id")
}

/// Per-session directory path: `~/.mira/sessions/{session_id}/`
fn per_session_dir(session_id: &str) -> Option<PathBuf> {
    // Empty string passes `.chars().all()` vacuously — reject it explicitly
    if session_id.is_empty() {
        return None;
    }
    // Validate session_id to prevent path traversal
    if !session_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        tracing::warn!(
            "Invalid characters in session_id for per-session dir, skipping: {:?}",
            session_id
        );
        return None;
    }
    Some(home_dir_or_fallback().join(format!(".mira/sessions/{}/", session_id)))
}

/// Write a file to both the global path and the per-session directory.
/// The global file is written for backward compatibility; the per-session copy
/// provides session isolation. Per-session writes are best-effort.
fn write_global_and_per_session(
    global_path: &std::path::Path,
    session_id: Option<&str>,
    filename: &str,
    content: &str,
) -> std::io::Result<()> {
    // Always write the global file
    write_file_restricted(global_path, content)?;

    // Also write per-session copy when session_id is available
    if let Some(sid) = session_id
        && let Some(session_dir) = per_session_dir(sid)
    {
        if let Err(e) = fs::create_dir_all(&session_dir) {
            tracing::warn!("Failed to create per-session dir: {e}");
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&session_dir, fs::Permissions::from_mode(0o700));
            }
            let per_session_path = session_dir.join(filename);
            if let Err(e) = write_file_restricted(&per_session_path, content) {
                tracing::warn!("Failed to write per-session file {filename}: {e}");
            }
        }
    }

    Ok(())
}

/// Marker file path for tracking whether goals were already shown this session.
/// Other hooks can check this to avoid re-injecting goals.
/// When session_id is provided, uses a per-session path to avoid cross-session clobbering.
fn goals_shown_path_for(session_id: Option<&str>) -> PathBuf {
    if let Some(sid) = session_id
        && let Some(session_dir) = per_session_dir(sid)
    {
        return session_dir.join("goals_shown");
    }
    // Fallback to global tmp path
    home_dir_or_fallback().join(".mira/tmp/goals_shown")
}

/// Write a file with restricted permissions (0o600 on Unix).
/// Used for sensitive session files (session ID, cwd, task list ID, goals marker).
pub(super) fn write_file_restricted(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    f.write_all(content.as_bytes())
}

/// Mark that goals have been injected into the session context.
/// Called by SessionStart after injecting goals, so other hooks can skip them.
pub(super) fn mark_goals_shown(session_id: Option<&str>) {
    let path = goals_shown_path_for(session_id);
    if let Some(parent) = path.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        tracing::debug!("Failed to create goals_shown dir: {e}");
    }
    if let Err(e) = write_file_restricted(&path, &Utc::now().to_rfc3339()) {
        tracing::debug!("Failed to write goals_shown marker: {e}");
    }
}

/// Check whether goals have already been shown this session.
/// Returns true if goals were shown within the last 30 minutes
/// (stale markers from crashed sessions are ignored).
pub fn were_goals_shown(session_id: Option<&str>) -> bool {
    let path = goals_shown_path_for(session_id);
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    // Parse the timestamp and check if it's recent (within 30 minutes)
    if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(content.trim()) {
        let age = Utc::now().signed_duration_since(ts);
        age.num_minutes() < 30
    } else {
        false
    }
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

/// Handle SessionStart hook from Claude Code
/// Extracts session_id, cwd, and source from stdin JSON and writes to files
/// On resume, injects context about previous session work
pub async fn run() -> Result<()> {
    let input = super::read_hook_input().context("Failed to parse hook input from stdin")?;

    // Log hook input keys for debugging
    tracing::debug!(
        keys = ?input.as_object().map(|obj| obj.keys().collect::<Vec<_>>()),
        "SessionStart hook input keys"
    );

    // Ensure .mira directory exists
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            eprintln!("[Mira] WARNING: HOME directory not set, skipping SessionStart hook");
            super::write_hook_output(&serde_json::json!({}));
            return Ok(());
        }
    };
    let mira_dir = home.join(".mira");
    fs::create_dir_all(&mira_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&mira_dir, fs::Permissions::from_mode(0o700));
    }

    // Extract session_id from Claude's hook input
    let session_id = input.get("session_id").and_then(|v| v.as_str());
    if let Some(sid) = session_id {
        // Create per-session directory early so subsequent writes can use it
        if let Some(session_dir) = per_session_dir(sid) {
            if let Err(e) = fs::create_dir_all(&session_dir) {
                tracing::debug!("Failed to create per-session dir: {e}");
            } else {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(&session_dir, fs::Permissions::from_mode(0o700));
                }
            }
        }
        write_global_and_per_session(&session_file_path(), Some(sid), "claude-session-id", sid)?;
        tracing::debug!(session_id = sid, "Captured Claude session");
    }

    // Extract cwd from Claude's hook input for auto-project detection
    let cwd = input.get("cwd").and_then(|v| v.as_str());
    if let Some(cwd_val) = cwd {
        write_global_and_per_session(&cwd_file_path(), session_id, "claude-cwd", cwd_val)?;
        tracing::debug!(cwd = cwd_val, "Captured Claude cwd");
    }

    // Determine session source (startup vs resume)
    // Claude Code passes "resumed" or similar flag when using --resume
    let source = input
        .get("source")
        .and_then(|v| v.as_str())
        .or_else(|| {
            // Check for resumed flag as fallback
            if input
                .get("resumed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                Some("resume")
            } else {
                None
            }
        })
        .unwrap_or("startup");

    // Write source info atomically (temp file + rename) for global path
    let source_info = SourceInfo::new(session_id.map(String::from), source);
    let source_json = serde_json::to_string(&source_info)?;
    let source_path = source_file_path();
    let temp_path = source_path.with_extension("tmp");
    write_file_restricted(&temp_path, &source_json)?;
    fs::rename(&temp_path, &source_path)?;
    // Also write per-session copy (non-atomic is fine for this)
    if let Some(sid) = session_id
        && let Some(session_dir) = per_session_dir(sid)
    {
        let per_session_path = session_dir.join("claude-source.json");
        if let Err(e) = write_file_restricted(&per_session_path, &source_json) {
            tracing::debug!("Failed to write per-session source file: {e}");
        }
    }
    tracing::debug!(source = source, "Captured Claude source");

    // Extract task_list_id from Claude's hook input or env var
    let task_list_id = input
        .get("task_list_id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| std::env::var("CLAUDE_CODE_TASK_LIST_ID").ok());

    if let Some(ref list_id) = task_list_id {
        write_global_and_per_session(
            &task_list_file_path(),
            session_id,
            "claude-task-list-id",
            list_id,
        )?;
        tracing::debug!(task_list_id = %list_id, "Captured Claude task list");
    }

    // Connect to MCP server via IPC (falls back to direct DB)
    let mut client = HookClient::connect().await;

    // Register session in DB so the background loop sees activity
    if let (Some(sid), Some(cwd_val)) = (session_id, cwd) {
        match client.register_session(sid, cwd_val, source).await {
            Some(pid) => tracing::debug!(project_id = pid, "Session registered in DB"),
            None => tracing::warn!("Failed to register session"),
        }
    }

    // Detect team membership and register in DB
    if let Some(sid) = session_id {
        let detection = detect_team_membership(&input, Some(sid), cwd);
        if let Some(det) = detection {
            tracing::info!(
                team = %det.team_name,
                role = %det.role,
                member = %det.member_name,
                "Team detected"
            );

            let membership = async {
                let team_id = client
                    .register_team_session(
                        &det.team_name,
                        &det.config_path,
                        &det.member_name,
                        &det.role,
                        det.agent_type.as_deref(),
                        sid,
                        cwd,
                    )
                    .await?;
                Some(TeamMembership {
                    team_id,
                    team_name: det.team_name.clone(),
                    member_name: det.member_name.clone(),
                    role: det.role.clone(),
                    config_path: det.config_path.clone(),
                })
            }
            .await;

            if let Some(ref m) = membership {
                if let Err(e) = write_team_membership(sid, m) {
                    tracing::warn!("failed to write team membership: {e}");
                }
                tracing::debug!(team_id = m.team_id, "Team session registered");
            }
        }
    }

    // Inject context about previous work via IPC
    let cwd_owned = cwd.map(String::from);
    let session_id_owned = session_id.map(String::from);

    let context = if source == "resume" {
        client
            .get_resume_context(cwd_owned.as_deref(), session_id_owned.as_deref())
            .await
    } else {
        client
            .get_startup_context(cwd_owned.as_deref(), session_id_owned.as_deref())
            .await
    };

    if let Some(ctx) = context {
        let output = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "SessionStart",
                "additionalContext": ctx
            }
        });
        super::write_hook_output(&output);
    } else {
        super::write_hook_output(&serde_json::json!({}));
    }
    Ok(())
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

/// Remove the per-session directory and all its contents.
/// Best-effort: logs a warning on failure but never panics.
pub(crate) fn cleanup_per_session_dir(session_id: &str) {
    if let Some(dir) = per_session_dir(session_id)
        && dir.exists()
        && let Err(e) = fs::remove_dir_all(&dir)
    {
        tracing::warn!("Failed to clean up per-session dir: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use context::infer_activity_from_tools;

    // ============================================================================
    // read_claude_session_id tests
    // ============================================================================

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

    // ============================================================================
    // build_working_on_summary tests
    // ============================================================================

    #[test]
    fn test_build_working_on_summary_with_edit_tools_and_files() {
        let snapshot = serde_json::json!({
            "tool_count": 15,
            "top_tools": [
                {"name": "Edit", "count": 8},
                {"name": "Read", "count": 5},
            ],
            "files_modified": ["/home/user/project/src/main.rs", "/home/user/project/src/lib.rs"],
        });
        let result = build_working_on_summary(&snapshot);
        assert!(result.is_some());
        let summary = result.unwrap();
        assert!(summary.contains("code editing"), "got: {}", summary);
        assert!(summary.contains("main.rs"), "got: {}", summary);
        assert!(summary.contains("lib.rs"), "got: {}", summary);
    }

    #[test]
    fn test_build_working_on_summary_with_bash_tools() {
        let snapshot = serde_json::json!({
            "tool_count": 5,
            "top_tools": [
                {"name": "Bash", "count": 4},
            ],
            "files_modified": [],
        });
        let result = build_working_on_summary(&snapshot);
        assert!(result.is_some());
        assert!(result.unwrap().contains("running commands"));
    }

    #[test]
    fn test_build_working_on_summary_empty_snapshot() {
        let snapshot = serde_json::json!({
            "tool_count": 0,
            "top_tools": [],
            "files_modified": [],
        });
        let result = build_working_on_summary(&snapshot);
        assert!(result.is_none());
    }

    #[test]
    fn test_build_working_on_summary_fallback_to_tool_count() {
        let snapshot = serde_json::json!({
            "tool_count": 10,
            "top_tools": [
                {"name": "SomeUnknownTool", "count": 10},
            ],
            "files_modified": [],
        });
        let result = build_working_on_summary(&snapshot);
        assert!(result.is_some());
        assert!(result.unwrap().contains("10 tool calls"));
    }

    // ============================================================================
    // infer_activity_from_tools tests
    // ============================================================================

    #[test]
    fn test_infer_activity_edit() {
        assert_eq!(infer_activity_from_tools(&["Edit", "Read"]), "code editing");
    }

    #[test]
    fn test_infer_activity_write() {
        assert_eq!(infer_activity_from_tools(&["Write"]), "code editing");
    }

    #[test]
    fn test_infer_activity_bash() {
        assert_eq!(infer_activity_from_tools(&["Bash"]), "running commands");
    }

    #[test]
    fn test_infer_activity_exploration() {
        assert_eq!(
            infer_activity_from_tools(&["Read", "Glob"]),
            "code exploration"
        );
    }

    #[test]
    fn test_infer_activity_unknown() {
        assert_eq!(infer_activity_from_tools(&["SomeTool"]), "");
    }

    // ============================================================================
    // build_compaction_summary tests
    // ============================================================================

    #[test]
    fn test_compaction_summary_all_categories() {
        let snapshot = serde_json::json!({
            "tool_count": 10,
            "compaction_context": {
                "user_intent": "Refactor the database layer",
                "decisions": ["chose builder pattern for Config"],
                "pending_tasks": ["add validation for user input"],
                "issues": ["connection refused when connecting to database"],
                "active_work": ["working on database migration"],
                "files_referenced": ["src/db.rs", "src/main.rs"]
            }
        });
        let result = build_compaction_summary(&snapshot);
        assert!(result.is_some());
        let summary = result.unwrap();
        assert!(
            summary.contains("Pre-compaction context:"),
            "got: {}",
            summary
        );
        assert!(
            summary.contains("Original request: Refactor the database layer"),
            "got: {}",
            summary
        );
        assert!(summary.contains("Decisions:"), "got: {}", summary);
        assert!(summary.contains("builder pattern"), "got: {}", summary);
        assert!(summary.contains("Active work:"), "got: {}", summary);
        assert!(summary.contains("Issues:"), "got: {}", summary);
        assert!(summary.contains("Remaining tasks:"), "got: {}", summary);
        assert!(
            summary.contains("Files discussed: src/db.rs, src/main.rs"),
            "got: {}",
            summary
        );

        // Verify ordering: user_intent before decisions, decisions before active_work,
        // active_work before issues, issues before pending, pending before files
        let intent_pos = summary.find("Original request:").unwrap();
        let decisions_pos = summary.find("Decisions:").unwrap();
        let active_pos = summary.find("Active work:").unwrap();
        let issues_pos = summary.find("Issues:").unwrap();
        let pending_pos = summary.find("Remaining tasks:").unwrap();
        let files_pos = summary.find("Files discussed:").unwrap();
        assert!(
            intent_pos < decisions_pos,
            "intent should come before decisions"
        );
        assert!(
            decisions_pos < active_pos,
            "decisions should come before active work"
        );
        assert!(
            active_pos < issues_pos,
            "active work should come before issues"
        );
        assert!(
            issues_pos < pending_pos,
            "issues should come before pending"
        );
        assert!(pending_pos < files_pos, "pending should come before files");
    }

    #[test]
    fn test_compaction_summary_partial_categories() {
        let snapshot = serde_json::json!({
            "compaction_context": {
                "decisions": ["chose SQLite"],
                "pending_tasks": [],
                "issues": [],
                "active_work": []
            }
        });
        let result = build_compaction_summary(&snapshot);
        assert!(result.is_some());
        let summary = result.unwrap();
        assert!(summary.contains("Decisions:"), "got: {}", summary);
        assert!(!summary.contains("Remaining tasks:"), "got: {}", summary);
        assert!(!summary.contains("Issues:"), "got: {}", summary);
    }

    #[test]
    fn test_compaction_summary_none_when_absent() {
        let snapshot = serde_json::json!({
            "tool_count": 5,
            "top_tools": [],
        });
        let result = build_compaction_summary(&snapshot);
        assert!(result.is_none());
    }

    #[test]
    fn test_compaction_summary_none_when_all_empty() {
        let snapshot = serde_json::json!({
            "compaction_context": {
                "decisions": [],
                "pending_tasks": [],
                "issues": [],
                "active_work": []
            }
        });
        let result = build_compaction_summary(&snapshot);
        assert!(result.is_none());
    }

    // ============================================================================
    // per_session_dir tests (M1)
    // ============================================================================

    #[test]
    fn test_per_session_dir_empty_string() {
        assert!(
            per_session_dir("").is_none(),
            "empty string should return None"
        );
    }

    #[test]
    fn test_per_session_dir_valid_id() {
        let result = per_session_dir("abc-123");
        assert!(result.is_some());
        let path = result.unwrap();
        assert!(path.to_string_lossy().contains("sessions/abc-123"));
    }

    #[test]
    fn test_per_session_dir_invalid_chars() {
        // Path traversal attempt
        assert!(per_session_dir("../etc/passwd").is_none());
        // Slash
        assert!(per_session_dir("foo/bar").is_none());
        // Spaces
        assert!(per_session_dir("foo bar").is_none());
    }

    #[test]
    fn test_cleanup_per_session_dir_nonexistent() {
        // Should not panic when the directory doesn't exist
        cleanup_per_session_dir("nonexistent-session-id-12345");
    }

    #[test]
    fn test_compaction_summary_limits_items() {
        let snapshot = serde_json::json!({
            "compaction_context": {
                "decisions": ["d1", "d2", "d3", "d4", "d5"],
                "pending_tasks": [],
                "issues": [],
                "active_work": []
            }
        });
        let result = build_compaction_summary(&snapshot);
        assert!(result.is_some());
        let summary = result.unwrap();
        // take(3) limits to 3 decisions shown
        assert!(summary.contains("d1"), "got: {}", summary);
        assert!(summary.contains("d3"), "got: {}", summary);
        assert!(!summary.contains("d4"), "got: {}", summary);
    }

    #[test]
    fn test_compaction_summary_user_intent_only() {
        let snapshot = serde_json::json!({
            "compaction_context": {
                "user_intent": "Implement caching for API responses",
                "decisions": [],
                "pending_tasks": [],
                "issues": [],
                "active_work": []
            }
        });
        let result = build_compaction_summary(&snapshot);
        assert!(result.is_some());
        let summary = result.unwrap();
        assert!(
            summary.contains("Original request: Implement caching for API responses"),
            "got: {}",
            summary
        );
    }

    #[test]
    fn test_compaction_summary_empty_user_intent_skipped() {
        let snapshot = serde_json::json!({
            "compaction_context": {
                "user_intent": "",
                "decisions": ["chose SQLite"],
                "pending_tasks": [],
                "issues": [],
                "active_work": []
            }
        });
        let result = build_compaction_summary(&snapshot);
        assert!(result.is_some());
        let summary = result.unwrap();
        assert!(
            !summary.contains("Original request:"),
            "empty intent should be skipped, got: {}",
            summary
        );
        assert!(summary.contains("Decisions:"), "got: {}", summary);
    }

    #[test]
    fn test_compaction_summary_files_referenced() {
        let snapshot = serde_json::json!({
            "compaction_context": {
                "decisions": [],
                "pending_tasks": [],
                "issues": [],
                "active_work": [],
                "files_referenced": ["src/main.rs", "src/lib.rs", "Cargo.toml"]
            }
        });
        let result = build_compaction_summary(&snapshot);
        assert!(result.is_some());
        let summary = result.unwrap();
        assert!(
            summary.contains("Files discussed: src/main.rs, src/lib.rs, Cargo.toml"),
            "got: {}",
            summary
        );
    }

    #[test]
    fn test_compaction_summary_files_referenced_limits_to_8() {
        let snapshot = serde_json::json!({
            "compaction_context": {
                "decisions": [],
                "pending_tasks": [],
                "issues": [],
                "active_work": [],
                "files_referenced": [
                    "f1.rs", "f2.rs", "f3.rs", "f4.rs",
                    "f5.rs", "f6.rs", "f7.rs", "f8.rs",
                    "f9.rs", "f10.rs"
                ]
            }
        });
        let result = build_compaction_summary(&snapshot);
        assert!(result.is_some());
        let summary = result.unwrap();
        assert!(
            summary.contains("f8.rs"),
            "should include 8th file, got: {}",
            summary
        );
        assert!(
            !summary.contains("f9.rs"),
            "should exclude 9th file, got: {}",
            summary
        );
    }

    #[test]
    fn test_compaction_summary_backward_compat_old_snapshot() {
        // Old snapshots without user_intent or files_referenced should still work
        let snapshot = serde_json::json!({
            "compaction_context": {
                "decisions": ["chose builder pattern"],
                "pending_tasks": ["finish tests"],
                "issues": [],
                "active_work": ["refactoring config"]
            }
        });
        let result = build_compaction_summary(&snapshot);
        assert!(result.is_some());
        let summary = result.unwrap();
        assert!(summary.contains("Decisions:"), "got: {}", summary);
        assert!(summary.contains("Active work:"), "got: {}", summary);
        assert!(summary.contains("Remaining tasks:"), "got: {}", summary);
        assert!(
            !summary.contains("Original request:"),
            "missing field should be skipped, got: {}",
            summary
        );
        assert!(
            !summary.contains("Files discussed:"),
            "missing field should be skipped, got: {}",
            summary
        );
    }

    #[test]
    fn test_compaction_summary_null_new_fields_backward_compat() {
        // Explicitly null values for new fields (as serde(default) would produce)
        let snapshot = serde_json::json!({
            "compaction_context": {
                "user_intent": null,
                "decisions": ["d1"],
                "pending_tasks": [],
                "issues": [],
                "active_work": [],
                "files_referenced": null
            }
        });
        let result = build_compaction_summary(&snapshot);
        assert!(result.is_some());
        let summary = result.unwrap();
        assert!(summary.contains("Decisions: d1"), "got: {}", summary);
        assert!(
            !summary.contains("Original request:"),
            "null intent should be skipped, got: {}",
            summary
        );
        assert!(
            !summary.contains("Files discussed:"),
            "null files should be skipped, got: {}",
            summary
        );
    }
}
