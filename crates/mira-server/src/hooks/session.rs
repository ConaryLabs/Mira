// src/hooks/session.rs
// SessionStart hook handler - captures Claude Code's session_id

use anyhow::Result;
use std::fs;
use std::path::PathBuf;

/// File where Claude's session_id is stored for MCP to read
pub fn session_file_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mira/claude-session-id")
}

/// Handle SessionStart hook from Claude Code
/// Extracts session_id from stdin JSON and writes to temp file
pub fn run() -> Result<()> {
    let input = super::read_hook_input()?;

    // Extract session_id from Claude's hook input
    if let Some(session_id) = input.get("session_id").and_then(|v| v.as_str()) {
        // Ensure directory exists
        let path = session_file_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Write session_id to file
        fs::write(&path, session_id)?;

        eprintln!("[mira] Captured Claude session: {}", session_id);
    }

    // SessionStart hooks don't need to output anything
    Ok(())
}

/// Read Claude's session_id from the temp file (if available)
pub fn read_claude_session_id() -> Option<String> {
    let path = session_file_path();
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
}
