// src/mcp/tools/project.rs
// Project management tools

use crate::hooks::session::read_claude_session_id;
use crate::mcp::{MiraServer, ProjectContext};

/// Initialize session with project
pub async fn session_start(
    server: &MiraServer,
    project_path: String,
    name: Option<String>,
    session_id: Option<String>,
) -> Result<String, String> {
    // Set project - now returns (id, detected_name)
    let (project_id, project_name) = server
        .db
        .get_or_create_project(&project_path, name.as_deref())
        .map_err(|e| e.to_string())?;

    let ctx = ProjectContext {
        id: project_id,
        path: project_path.clone(),
        name: project_name.clone(),
    };

    *server.project.write().await = Some(ctx);

    // Set session ID (use provided, or Claude's from hook, or generate new)
    let sid = session_id
        .or_else(read_claude_session_id)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    server.db.create_session(&sid, Some(project_id)).map_err(|e| e.to_string())?;
    *server.session_id.write().await = Some(sid.clone());

    // Detect project type
    let project_type = detect_project_type(&project_path);

    // Build response with context
    let display_name = project_name.as_deref().unwrap_or("unnamed");
    let mut response = format!("Project: {} ({})\n", display_name, project_type);

    // Show recent sessions (skip current, show last 3)
    let recent_sessions = server
        .db
        .get_recent_sessions(project_id, 4)
        .unwrap_or_default();

    let previous_sessions: Vec<_> = recent_sessions
        .iter()
        .filter(|s| s.id != sid)
        .take(3)
        .collect();

    if !previous_sessions.is_empty() {
        response.push_str("\nRecent sessions:\n");
        for sess in &previous_sessions {
            let short_id = &sess.id[..8];
            let timestamp = &sess.last_activity[..16]; // YYYY-MM-DD HH:MM

            // Get session stats
            let (tool_count, tools) = server
                .db
                .get_session_stats(&sess.id)
                .unwrap_or((0, vec![]));

            if let Some(ref summary) = sess.summary {
                response.push_str(&format!("  [{}] {} - {}\n", short_id, timestamp, summary));
            } else if tool_count > 0 {
                let tools_str = tools.join(", ");
                response.push_str(&format!(
                    "  [{}] {} - {} tool calls ({})\n",
                    short_id, timestamp, tool_count, tools_str
                ));
            } else {
                response.push_str(&format!("  [{}] {} - (no activity)\n", short_id, timestamp));
            }
        }
        response.push_str(&format!(
            "  Use session_history(action=\"get_history\", session_id=\"...\") to view details\n"
        ));
    }

    // Load preferences
    let preferences = server
        .db
        .get_preferences(Some(project_id))
        .map_err(|e| e.to_string())?;

    if !preferences.is_empty() {
        response.push_str("\nPreferences:\n");
        for pref in &preferences {
            let category = pref.category.as_deref().unwrap_or("general");
            response.push_str(&format!("  [{}] {}\n", category, pref.content));
        }
    }

    // Load recent memories (excluding preferences)
    let memories = server
        .db
        .search_memories(Some(project_id), "", 5)
        .map_err(|e| e.to_string())?;

    let non_pref_memories: Vec<_> = memories
        .iter()
        .filter(|m| m.fact_type != "preference")
        .take(5)
        .collect();

    if !non_pref_memories.is_empty() {
        response.push_str("\nRecent context:\n");
        for mem in non_pref_memories {
            let preview = if mem.content.len() > 80 {
                format!("{}...", &mem.content[..80])
            } else {
                mem.content.clone()
            };
            response.push_str(&format!("  - {}\n", preview));
        }
    }

    // Show database path
    if let Some(db_path) = server.db.path() {
        response.push_str(&format!("\nDatabase: {}\n", db_path));
    }

    response.push_str("\nReady.");
    Ok(response)
}

/// Detect project type from path
fn detect_project_type(path: &str) -> &'static str {
    use std::path::Path;
    let p = Path::new(path);

    if p.join("Cargo.toml").exists() {
        "rust"
    } else if p.join("package.json").exists() {
        "node"
    } else if p.join("pyproject.toml").exists() || p.join("setup.py").exists() {
        "python"
    } else if p.join("go.mod").exists() {
        "go"
    } else if p.join("pom.xml").exists() || p.join("build.gradle").exists() {
        "java"
    } else {
        "unknown"
    }
}

/// Set active project
pub async fn set_project(
    server: &MiraServer,
    project_path: String,
    name: Option<String>,
) -> Result<String, String> {
    let (project_id, project_name) = server
        .db
        .get_or_create_project(&project_path, name.as_deref())
        .map_err(|e| e.to_string())?;

    let ctx = ProjectContext {
        id: project_id,
        path: project_path.clone(),
        name: project_name.clone(),
    };

    *server.project.write().await = Some(ctx);

    let display_name = project_name.as_deref().unwrap_or(&project_path);
    Ok(format!("Project set: {} (id: {})", display_name, project_id))
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
