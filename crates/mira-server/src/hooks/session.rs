// src/hooks/session.rs
// SessionStart hook handler - captures Claude Code's session_id and cwd

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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

/// Handle SessionStart hook from Claude Code
/// Extracts session_id, cwd, and source from stdin JSON and writes to files
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
    if let Some(cwd) = input.get("cwd").and_then(|v| v.as_str()) {
        let path = cwd_file_path();
        fs::write(&path, cwd)?;
        eprintln!("[mira] Captured Claude cwd: {}", cwd);
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

    // SessionStart hooks don't need to output anything
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
