//! Unified Claude tools (claude_task, claude_close, claude_status, discuss, reply_to_mira)

use crate::tools::core::ToolContext;

/// Send task to Claude Code - requires claude_manager (web-only)
pub async fn claude_task<C: ToolContext>(
    ctx: &C,
    task: String,
) -> Result<String, String> {
    let manager = ctx.claude_manager()
        .ok_or("Claude Code is only available in web chat interface".to_string())?;

    let project = ctx.get_project().await
        .ok_or("No project selected. Use set_project first.".to_string())?;

    let id = manager.send_task(&project.path, &task).await
        .map_err(|e| e.to_string())?;

    Ok(format!(
        "Task sent to Claude Code for project '{}' (instance {})\n\n{}",
        project.name.as_deref().unwrap_or(&project.path),
        id,
        "## Claude Code Instance Guide\n\nYou now have a Claude Code instance running. Use the instance_id for follow-ups."
    ))
}

/// Close Claude Code instance for current project - requires claude_manager (web-only)
pub async fn claude_close<C: ToolContext>(
    ctx: &C,
) -> Result<String, String> {
    let manager = ctx.claude_manager()
        .ok_or("Claude Code is only available in web chat interface".to_string())?;

    let project = ctx.get_project().await
        .ok_or("No project selected. Use set_project first.".to_string())?;

    manager.close_project(&project.path).await
        .map_err(|e| e.to_string())?;

    Ok(format!(
        "Claude Code closed for project '{}'",
        project.name.as_deref().unwrap_or(&project.path)
    ))
}

/// Get Claude Code status for current project - requires claude_manager (web-only)
pub async fn claude_status<C: ToolContext>(
    ctx: &C,
) -> Result<String, String> {
    let manager = ctx.claude_manager()
        .ok_or("Claude Code is only available in web chat interface".to_string())?;

    let project = ctx.get_project().await
        .ok_or("No project selected. Use set_project first.".to_string())?;

    let project_name = project.name.as_deref().unwrap_or(&project.path);
    let has_instance = manager.has_instance(&project.path).await;

    if has_instance {
        let instance_id = manager.get_instance_id(&project.path).await
            .unwrap_or_else(|| "unknown".to_string());
        Ok(format!(
            "Claude Code is running for '{}' (instance {})",
            project_name, instance_id
        ))
    } else {
        Ok(format!("No Claude Code running for '{}'", project_name))
    }
}

/// Discuss with Claude - requires claude_manager (web-only)
pub async fn discuss<C: ToolContext>(
    ctx: &C,
    _message: String,
) -> Result<String, String> {
    let _manager = ctx.claude_manager()
        .ok_or("Discuss is only available in web chat interface".to_string())?;

    // In web chat, this spawns a collaborator
    // For now, return a message about using web interface
    Err("Discuss tool not yet implemented in unified core. Use web chat interface.".to_string())
}

/// Reply to Mira message - requires pending_responses (web-only)
pub async fn reply_to_mira<C: ToolContext>(
    ctx: &C,
    in_reply_to: String,
    content: String,
) -> Result<String, String> {
    let pending = ctx.pending_responses()
        .ok_or("reply_to_mira is only available in web chat interface".to_string())?;
    
    let mut pending_lock = pending.write().await;
    if let Some(sender) = pending_lock.remove(&in_reply_to) {
        let _ = sender.send(content);
        Ok("Reply sent to Mira".to_string())
    } else {
        Err("No pending message found with that ID".to_string())
    }
}
