// src/mcp/tools/dev.rs
// Developer experience tools

use crate::mcp::MiraServer;

/// Get session recap for MCP clients
/// Returns recent context, preferences, and project state
pub async fn get_session_recap(server: &MiraServer) -> Result<String, String> {
    let project_id = server.project.read().await.as_ref().map(|p| p.id);
    let recap = server.db.build_session_recap(project_id);
    if recap.is_empty() {
        Ok("No session recap available.".to_string())
    } else {
        Ok(recap)
    }
}
