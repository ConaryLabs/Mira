// crates/mira-server/src/tools/core/project.rs
// Unified project tools

use mira_types::{MemoryFact, ProjectContext};
use std::path::Path;
use std::process::Command;

use crate::cartographer;
use crate::db::Database;
use crate::tools::core::claude_local;
use crate::tools::core::ToolContext;

/// Sync helper: search memories by text (for use inside run_blocking)
fn search_memories_sync(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    query: &str,
    limit: usize,
) -> Result<Vec<MemoryFact>, String> {
    use rusqlite::params;

    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let pattern = format!("%{}%", escaped);

    let mut stmt = conn
        .prepare(
            "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                    session_count, first_session_id, last_session_id, status
             FROM memory_facts
             WHERE (project_id = ? OR project_id IS NULL) AND content LIKE ? ESCAPE '\\'
             ORDER BY updated_at DESC
             LIMIT ?",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![project_id, pattern, limit as i64], |row| {
            Ok(MemoryFact {
                id: row.get(0)?,
                project_id: row.get(1)?,
                key: row.get(2)?,
                content: row.get(3)?,
                fact_type: row.get(4)?,
                category: row.get(5)?,
                confidence: row.get(6)?,
                created_at: row.get(7)?,
                session_count: row.get(8).unwrap_or(1),
                first_session_id: row.get(9).ok(),
                last_session_id: row.get(10).ok(),
                status: row.get(11).unwrap_or_else(|_| "candidate".to_string()),
            })
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

/// Sync helper: get preferences (for use inside run_blocking)
fn get_preferences_sync(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
) -> Result<Vec<MemoryFact>, String> {
    use rusqlite::params;

    let mut stmt = conn
        .prepare(
            "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                    session_count, first_session_id, last_session_id, status
             FROM memory_facts
             WHERE (project_id = ? OR project_id IS NULL) AND fact_type = 'preference'
             ORDER BY category, created_at DESC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![project_id], |row| {
            Ok(MemoryFact {
                id: row.get(0)?,
                project_id: row.get(1)?,
                key: row.get(2)?,
                content: row.get(3)?,
                fact_type: row.get(4)?,
                category: row.get(5)?,
                confidence: row.get(6)?,
                created_at: row.get(7)?,
                session_count: row.get(8).unwrap_or(1),
                first_session_id: row.get(9).ok(),
                last_session_id: row.get(10).ok(),
                status: row.get(11).unwrap_or_else(|_| "candidate".to_string()),
            })
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

/// Sync helper: get health alerts (for use inside run_blocking)
fn get_health_alerts_sync(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    limit: usize,
) -> Result<Vec<MemoryFact>, String> {
    use rusqlite::params;

    let mut stmt = conn
        .prepare(
            "SELECT id, project_id, key, content, fact_type, category, confidence, created_at,
                    session_count, first_session_id, last_session_id, status
             FROM memory_facts
             WHERE (project_id = ? OR project_id IS NULL)
               AND fact_type = 'health'
               AND confidence >= 0.7
             ORDER BY confidence DESC, updated_at DESC
             LIMIT ?",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![project_id, limit as i64], |row| {
            Ok(MemoryFact {
                id: row.get(0)?,
                project_id: row.get(1)?,
                key: row.get(2)?,
                content: row.get(3)?,
                fact_type: row.get(4)?,
                category: row.get(5)?,
                confidence: row.get(6)?,
                created_at: row.get(7)?,
                session_count: row.get(8).unwrap_or(1),
                first_session_id: row.get(9).ok(),
                last_session_id: row.get(10).ok(),
                status: row.get(11).unwrap_or_else(|_| "candidate".to_string()),
            })
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

/// Sync helper: create or update session (for use inside pool.interact)
fn create_session_sync(
    conn: &rusqlite::Connection,
    session_id: &str,
    project_id: Option<i64>,
) -> Result<(), String> {
    use rusqlite::params;
    conn.execute(
        "INSERT INTO sessions (id, project_id, status, started_at, last_activity)
         VALUES (?1, ?2, 'active', datetime('now'), datetime('now'))
         ON CONFLICT(id) DO UPDATE SET last_activity = datetime('now')",
        params![session_id, project_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Sync helper: get or create project (for use inside pool.interact)
fn get_or_create_project_sync(
    conn: &rusqlite::Connection,
    path: &str,
    name: Option<&str>,
) -> Result<(i64, Option<String>), String> {
    use rusqlite::params;

    // UPSERT: insert or get existing.
    let (id, stored_name): (i64, Option<String>) = conn
        .query_row(
            "INSERT INTO projects (path, name) VALUES (?, ?)
             ON CONFLICT(path) DO UPDATE SET
                 name = COALESCE(projects.name, excluded.name),
                 created_at = projects.created_at
             RETURNING id, name",
            params![path, name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| e.to_string())?;

    // If we have a name, return it
    if stored_name.is_some() {
        return Ok((id, stored_name));
    }

    // Auto-detect name from project files
    let detected_name = detect_project_name(path);

    if detected_name.is_some() {
        conn.execute(
            "UPDATE projects SET name = ? WHERE id = ?",
            params![&detected_name, id],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok((id, detected_name))
}

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
    let (project_id, project_name) = ctx
        .pool()
        .interact(move |conn| {
            get_or_create_project_sync(conn, &path_owned, name_owned.as_deref())
                .map_err(|e| anyhow::anyhow!(e))
        })
        .await
        .map_err(|e| e.to_string())?;

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

    // Persist active project for restart recovery (still use legacy db for this non-critical operation)
    if let Err(e) = ctx.db().save_active_project(project_path) {
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

    // Initialize project (shared with set_project)
    let (project_id, project_name) = init_project(ctx, &project_path, name.as_deref()).await?;

    // Set session ID (use provided, or generate new)
    let sid = session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Create session via pool (same database as project)
    let sid_clone = sid.clone();
    ctx.pool()
        .interact(move |conn| {
            create_session_sync(conn, &sid_clone, Some(project_id)).map_err(|e| anyhow::anyhow!(e))
        })
        .await
        .map_err(|e| e.to_string())?;

    ctx.set_session_id(sid.clone()).await;

    // Persist active session ID for restart recovery (CLI tools)
    if let Err(e) = db.set_server_state("active_session_id", &sid) {
        tracing::warn!("Failed to persist active session ID: {}", e);
    }

    // Gather and store system context (for bash tool awareness)
    gather_system_context(db);

    // Import CLAUDE.local.md entries as memories (if file exists)
    let imported_count = claude_local::import_claude_local_md_async(ctx.pool(), project_id, &project_path)
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

    // Check for "What's New" briefing (git changes since last session)
    if let Ok(Some(briefing)) = db.get_project_briefing(project_id) {
        if let Some(text) = &briefing.briefing_text {
            response.push_str(&format!("\nWhat's new: {}\n", text));
        }
    }

    // Mark that a session occurred (clears briefing for next time)
    if let Err(e) = db.mark_session_for_briefing(project_id) {
        tracing::warn!("Failed to mark session for briefing: {}", e);
    }

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

    // Load preferences, memories, and health alerts in a single pool call
    let (preferences, memories, health_alerts): (Vec<MemoryFact>, Vec<MemoryFact>, Vec<MemoryFact>) =
        ctx.pool()
            .interact(move |conn| {
                // Get preferences
                let preferences = get_preferences_sync(conn, Some(project_id)).unwrap_or_default();

                // Get recent memories
                let memories = search_memories_sync(conn, Some(project_id), "", 10).unwrap_or_default();

                // Get health alerts
                let health_alerts = get_health_alerts_sync(conn, Some(project_id), 5).unwrap_or_default();

                Ok((preferences, memories, health_alerts))
            })
            .await
            .map_err(|e| e.to_string())?;

    if !preferences.is_empty() {
        response.push_str("\nPreferences:\n");
        for pref in &preferences {
            let category = pref.category.as_deref().unwrap_or("general");
            response.push_str(&format!("  [{}] {}\n", category, pref.content));
        }
    }

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
        let db_clone = db.clone();
        match cartographer::get_or_generate_map_async(
            db_clone,
            project_id,
            &project_path,
            display_name,
            project_type,
        ).await {
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
        if let Err(e) = db.store_memory(
            None, // global, not project-specific
            Some("system_context"),
            &content,
            "context",
            Some("system"),
            1.0,
        ) {
            tracing::warn!("Failed to store system context memory: {}", e);
        }
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
