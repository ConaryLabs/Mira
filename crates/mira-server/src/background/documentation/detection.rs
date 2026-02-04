// crates/mira-server/src/background/documentation/detection.rs
// Documentation gap and staleness detection

use crate::db::documentation::{
    DocGap, create_doc_task, get_inventory_for_stale_check, mark_doc_stale,
};
use crate::db::pool::DatabasePool;
use crate::db::{
    get_documented_by_category_sync, get_indexed_project_ids_sync, get_lib_symbols_sync,
    get_modules_for_doc_gaps_sync, get_project_paths_by_ids_sync, get_symbols_for_file_sync,
};
use crate::utils::ResultExt;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use super::{get_git_head, is_ancestor, mark_documentation_scanned_sync, read_file_content};

/// Scan for documentation gaps and stale docs.
///
/// - `main_pool`: for documentation_inventory, memory_facts, doc tasks
/// - `code_pool`: for code_symbols, codebase_modules
///
/// Returns number of new tasks created
pub async fn scan_documentation_gaps(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
) -> Result<usize, String> {
    // Step 1: Get indexed project IDs from code DB
    let project_ids = code_pool
        .interact(move |conn| {
            get_indexed_project_ids_sync(conn).map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .str_err()?;

    if project_ids.is_empty() {
        return Ok(0);
    }

    // Step 2: Get project paths from main DB
    let ids = project_ids.clone();
    let projects = main_pool
        .interact(move |conn| {
            get_project_paths_by_ids_sync(conn, &ids).map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .str_err()?;

    let mut total_created = 0;

    for (project_id, project_path) in projects {
        // Check if project needs scan (main DB - memory_facts)
        let project_path_clone = project_path.clone();
        let needs_scan = main_pool
            .interact(move |conn| {
                super::needs_documentation_scan(conn, project_id, &project_path_clone)
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
            .str_err()?;

        if !needs_scan {
            continue;
        }

        let project_path_ref = Path::new(&project_path);
        if !project_path_ref.exists() {
            continue;
        }

        // First, scan existing documentation to build inventory (main DB)
        match super::inventory::scan_existing_docs(
            main_pool,
            project_id,
            project_path_ref.to_str().unwrap_or(""),
        )
        .await
        {
            Ok(scanned) if scanned > 0 => {
                tracing::debug!(
                    "Documentation: scanned {} existing docs for project {}",
                    scanned,
                    project_id
                );
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(
                    "Failed to scan existing docs for project {}: {}",
                    project_id,
                    e
                );
            }
        }

        // Reset orphaned tasks (main DB)
        let project_path_for_reset = project_path.clone();
        let reset_count = main_pool
            .interact(move |conn| {
                crate::db::documentation::reset_orphaned_doc_tasks(
                    conn,
                    project_id,
                    &project_path_for_reset,
                )
                .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
            .str_err()?;
        if reset_count > 0 {
            tracing::info!(
                "Documentation: reset {} orphaned tasks for project {}",
                reset_count,
                project_id
            );
        }

        // Scan for gaps (uses both pools)
        let created =
            detect_gaps_for_project(main_pool, code_pool, project_id, project_path_ref).await?;
        total_created += created;

        // Scan for stale docs (uses both pools)
        let stale =
            detect_stale_docs_for_project(main_pool, code_pool, project_id, project_path_ref)
                .await?;
        if stale > 0 {
            tracing::info!(
                "Documentation: detected {} stale docs for project {}",
                stale,
                project_id
            );
        }

        // Mark as scanned (main DB)
        let project_path_for_scan = project_path.clone();
        main_pool
            .interact(move |conn| {
                mark_documentation_scanned_sync(conn, project_id, &project_path_for_scan)
                    .map_err(|e| anyhow::anyhow!("{}", e))
            })
            .await
            .str_err()?;
    }

    Ok(total_created)
}

/// Detect missing documentation for a project
async fn detect_gaps_for_project(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
    project_id: i64,
    project_path: &Path,
) -> Result<usize, String> {
    let mut gaps = Vec::new();
    let project_str = project_path.to_str().unwrap_or("");

    // Get current git commit
    let git_commit = get_git_head(project_str);

    // Detect MCP tool documentation gaps (main DB only)
    gaps.extend(detect_mcp_tool_gaps(main_pool, project_id, project_path).await?);

    // Detect public API documentation gaps (code_pool for symbols, main_pool for docs)
    gaps.extend(detect_public_api_gaps(main_pool, code_pool, project_id, project_path).await?);

    // Detect module documentation gaps (code_pool for modules, main_pool for docs)
    gaps.extend(detect_module_doc_gaps(main_pool, code_pool, project_id, project_path).await?);

    // Create tasks for all gaps (main DB)
    let gaps_to_create = gaps.clone();

    let created = main_pool
        .interact(move |conn| {
            let mut count = 0;
            for gap in gaps_to_create {
                match create_doc_task(conn, &gap, git_commit.as_deref()) {
                    Ok(_) => count += 1,
                    Err(e) => {
                        // Likely a uniqueness constraint violation - task already exists
                        tracing::debug!(
                            "Documentation task already exists for {:?}: {}",
                            gap.target_doc_path,
                            e
                        );
                    }
                }
            }
            Ok::<usize, anyhow::Error>(count)
        })
        .await
        .str_err()?;

    if created > 0 {
        tracing::info!(
            "Documentation: created {} tasks for project {}",
            created,
            project_id
        );
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

    let content = tokio::task::spawn_blocking(move || read_file_content(&mcp_mod_path))
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
        if trimmed.starts_with("async fn ")
            && let Some(fn_name) = trimmed.strip_prefix("async fn ") {
                let fn_name = fn_name.split('(').next().unwrap_or("").trim().to_string();
                if !fn_name.is_empty() {
                    tool_names.insert(fn_name);
                }
            }
    }

    // Check which tools have documentation (main DB)
    let documented: HashSet<String> = pool
        .interact(move |conn| {
            get_documented_by_category_sync(conn, project_id, "mcp_tool")
                .map(|v| v.into_iter().collect())
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .str_err()?;

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

/// Detect undocumented public APIs
async fn detect_public_api_gaps(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
    project_id: i64,
    _project_path: &Path,
) -> Result<Vec<DocGap>, String> {
    let mut gaps = Vec::new();

    // Find public symbols in lib.rs (code DB)
    let lib_symbols = code_pool
        .interact(move |conn| {
            get_lib_symbols_sync(conn, project_id).map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .str_err()?;

    // Get documented public APIs (main DB)
    let documented: HashSet<String> = main_pool
        .interact(move |conn| {
            get_documented_by_category_sync(conn, project_id, "public_api")
                .map(|v| v.into_iter().collect())
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .str_err()?;

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
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
    project_id: i64,
    _project_path: &Path,
) -> Result<Vec<DocGap>, String> {
    let mut gaps = Vec::new();

    // Get all modules from codebase_modules (code DB)
    let modules = code_pool
        .interact(move |conn| {
            get_modules_for_doc_gaps_sync(conn, project_id).map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .str_err()?;

    // Get documented modules (main DB)
    let documented: HashSet<String> = main_pool
        .interact(move |conn| {
            get_documented_by_category_sync(conn, project_id, "module")
                .map(|v| v.into_iter().collect())
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .str_err()?;

    for (module_id, path, purpose) in modules {
        let doc_path = format!("docs/modules/{}.md", module_id);

        // Skip if no purpose defined (likely internal module)
        if purpose.is_none() || purpose.as_ref().is_none_or(|p| p.is_empty()) {
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
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
    project_id: i64,
    project_path: &Path,
) -> Result<usize, String> {
    let mut stale_count = 0;
    let project_str = project_path.to_str().unwrap_or("");
    let current_commit = get_git_head(project_str);

    // Get all documented items with source info (main DB)
    let inventory = main_pool
        .interact(move |conn| {
            get_inventory_for_stale_check(conn, project_id).map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .str_err()?;

    for item in inventory {
        let mut is_stale = false;
        let mut reason = String::new();

        // Check 1: Git commit changed (with ancestor check for rebases)
        if let (Some(stored_commit), Some(current)) = (&item.last_seen_commit, &current_commit)
            && stored_commit != current {
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

        // Check 2: Source signature hash changed (code DB for symbols)
        if let Some(source_file) = item.source_symbols.as_ref().and_then(|s| {
            if s.contains("source_file:") {
                s.split("source_file:").nth(1).map(|p| p.to_string())
            } else {
                None
            }
        })
            && let Some(new_hash) =
                check_source_signature_changed(code_pool, project_id, &source_file).await?
                && item.source_signature_hash.as_ref() != Some(&new_hash) {
                    is_stale = true;
                    if reason.is_empty() {
                        reason = "Source signatures changed".to_string();
                    }
                }

        if is_stale {
            // Mark as stale (main DB)
            let doc_path = item.doc_path.clone();
            let reason_clone = reason.clone();
            main_pool
                .interact(move |conn| {
                    mark_doc_stale(conn, project_id, &doc_path, &reason_clone)
                        .map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .str_err()?;
            stale_count += 1;
            tracing::debug!("Stale documentation: {} - {}", item.doc_path, reason);
        }
    }

    Ok(stale_count)
}

/// Check if source signatures have changed (reads code_symbols from code DB)
async fn check_source_signature_changed(
    code_pool: &Arc<DatabasePool>,
    project_id: i64,
    source_path: &str,
) -> Result<Option<String>, String> {
    use sha2::Digest;

    let source_path = source_path.to_string();

    // Get symbols from code DB
    let symbols = code_pool
        .interact(move |conn| {
            get_symbols_for_file_sync(conn, project_id, &source_path)
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .str_err()?;

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
