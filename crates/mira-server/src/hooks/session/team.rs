// crates/mira-server/src/hooks/session/team.rs
//! Team detection and membership management.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Team membership info cached per-session to avoid cross-session clobbering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMembership {
    pub team_id: i64,
    pub team_name: String,
    pub member_name: String,
    pub role: String,
    pub config_path: String,
}

/// Result of team detection
pub struct TeamDetectionResult {
    pub team_name: String,
    pub config_path: String,
    pub member_name: String,
    pub role: String,
    pub agent_type: Option<String>,
}

struct TeamConfigInfo {
    team_name: String,
    config_path: String,
}

/// Per-session team membership file (avoids cross-session clobbering).
pub fn team_file_path_for_session(session_id: &str) -> Option<PathBuf> {
    if !session_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        tracing::warn!(
            "Invalid characters in session_id for team file path, skipping: {:?}",
            session_id
        );
        return None;
    }
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            eprintln!("[Mira] WARNING: HOME directory not set, cannot resolve team file path");
            return None;
        }
    };
    Some(home.join(format!(".mira/claude-team-{}.json", session_id)))
}

/// Read team membership for the current session (filesystem-based).
/// Prefer `read_team_membership_from_db` when a pool and session_id are available.
pub fn read_team_membership() -> Option<TeamMembership> {
    let session_id = super::read_claude_session_id()?;
    let path = team_file_path_for_session(&session_id)?;
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Read team membership from DB for a specific session (session-isolated).
/// DB is the sole source of truth — no filesystem fallback, which could
/// revive stale membership from crashed/partially-cleaned sessions.
pub async fn read_team_membership_from_db(
    pool: &std::sync::Arc<crate::db::pool::DatabasePool>,
    session_id: &str,
) -> Option<TeamMembership> {
    if session_id.is_empty() {
        return None;
    }
    let pool_clone = pool.clone();
    let sid = session_id.to_string();
    pool_clone
        .interact(move |conn| {
            Ok::<_, anyhow::Error>(crate::db::get_team_membership_for_session_sync(conn, &sid))
        })
        .await
        .ok()
        .flatten()
}

/// Write team membership atomically (temp + rename) with restricted permissions (0o600).
pub fn write_team_membership(session_id: &str, membership: &TeamMembership) -> Result<()> {
    let path = team_file_path_for_session(session_id)
        .ok_or_else(|| anyhow::anyhow!("Invalid session_id for team file path: {session_id:?}"))?;
    let json = serde_json::to_string(membership)?;
    let temp_path = path.with_extension("tmp");

    // Write temp file with restricted permissions (0o600)
    {
        use std::io::Write;
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&temp_path)?;
        f.write_all(json.as_bytes())?;
    }

    fs::rename(&temp_path, &path)?;
    Ok(())
}

/// Clean up per-session team file.
pub fn cleanup_team_file(session_id: &str) {
    let Some(path) = team_file_path_for_session(session_id) else {
        return;
    };
    let _ = fs::remove_file(&path);
}

/// Detect team membership from Claude Code's Agent Teams config files.
///
/// Scans `~/.claude/teams/*/config.json` for team configs that reference
/// the current session or working directory. Also checks the SessionStart
/// input for an `agent_type` field.
pub fn detect_team_membership(
    input: &serde_json::Value,
    session_id: Option<&str>,
    cwd: Option<&str>,
) -> Option<TeamDetectionResult> {
    // Primary: Check SessionStart input for agent_type (Claude Code provides this)
    let agent_type = input.get("agent_type").and_then(|v| v.as_str());
    let member_name = input
        .get("agent_name")
        .and_then(|v| v.as_str())
        .or_else(|| input.get("member_name").and_then(|v| v.as_str()));

    // If agent_type is set, this is a team member
    if let Some(agent_type) = agent_type {
        // Try to find the team config
        if let Some(team_config) = scan_team_configs(cwd) {
            let role = if agent_type == "lead" {
                "lead"
            } else {
                "teammate"
            };
            return Some(TeamDetectionResult {
                team_name: team_config.team_name,
                config_path: team_config.config_path,
                member_name: member_name.unwrap_or(agent_type).to_string(),
                role: role.to_string(),
                agent_type: Some(agent_type.to_string()),
            });
        }
    }

    // Secondary: Scan filesystem for team configs
    if let Some(team_config) = scan_team_configs(cwd) {
        // Derive member name from session_id or config
        let name = member_name
            .map(|s| s.to_string())
            .or_else(|| session_id.map(|s| format!("member-{}", &s[..8.min(s.len())])))
            .unwrap_or_else(|| "unknown".to_string());

        return Some(TeamDetectionResult {
            team_name: team_config.team_name,
            config_path: team_config.config_path,
            member_name: name,
            role: "teammate".to_string(),
            agent_type: agent_type.map(String::from),
        });
    }

    None
}

/// Scan `~/.claude/teams/*/config.json` for team configs.
/// When multiple teams match, prefer the most specific project_path
/// (longest path that is still an ancestor of cwd) for determinism.
fn scan_team_configs(cwd: Option<&str>) -> Option<TeamConfigInfo> {
    let home = dirs::home_dir()?;
    let teams_dir = home.join(".claude/teams");

    if !teams_dir.is_dir() {
        return None;
    }

    let entries = fs::read_dir(&teams_dir).ok()?;
    let mut candidates: Vec<(usize, TeamConfigInfo)> = Vec::new();
    let mut fallback: Vec<TeamConfigInfo> = Vec::new();

    for entry in entries.flatten() {
        let config_path = entry.path().join("config.json");
        if !config_path.is_file() {
            continue;
        }

        let content = match fs::read_to_string(&config_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let config: serde_json::Value = match serde_json::from_str(&content) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let team_name_val = config
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| entry.file_name().to_string_lossy().to_string());

        // Check if cwd is under the team's project path (not the reverse —
        // matching proj_p.starts_with(cwd_p) would be overly broad, e.g.
        // cwd="/home/peter" would match project="/home/peter/Mira").
        if let Some(project_path) = config.get("project_path").and_then(|v| v.as_str())
            && let Some(cwd_val) = cwd
        {
            let cwd_p = std::path::Path::new(cwd_val);
            let proj_p = std::path::Path::new(project_path);
            if cwd_p.starts_with(proj_p) {
                let specificity = proj_p.components().count();
                candidates.push((
                    specificity,
                    TeamConfigInfo {
                        team_name: team_name_val,
                        config_path: config_path.to_string_lossy().to_string(),
                    },
                ));
                continue;
            }
        }

        // If we don't have cwd, collect as fallback (but prefer cwd matches)
        if cwd.is_none() {
            fallback.push(TeamConfigInfo {
                team_name: team_name_val,
                config_path: config_path.to_string_lossy().to_string(),
            });
        }
    }

    if !candidates.is_empty() {
        // Most specific project path wins; tie-break on team name, then config path
        candidates.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.team_name.cmp(&b.1.team_name))
                .then_with(|| a.1.config_path.cmp(&b.1.config_path))
        });
        return candidates.into_iter().next().map(|(_, info)| info);
    }

    if !fallback.is_empty() {
        // Deterministic: sort by team name, then config path for full tie-break
        fallback.sort_by(|a, b| {
            a.team_name
                .cmp(&b.team_name)
                .then_with(|| a.config_path.cmp(&b.config_path))
        });
        return fallback.into_iter().next();
    }

    None
}
