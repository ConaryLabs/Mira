// crates/mira-server/src/hooks/teammate_idle.rs
// Hook handler for TeammateIdle events - monitors team member activity

use crate::hooks::{HookTimer, read_hook_input, write_hook_output};
use anyhow::{Context, Result};

/// TeammateIdle hook input from Claude Code
#[derive(Debug)]
struct TeammateIdleInput {
    session_id: String,
    teammate_name: String,
    team_name: String,
}

impl TeammateIdleInput {
    fn from_json(json: &serde_json::Value) -> Self {
        Self {
            session_id: json
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            teammate_name: json
                .get("teammate_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            team_name: json
                .get("team_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }
    }
}

/// Run TeammateIdle hook
///
/// This hook fires when an Agent Teams member is about to go idle.
/// For v1, we just log the event to session_behavior_log for tracking.
pub async fn run() -> Result<()> {
    let _timer = HookTimer::start("TeammateIdle");
    let input = read_hook_input().context("Failed to parse hook input from stdin")?;
    let idle_input = TeammateIdleInput::from_json(&input);

    tracing::debug!(
        teammate = %idle_input.teammate_name,
        team = %idle_input.team_name,
        "TeammateIdle hook triggered"
    );

    // Connect to MCP server via IPC (falls back to direct DB if server unavailable)
    let mut client = crate::ipc::client::HookClient::connect().await;

    // Get current project
    let Some((project_id, _)) = client.resolve_project(None).await else {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    // Log idle event
    let data = serde_json::json!({
        "behavior_type": "teammate_idle",
        "teammate_name": idle_input.teammate_name,
        "team_name": idle_input.team_name,
    });
    client
        .log_behavior(&idle_input.session_id, project_id, "tool_use", data)
        .await;

    write_hook_output(&serde_json::json!({}));
    Ok(())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_input_parses_all_fields() {
        let input = TeammateIdleInput::from_json(&serde_json::json!({
            "session_id": "sess-1",
            "teammate_name": "researcher",
            "team_name": "dev-team"
        }));
        assert_eq!(input.session_id, "sess-1");
        assert_eq!(input.teammate_name, "researcher");
        assert_eq!(input.team_name, "dev-team");
    }

    #[test]
    fn idle_input_defaults_on_empty_json() {
        let input = TeammateIdleInput::from_json(&serde_json::json!({}));
        assert!(input.session_id.is_empty());
        assert!(input.teammate_name.is_empty());
        assert!(input.team_name.is_empty());
    }

    #[test]
    fn idle_input_ignores_wrong_types() {
        let input = TeammateIdleInput::from_json(&serde_json::json!({
            "session_id": 42,
            "teammate_name": true,
            "team_name": []
        }));
        assert!(input.session_id.is_empty());
        assert!(input.teammate_name.is_empty());
        assert!(input.team_name.is_empty());
    }
}
