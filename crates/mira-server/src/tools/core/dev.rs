// crates/mira-server/src/tools/core/dev.rs
// Developer experience tools

use crate::tools::core::ToolContext;

/// Get session recap for MCP clients
/// Returns recent context, preferences, and project state
pub async fn get_session_recap<C: ToolContext>(ctx: &C) -> Result<String, String> {
    let project_id = ctx.project_id().await;
    let recap = ctx.db().build_session_recap(project_id);
    if recap.is_empty() {
        Ok("No session recap available.".to_string())
    } else {
        Ok(recap)
    }
}
