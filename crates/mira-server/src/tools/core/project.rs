// crates/mira-server/src/tools/core/project.rs
// Unified project tools

use mira_types::ProjectContext;
use std::path::Path;
use std::process::Command;

use crate::cartographer;
use crate::db::Database;
use crate::hooks::session::read_claude_session_id;
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

/// Initialize session with project
pub async fn session_start<C: ToolContext>(
    ctx: &C,
    project_path: String,
    name: Option<String>,
    session_id: Option<String>,
) -> Result<String, String> {
    let db = ctx.db();

    // Set project
    let (project_id, project_name) = db
        .get_or_create_project(&project_path, name.as_deref())
        .map_err(|e| e.to_string())?;

    let project_ctx = ProjectContext {
        id: project_id,
        path: project_path.clone(),
        name: project_name.clone(),
    };

    ctx.set_project(project_ctx).await;

    // Register project with file watcher for automatic incremental indexing
    if let Some(watcher) = ctx.watcher() {
        watcher.watch(project_id, std::path::PathBuf::from(&project_path)).await;
    }

    // Persist active project for restart recovery
    if let Err(e) = db.save_active_project(&project_path) {
        tracing::warn!("Failed to persist active project: {}", e);
    }

    // Set session ID (use provided, or Claude's from hook, or generate new)
    let sid = session_id
        .or_else(read_claude_session_id)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    db.create_session(&sid, Some(project_id)).map_err(|e| e.to_string())?;
    ctx.set_session_id(sid.clone()).await;

    // Gather and store system context (for bash tool awareness)
    gather_system_context(db);

    // Detect project type
    let project_type = detect_project_type(&project_path);

    // Build response with context
    let display_name = project_name.as_deref().unwrap_or("unnamed");
    let mut response = format!("Project: {} ({})\n", display_name, project_type);

    // Check for "What's New" briefing (git changes since last session)
    if let Ok(Some(briefing)) = db.get_project_briefing(project_id) {
        if let Some(text) = &briefing.briefing_text {
            response.push_str(&format!("\nWhat's new: {}\n", text));
        }
    }

    // Mark that a session occurred (clears briefing for next time)
    let _ = db.mark_session_for_briefing(project_id);

    // Show recent sessions (skip current, show last 3)
    let recent_sessions = db
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
            let (tool_count, tools) = db
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
        response.push_str(
            "  Use session_history(action=\"get_history\", session_id=\"...\") to view details\n"
        );
    }

    // Load preferences
    let preferences = db
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
    let memories = db
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

    // Load health alerts (high-confidence issues found by background scanner)
    let health_alerts = db
        .get_health_alerts(Some(project_id), 5)
        .unwrap_or_default();

    if !health_alerts.is_empty() {
        response.push_str("\nHealth alerts:\n");
        for alert in health_alerts {
            let category = alert.category.as_deref().unwrap_or("issue");
            let preview = if alert.content.len() > 100 {
                format!("{}...", &alert.content[..100])
            } else {
                alert.content.clone()
            };
            response.push_str(&format!("  [{}] {}\n", category, preview));
        }
    }

    // Load codebase map (only for Rust projects for now)
    if project_type == "rust" {
        if let Ok(map) = cartographer::get_or_generate_map(
            db,
            project_id,
            &project_path,
            display_name,
            project_type,
        ) {
            if !map.modules.is_empty() {
                let formatted = cartographer::format_compact(&map);
                response.push_str(&formatted);
            }
        }
    }

    // Show database path
    if let Some(db_path) = db.path() {
        response.push_str(&format!("\nDatabase: {}\n", db_path));
    }

    response.push_str("\nReady.");
    Ok(response)
}

/// Gather and store system context for bash tool usage
fn gather_system_context(db: &Database) {
    let mut context_parts = Vec::new();

    // OS info
    if let Ok(output) = Command::new("uname").args(["-s", "-r"]).output() {
        if output.status.success() {
            let os = String::from_utf8_lossy(&output.stdout).trim().to_string();
            context_parts.push(format!("OS: {}", os));
        }
    }

    // Distro (Linux)
    if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
        for line in content.lines() {
            if line.starts_with("PRETTY_NAME=") {
                let name = line
                    .trim_start_matches("PRETTY_NAME=")
                    .trim_matches('"');
                context_parts.push(format!("Distro: {}", name));
                break;
            }
        }
    }

    // Shell
    if let Ok(shell) = std::env::var("SHELL") {
        context_parts.push(format!("Shell: {}", shell));
    }

    // User (try env, fallback to whoami)
    if let Ok(user) = std::env::var("USER") {
        if !user.is_empty() {
            context_parts.push(format!("User: {}", user));
        }
    } else if let Ok(output) = Command::new("whoami").output() {
        if output.status.success() {
            let user = String::from_utf8_lossy(&output.stdout).trim().to_string();
            context_parts.push(format!("User: {}", user));
        }
    }

    // Home directory (try env, fallback to ~)
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            context_parts.push(format!("Home: {}", home));
        }
    } else if let Ok(output) = Command::new("sh").args(["-c", "echo ~"]).output() {
        if output.status.success() {
            let home = String::from_utf8_lossy(&output.stdout).trim().to_string();
            context_parts.push(format!("Home: {}", home));
        }
    }

    // Timezone
    if let Ok(output) = Command::new("date").arg("+%Z (UTC%:z)").output() {
        if output.status.success() {
            let tz = String::from_utf8_lossy(&output.stdout).trim().to_string();
            context_parts.push(format!("Timezone: {}", tz));
        }
    }

    // Available tools (check common ones with single command)
    let tools_to_check = "git cargo rustc npm node python3 docker systemctl curl jq";
    if let Ok(output) = Command::new("sh")
        .args(["-c", &format!("which {} 2>/dev/null | xargs -n1 basename", tools_to_check)])
        .output()
    {
        if output.status.success() {
            let output_str = String::from_utf8_lossy(&output.stdout);
            let tools: Vec<&str> = output_str
                .lines()
                .filter(|s| !s.is_empty())
                .collect();
            if !tools.is_empty() {
                context_parts.push(format!("Available tools: {}", tools.join(", ")));
            }
        }
    }

    // Store as memory with key for upsert
    if !context_parts.is_empty() {
        let content = context_parts.join("\n");
        let _ = db.store_memory(
            None, // global, not project-specific
            Some("system_context"),
            &content,
            "context",
            Some("system"),
            1.0,
        );
    }
}

/// Detect project type from path
pub fn detect_project_type(path: &str) -> &'static str {
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
