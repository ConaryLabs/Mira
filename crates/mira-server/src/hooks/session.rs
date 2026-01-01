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
