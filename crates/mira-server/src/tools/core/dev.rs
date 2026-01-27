// crates/mira-server/src/tools/core/dev.rs
// Developer experience tools

use crate::db::build_session_recap_sync;
use crate::tools::core::session_notes;
use crate::tools::core::ToolContext;

/// Get session recap for MCP clients
/// Returns recent context, preferences, project state, and Claude Code session notes
pub async fn get_session_recap<C: ToolContext>(ctx: &C) -> Result<String, String> {
    let project = ctx.get_project().await;
    let project_id = project.as_ref().map(|p| p.id);

    let mut recap = ctx
        .pool()
        .run(move |conn| Ok::<_, String>(build_session_recap_sync(conn, project_id)))
        .await?;

    // Add Claude Code session notes if available
    if let Some(proj) = &project {
        let notes = session_notes::get_recent_session_notes(&proj.path, 3);
        if !notes.is_empty() {
            recap.push_str(&session_notes::format_session_notes(&notes));
        }
    }

    if recap.is_empty() {
        Ok("No session recap available.".to_string())
    } else {
        Ok(recap)
    }
}
