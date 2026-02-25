// crates/mira-server/src/ipc/client/team_ops.rs
//! HookClient methods for team membership, file ownership, conflicts,
//! and deactivation.

use super::{Backend, FileConflictInfo, TeamMembershipInfo};
use serde_json::json;

impl super::HookClient {
    /// Get team membership for a session.
    /// Returns (team_id, team_name, member_name, role) if found.
    pub async fn get_team_membership(&mut self, session_id: &str) -> Option<TeamMembershipInfo> {
        if self.is_ipc() {
            let params = json!({"session_id": session_id});
            if let Ok(result) = self.call("get_team_membership", params).await {
                if result
                    .get("found")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    return Some(TeamMembershipInfo {
                        team_id: result.get("team_id")?.as_i64()?,
                        team_name: result.get("team_name")?.as_str()?.to_string(),
                        member_name: result.get("member_name")?.as_str()?.to_string(),
                        role: result.get("role")?.as_str()?.to_string(),
                    });
                }
                return None;
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let session_id = session_id.to_string();
            let membership = pool
                .interact(move |conn| {
                    Ok::<_, anyhow::Error>(crate::db::get_team_membership_for_session_sync(
                        conn,
                        &session_id,
                    ))
                })
                .await
                .ok()
                .flatten()?;
            return Some(TeamMembershipInfo {
                team_id: membership.team_id,
                team_name: membership.team_name,
                member_name: membership.member_name,
                role: membership.role,
            });
        }
        None
    }

    /// Record file ownership for team conflict detection. Fire-and-forget.
    pub async fn record_file_ownership(
        &mut self,
        team_id: i64,
        session_id: &str,
        member_name: &str,
        file_path: &str,
        tool_name: &str,
    ) {
        if self.is_ipc() {
            let params = json!({
                "team_id": team_id,
                "session_id": session_id,
                "member_name": member_name,
                "file_path": file_path,
                "tool_name": tool_name,
            });
            if self.call("record_file_ownership", params).await.is_ok() {
                return;
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let session_id = session_id.to_string();
            let member_name = member_name.to_string();
            let file_path = file_path.to_string();
            let tool_name = tool_name.to_string();
            pool.try_interact("record_file_ownership", move |conn| {
                crate::db::record_file_ownership_sync(
                    conn,
                    team_id,
                    &session_id,
                    &member_name,
                    &file_path,
                    &tool_name,
                )
                .map_err(|e| anyhow::anyhow!("{e}"))?;
                Ok(())
            })
            .await;
        }
    }

    /// Get file conflicts for a team session.
    pub async fn get_file_conflicts(
        &mut self,
        team_id: i64,
        session_id: &str,
    ) -> Vec<FileConflictInfo> {
        if self.is_ipc() {
            let params = json!({"team_id": team_id, "session_id": session_id});
            if let Ok(result) = self.call("get_file_conflicts", params).await {
                return result
                    .get("conflicts")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|c| {
                                Some(FileConflictInfo {
                                    file_path: c.get("file_path")?.as_str()?.to_string(),
                                    other_member_name: c
                                        .get("other_member_name")?
                                        .as_str()?
                                        .to_string(),
                                    operation: c.get("operation")?.as_str()?.to_string(),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let session_id = session_id.to_string();
            let conflicts = pool
                .interact(move |conn| {
                    Ok::<_, anyhow::Error>(crate::db::get_file_conflicts_sync(
                        conn,
                        team_id,
                        &session_id,
                    ))
                })
                .await
                .unwrap_or_default();
            return conflicts
                .into_iter()
                .map(|c| FileConflictInfo {
                    file_path: c.file_path,
                    other_member_name: c.other_member_name,
                    operation: c.operation,
                })
                .collect();
        }
        Vec::new()
    }

    /// Deactivate a team session. Fire-and-forget.
    pub async fn deactivate_team_session(&mut self, session_id: &str) {
        if self.is_ipc() {
            let params = json!({"session_id": session_id});
            if self.call("deactivate_team_session", params).await.is_ok() {
                return;
            }
        }
        if let Backend::Direct { pool } = &self.inner {
            let pool = pool.clone();
            let session_id = session_id.to_string();
            if let Err(e) = pool
                .run(move |conn| crate::db::deactivate_team_session_sync(conn, &session_id))
                .await
            {
                tracing::warn!("[mira] Failed to deactivate team session: {}", e);
            }
        }
    }

}
