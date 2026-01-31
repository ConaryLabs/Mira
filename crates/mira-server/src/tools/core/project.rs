// crates/mira-server/src/tools/core/project.rs
// Unified project tools

use mira_types::{MemoryFact, ProjectContext};
use std::path::Path;
use std::process::Command;

use crate::cartographer;
use crate::db::documentation::count_doc_tasks_by_status;
use crate::db::{
    StoreMemoryParams, get_health_alerts_sync, get_or_create_project_sync, get_preferences_sync,
    get_project_briefing_sync, get_recent_sessions_sync, get_session_stats_sync,
    mark_session_for_briefing_sync, save_active_project_sync, search_memories_text_sync,
    set_server_state_sync, store_memory_sync, update_project_name_sync,
    upsert_session_with_branch_sync,
};
use crate::git::get_git_branch;
use crate::proactive::{ProactiveConfig, interventions};
use crate::tools::core::ToolContext;
use crate::tools::core::claude_local;
use crate::utils::ResultExt;

// Helper functions moved to db/project.rs:
// - search_memories_text_sync
// - get_preferences_sync
// - get_health_alerts_sync

// Sync helpers moved to db modules:
// - get_doc_task_counts_sync -> uses count_doc_tasks_by_status directly
// - create_session_sync -> upsert_session_sync in db/project.rs
// - get_or_create_project_sync -> db/project.rs

/// Auto-detect project name from path (sync helper)
fn detect_project_name(path: &str) -> Option<String> {
    let path = Path::new(path);
    let dir_name = || {
        path.file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
    };

    // Try Cargo.toml for Rust projects
    let cargo_toml = path.join("Cargo.toml");
    if cargo_toml.exists() {
        if let Ok(content) = std::fs::read_to_string(&cargo_toml) {
            if content.contains("[workspace]") {
                return dir_name();
            }

            let mut in_package = false;
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with('[') {
                    in_package = line == "[package]";
                } else if in_package && line.starts_with("name") {
                    if let Some(name) = line.split('=').nth(1) {
                        let name = name.trim().trim_matches('"').trim_matches('\'');
                        if !name.is_empty() {
                            return Some(name.to_string());
                        }
                    }
                }
            }
        }
    }

    // Try package.json for Node projects
    let package_json = path.join("package.json");
    if package_json.exists() {
        if let Ok(content) = std::fs::read_to_string(&package_json) {
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with("\"name\"") {
                    if let Some(name) = line.split(':').nth(1) {
                        let name = name.trim().trim_matches(',').trim_matches('"').trim();
                        if !name.is_empty() {
                            return Some(name.to_string());
                        }
                    }
                }
            }
        }
    }

    // Fall back to directory name
    dir_name()
}

/// Shared project initialization logic
async fn init_project<C: ToolContext>(
    ctx: &C,
    project_path: &str,
    name: Option<&str>,
) -> Result<(i64, Option<String>), String> {
    // Use pool for project creation (ensures same database as memory operations)
    let path_owned = project_path.to_string();
    let name_owned = name.map(|s| s.to_string());
    let (project_id, stored_name) = ctx
        .pool()
        .run(move |conn| get_or_create_project_sync(conn, &path_owned, name_owned.as_deref()))
        .await?;

    // If we have a stored name, use it; otherwise detect from files
    let project_name = if stored_name.is_some() {
        stored_name
    } else {
        let detected = detect_project_name(project_path);
        if let Some(ref name) = detected {
            // Update the project with the detected name
            let name_clone = name.clone();
            ctx.pool()
                .run(move |conn| update_project_name_sync(conn, project_id, &name_clone))
                .await?;
        }
        detected
    };

    let project_ctx = ProjectContext {
        id: project_id,
        path: project_path.to_string(),
        name: project_name.clone(),
    };

    ctx.set_project(project_ctx).await;

    // Register project with file watcher for automatic incremental indexing
    if let Some(watcher) = ctx.watcher() {
        watcher
            .watch(project_id, std::path::PathBuf::from(project_path))
            .await;
    }

    // Persist active project for restart recovery
    let path_for_save = project_path.to_string();
    if let Err(e) = ctx
        .pool()
        .run(move |conn| save_active_project_sync(conn, &path_for_save))
        .await
    {
        tracing::warn!("Failed to persist active project: {}", e);
    }

    Ok((project_id, project_name))
}

/// Set current project
pub async fn set_project<C: ToolContext>(
    ctx: &C,
    project_path: String,
    name: Option<String>,
) -> Result<String, String> {
    let (project_id, project_name) = init_project(ctx, &project_path, name.as_deref()).await?;

    let display_name = project_name.as_deref().unwrap_or(&project_path);
    Ok(format!(
        "Project set: {} (id: {})",
        display_name, project_id
    ))
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

/// Format recent sessions for display
fn format_recent_sessions(
    sessions: &[(String, String, Option<String>, usize, Vec<String>)],
) -> String {
    let mut out = String::from("\nRecent sessions:\n");
    for (sess_id, last_activity, summary, tool_count, tools) in sessions {
        let short_id = &sess_id[..8.min(sess_id.len())];
        let timestamp = &last_activity[..16.min(last_activity.len())];

        if let Some(sum) = summary {
            out.push_str(&format!("  [{}] {} - {}\n", short_id, timestamp, sum));
        } else if *tool_count > 0 {
            let tools_str = tools.join(", ");
            out.push_str(&format!(
                "  [{}] {} - {} tool calls ({})\n",
                short_id, timestamp, tool_count, tools_str
            ));
        } else {
            out.push_str(&format!("  [{}] {} - (no activity)\n", short_id, timestamp));
        }
    }
    out.push_str(
        "  Use session_history(action=\"get_history\", session_id=\"...\") to view details\n",
    );
    out
}

/// Format preferences, context, health alerts, and interventions for display
fn format_session_insights(
    preferences: &[MemoryFact],
    memories: &[MemoryFact],
    health_alerts: &[MemoryFact],
    pending_interventions: &[interventions::PendingIntervention],
    doc_task_counts: &[(String, i64)],
) -> String {
    let mut out = String::new();

    if !preferences.is_empty() {
        out.push_str("\nPreferences:\n");
        for pref in preferences {
            let category = pref.category.as_deref().unwrap_or("general");
            out.push_str(&format!("  [{}] {}\n", category, pref.content));
        }
    }

    let non_pref_memories: Vec<_> = memories
        .iter()
        .filter(|m| m.fact_type != "preference")
        .take(5)
        .collect();

    if !non_pref_memories.is_empty() {
        out.push_str("\nRecent context:\n");
        for mem in non_pref_memories {
            let preview = if mem.content.len() > 80 {
                format!("{}...", &mem.content[..80])
            } else {
                mem.content.clone()
            };
            out.push_str(&format!("  - {}\n", preview));
        }
    }

    if !health_alerts.is_empty() {
        out.push_str("\nHealth alerts:\n");
        for alert in health_alerts {
            let category = alert.category.as_deref().unwrap_or("issue");
            let preview = if alert.content.len() > 100 {
                format!("{}...", &alert.content[..100])
            } else {
                alert.content.clone()
            };
            out.push_str(&format!("  [{}] {}\n", category, preview));
        }
    }

    if !pending_interventions.is_empty() {
        out.push_str("\nInsights (from background analysis):\n");
        for intervention in pending_interventions {
            out.push_str(&format!("  {}\n", intervention.format()));
        }
    }

    let pending_doc_count = doc_task_counts
        .iter()
        .find(|(status, _)| status == "pending")
        .map(|(_, count)| *count)
        .unwrap_or(0);

    if pending_doc_count > 0 {
        out.push_str(&format!(
            "\nDocumentation: {} items need docs\n  Use `documentation(action=\"list\")` to see them\n",
            pending_doc_count
        ));
    }

    out
}

/// Initialize session with project
pub async fn session_start<C: ToolContext>(
    ctx: &C,
    project_path: String,
    name: Option<String>,
    session_id: Option<String>,
) -> Result<String, String> {
    // Initialize project (shared with set_project)
    let (project_id, project_name) = init_project(ctx, &project_path, name.as_deref()).await?;

    // Set session ID (use provided, or generate new)
    let sid = session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Detect git branch for branch-aware context
    let branch = get_git_branch(&project_path);

    // Gather system context content (synchronous, no DB needed)
    let system_context = gather_system_context_content();

    // Create session, persist state, store system context, and get briefing in one pool call
    let sid_for_db = sid.clone();
    let branch_for_db = branch.clone();
    let briefing_text: Option<String> = ctx
        .pool()
        .run(move |conn| {
            // Create/update session with branch
            upsert_session_with_branch_sync(
                conn,
                &sid_for_db,
                Some(project_id),
                branch_for_db.as_deref(),
            )
            .str_err()?;

            // Persist active session ID for restart recovery
            set_server_state_sync(conn, "active_session_id", &sid_for_db)
                .str_err()?;

            // Store system context as memory
            if let Some(ref content) = system_context {
                let _ = store_memory_sync(
                    conn,
                    StoreMemoryParams {
                        project_id: None, // global
                        key: Some("system_context"),
                        content,
                        fact_type: "context",
                        category: Some("system"),
                        confidence: 1.0,
                        session_id: None,
                        user_id: None,
                        scope: "project",
                        branch: None,
                    },
                );
            }

            // Get project briefing
            let briefing = get_project_briefing_sync(conn, project_id)
                .ok()
                .flatten()
                .and_then(|b| b.briefing_text);

            // Mark session for briefing (clears briefing for next time)
            let _ = mark_session_for_briefing_sync(conn, project_id);

            Ok::<_, String>(briefing)
        })
        .await?;

    ctx.set_session_id(sid.clone()).await;

    // Set branch in context
    ctx.set_branch(branch.clone()).await;

    // Import CLAUDE.local.md entries as memories (if file exists)
    let imported_count =
        claude_local::import_claude_local_md_async(ctx.pool(), project_id, &project_path)
            .await
            .unwrap_or(0);

    // Detect project type
    let project_type = detect_project_type(&project_path);

    // Build response with context
    let display_name = project_name.as_deref().unwrap_or("unnamed");
    let mut response = format!("Project: {} ({})\n", display_name, project_type);

    // Report CLAUDE.local.md imports
    if imported_count > 0 {
        response.push_str(&format!(
            "\nImported {} entries from CLAUDE.local.md\n",
            imported_count
        ));
    }

    // Check for "What's New" briefing
    if let Some(text) = briefing_text {
        response.push_str(&format!("\nWhat's new: {}\n", text));
    }

    // Get recent sessions and their stats in one pool call
    let sid_for_filter = sid.clone();
    let recent_session_data: Vec<(String, String, Option<String>, usize, Vec<String>)> = ctx
        .pool()
        .run(move |conn| {
            let sessions = get_recent_sessions_sync(conn, project_id, 4).unwrap_or_default();
            let mut result = Vec::new();
            for sess in sessions
                .into_iter()
                .filter(|s| s.id != sid_for_filter)
                .take(3)
            {
                let (tool_count, tools) =
                    get_session_stats_sync(conn, &sess.id).unwrap_or((0, vec![]));
                result.push((sess.id, sess.last_activity, sess.summary, tool_count, tools));
            }
            Ok::<_, String>(result)
        })
        .await?;

    if !recent_session_data.is_empty() {
        response.push_str(&format_recent_sessions(&recent_session_data));
    }

    // Load preferences, memories, health alerts, doc task counts, and interventions in a single pool call
    let (preferences, memories, health_alerts, doc_task_counts, pending_interventions): (
        Vec<MemoryFact>,
        Vec<MemoryFact>,
        Vec<MemoryFact>,
        Vec<(String, i64)>,
        Vec<interventions::PendingIntervention>,
    ) = ctx
        .pool()
        .run(move |conn| {
            // Get preferences
            let preferences = get_preferences_sync(conn, Some(project_id)).unwrap_or_default();

            // Get recent memories
            let memories =
                search_memories_text_sync(conn, Some(project_id), "", 10).unwrap_or_default();

            // Get health alerts
            let health_alerts =
                get_health_alerts_sync(conn, Some(project_id), 5).unwrap_or_default();

            // Get documentation task counts
            let doc_task_counts =
                count_doc_tasks_by_status(conn, Some(project_id)).unwrap_or_default();

            // Get pending proactive interventions
            let config = ProactiveConfig::default();
            let interventions_list =
                interventions::get_pending_interventions_sync(conn, project_id, &config)
                    .unwrap_or_default();

            Ok::<_, String>((
                preferences,
                memories,
                health_alerts,
                doc_task_counts,
                interventions_list,
            ))
        })
        .await?;

    response.push_str(&format_session_insights(
        &preferences,
        &memories,
        &health_alerts,
        &pending_interventions,
        &doc_task_counts,
    ));

    // Record that interventions were shown
    if !pending_interventions.is_empty() {
        let interventions_to_record = pending_interventions.clone();
        let sid_for_record = sid.clone();
        let _ = ctx
            .pool()
            .run(move |conn| {
                for intervention in &interventions_to_record {
                    let _ = interventions::record_intervention_sync(
                        conn,
                        project_id,
                        Some(&sid_for_record),
                        intervention,
                    );
                }
                Ok::<_, String>(())
            })
            .await;
    }

    // Load codebase map (only for Rust projects for now)
    if project_type == "rust" {
        match cartographer::get_or_generate_map_pool(
            ctx.pool().clone(),
            project_id,
            project_path.clone(),
            display_name.to_string(),
            project_type.to_string(),
        )
        .await
        {
            Ok(map) => {
                if !map.modules.is_empty() {
                    let formatted = cartographer::format_compact(&map);
                    response.push_str(&formatted);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to generate codebase map: {}", e);
                // Continue without map - non-fatal error
            }
        }
    }

    // Show database path
    if let Some(db_path) = ctx.pool().path() {
        response.push_str(&format!("\nDatabase: {}\n", db_path.display()));
    }

    response.push_str("\nReady.");
    Ok(response)
}

/// Gather system context content for bash tool usage (returns content string, does not store)
fn gather_system_context_content() -> Option<String> {
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
                let name = line.trim_start_matches("PRETTY_NAME=").trim_matches('"');
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
        .args([
            "-c",
            &format!("which {} 2>/dev/null | xargs -n1 basename", tools_to_check),
        ])
        .output()
    {
        if output.status.success() {
            let output_str = String::from_utf8_lossy(&output.stdout);
            let tools: Vec<&str> = output_str.lines().filter(|s| !s.is_empty()).collect();
            if !tools.is_empty() {
                context_parts.push(format!("Available tools: {}", tools.join(", ")));
            }
        }
    }

    if context_parts.is_empty() {
        None
    } else {
        Some(context_parts.join("\n"))
    }
}

use crate::mcp::requests::ProjectAction;

/// Unified project tool with action parameter
/// Actions: start (session_start), set (set_project), get (get_project)
pub async fn project<C: ToolContext>(
    ctx: &C,
    action: ProjectAction,
    project_path: Option<String>,
    name: Option<String>,
    session_id: Option<String>,
) -> Result<String, String> {
    match action {
        ProjectAction::Start => {
            let path = project_path.ok_or("project_path is required for action 'start'")?;
            session_start(ctx, path, name, session_id).await
        }
        ProjectAction::Set => {
            let path = project_path.ok_or("project_path is required for action 'set'")?;
            set_project(ctx, path, name).await
        }
        ProjectAction::Get => get_project(ctx).await,
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
