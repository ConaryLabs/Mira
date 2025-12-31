// src/mcp/tools/project.rs
// Project management tools

use crate::mcp::{MiraServer, ProjectContext};

/// Initialize session with project
pub async fn session_start(
    server: &MiraServer,
    project_path: String,
    name: Option<String>,
) -> Result<String, String> {
    // Set project
    let project_id = server
        .db
        .get_or_create_project(&project_path, name.as_deref())
        .map_err(|e| e.to_string())?;

    let ctx = ProjectContext {
        id: project_id,
        path: project_path.clone(),
        name: name.clone(),
    };

    *server.project.write().await = Some(ctx);

    // Build response with context
    let mut response = format!("Project: {} ({})\n", name.as_deref().unwrap_or("unnamed"),
        if project_path.contains("Mira") { "rust" } else { "unknown" });

    // Load recent memories
    let memories = server
        .db
        .search_memories(Some(project_id), "", 5)
        .map_err(|e| e.to_string())?;

    if !memories.is_empty() {
        response.push_str("\nRecent memories:\n");
        for mem in memories.iter().take(5) {
            let preview = if mem.content.len() > 80 {
                format!("{}...", &mem.content[..80])
            } else {
                mem.content.clone()
            };
            response.push_str(&format!("  - {}\n", preview));
        }
    }

    response.push_str("\nReady.");
    Ok(response)
}

/// Set active project
pub async fn set_project(
    server: &MiraServer,
    project_path: String,
    name: Option<String>,
) -> Result<String, String> {
    let project_id = server
        .db
        .get_or_create_project(&project_path, name.as_deref())
        .map_err(|e| e.to_string())?;

    let ctx = ProjectContext {
        id: project_id,
        path: project_path.clone(),
        name: name.clone(),
    };

    *server.project.write().await = Some(ctx);

    Ok(format!(
        "Project set: {} (id: {})",
        name.as_deref().unwrap_or(&project_path),
        project_id
    ))
}

/// Get current project
pub async fn get_project(server: &MiraServer) -> Result<String, String> {
    let project = server.project.read().await;

    match project.as_ref() {
        Some(ctx) => Ok(format!(
            "Current project:\n  Path: {}\n  Name: {}\n  ID: {}",
            ctx.path,
            ctx.name.as_deref().unwrap_or("(unnamed)"),
            ctx.id
        )),
        None => Ok("No active project. Call session_start or set_project first.".to_string()),
    }
}
