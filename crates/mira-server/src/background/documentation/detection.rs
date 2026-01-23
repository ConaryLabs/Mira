// crates/mira-server/src/background/documentation/detection.rs
// Documentation gap and staleness detection

use crate::db::documentation::{create_doc_task, mark_doc_stale, DocGap, get_inventory_for_stale_check};
use crate::db::{
    get_indexed_projects_sync, get_documented_by_category_sync, get_lib_symbols_sync,
    get_modules_for_doc_gaps_sync, get_symbols_for_file_sync,
};
use crate::db::pool::DatabasePool;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use super::{get_git_head, is_ancestor, read_file_content, mark_documentation_scanned_sync};

/// Scan for documentation gaps and stale docs
/// Returns number of new tasks created
pub async fn scan_documentation_gaps(pool: &Arc<DatabasePool>) -> Result<usize, String> {
    // Get all indexed projects
    let projects = pool.interact(move |conn| {
        get_indexed_projects_sync(conn).map_err(|e| anyhow::anyhow!("{}", e))
    })
    .await
    .map_err(|e| e.to_string())?;

    let mut total_created = 0;

    for (project_id, project_path) in projects {
        // Check if project needs scan
        let project_path_clone = project_path.clone();
        let needs_scan = pool.interact(move |conn| {
            super::needs_documentation_scan(conn, project_id, &project_path_clone)
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .map_err(|e| e.to_string())?;

        if !needs_scan {
            continue;
        }

        let project_path_ref = Path::new(&project_path);
        if !project_path_ref.exists() {
            continue;
        }

        // First, scan existing documentation to build inventory
        match super::inventory::scan_existing_docs(pool, project_id, project_path_ref.to_str().unwrap_or("")).await {
            Ok(scanned) if scanned > 0 => {
                tracing::debug!("Documentation: scanned {} existing docs for project {}", scanned, project_id);
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("Failed to scan existing docs for project {}: {}", project_id, e);
            }
        }

        // Scan for gaps
        let created = detect_gaps_for_project(pool, project_id, project_path_ref).await?;
        total_created += created;

        // Scan for stale docs
        let stale = detect_stale_docs_for_project(pool, project_id, project_path_ref).await?;
        if stale > 0 {
            tracing::info!("Documentation: detected {} stale docs for project {}", stale, project_id);
        }

        // Mark as scanned
        let project_path_for_scan = project_path.clone();
        pool.interact(move |conn| {
            mark_documentation_scanned_sync(conn, project_id, &project_path_for_scan)
                .map_err(|e| anyhow::anyhow!("{}", e))
        }).await.map_err(|e| e.to_string())?;
    }

    Ok(total_created)
}

/// Detect missing documentation for a project
async fn detect_gaps_for_project(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    project_path: &Path,
) -> Result<usize, String> {
    let mut gaps = Vec::new();
    let project_str = project_path.to_str().unwrap_or("");

    // Get current git commit
    let git_commit = get_git_head(project_str);


    // Detect MCP tool documentation gaps
    gaps.extend(detect_mcp_tool_gaps(pool, project_id, project_path).await?);

    // Detect CLI command documentation gaps
    gaps.extend(detect_cli_gaps(pool, project_id, project_path).await?);

    // Detect public API documentation gaps
    gaps.extend(detect_public_api_gaps(pool, project_id, project_path).await?);

    // Detect module documentation gaps
    gaps.extend(detect_module_doc_gaps(pool, project_id, project_path).await?);

    // Create tasks for all gaps
    let gaps_to_create = gaps.clone();

    let created = pool.interact(move |conn| {
        let mut count = 0;
        for gap in gaps_to_create {
            match create_doc_task(conn, &gap, git_commit.as_deref()) {
                Ok(_) => count += 1,
                Err(e) => {
                    // Likely a uniqueness constraint violation - task already exists
                    tracing::debug!("Documentation task already exists for {:?}: {}", gap.target_doc_path, e);
                }
            }
        }
        Ok::<usize, anyhow::Error>(count)
    })
    .await
    .map_err(|e| e.to_string())?;

    if created > 0 {
        tracing::info!("Documentation: created {} tasks for project {}", created, project_id);
    }

    Ok(created)
}

/// Detect undocumented MCP tools by parsing mcp/mod.rs
async fn detect_mcp_tool_gaps(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    project_path: &Path,
) -> Result<Vec<DocGap>, String> {
    let mut gaps = Vec::new();

    // Read mcp/mod.rs to find #[tool(...)] declarations
    let mcp_mod_path = project_path.join("crates/mira-server/src/mcp/mod.rs");
    if !mcp_mod_path.exists() {
        return Ok(gaps);
    }

    let content = tokio::task::spawn_blocking(move || {
        read_file_content(&mcp_mod_path)
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))?
    .map_err(|e| format!("Failed to read mcp/mod.rs: {}", e))?;

    // Extract tool names from #[tool(...)] annotations
    let mut tool_names = HashSet::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[tool(") {
            // Look for the next async fn line to get the tool name
            continue;
        }
        if trimmed.starts_with("async fn ") {
            if let Some(fn_name) = trimmed.strip_prefix("async fn ") {
                let fn_name = fn_name
                    .split('(')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !fn_name.is_empty() {
                    tool_names.insert(fn_name);
                }
            }
        }
    }

    // Check which tools have documentation
    let documented: HashSet<String> = pool.interact(move |conn| {
        get_documented_by_category_sync(conn, project_id, "mcp_tool")
            .map(|v| v.into_iter().collect())
            .map_err(|e| anyhow::anyhow!("{}", e))
    })
    .await
    .map_err(|e| e.to_string())?;

    // Create gaps for undocumented tools
    for tool_name in tool_names {
        let doc_path = format!("docs/tools/{}.md", tool_name);
        if !documented.contains(&doc_path) {
            gaps.push(DocGap {
                project_id,
                doc_type: "api".to_string(),
                doc_category: "mcp_tool".to_string(),
                source_file_path: Some("crates/mira-server/src/mcp/mod.rs".to_string()),
                target_doc_path: doc_path,
                priority: "high".to_string(),
                reason: format!("MCP tool '{}' needs documentation", tool_name),
                source_signature_hash: None,
            });
        }
    }

    Ok(gaps)
}

/// Detect undocumented CLI commands
async fn detect_cli_gaps(
    _pool: &Arc<DatabasePool>,
    _project_id: i64,
    _project_path: &Path,
) -> Result<Vec<DocGap>, String> {
    // For now, return empty - CLI commands are documented in README
    // This could be extended to parse main.rs clap Command definitions
    Ok(Vec::new())
}

/// Detect undocumented public APIs
async fn detect_public_api_gaps(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    _project_path: &Path,
) -> Result<Vec<DocGap>, String> {
    let mut gaps = Vec::new();

    // Find public symbols in lib.rs
    let lib_symbols = pool.interact(move |conn| {
        get_lib_symbols_sync(conn, project_id).map_err(|e| anyhow::anyhow!("{}", e))
    })
    .await
    .map_err(|e| e.to_string())?;

    // Get documented public APIs
    let documented: HashSet<String> = pool.interact(move |conn| {
        get_documented_by_category_sync(conn, project_id, "public_api")
            .map(|v| v.into_iter().collect())
            .map_err(|e| anyhow::anyhow!("{}", e))
    })
    .await
    .map_err(|e| e.to_string())?;

    for (name, _signature) in lib_symbols {
        let doc_path = format!("docs/api/{}.md", name);
        if !documented.contains(&doc_path) {
            gaps.push(DocGap {
                project_id,
                doc_type: "api".to_string(),
                doc_category: "public_api".to_string(),
                source_file_path: Some("src/lib.rs".to_string()),
                target_doc_path: doc_path,
                priority: "medium".to_string(),
                reason: format!("Public API '{}' needs documentation", name),
                source_signature_hash: None,
            });
        }
    }

    Ok(gaps)
}

/// Detect undocumented modules
async fn detect_module_doc_gaps(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    _project_path: &Path,
) -> Result<Vec<DocGap>, String> {
    let mut gaps = Vec::new();

    // Get all modules from codebase_modules
    let modules = pool.interact(move |conn| {
        get_modules_for_doc_gaps_sync(conn, project_id).map_err(|e| anyhow::anyhow!("{}", e))
    })
    .await
    .map_err(|e| e.to_string())?;

    // Get documented modules
    let documented: HashSet<String> = pool.interact(move |conn| {
        get_documented_by_category_sync(conn, project_id, "module")
            .map(|v| v.into_iter().collect())
            .map_err(|e| anyhow::anyhow!("{}", e))
    })
    .await
    .map_err(|e| e.to_string())?;

    for (module_id, path, purpose) in modules {
        let doc_path = format!("docs/modules/{}.md", module_id);

        // Skip if no purpose defined (likely internal module)
        if purpose.is_none() || purpose.as_ref().map_or(true, |p| p.is_empty()) {
            continue;
        }

        if !documented.contains(&doc_path) {
            gaps.push(DocGap {
                project_id,
                doc_type: "architecture".to_string(),
                doc_category: "module".to_string(),
                source_file_path: Some(path),
                target_doc_path: doc_path,
                priority: "medium".to_string(),
                reason: format!("Module '{}' needs documentation", module_id),
                source_signature_hash: None,
            });
        }
    }

    Ok(gaps)
}

/// Detect stale documentation
async fn detect_stale_docs_for_project(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    project_path: &Path,
) -> Result<usize, String> {
    let mut stale_count = 0;
    let project_str = project_path.to_str().unwrap_or("");
    let current_commit = get_git_head(project_str);

    // Get all documented items with source info
    let inventory = pool.interact(move |conn| {
        get_inventory_for_stale_check(conn, project_id).map_err(|e| anyhow::anyhow!("{}", e))
    })
    .await
    .map_err(|e| e.to_string())?;

    for item in inventory {
        let mut is_stale = false;
        let mut reason = String::new();

        // Check 1: Git commit changed (with ancestor check for rebases)
        if let (Some(stored_commit), Some(current)) = (&item.last_seen_commit, &current_commit) {
            if stored_commit != current {
                // Verify it's a real change, not just a rebase
                if !is_ancestor(project_str, stored_commit) {
                    is_stale = true;
                    reason = format!(
                        "Git history changed (rebase/force-push detected). Old: {}, Current: {}",
                        &stored_commit[..8.min(stored_commit.len())],
                        &current[..8.min(current.len())]
                    );
                }
            }
        }

        // Check 2: Source signature hash changed
        if let Some(source_file) = item.source_symbols.as_ref().and_then(|s| {
            // Try to extract source file path from inventory
            if s.contains("source_file:") {
                s.split("source_file:").nth(1).map(|p| p.to_string())
            } else {
                None
            }
        }) {
            if let Some(new_hash) = check_source_signature_changed(pool, project_id, &source_file).await? {
                if item.source_signature_hash.as_ref() != Some(&new_hash) {
                    is_stale = true;
                    if reason.is_empty() {
                        reason = "Source signatures changed".to_string();
                    }
                }
            }
        }

        if is_stale {
            // Mark as stale (synchronous DB operation)
            let doc_path = item.doc_path.clone();
            let reason_clone = reason.clone();
            pool.interact(move |conn| {
                mark_doc_stale(conn, project_id, &doc_path, &reason_clone)
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
            .map_err(|e| e.to_string())?;
            stale_count += 1;
            tracing::debug!("Stale documentation: {} - {}", item.doc_path, reason);
        }
    }

    Ok(stale_count)
}

/// Check if source signatures have changed
async fn check_source_signature_changed(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    source_path: &str,
) -> Result<Option<String>, String> {
    use sha2::Digest;

    let source_path = source_path.to_string();

    // Get symbols from db - returns (id, name, symbol_type, start_line, end_line, signature)
    let symbols = pool.interact(move |conn| {
        get_symbols_for_file_sync(conn, project_id, &source_path)
            .map_err(|e| anyhow::anyhow!("{}", e))
    })
    .await
    .map_err(|e| e.to_string())?;

    if symbols.is_empty() {
        return Ok(None);
    }

    // Calculate hash from signatures (tuple index 5)
    let normalized: Vec<String> = symbols
        .iter()
        .filter_map(|(_, _, _, _, _, sig)| sig.as_ref())
        .map(|sig| sig.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();

    if normalized.is_empty() {
        return Ok(None);
    }

    let combined = normalized.join("\n");
    let hash = sha2::Sha256::digest(combined.as_bytes());
    Ok(Some(format!("{:x}", hash)))
}
