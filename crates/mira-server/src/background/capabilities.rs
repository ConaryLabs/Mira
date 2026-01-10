// crates/mira-server/src/background/capabilities.rs
// Background worker for generating codebase capabilities inventory

use crate::cartographer;
use crate::db::Database;
use crate::embeddings::EmbeddingClient;
use crate::web::deepseek::{DeepSeekClient, Message};
use rusqlite::params;
use std::path::Path;
use std::sync::Arc;

/// Check if capabilities inventory needs regeneration and process if so
pub async fn process_capabilities(
    db: &Arc<Database>,
    deepseek: &Arc<DeepSeekClient>,
    embeddings: Option<&Arc<EmbeddingClient>>,
) -> Result<usize, String> {
    // Get projects that need capability scanning
    let projects = get_projects_needing_scan(db)?;

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

                // Update last scan timestamp
                mark_capabilities_scanned(db, project_id)?;
            }
            Err(e) => {
                tracing::warn!("Failed to generate capabilities for {}: {}", project_path, e);
            }
        }
    }

    Ok(processed)
}

/// Get projects that haven't had a capabilities scan recently
fn get_projects_needing_scan(db: &Database) -> Result<Vec<(i64, String)>, String> {
    let conn = db.conn();

    // Projects where:
    // 1. No capabilities scan ever (no memory with key starting with 'capabilities:')
    // 2. OR last scan was > 7 days ago
    // 3. AND project has been indexed (has modules)
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT p.id, p.path
             FROM projects p
             JOIN codebase_modules m ON m.project_id = p.id
             WHERE NOT EXISTS (
                 SELECT 1 FROM memory_facts mf
                 WHERE mf.project_id = p.id
                 AND mf.key = 'capabilities_scan_time'
                 AND mf.updated_at > datetime('now', '-7 days')
             )
             LIMIT 1",
        )
        .map_err(|e| e.to_string())?;

    let projects: Vec<(i64, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(projects)
}

/// Mark that we've scanned a project's capabilities
fn mark_capabilities_scanned(db: &Database, project_id: i64) -> Result<(), String> {
    db.store_memory(
        Some(project_id),
        Some("capabilities_scan_time"),
        &chrono::Utc::now().to_rfc3339(),
        "system",
        Some("capabilities"),
        1.0,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Generate the full capabilities inventory for a project
async fn generate_capabilities_inventory(
    db: &Arc<Database>,
    deepseek: &Arc<DeepSeekClient>,
    embeddings: Option<&Arc<EmbeddingClient>>,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    // Get the codebase map with module info
    let modules = cartographer::get_modules_with_purposes(db, project_id)
        .map_err(|e| e.to_string())?;

    if modules.is_empty() {
        return Ok(0);
    }

    // Build context about the codebase
    let path = Path::new(project_path);
    let mut module_context = String::new();

    for module in &modules {
        module_context.push_str(&format!("\n## {}\n", module.id));
        if let Some(ref purpose) = module.purpose {
            module_context.push_str(&format!("Purpose: {}\n", purpose));
        }
        if !module.exports.is_empty() {
            module_context.push_str(&format!("Exports: {}\n", module.exports.join(", ")));
        }
        if !module.depends_on.is_empty() {
            module_context.push_str(&format!("Dependencies: {}\n", module.depends_on.join(", ")));
        }

        // Get a code preview
        let preview = cartographer::get_module_code_preview(path, &module.path);
        if !preview.is_empty() {
            let truncated = if preview.len() > 500 {
                format!("{}...", &preview[..500])
            } else {
                preview
            };
            module_context.push_str(&format!("Code preview:\n```\n{}\n```\n", truncated));
        }
    }

    // Ask Reasoner to extract capabilities
    let prompt = format!(
        r#"Analyze this codebase and extract its CAPABILITIES - what can users/developers DO with it.

For each capability found:
1. Describe what the system CAN DO (action-oriented)
2. Note which module provides it
3. Flag any capabilities that appear INCOMPLETE or UNUSED (dead code, stub implementations, unfinished features)

Format your response as a structured list:

CAPABILITIES:
- [module_name] Can do X via function/tool Y
- [module_name] Can do Z via endpoint/command W
...

ISSUES (incomplete/unused):
- [module_name] Function X appears unused (never called)
- [module_name] Feature Y is stubbed but not implemented
...

Be specific and action-oriented. Focus on what the system provides to its users.

Codebase modules:
{}"#,
        module_context
    );

    let messages = vec![Message::user(prompt)];

    let result = deepseek
        .chat(messages, None)
        .await
        .map_err(|e| format!("DeepSeek request failed: {}", e))?;

    let content = result
        .content
        .ok_or("No content in DeepSeek response")?;

    // Parse and store capabilities
    let stored = parse_and_store_capabilities(db, embeddings, project_id, &content).await?;

    Ok(stored)
}

/// Parse the Reasoner response and store as memories with embeddings
async fn parse_and_store_capabilities(
    db: &Database,
    embeddings: Option<&Arc<EmbeddingClient>>,
    project_id: i64,
    response: &str,
) -> Result<usize, String> {
    let mut stored = 0;

    // Clear old capabilities for this project (refresh)
    clear_old_capabilities(db, project_id)?;

    let lines: Vec<&str> = response.lines().collect();
    let mut in_capabilities = false;
    let mut in_issues = false;
    let mut capability_index = 0;
    let mut issue_index = 0;

    for line in lines {
        let trimmed = line.trim();

        if trimmed.starts_with("CAPABILITIES:") || trimmed.starts_with("**CAPABILITIES") {
            in_capabilities = true;
            in_issues = false;
            continue;
        }

        if trimmed.starts_with("ISSUES") || trimmed.starts_with("**ISSUES") {
            in_capabilities = false;
            in_issues = true;
            continue;
        }

        if trimmed.starts_with("- ") {
            let content = trimmed.trim_start_matches("- ").trim();
            if content.is_empty() {
                continue;
            }

            if in_capabilities {
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

                // Generate and store embedding
                if let Some(emb_client) = embeddings {
                    if let Ok(embedding) = emb_client.embed(content).await {
                        store_embedding(db, id, content, &embedding)?;
                    }
                }

                capability_index += 1;
                stored += 1;
            } else if in_issues {
                // Store as issue with embedding
                let key = format!("capability_issue:{}", issue_index);
                let id = db.store_memory(
                    Some(project_id),
                    Some(&key),
                    content,
                    "issue",
                    Some("codebase"),
                    0.9, // Slightly lower confidence for issues
                )
                .map_err(|e| e.to_string())?;

                // Generate and store embedding
                if let Some(emb_client) = embeddings {
                    if let Ok(embedding) = emb_client.embed(content).await {
                        store_embedding(db, id, content, &embedding)?;
                    }
                }

                issue_index += 1;
                stored += 1;
            }
        }
    }

    Ok(stored)
}

/// Store embedding for a memory fact
fn store_embedding(db: &Database, fact_id: i64, content: &str, embedding: &[f32]) -> Result<(), String> {
    let conn = db.conn();

    let embedding_bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();

    conn.execute(
        "INSERT OR REPLACE INTO vec_memory (rowid, embedding, fact_id, content) VALUES (?, ?, ?, ?)",
        params![fact_id, embedding_bytes, fact_id, content],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// Clear old capabilities before refresh
fn clear_old_capabilities(db: &Database, project_id: i64) -> Result<(), String> {
    let conn = db.conn();

    // Delete old capabilities and issues
    conn.execute(
        "DELETE FROM memory_facts WHERE project_id = ? AND fact_type IN ('capability', 'issue') AND category = 'codebase'",
        [project_id],
    )
    .map_err(|e| e.to_string())?;

    // Also clear from vec_memory
    conn.execute(
        "DELETE FROM vec_memory WHERE fact_id NOT IN (SELECT id FROM memory_facts)",
        [],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}
