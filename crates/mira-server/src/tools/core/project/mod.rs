// tools/core/project/mod.rs
// Unified project tools

mod detection;
mod formatting;
mod session_start;

use mira_types::{MemoryFact, ProjectContext};

use crate::db::{get_or_create_project_sync, save_active_project_sync, update_project_name_sync};
use crate::error::MiraError;
use crate::mcp::requests::ProjectAction;
use crate::mcp::responses::Json;
use crate::mcp::responses::{ProjectData, ProjectGetData, ProjectOutput, ProjectSetData};
use crate::proactive::interventions;
use crate::tools::core::{NO_ACTIVE_PROJECT_ERROR, ToolContext};

pub use detection::detect_project_type;
pub use session_start::session_start;

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

/// Shared project initialization logic
async fn init_project<C: ToolContext>(
    ctx: &C,
    project_path: &str,
    name: Option<&str>,
) -> Result<(i64, Option<String>), MiraError> {
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
        let detected = detection::detect_project_name(project_path);
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
) -> Result<Json<ProjectOutput>, MiraError> {
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
pub async fn get_project<C: ToolContext>(ctx: &C) -> Result<Json<ProjectOutput>, MiraError> {
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

/// Unified project tool with action parameter
/// Actions: start (session_start), set (set_project), get (get_project)
pub async fn project<C: ToolContext>(
    ctx: &C,
    action: ProjectAction,
    project_path: Option<String>,
    name: Option<String>,
    session_id: Option<String>,
) -> Result<Json<ProjectOutput>, MiraError> {
    match action {
        ProjectAction::Start => {
            let path = project_path.ok_or_else(|| {
                MiraError::InvalidInput(
                    "project_path is required for project(action=start)".to_string(),
                )
            })?;
            session_start(ctx, path, name, session_id).await
        }
        ProjectAction::Set => {
            let path = project_path.ok_or_else(|| {
                MiraError::InvalidInput(
                    "project_path is required for project(action=set)".to_string(),
                )
            })?;
            set_project(ctx, path, name).await
        }
        ProjectAction::Get => get_project(ctx).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use detection::{detect_project_name, detect_project_type};
    use formatting::{format_recent_sessions, format_session_insights};
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
