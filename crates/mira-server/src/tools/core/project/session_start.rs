// tools/core/project/session_start.rs
// Session initialization: start, persist, load history, recap data, onboarding

use crate::cartographer;
use crate::db::documentation::count_doc_tasks_by_status;
use crate::db::{
    StoreObservationParams, get_project_briefing_sync, get_recent_sessions_sync,
    get_session_stats_sync, mark_session_for_briefing_sync, set_server_state_sync,
    store_observation_sync, upsert_session_with_branch_sync,
};
use crate::error::MiraError;
use crate::git::get_git_branch;
use crate::mcp::responses::Json;
use crate::mcp::responses::{ProjectData, ProjectOutput, ProjectStartData};
use crate::tools::core::ToolContext;

use super::detection::{detect_project_type, detect_project_types, gather_system_context_content};
use super::formatting::{format_recent_sessions, format_session_insights};
use super::init_project;
use super::{RecapData, SessionInfo};

/// Persist session: create session record, store system context, retrieve briefing
async fn persist_session<C: ToolContext>(
    ctx: &C,
    project_id: i64,
    sid: &str,
    branch: Option<&str>,
) -> Result<Option<String>, MiraError> {
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
            )?;

            set_server_state_sync(conn, "active_session_id", &sid_owned)?;

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

            Ok::<_, rusqlite::Error>(briefing)
        })
        .await
}

/// Load recent sessions (excluding current), with stats
async fn load_recent_sessions<C: ToolContext>(
    ctx: &C,
    project_id: i64,
    current_sid: &str,
) -> Result<Vec<SessionInfo>, MiraError> {
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

/// Load recap data: doc counts
async fn load_recap_data<C: ToolContext>(ctx: &C, project_id: i64) -> Result<RecapData, MiraError> {
    ctx.pool()
        .run(move |conn| {
            let doc_task_counts =
                count_doc_tasks_by_status(conn, Some(project_id)).unwrap_or_default();
            Ok::<_, String>(doc_task_counts)
        })
        .await
}

/// Initialize session with project
pub async fn session_start<C: ToolContext>(
    ctx: &C,
    project_path: String,
    name: Option<String>,
    session_id: Option<String>,
) -> Result<Json<ProjectOutput>, MiraError> {
    let (project_id, project_name) = init_project(ctx, &project_path, name.as_deref()).await?;
    let sid = session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let branch = get_git_branch(&project_path);

    // Phase 1: Persist session + retrieve briefing
    let briefing_text = persist_session(ctx, project_id, &sid, branch.as_deref()).await?;
    ctx.set_session_id(sid.clone()).await;
    ctx.set_branch(branch.clone()).await;

    // Phase 2: Build response
    let project_types = detect_project_types(&project_path);
    let project_type = detect_project_type(&project_path);
    let display_name = project_name.as_deref().unwrap_or("unnamed");
    let type_label = if project_types.len() > 1 {
        project_types.join(", ")
    } else {
        project_type.to_string()
    };
    let mut response = format!("Project: {} ({})\n", display_name, type_label);
    // Warn for unsupported languages present in the project
    for lang in project_types.iter().filter(|t| matches!(**t, "java")) {
        response.push_str(&format!(
            "Note: {} project detected but not yet supported for code intelligence. Indexing will use file-level analysis only.\n",
            lang
        ));
    }
    if let Some(text) = briefing_text {
        response.push_str(&format!("\nWhat's new: {}\n", text));
    }

    // Phase 3: Load session history + recap data
    let recent_session_data = load_recent_sessions(ctx, project_id, &sid).await?;
    if !recent_session_data.is_empty() {
        response.push_str(&format_recent_sessions(&recent_session_data));
    } else {
        // First session — try onboarding via elicitation
        let onboarding_done = run_first_session_onboarding(ctx, project_id, &sid).await;
        if onboarding_done {
            response.push_str("\nFirst session — onboarding preferences saved.\n");
        } else {
            response.push_str(
                "\nFirst session for this project. Tip: use session(action=\"recap\") for context, or memory(action=\"remember\", content=\"...\") to store a decision.\n",
            );
        }
    }

    let doc_task_counts = load_recap_data(ctx, project_id).await?;

    response.push_str(&format_session_insights(&doc_task_counts));

    // Phase 4: Codebase map (supported languages: rust, python, node, go)
    let supported_for_map = project_types
        .iter()
        .any(|t| matches!(*t, "rust" | "python" | "node" | "go"));
    if supported_for_map {
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

    // Status line: symbol count, active goal count
    let symbol_count: i64 = ctx
        .code_pool()
        .run(move |conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM code_symbols WHERE project_id = ?",
                rusqlite::params![project_id],
                |row| row.get(0),
            )
        })
        .await
        .unwrap_or(0);

    let goal_count: i64 = ctx
        .pool()
        .run(move |conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM goals WHERE project_id = ? AND status NOT IN ('completed', 'abandoned')",
                rusqlite::params![project_id],
                |row| row.get(0),
            )
        })
        .await
        .unwrap_or(0);

    // Capability mode detection (background LLM removed; only embeddings matter)
    let has_embeddings = ctx.embeddings().is_some();

    let (mode, mode_detail) = if has_embeddings {
        ("semantic", None)
    } else {
        (
            "local",
            Some("keyword + fuzzy search active | add OPENAI_API_KEY for semantic search"),
        )
    };

    response.push_str(&format!(
        "\nMira: {} symbols indexed | {} active goals | mode: {}\n",
        symbol_count, goal_count, mode
    ));

    if let Some(detail) = mode_detail {
        response.push_str(&format!("  {}\n", detail));
    }

    // Lightweight stale index detection: compare last indexed_at against git HEAD
    if symbol_count > 0 {
        let pp = project_path.clone();
        let stale_check = async {
            // Get the newest indexed_at timestamp for this project
            let last_indexed: Option<String> = ctx
                .code_pool()
                .run(move |conn| {
                    conn.query_row(
                        "SELECT MAX(indexed_at) FROM code_symbols WHERE project_id = ?",
                        rusqlite::params![project_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| MiraError::Other(e.to_string()))
                })
                .await
                .ok();

            let last_indexed = last_indexed?;

            // Validate last_indexed before interpolating into git argument.
            // Only allow standard datetime characters to prevent argument injection.
            if !is_safe_datetime_string(&last_indexed) {
                tracing::warn!("Stale index check: skipping due to unexpected last_indexed format");
                return None;
            }

            // Check if git HEAD has changed since the last index
            // by looking for commits after the indexed_at timestamp.
            // Use spawn_blocking since Command::output is a blocking call.
            let after_arg = format!("--after={}", last_indexed);
            let output = tokio::task::spawn_blocking(move || {
                std::process::Command::new("git")
                    .args(["log", "--oneline", "-1", &after_arg])
                    .current_dir(&pp)
                    .output()
                    .ok()
            })
            .await
            .ok()
            .flatten()?;
            if !output.status.success() {
                return None;
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.trim().is_empty() {
                // No commits after indexed_at — index is up to date
                None
            } else {
                Some(())
            }
        };

        if stale_check.await.is_some() {
            if ctx.watcher().is_some() {
                response.push_str(
                    "\nNote: Code index may be stale (new commits detected). Background re-index triggered; run index(action='project') to force a full refresh.\n",
                );
            } else {
                response.push_str(
                    "\nNote: Code index may be stale (new commits detected). Run index(action='project') to refresh.\n",
                );
            }
            // Trigger background re-index if file watcher is available
            if let Some(watcher) = ctx.watcher() {
                tracing::info!(
                    "Stale index detected for project {}, triggering background re-index",
                    project_id
                );
                watcher
                    .watch(project_id, std::path::PathBuf::from(&project_path))
                    .await;
            }
        }
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

/// Run first-session onboarding via elicitation.
///
/// Memory storage removed. Returns false (no-op).
async fn run_first_session_onboarding<C: ToolContext>(
    _ctx: &C,
    _project_id: i64,
    _session_id: &str,
) -> bool {
    false
}

/// Validate that a string contains only characters safe for use as a git
/// `--after=<date>` argument. Prevents argument injection via a malformed
/// indexed_at timestamp stored in the DB.
///
/// Allowed characters: ASCII alphanumeric, T, :, ., -, +, Z, space
pub(crate) fn is_safe_datetime_string(s: &str) -> bool {
    !s.is_empty()
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, 'T' | ':' | '.' | '-' | '+' | 'Z' | ' ')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // is_safe_datetime_string: argument injection guard
    // =========================================================================

    #[test]
    fn valid_iso8601_datetime_passes() {
        assert!(is_safe_datetime_string("2026-01-15T10:30:00Z"));
        assert!(is_safe_datetime_string("2026-01-15T10:30:00.123Z"));
        assert!(is_safe_datetime_string("2026-01-15T10:30:00+05:30"));
        assert!(is_safe_datetime_string("2026-01-15T10:30:00-08:00"));
    }

    #[test]
    fn valid_sqlite_datetime_passes() {
        // SQLite datetime() returns "YYYY-MM-DD HH:MM:SS"
        assert!(is_safe_datetime_string("2026-01-15 10:30:00"));
        assert!(is_safe_datetime_string("2026-02-24 08:00:00"));
    }

    #[test]
    fn empty_string_fails() {
        assert!(!is_safe_datetime_string(""));
    }

    #[test]
    fn string_with_semicolon_fails() {
        // Semicolon could be used to inject shell commands in some contexts
        assert!(!is_safe_datetime_string("2026-01-15T10:30:00Z; rm -rf /"));
    }

    #[test]
    fn string_with_shell_special_chars_fails() {
        assert!(!is_safe_datetime_string("2026-01-15`whoami`"));
        assert!(!is_safe_datetime_string("2026-01-15$(cmd)"));
        assert!(!is_safe_datetime_string("2026-01-15\nnewline"));
        assert!(!is_safe_datetime_string("2026-01-15|pipe"));
        assert!(!is_safe_datetime_string("2026-01-15&amp"));
    }

    #[test]
    fn string_with_quote_chars_fails() {
        assert!(!is_safe_datetime_string("2026-01-15'quote"));
        assert!(!is_safe_datetime_string("2026-01-15\"dquote"));
    }

    #[test]
    fn string_with_slash_fails() {
        // Slashes are not needed in datetime strings and could indicate a path
        assert!(!is_safe_datetime_string("2026/01/15"));
    }

    // =========================================================================
    // detect_project_type (re-exported from detection.rs)
    // These tests run against the real Mira repo on disk.
    // =========================================================================

    #[test]
    fn detects_rust_project_from_cargo_toml() {
        // Mira itself is a Rust workspace
        let project_root = env!("CARGO_MANIFEST_DIR")
            .trim_end_matches("/crates/mira-server")
            .to_string();
        // Walk up until we find Cargo.toml at root
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        // The workspace root is two levels up from crates/mira-server
        let root = path.parent().and_then(|p| p.parent()).unwrap_or(path);
        if root.join("Cargo.toml").exists() {
            let project_type = super::super::detection::detect_project_type(
                root.to_str().unwrap_or(&project_root),
            );
            assert_eq!(project_type, "rust");
        }
    }

    #[test]
    fn detects_unknown_for_empty_temp_dir() {
        let dir = std::env::temp_dir().join("mira_test_no_project_files");
        std::fs::create_dir_all(&dir).ok();
        let project_type =
            super::super::detection::detect_project_type(dir.to_str().unwrap_or("/tmp"));
        assert_eq!(project_type, "unknown");
    }

    #[test]
    fn detects_node_project() {
        let dir = std::env::temp_dir().join("mira_test_node_project");
        std::fs::create_dir_all(&dir).ok();
        std::fs::write(
            dir.join("package.json"),
            r#"{"name":"test","version":"1.0.0"}"#,
        )
        .ok();
        let project_type =
            super::super::detection::detect_project_type(dir.to_str().unwrap_or("/tmp"));
        assert_eq!(project_type, "node");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn detects_python_project_pyproject_toml() {
        let dir = std::env::temp_dir().join("mira_test_python_project");
        std::fs::create_dir_all(&dir).ok();
        std::fs::write(
            dir.join("pyproject.toml"),
            "[tool.poetry]\nname = \"test\"\n",
        )
        .ok();
        let project_type =
            super::super::detection::detect_project_type(dir.to_str().unwrap_or("/tmp"));
        assert_eq!(project_type, "python");
        std::fs::remove_dir_all(&dir).ok();
    }
}
