// crates/mira-server/src/background/capabilities.rs
// Background worker for generating codebase capabilities inventory

use crate::cartographer;
use crate::db::Database;
use crate::embeddings::EmbeddingClient;
use crate::llm::{DeepSeekClient, PromptBuilder};
use crate::search::embedding_to_bytes;
use rusqlite::params;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

/// Check if capabilities inventory needs regeneration and process if so
pub async fn process_capabilities(
    db: &Arc<Database>,
    deepseek: &Arc<DeepSeekClient>,
    embeddings: Option<&Arc<EmbeddingClient>>,
) -> Result<usize, String> {
    // Get projects that need capability scanning (run on blocking thread)
    let db_clone = db.clone();
    let projects = tokio::task::spawn_blocking(move || {
        let conn = db_clone.conn();
        get_projects_needing_scan(&conn)
    }).await.map_err(|e| format!("spawn_blocking panicked: {}", e))??;
    if !projects.is_empty() {
        tracing::info!("Capabilities: found {} projects needing scan", projects.len());
    }

    let mut processed = 0;

    for (project_id, project_path) in projects {
        // Check if project path exists
        if !Path::new(&project_path).exists() {
            continue;
        }

        // Generate capabilities inventory
        match generate_capabilities_inventory(db, deepseek, embeddings, project_id, &project_path).await {
            Ok(count) => {
                tracing::info!(
                    "Generated {} capabilities for project {} ({})",
                    count,
                    project_id,
                    project_path
                );
                processed += count;

                // Update last scan timestamp with git commit
                mark_capabilities_scanned(db, project_id, &project_path)?;
            }
            Err(e) => {
                tracing::warn!("Failed to generate capabilities for {}: {}", project_path, e);
            }
        }
    }

    Ok(processed)
}

/// Get projects that need a capabilities scan
/// Triggers on:
/// 1. First scan (never scanned before)
/// 2. Git HEAD changed AND last scan > 1 day ago (rate limited)
/// 3. Last scan > 7 days ago (periodic refresh)
fn get_projects_needing_scan(conn: &rusqlite::Connection) -> Result<Vec<(i64, String)>, String> {
    // Get all indexed projects
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT p.id, p.path
             FROM projects p
             JOIN codebase_modules m ON m.project_id = p.id",
        )
        .map_err(|e| e.to_string())?;

    let all_projects: Vec<(i64, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let mut needing_scan = Vec::new();

    for (project_id, project_path) in all_projects {
        if needs_capabilities_scan(conn, project_id, &project_path)? {
            tracing::debug!("Capabilities: project {} needs scan", project_id);
            needing_scan.push((project_id, project_path));
            // Only process one project per cycle to avoid long delays
            break;
        }
    }

    Ok(needing_scan)
}

/// Check if a specific project needs a capabilities scan
fn needs_capabilities_scan(conn: &rusqlite::Connection, project_id: i64, project_path: &str) -> Result<bool, String> {
    // Get last scan info
    let scan_info: Option<(String, String)> = conn
        .query_row(
            "SELECT content, updated_at FROM memory_facts
             WHERE project_id = ? AND key = 'capabilities_scan_time'",
            [project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();

    let (last_commit, last_scan_time) = match scan_info {
        Some((commit, time)) => (Some(commit), Some(time)),
        None => (None, None), // Never scanned
    };

    // Case 1: Never scanned
    if last_commit.is_none() {
        tracing::debug!("Project {} needs scan: never scanned", project_id);
        return Ok(true);
    }

    // Get current git HEAD
    let current_commit = get_git_head(project_path);

    // Case 2: Git changed AND rate limit passed (> 1 day since last scan)
    if let (Some(last), Some(current)) = (&last_commit, &current_commit) {
        if last != current {
            // Check rate limit - only rescan if last scan was > 1 day ago
            if let Some(ref scan_time) = last_scan_time {
                let older_than_1_day: bool = conn
                    .query_row(
                        "SELECT datetime(?) < datetime('now', '-1 day')",
                        [scan_time],
                        |row| row.get(0),
                    )
                    .unwrap_or(false);

                if older_than_1_day {
                    tracing::debug!(
                        "Project {} needs scan: git changed ({} -> {}) and rate limit passed",
                        project_id, &last[..8.min(last.len())], &current[..8.min(current.len())]
                    );
                    return Ok(true);
                }
            }
        }
    }

    // Case 3: Periodic refresh (> 7 days since last scan)
    if let Some(ref scan_time) = last_scan_time {
        let older_than_7_days: bool = conn
            .query_row(
                "SELECT datetime(?) < datetime('now', '-7 days')",
                [scan_time],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if older_than_7_days {
            tracing::debug!("Project {} needs scan: periodic refresh (> 7 days)", project_id);
            return Ok(true);
        }
    }

    Ok(false)
}

/// Get the current git HEAD commit hash
fn get_git_head(project_path: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(project_path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Mark that we've scanned a project's capabilities (stores git commit)
fn mark_capabilities_scanned(db: &Database, project_id: i64, project_path: &str) -> Result<(), String> {
    // Store the current git commit as the scan marker
    let commit = get_git_head(project_path).unwrap_or_else(|| "unknown".to_string());

    db.store_memory(
        Some(project_id),
        Some("capabilities_scan_time"),
        &commit,
        "system",
        Some("capabilities"),
        1.0,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Max bytes of code to send per module (30KB each)
const MAX_MODULE_CODE_BYTES: usize = 30_000;

/// Max total bytes for all module context (200KB ≈ 50K tokens, fits in DeepSeek's 64K limit)
const MAX_TOTAL_CONTEXT_BYTES: usize = 200_000;

/// Generate the full capabilities inventory for a project
async fn generate_capabilities_inventory(
    db: &Arc<Database>,
    deepseek: &Arc<DeepSeekClient>,
    embeddings: Option<&Arc<EmbeddingClient>>,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    // Get the codebase map with module info
    let modules = cartographer::get_modules_with_purposes_async(db.clone(), project_id)
        .await
        .map_err(|e| e.to_string())?;

    if modules.is_empty() {
        return Ok(0);
    }

    // Build context about the codebase with FULL code
    let path = Path::new(project_path);
    let mut module_context = String::new();

    for module in &modules {
        // Check if we're approaching the limit
        if module_context.len() >= MAX_TOTAL_CONTEXT_BYTES {
            tracing::info!(
                "Capabilities: stopping at {} bytes (limit: {}), included {} modules",
                module_context.len(),
                MAX_TOTAL_CONTEXT_BYTES,
                modules.iter().take_while(|m| m.id != module.id).count()
            );
            break;
        }

        let mut module_section = format!("\n## Module: {}\n", module.id);
        if let Some(ref purpose) = module.purpose {
            module_section.push_str(&format!("Purpose: {}\n", purpose));
        }
        if !module.exports.is_empty() {
            let exports_preview: Vec<_> = module.exports.iter().take(30).cloned().collect();
            module_section.push_str(&format!("Key exports: {}\n", exports_preview.join(", ")));
        }

        // Get FULL module code (not just preview)
        let full_code = cartographer::get_module_full_code(path, &module.path, MAX_MODULE_CODE_BYTES);
        if !full_code.is_empty() {
            module_section.push_str(&format!("\n```rust\n{}\n```\n", full_code));
        }

        // Only add if it won't exceed the limit
        if module_context.len() + module_section.len() <= MAX_TOTAL_CONTEXT_BYTES {
            module_context.push_str(&module_section);
        } else {
            tracing::info!(
                "Capabilities: skipping module {} (would exceed limit)",
                module.id
            );
        }
    }

    tracing::info!(
        "Capabilities: sending {} bytes of context to DeepSeek (~{} tokens)",
        module_context.len(),
        module_context.len() / 4  // Rough estimate: 4 chars per token for code
    );

    // Ask Reasoner to extract capabilities (NO issues - that's handled by code_health scanner)
    let prompt = format!(
        r#"Analyze this Rust codebase and list its CAPABILITIES - what can users and developers DO with it.

Focus on:
- MCP tools (functions exposed to Claude Code via the MCP protocol)
- API endpoints (HTTP routes in the web module)
- CLI commands (if any)
- Background automation features
- Key public APIs and their purposes

For each capability, describe:
1. What action users/developers can perform
2. Which module provides it
3. The key function or endpoint name

Format your response as:

CAPABILITIES:
- [module_name] Description of what users can do (via function_name or /endpoint)

Only list working, implemented capabilities. Do NOT list problems, issues, or incomplete features.

=== CODEBASE ===
{}"#,
        module_context
    );

    let messages = PromptBuilder::for_capabilities()
        .build_messages(prompt);

    let result = deepseek
        .chat(messages, None)
        .await
        .map_err(|e| format!("DeepSeek request failed: {}", e))?;

    let content = result
        .content
        .ok_or("No content in DeepSeek response")?;

    // Parse and store capabilities only
    let stored = parse_and_store_capabilities(db, embeddings, project_id, &content).await?;

    Ok(stored)
}

/// Parse the Reasoner response and store capabilities as memories with embeddings
async fn parse_and_store_capabilities(
    db: &Arc<Database>,
    embeddings: Option<&Arc<EmbeddingClient>>,
    project_id: i64,
    response: &str,
) -> Result<usize, String> {
    let mut stored = 0;

    // Clear old capabilities for this project (run on blocking thread)
    let db_clone = db.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db_clone.conn();
        clear_old_capabilities(&conn, project_id)
    }).await.map_err(|e| format!("spawn_blocking panicked: {}", e))??;

    let mut in_capabilities = false;
    let mut capability_index = 0;

    for line in response.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("CAPABILITIES:") || trimmed.starts_with("**CAPABILITIES") {
            in_capabilities = true;
            continue;
        }

        // Stop if we hit a different section
        if in_capabilities && (trimmed.starts_with("ISSUES") || trimmed.starts_with("**ISSUES")
            || trimmed.starts_with("NOTES") || trimmed.starts_with("**NOTES")) {
            break;
        }

        if in_capabilities && trimmed.starts_with("- ") {
            let content = trimmed.trim_start_matches("- ").trim();
            if content.is_empty() {
                continue;
            }

            // Store as capability with embedding
            let key = format!("capability:{}", capability_index);
            let id = db.store_memory(
                Some(project_id),
                Some(&key),
                content,
                "capability",
                Some("codebase"),
                1.0,
            )
            .map_err(|e| e.to_string())?;

            // Generate and store embedding (run on blocking thread)
            if let Some(emb_client) = embeddings {
                if let Ok(embedding) = emb_client.embed(content).await {
                    let db_clone = db.clone();
                    let content_owned = content.to_string();
                    tokio::task::spawn_blocking(move || {
                        let conn = db_clone.conn();
                        store_embedding(&conn, id, &content_owned, &embedding)
                    }).await.map_err(|e| format!("spawn_blocking panicked: {}", e))??;
                }
            }

            capability_index += 1;
            stored += 1;
        }
    }

    Ok(stored)
}

/// Store embedding for a memory fact
fn store_embedding(conn: &rusqlite::Connection, fact_id: i64, content: &str, embedding: &[f32]) -> Result<(), String> {
    let embedding_bytes = embedding_to_bytes(embedding);

    conn.execute(
        "INSERT OR REPLACE INTO vec_memory (rowid, embedding, fact_id, content) VALUES (?, ?, ?, ?)",
        params![fact_id, embedding_bytes, fact_id, content],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// Clear old capabilities before refresh (issues are handled by code_health scanner)
fn clear_old_capabilities(conn: &rusqlite::Connection, project_id: i64) -> Result<(), String> {
    // Delete old capabilities only (issues managed by code_health scanner)
    conn.execute(
        "DELETE FROM memory_facts WHERE project_id = ? AND fact_type = 'capability' AND category = 'codebase'",
        [project_id],
    )
    .map_err(|e| e.to_string())?;

    // Clean up orphaned embeddings
    conn.execute(
        "DELETE FROM vec_memory WHERE fact_id NOT IN (SELECT id FROM memory_facts)",
        [],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}
