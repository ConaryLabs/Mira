// crates/mira-server/src/tools/core/project.rs
// Unified project tools

use mira_types::{MemoryFact, ProjectContext};
use std::path::Path;
use std::process::Command;

use crate::cartographer;
use crate::db::documentation::count_doc_tasks_by_status;
use crate::db::{
    StoreObservationParams, get_health_alerts_sync, get_or_create_project_sync,
    get_preferences_sync, get_project_briefing_sync, get_recent_sessions_sync,
    get_session_stats_sync, mark_session_for_briefing_sync, save_active_project_sync,
    search_memories_text_sync, set_server_state_sync, store_observation_sync,
    update_project_name_sync, upsert_session_with_branch_sync,
};
use crate::git::get_git_branch;
use crate::mcp::responses::Json;
use crate::mcp::responses::{
    ProjectData, ProjectGetData, ProjectOutput, ProjectSetData, ProjectStartData,
};
use crate::proactive::{ProactiveConfig, interventions};
use crate::tools::core::claude_local;
use crate::tools::core::{NO_ACTIVE_PROJECT_ERROR, ToolContext};
use crate::utils::{ResultExt, truncate};

/// Session info tuple: (session_id, last_activity, summary, tool_count, tool_names)
type SessionInfo = (String, String, Option<String>, usize, Vec<String>);

/// Recap data: (preferences, memories, health_alerts, doc_task_counts, pending_interventions)
type RecapData = (
    Vec<MemoryFact>,
    Vec<MemoryFact>,
    Vec<MemoryFact>,
    Vec<(String, i64)>,
    Vec<interventions::PendingIntervention>,
);

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
    if cargo_toml.exists()
        && let Ok(content) = std::fs::read_to_string(&cargo_toml)
    {
        if content.contains("[workspace]") {
            return dir_name();
        }

        let mut in_package = false;
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_package = line == "[package]";
            } else if in_package
                && line.starts_with("name")
                && let Some(name) = line.split('=').nth(1)
            {
                let name = name.trim().trim_matches('"').trim_matches('\'');
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
        }
    }

    // Try package.json for Node projects
    let package_json = path.join("package.json");
    if package_json.exists()
        && let Ok(contents) = std::fs::read_to_string(&package_json)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents)
        && let Some(name) = value["name"].as_str()
        && !name.is_empty()
    {
        return Some(name.to_string());
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
) -> Result<Json<ProjectOutput>, String> {
    let (project_id, project_name) = init_project(ctx, &project_path, name.as_deref()).await?;

    let display_name = project_name.as_deref().unwrap_or(&project_path);
    Ok(Json(ProjectOutput {
        action: "set".into(),
        message: format!("Project set: {} (id: {})", display_name, project_id),
        data: Some(ProjectData::Set(ProjectSetData {
            project_id,
            project_name,
        })),
    }))
}

/// Get current project info
pub async fn get_project<C: ToolContext>(ctx: &C) -> Result<Json<ProjectOutput>, String> {
    let project = ctx.get_project().await;

    match project {
        Some(p) => Ok(Json(ProjectOutput {
            action: "get".into(),
            message: format!(
                "Current project:\n  Path: {}\n  Name: {}\n  ID: {}",
                p.path,
                p.name.as_deref().unwrap_or("(unnamed)"),
                p.id
            ),
            data: Some(ProjectData::Get(ProjectGetData {
                project_id: p.id,
                project_name: p.name,
                project_path: p.path,
            })),
        })),
        None => Ok(Json(ProjectOutput {
            action: "get".into(),
            message: NO_ACTIVE_PROJECT_ERROR.to_string(),
            data: None,
        })),
    }
}

/// Format recent sessions for display
fn format_recent_sessions(sessions: &[SessionInfo]) -> String {
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
    out.push_str("  Use session(action=\"recap\") for current session context\n");
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
            let preview = truncate(&mem.content, 80);
            out.push_str(&format!("  - {}\n", preview));
        }
    }

    if !health_alerts.is_empty() {
        out.push_str("\nHealth alerts:\n");
        for alert in health_alerts {
            let category = alert.category.as_deref().unwrap_or("issue");
            let preview = truncate(&alert.content, 100);
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
            "\nDocumentation: {} items need docs\n  CLI: `mira tool documentation '{{\"action\":\"list\"}}'`\n",
            pending_doc_count
        ));
    }

    out
}

/// Persist session: create session record, store system context, retrieve briefing
async fn persist_session<C: ToolContext>(
    ctx: &C,
    project_id: i64,
    sid: &str,
    branch: Option<&str>,
) -> Result<Option<String>, String> {
    let system_context = gather_system_context_content();
    let sid_owned = sid.to_string();
    let branch_owned = branch.map(|b| b.to_string());

    ctx.pool()
        .run(move |conn| {
            upsert_session_with_branch_sync(
                conn,
                &sid_owned,
                Some(project_id),
                branch_owned.as_deref(),
            )
            .str_err()?;

            set_server_state_sync(conn, "active_session_id", &sid_owned).str_err()?;

            if let Some(ref content) = system_context
                && let Err(e) = store_observation_sync(
                    conn,
                    StoreObservationParams {
                        project_id: None,
                        key: Some("system_context"),
                        content,
                        observation_type: "system",
                        category: Some("system"),
                        confidence: 1.0,
                        source: "project",
                        session_id: None,
                        team_id: None,
                        scope: "project",
                        expires_at: None,
                    },
                )
            {
                tracing::warn!("Failed to store system_context memory: {}", e);
            }

            let briefing = get_project_briefing_sync(conn, project_id)
                .ok()
                .flatten()
                .and_then(|b| b.briefing_text);
            if let Err(e) = mark_session_for_briefing_sync(conn, project_id) {
                tracing::warn!("Failed to mark session for briefing: {}", e);
            }

            Ok::<_, String>(briefing)
        })
        .await
}

/// Load recent sessions (excluding current), with stats
async fn load_recent_sessions<C: ToolContext>(
    ctx: &C,
    project_id: i64,
    current_sid: &str,
) -> Result<Vec<SessionInfo>, String> {
    let sid_owned = current_sid.to_string();
    ctx.pool()
        .run(move |conn| {
            let sessions = get_recent_sessions_sync(conn, project_id, 4).unwrap_or_default();
            let mut result = Vec::new();
            for sess in sessions.into_iter().filter(|s| s.id != sid_owned).take(3) {
                let (tool_count, tools) =
                    get_session_stats_sync(conn, &sess.id).unwrap_or((0, vec![]));
                result.push((sess.id, sess.last_activity, sess.summary, tool_count, tools));
            }
            Ok::<_, String>(result)
        })
        .await
}

/// Load recap data: preferences, memories, health alerts, doc counts, interventions
async fn load_recap_data<C: ToolContext>(ctx: &C, project_id: i64) -> Result<RecapData, String> {
    let user_id = ctx.get_user_identity();
    let team_id: Option<i64> = ctx.get_team_membership().map(|m| m.team_id);
    ctx.pool()
        .run(move |conn| {
            let preferences =
                get_preferences_sync(conn, Some(project_id), user_id.as_deref(), team_id)
                    .unwrap_or_default();
            let memories = search_memories_text_sync(
                conn,
                Some(project_id),
                "",
                10,
                user_id.as_deref(),
                team_id,
            )
            .unwrap_or_default();
            let health_alerts =
                get_health_alerts_sync(conn, Some(project_id), 5, user_id.as_deref(), team_id)
                    .unwrap_or_default();
            let doc_task_counts =
                count_doc_tasks_by_status(conn, Some(project_id)).unwrap_or_default();
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
        .await
}

/// Initialize session with project
pub async fn session_start<C: ToolContext>(
    ctx: &C,
    project_path: String,
    name: Option<String>,
    session_id: Option<String>,
) -> Result<Json<ProjectOutput>, String> {
    let (project_id, project_name) = init_project(ctx, &project_path, name.as_deref()).await?;
    let sid = session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let branch = get_git_branch(&project_path);

    // Phase 1: Persist session + retrieve briefing
    let briefing_text = persist_session(ctx, project_id, &sid, branch.as_deref()).await?;
    ctx.set_session_id(sid.clone()).await;
    ctx.set_branch(branch.clone()).await;

    // Phase 2: Import CLAUDE.local.md
    let imported_count =
        claude_local::import_claude_local_md_async(ctx.pool(), project_id, &project_path)
            .await
            .unwrap_or(0);

    // Phase 3: Build response
    let project_type = detect_project_type(&project_path);
    let display_name = project_name.as_deref().unwrap_or("unnamed");
    let mut response = format!("Project: {} ({})\n", display_name, project_type);

    if imported_count > 0 {
        response.push_str(&format!(
            "\nImported {} entries from CLAUDE.local.md\n",
            imported_count
        ));
    }
    if let Some(text) = briefing_text {
        response.push_str(&format!("\nWhat's new: {}\n", text));
    }

    // Phase 4: Load session history + recap data
    let recent_session_data = load_recent_sessions(ctx, project_id, &sid).await?;
    if !recent_session_data.is_empty() {
        response.push_str(&format_recent_sessions(&recent_session_data));
    }

    let (preferences, memories, health_alerts, doc_task_counts, pending_interventions) =
        load_recap_data(ctx, project_id).await?;

    response.push_str(&format_session_insights(
        &preferences,
        &memories,
        &health_alerts,
        &pending_interventions,
        &doc_task_counts,
    ));

    // Record shown interventions
    if !pending_interventions.is_empty() {
        let interventions_to_record = pending_interventions.clone();
        let sid_for_record = sid.clone();
        if let Err(e) = ctx
            .pool()
            .run(move |conn| {
                for intervention in &interventions_to_record {
                    if let Err(e) = interventions::record_intervention_sync(
                        conn,
                        project_id,
                        Some(&sid_for_record),
                        intervention,
                    ) {
                        tracing::warn!("Failed to record intervention: {}", e);
                    }
                }
                Ok::<_, String>(())
            })
            .await
        {
            tracing::warn!("Failed to record shown interventions: {}", e);
        }
    }

    // Phase 5: Codebase map
    if project_type == "rust" {
        match cartographer::get_or_generate_map_pool(
            ctx.code_pool().clone(),
            project_id,
            project_path.clone(),
            display_name.to_string(),
            project_type.to_string(),
        )
        .await
        {
            Ok(map) => {
                if !map.modules.is_empty() {
                    response.push_str(&cartographer::format_compact(&map));
                }
            }
            Err(e) => {
                tracing::warn!("Failed to generate codebase map: {}", e);
            }
        }
    }

    if let Some(db_path) = ctx.pool().path() {
        response.push_str(&format!("\nDatabase: {}\n", db_path.display()));
    }

    response.push_str("\nReady.");
    Ok(Json(ProjectOutput {
        action: "start".into(),
        message: response,
        data: Some(ProjectData::Start(ProjectStartData {
            project_id,
            project_name,
            project_path: project_path.clone(),
            project_type: project_type.to_string(),
        })),
    }))
}

/// Gather system context content for bash tool usage (returns content string, does not store)
fn gather_system_context_content() -> Option<String> {
    let mut context_parts = Vec::new();

    // OS info
    if let Ok(output) = Command::new("uname").args(["-s", "-r"]).output()
        && output.status.success()
    {
        let os = String::from_utf8_lossy(&output.stdout).trim().to_string();
        context_parts.push(format!("OS: {}", os));
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
    } else if let Ok(output) = Command::new("whoami").output()
        && output.status.success()
    {
        let user = String::from_utf8_lossy(&output.stdout).trim().to_string();
        context_parts.push(format!("User: {}", user));
    }

    // Home directory (try env, fallback to ~)
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            context_parts.push(format!("Home: {}", home));
        }
    } else if let Ok(output) = Command::new("sh").args(["-c", "echo ~"]).output()
        && output.status.success()
    {
        let home = String::from_utf8_lossy(&output.stdout).trim().to_string();
        context_parts.push(format!("Home: {}", home));
    }

    // Timezone
    if let Ok(output) = Command::new("date").arg("+%Z (UTC%:z)").output()
        && output.status.success()
    {
        let tz = String::from_utf8_lossy(&output.stdout).trim().to_string();
        context_parts.push(format!("Timezone: {}", tz));
    }

    // Available tools (check common ones via PATH scan)
    let tools_to_check = [
        "git",
        "cargo",
        "rustc",
        "npm",
        "node",
        "python3",
        "docker",
        "systemctl",
        "curl",
        "jq",
    ];
    if let Ok(path_var) = std::env::var("PATH") {
        let path_dirs: Vec<std::path::PathBuf> = std::env::split_paths(&path_var).collect();
        let found: Vec<&str> = tools_to_check
            .iter()
            .filter(|tool| path_dirs.iter().any(|dir| dir.join(tool).is_file()))
            .copied()
            .collect();
        if !found.is_empty() {
            context_parts.push(format!("Available tools: {}", found.join(", ")));
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
) -> Result<Json<ProjectOutput>, String> {
    match action {
        ProjectAction::Start => {
            let path = project_path.ok_or("project_path is required for project(action=start)")?;
            session_start(ctx, path, name, session_id).await
        }
        ProjectAction::Set => {
            let path = project_path.ok_or("project_path is required for project(action=set)")?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_file(dir: &std::path::Path, name: &str, content: &str) {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    fn make_fact(content: &str, fact_type: &str, category: Option<&str>) -> MemoryFact {
        MemoryFact {
            id: 1,
            project_id: Some(1),
            key: None,
            content: content.to_string(),
            fact_type: fact_type.to_string(),
            category: category.map(|s| s.to_string()),
            confidence: 0.8,
            created_at: "2026-01-01T00:00:00".to_string(),
            session_count: 1,
            first_session_id: None,
            last_session_id: None,
            status: "active".to_string(),
            user_id: None,
            scope: "project".to_string(),
            team_id: None,
            updated_at: None,
            branch: None,
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // detect_project_name
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_detect_name_cargo_package() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "Cargo.toml",
            "[package]\nname = \"my-crate\"\nversion = \"0.1.0\"\n",
        );
        let result = detect_project_name(dir.path().to_str().unwrap());
        assert_eq!(result.unwrap(), "my-crate");
    }

    #[test]
    fn test_detect_name_cargo_workspace() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\"]\n",
        );
        // Workspace has no package name, falls back to directory name
        let result = detect_project_name(dir.path().to_str().unwrap());
        let dir_name = dir.path().file_name().unwrap().to_str().unwrap();
        assert_eq!(result.unwrap(), dir_name);
    }

    #[test]
    fn test_detect_name_package_json() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "package.json",
            r#"{"name": "my-app", "version": "1.0.0"}"#,
        );
        let result = detect_project_name(dir.path().to_str().unwrap());
        assert_eq!(result.unwrap(), "my-app");
    }

    #[test]
    fn test_detect_name_package_json_empty_name() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "package.json",
            r#"{"name": "", "version": "1.0.0"}"#,
        );
        // Empty name should fall through to directory name
        let result = detect_project_name(dir.path().to_str().unwrap());
        let dir_name = dir.path().file_name().unwrap().to_str().unwrap();
        assert_eq!(result.unwrap(), dir_name);
    }

    #[test]
    fn test_detect_name_no_manifest() {
        let dir = tempfile::tempdir().unwrap();
        // Falls back to directory name
        let result = detect_project_name(dir.path().to_str().unwrap());
        let dir_name = dir.path().file_name().unwrap().to_str().unwrap();
        assert_eq!(result.unwrap(), dir_name);
    }

    #[test]
    fn test_detect_name_cargo_name_with_quotes() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "Cargo.toml",
            "[package]\nname = 'single-quoted'\n",
        );
        let result = detect_project_name(dir.path().to_str().unwrap());
        assert_eq!(result.unwrap(), "single-quoted");
    }

    #[test]
    fn test_detect_name_cargo_ignores_non_package_sections() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "Cargo.toml",
            "[dependencies]\nname = \"serde\"\n\n[package]\nname = \"real-name\"\n",
        );
        let result = detect_project_name(dir.path().to_str().unwrap());
        assert_eq!(result.unwrap(), "real-name");
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // detect_project_type
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_detect_type_rust() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "Cargo.toml", "");
        assert_eq!(detect_project_type(dir.path().to_str().unwrap()), "rust");
    }

    #[test]
    fn test_detect_type_node() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "package.json", "{}");
        assert_eq!(detect_project_type(dir.path().to_str().unwrap()), "node");
    }

    #[test]
    fn test_detect_type_python_pyproject() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "pyproject.toml", "");
        assert_eq!(detect_project_type(dir.path().to_str().unwrap()), "python");
    }

    #[test]
    fn test_detect_type_python_setup() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "setup.py", "");
        assert_eq!(detect_project_type(dir.path().to_str().unwrap()), "python");
    }

    #[test]
    fn test_detect_type_go() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "go.mod", "");
        assert_eq!(detect_project_type(dir.path().to_str().unwrap()), "go");
    }

    #[test]
    fn test_detect_type_java_maven() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "pom.xml", "");
        assert_eq!(detect_project_type(dir.path().to_str().unwrap()), "java");
    }

    #[test]
    fn test_detect_type_java_gradle() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "build.gradle", "");
        assert_eq!(detect_project_type(dir.path().to_str().unwrap()), "java");
    }

    #[test]
    fn test_detect_type_unknown() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(detect_project_type(dir.path().to_str().unwrap()), "unknown");
    }

    #[test]
    fn test_detect_type_priority_rust_over_node() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "Cargo.toml", "");
        write_file(dir.path(), "package.json", "{}");
        // Rust takes priority
        assert_eq!(detect_project_type(dir.path().to_str().unwrap()), "rust");
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // format_recent_sessions
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_format_sessions_with_summary() {
        let sessions = vec![(
            "abc12345-6789".to_string(),
            "2026-01-15T10:30:00".to_string(),
            Some("Fixed auth bug".to_string()),
            5,
            vec!["Read".to_string(), "Edit".to_string()],
        )];
        let result = format_recent_sessions(&sessions);
        assert!(result.contains("[abc12345]"));
        assert!(result.contains("2026-01-15T10:30"));
        assert!(result.contains("Fixed auth bug"));
    }

    #[test]
    fn test_format_sessions_with_tools_no_summary() {
        let sessions = vec![(
            "def67890-abcd".to_string(),
            "2026-01-15T10:30:00".to_string(),
            None,
            3,
            vec!["Bash".to_string(), "Grep".to_string()],
        )];
        let result = format_recent_sessions(&sessions);
        assert!(result.contains("3 tool calls"));
        assert!(result.contains("Bash, Grep"));
    }

    #[test]
    fn test_format_sessions_no_activity() {
        let sessions = vec![(
            "aaa00000-0000".to_string(),
            "2026-01-15T10:30:00".to_string(),
            None,
            0,
            vec![],
        )];
        let result = format_recent_sessions(&sessions);
        assert!(result.contains("(no activity)"));
    }

    #[test]
    fn test_format_sessions_empty() {
        let result = format_recent_sessions(&[]);
        assert!(result.contains("Recent sessions:"));
        assert!(result.contains("session(action="));
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // format_session_insights
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_format_insights_preferences() {
        let prefs = vec![make_fact(
            "Use tabs not spaces",
            "preference",
            Some("coding"),
        )];
        let result = format_session_insights(&prefs, &[], &[], &[], &[]);
        assert!(result.contains("Preferences:"));
        assert!(result.contains("[coding] Use tabs not spaces"));
    }

    #[test]
    fn test_format_insights_filters_preferences_from_context() {
        let memories = vec![
            make_fact("I'm a preference", "preference", None),
            make_fact("Actual context", "decision", None),
        ];
        let result = format_session_insights(&[], &memories, &[], &[], &[]);
        assert!(result.contains("Recent context:"));
        assert!(result.contains("Actual context"));
        // Preferences should be filtered out from context section
        assert!(!result.contains("I'm a preference"));
    }

    #[test]
    fn test_format_insights_health_alerts() {
        let alerts = vec![make_fact(
            "[unused] dead function",
            "health",
            Some("unused"),
        )];
        let result = format_session_insights(&[], &[], &alerts, &[], &[]);
        assert!(result.contains("Health alerts:"));
        assert!(result.contains("[unused]"));
    }

    #[test]
    fn test_format_insights_doc_tasks() {
        let doc_counts = vec![("pending".to_string(), 5)];
        let result = format_session_insights(&[], &[], &[], &[], &doc_counts);
        assert!(result.contains("5 items need docs"));
    }

    #[test]
    fn test_format_insights_no_pending_docs() {
        let doc_counts = vec![("completed".to_string(), 3)];
        let result = format_session_insights(&[], &[], &[], &[], &doc_counts);
        assert!(!result.contains("items need docs"));
    }

    #[test]
    fn test_format_insights_all_empty() {
        let result = format_session_insights(&[], &[], &[], &[], &[]);
        assert!(result.is_empty());
    }
}
