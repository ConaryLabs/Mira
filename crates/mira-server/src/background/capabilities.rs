// crates/mira-server/src/background/capabilities.rs
// Background worker for generating codebase capabilities inventory

use crate::cartographer;
use crate::db::{
    get_indexed_projects_sync, get_scan_info_sync, is_time_older_than_sync,
    clear_old_capabilities_sync,
};
use crate::db::pool::DatabasePool;
use crate::embeddings::EmbeddingClient;
use crate::llm::{record_llm_usage, LlmClient, PromptBuilder};
use crate::search::embedding_to_bytes;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

/// Check if capabilities inventory needs regeneration and process if so
pub async fn process_capabilities(
    pool: &Arc<DatabasePool>,
    client: &Arc<dyn LlmClient>,
    embeddings: Option<&Arc<EmbeddingClient>>,
) -> Result<usize, String> {
    // Get projects that need capability scanning
    let projects = pool.interact(move |conn| {
        get_projects_needing_scan(conn)
            .map_err(|e| anyhow::anyhow!("{}", e))
    }).await.map_err(|e| e.to_string())?;
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
        match generate_capabilities_inventory(pool, client, embeddings, project_id, &project_path).await {
            Ok(count) => {
                tracing::info!(
                    "Generated {} capabilities for project {} ({})",
                    count,
                    project_id,
                    project_path
                );
                processed += count;

                // Update last scan timestamp with git commit
                mark_capabilities_scanned(pool, project_id, &project_path).await?;
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
    let all_projects = get_indexed_projects_sync(conn).map_err(|e| e.to_string())?;

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
    let scan_info = get_scan_info_sync(conn, project_id, "capabilities_scan_time");

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
                if is_time_older_than_sync(conn, scan_time, "-1 day") {
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
        if is_time_older_than_sync(conn, scan_time, "-7 days") {
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
async fn mark_capabilities_scanned(pool: &Arc<DatabasePool>, project_id: i64, project_path: &str) -> Result<(), String> {
    use crate::db::{store_memory_sync, StoreMemoryParams};

    // Store the current git commit as the scan marker
    let commit = get_git_head(project_path).unwrap_or_else(|| "unknown".to_string());

    pool.interact(move |conn| {
        store_memory_sync(conn, StoreMemoryParams {
            project_id: Some(project_id),
            key: Some("capabilities_scan_time"),
            content: &commit,
            fact_type: "system",
            category: Some("capabilities"),
            confidence: 1.0,
            session_id: None,
            user_id: None,
            scope: "project",
            branch: None,
        }).map_err(|e| anyhow::anyhow!("Failed to store: {}", e))
    }).await.map_err(|e| e.to_string())?;
    Ok(())
}

/// Max bytes of code to send per module (30KB each)
const MAX_MODULE_CODE_BYTES: usize = 30_000;

/// Max total bytes for all module context (200KB ≈ 50K tokens, fits in DeepSeek's 64K limit)
const MAX_TOTAL_CONTEXT_BYTES: usize = 200_000;

/// Generate the full capabilities inventory for a project
async fn generate_capabilities_inventory(
    pool: &Arc<DatabasePool>,
    client: &Arc<dyn LlmClient>,
    embeddings: Option<&Arc<EmbeddingClient>>,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    // Get the codebase map with module info
    let modules = cartographer::get_modules_with_purposes_pool(pool, project_id)
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

    let result = client
        .chat(messages, None)
        .await
        .map_err(|e| format!("LLM request failed: {}", e))?;

    // Record usage
    record_llm_usage(
        pool,
        client.provider_type(),
        &client.model_name(),
        "background:capabilities",
        &result,
        Some(project_id),
        None,
    ).await;

    let content = result
        .content
        .ok_or("No content in DeepSeek response")?;

    // Parse and store capabilities only
    let stored = parse_and_store_capabilities(pool, embeddings, project_id, &content).await?;

    Ok(stored)
}

/// Parse the Reasoner response and store capabilities as memories with embeddings
async fn parse_and_store_capabilities(
    pool: &Arc<DatabasePool>,
    embeddings: Option<&Arc<EmbeddingClient>>,
    project_id: i64,
    response: &str,
) -> Result<usize, String> {
    use crate::db::{store_memory_sync, StoreMemoryParams};

    let mut stored = 0;

    // Clear old capabilities for this project
    pool.interact(move |conn| {
        clear_old_capabilities(conn, project_id)
            .map_err(|e| anyhow::anyhow!("{}", e))
    }).await.map_err(|e| e.to_string())?;

    // First pass: collect capabilities from response
    let mut capabilities: Vec<(String, String)> = Vec::new(); // (key, content)
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

            let key = format!("capability:{}", capability_index);
            capabilities.push((key, content.to_string()));
            capability_index += 1;
        }
    }

    // Second pass: store capabilities using pool
    for (key, content) in capabilities {
        let key_clone = key.clone();
        let content_clone = content.clone();
        let id = pool.interact(move |conn| {
            store_memory_sync(conn, StoreMemoryParams {
                project_id: Some(project_id),
                key: Some(&key_clone),
                content: &content_clone,
                fact_type: "capability",
                category: Some("codebase"),
                confidence: 1.0,
                session_id: None,
                user_id: None,
                scope: "project",
                branch: None,
            }).map_err(|e| anyhow::anyhow!("Failed to store: {}", e))
        }).await.map_err(|e| e.to_string())?;

        // Generate and store embedding (RETRIEVAL_DOCUMENT for storage)
        if let Some(emb_client) = embeddings {
            if let Ok(embedding) = emb_client.embed_for_storage(&content).await {
                let pool_clone = pool.clone();
                pool_clone.interact(move |conn| {
                    store_embedding(conn, id, &content, &embedding)
                        .map_err(|e| anyhow::anyhow!("{}", e))
                }).await.map_err(|e| e.to_string())?;
            }
        }

        stored += 1;
    }

    Ok(stored)
}

/// Store embedding for a memory fact
fn store_embedding(conn: &rusqlite::Connection, fact_id: i64, content: &str, embedding: &[f32]) -> Result<(), String> {
    use crate::db::store_embedding_sync;
    let embedding_bytes = embedding_to_bytes(embedding);
    store_embedding_sync(conn, fact_id, content, &embedding_bytes).map_err(|e| e.to_string())
}

/// Clear old capabilities before refresh (issues are handled by code_health scanner)
fn clear_old_capabilities(conn: &rusqlite::Connection, project_id: i64) -> Result<(), String> {
    clear_old_capabilities_sync(conn, project_id).map_err(|e| e.to_string())
}
