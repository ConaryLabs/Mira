// crates/mira-server/src/hooks/permission.rs
// Permission hook for Claude Code auto-approval

use crate::hooks::{read_hook_input, write_hook_output};
use anyhow::{Context, Result};

/// Run permission hook.
///
/// The permission_rules table was dropped (unused). This hook now always
/// passes through to let Claude Code handle permission decisions.
pub async fn run() -> Result<()> {
    // Read stdin so the hook protocol is satisfied
    let _input = read_hook_input().context("Failed to parse hook input from stdin")?;

    // No matching rule - let Claude Code handle it
    write_hook_output(&serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PermissionRequest"
        }
    }));
    Ok(())
}
