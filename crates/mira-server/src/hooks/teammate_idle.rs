// crates/mira-server/src/hooks/teammate_idle.rs
// Hook handler for TeammateIdle events - monitors team member activity

use crate::db::pool::DatabasePool;
use crate::hooks::{
    HookTimer, get_db_path, read_hook_input, resolve_project_id, write_hook_output,
};
use crate::proactive::EventType;
use crate::proactive::behavior::BehaviorTracker;
use anyhow::{Context, Result};
use std::sync::Arc;

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

    eprintln!(
        "[mira] TeammateIdle hook triggered (teammate: {}, team: {})",
        idle_input.teammate_name, idle_input.team_name,
    );

    // Open database
    let db_path = get_db_path();
    let pool = match DatabasePool::open_hook(&db_path).await {
        Ok(p) => Arc::new(p),
        Err(_) => {
            write_hook_output(&serde_json::json!({}));
            return Ok(());
        }
    };

    // Get current project
    let Some(project_id) = resolve_project_id(&pool).await else {
        write_hook_output(&serde_json::json!({}));
        return Ok(());
    };

    // Log idle event
    {
        let session_id = idle_input.session_id.clone();
        let teammate_name = idle_input.teammate_name.clone();
        let team_name = idle_input.team_name.clone();
        pool.try_interact("teammate idle logging", move |conn| {
            let mut tracker = BehaviorTracker::for_session(conn, session_id, project_id);
            let data = serde_json::json!({
                "behavior_type": "teammate_idle",
                "teammate_name": teammate_name,
                "team_name": team_name,
            });
            if let Err(e) = tracker.log_event(conn, EventType::ToolUse, data) {
                tracing::debug!("Failed to log teammate idle: {e}");
            }
            Ok(())
        })
        .await;
    }

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
