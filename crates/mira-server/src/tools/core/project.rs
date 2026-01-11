//! Unified project tools

use mira_types::ProjectContext;
use crate::tools::core::ToolContext;

/// Set current project
pub async fn set_project<C: ToolContext>(
    ctx: &C,
    project_path: String,
    name: Option<String>,
) -> Result<String, String> {
    let (project_id, project_name) = ctx
        .db()
        .get_or_create_project(&project_path, name.as_deref())
        .map_err(|e| e.to_string())?;

    let ctx_project = ProjectContext {
        id: project_id,
        path: project_path.clone(),
        name: project_name.clone(),
    };

    ctx.set_project(ctx_project).await;

    // Register project with file watcher for automatic incremental indexing
    if let Some(watcher) = ctx.watcher() {
        watcher.watch(project_id, std::path::PathBuf::from(&project_path)).await;
    }

    // Persist active project for restart recovery
    if let Err(e) = ctx.db().save_active_project(&project_path) {
        tracing::warn!("Failed to persist active project: {}", e);
    }

    let display_name = project_name.as_deref().unwrap_or(&project_path);
    Ok(format!("Project set: {} (id: {})", display_name, project_id))
}

/// Get current project info
pub async fn get_project<C: ToolContext>(ctx: &C) -> Result<String, String> {
    let project = ctx.get_project().await;

    match project {
        Some(ctx) => Ok(format!(
            "Current project:\n  Path: {}\n  Name: {}\n  ID: {}",
            ctx.path,
            ctx.name.as_deref().unwrap_or("(unnamed)"),
            ctx.id
        )),
        None => Ok("No active project. Call set_project first.".to_string()),
    }
}

/// List available projects
pub async fn list_projects<C: ToolContext>(ctx: &C) -> Result<String, String> {
    let projects = ctx.db().list_projects().map_err(|e| e.to_string())?;

    if projects.is_empty() {
        return Ok("No projects found. Use set_project to add one.".to_string());
    }

    let mut response = String::from("Projects:\n");
    for (id, path, name) in projects {
        let display_name = name.as_deref().unwrap_or("(unnamed)");
        response.push_str(&format!("  [{}] {} - {}\n", id, display_name, path));
    }

    Ok(response)
}
