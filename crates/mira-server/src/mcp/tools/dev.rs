// src/mcp/tools/dev.rs
// Developer experience tools

use crate::mcp::MiraServer;

/// Get session recap formatted exactly as it appears in system prompts
/// Uses the shared Database::build_session_recap for consistency with chat UI
pub async fn get_session_recap(server: &MiraServer) -> Result<String, String> {
    let project_id = server.project.read().await.as_ref().map(|p| p.id);
    let recap = server.db.build_session_recap(project_id);
    if recap.is_empty() {
        Ok("No session recap available.".to_string())
    } else {
        Ok(recap)
    }
}
