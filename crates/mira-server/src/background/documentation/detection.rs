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
use crate::utils::truncate_at_boundary;
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
    let project_ids = code_pool.run(get_indexed_project_ids_sync).await?;

    if project_ids.is_empty() {
        return Ok(0);
    }

    // Step 2: Get project paths from main DB
    let ids = project_ids.clone();
    let projects = main_pool
        .run(move |conn| get_project_paths_by_ids_sync(conn, &ids))
        .await?;

    let mut total_created = 0;

    for (project_id, project_path) in projects {
        // Check if project needs scan (main DB - memory_facts)
        let project_path_clone = project_path.clone();
        let needs_scan = main_pool
            .run(move |conn| super::needs_documentation_scan(conn, project_id, &project_path_clone))
            .await?;

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
            .run(move |conn| {
                crate::db::documentation::reset_orphaned_doc_tasks(
                    conn,
                    project_id,
                    &project_path_for_reset,
                )
            })
            .await?;
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
            .run(move |conn| {
                mark_documentation_scanned_sync(conn, project_id, &project_path_for_scan)
            })
            .await?;
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
        .run(move |conn| {
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
        .await?;

    if created > 0 {
        tracing::info!(
            "Documentation: created {} tasks for project {}",
            created,
            project_id
        );
    }

    Ok(created)
}

/// Detect undocumented MCP tools by parsing mcp/router.rs
async fn detect_mcp_tool_gaps(
    pool: &Arc<DatabasePool>,
    project_id: i64,
    project_path: &Path,
) -> Result<Vec<DocGap>, String> {
    let mut gaps = Vec::new();

    // Read mcp/router.rs to find #[tool(...)] declarations
    let router_path = project_path.join("crates/mira-server/src/mcp/router.rs");
    if !router_path.exists() {
        return Ok(gaps);
    }

    let content = tokio::task::spawn_blocking(move || read_file_content(&router_path))
        .await
        .map_err(|e| format!("spawn_blocking panicked: {}", e))?
        .map_err(|e| format!("Failed to read mcp/router.rs: {}", e))?;

    let tool_names = extract_tool_names_from_source(&content);

    // Check which tools have documentation (main DB)
    let documented: HashSet<String> = pool
        .run(move |conn| {
            get_documented_by_category_sync(conn, project_id, "mcp_tool")
                .map(|v| v.into_iter().collect())
        })
        .await?;

    // Create gaps for undocumented tools
    for tool_name in tool_names {
        let doc_path = format!("docs/tools/{}.md", tool_name);
        if !documented.contains(&doc_path) {
            gaps.push(DocGap {
                project_id,
                doc_type: "api".to_string(),
                doc_category: "mcp_tool".to_string(),
                source_file_path: Some("crates/mira-server/src/mcp/router.rs".to_string()),
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
        .run(move |conn| get_lib_symbols_sync(conn, project_id))
        .await?;

    // Get documented public APIs (main DB)
    let documented: HashSet<String> = main_pool
        .run(move |conn| {
            get_documented_by_category_sync(conn, project_id, "public_api")
                .map(|v| v.into_iter().collect())
        })
        .await?;

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
        .run(move |conn| get_modules_for_doc_gaps_sync(conn, project_id))
        .await?;

    // Get documented modules (main DB)
    let documented: HashSet<String> = main_pool
        .run(move |conn| {
            get_documented_by_category_sync(conn, project_id, "module")
                .map(|v| v.into_iter().collect())
        })
        .await?;

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
        .run(move |conn| get_inventory_for_stale_check(conn, project_id))
        .await?;

    for item in inventory {
        let mut is_stale = false;
        let mut reason = String::new();

        // Check 1: Git commit changed (with ancestor check for rebases)
        if let (Some(stored_commit), Some(current)) = (&item.last_seen_commit, &current_commit)
            && stored_commit != current
        {
            // Verify it's a real change, not just a rebase
            if !is_ancestor(project_str, stored_commit) {
                is_stale = true;
                reason = format!(
                    "Git history changed (rebase/force-push detected). Old: {}, Current: {}",
                    truncate_at_boundary(stored_commit, 8),
                    truncate_at_boundary(current, 8)
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
        }) && let Some(new_hash) =
            check_source_signature_changed(code_pool, project_id, &source_file).await?
            && item.source_signature_hash.as_ref() != Some(&new_hash)
        {
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
                .run(move |conn| mark_doc_stale(conn, project_id, &doc_path, &reason_clone))
                .await?;
            stale_count += 1;
            tracing::debug!("Stale documentation: {} - {}", item.doc_path, reason);
        }
    }

    Ok(stale_count)
}

/// Extract tool names from source code by parsing #[tool(...)] annotations.
///
/// Only collects `async fn` declarations that are immediately preceded by a
/// `#[tool()]` attribute. Multi-line attributes are handled by tracking
/// open/close parens.
fn extract_tool_names_from_source(content: &str) -> HashSet<String> {
    let mut tool_names = HashSet::new();
    let mut saw_tool_attr = false;
    let mut in_tool_attr = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[tool(") {
            saw_tool_attr = true;
            // Check if the attribute closes on the same line
            in_tool_attr = !trimmed.contains(")]");
            continue;
        }
        if in_tool_attr {
            // Still inside a multi-line #[tool(...)] attribute
            if trimmed.contains(")]") {
                in_tool_attr = false;
            }
            continue;
        }
        if trimmed.starts_with("async fn ") {
            if saw_tool_attr && let Some(fn_name) = trimmed.strip_prefix("async fn ") {
                let fn_name = fn_name.split('(').next().unwrap_or("").trim().to_string();
                if !fn_name.is_empty() {
                    tool_names.insert(fn_name);
                }
            }
            saw_tool_attr = false;
        } else if !trimmed.starts_with("#[") {
            // Reset flag if we hit a non-attribute line without seeing async fn
            saw_tool_attr = false;
        }
    }
    tool_names
}

/// Check if source signatures have changed (reads code_symbols from code DB)
async fn check_source_signature_changed(
    code_pool: &Arc<DatabasePool>,
    project_id: i64,
    source_path: &str,
) -> Result<Option<String>, String> {
    let source_path = source_path.to_string();

    // Get symbols from code DB
    let symbols = code_pool
        .run(move |conn| get_symbols_for_file_sync(conn, project_id, &source_path))
        .await?;

    if symbols.is_empty() {
        return Ok(None);
    }

    let sigs: Vec<&str> = symbols
        .iter()
        .filter_map(|(_, _, _, _, _, sig)| sig.as_deref())
        .collect();
    Ok(super::hash_normalized_signatures(&sigs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_tool_names_single_line_attr() {
        let content = r#"
#[tool(description = "Do something")]
async fn my_tool(&self) -> Result<()> {
    Ok(())
}
"#;
        let names = extract_tool_names_from_source(content);
        assert_eq!(names.len(), 1);
        assert!(names.contains("my_tool"));
    }

    #[test]
    fn test_extract_tool_names_multi_line_attr() {
        let content = r#"
#[tool(
    description = "A tool that does things",
    name = "fancy_tool"
)]
async fn fancy_tool(&self, arg: String) -> Result<()> {
    Ok(())
}
"#;
        let names = extract_tool_names_from_source(content);
        assert_eq!(names.len(), 1);
        assert!(names.contains("fancy_tool"));
    }

    #[test]
    fn test_extract_tool_names_no_tool_attr() {
        let content = r#"
/// Just a regular function
async fn helper_function(&self) -> Result<()> {
    Ok(())
}

fn sync_function() -> i32 {
    42
}
"#;
        let names = extract_tool_names_from_source(content);
        assert!(
            names.is_empty(),
            "Functions without #[tool] should not be collected"
        );
    }

    #[test]
    fn test_extract_tool_names_empty_content() {
        let names = extract_tool_names_from_source("");
        assert!(names.is_empty());
    }

    #[test]
    fn test_extract_tool_names_multiple_tools() {
        let content = r#"
#[tool(description = "Tool A")]
async fn tool_a(&self) -> Result<()> {
    Ok(())
}

/// Some other code between tools
fn helper() {}

#[tool(description = "Tool B")]
async fn tool_b(&self, x: i32) -> Result<()> {
    Ok(())
}
"#;
        let names = extract_tool_names_from_source(content);
        assert_eq!(names.len(), 2);
        assert!(names.contains("tool_a"));
        assert!(names.contains("tool_b"));
    }

    #[test]
    fn test_extract_tool_names_tool_attr_without_async_fn() {
        // #[tool()] followed by a non-async-fn line should not match
        let content = r#"
#[tool(description = "Orphaned attribute")]
// some comment
fn sync_tool() -> i32 { 42 }
"#;
        let names = extract_tool_names_from_source(content);
        assert!(
            names.is_empty(),
            "Tool attribute followed by non-async fn should not match"
        );
    }

    #[test]
    fn test_extract_tool_names_other_attributes_between() {
        // Other #[...] attributes between #[tool] and async fn should preserve the flag
        let content = r#"
#[tool(description = "With extra attrs")]
#[allow(unused)]
async fn tool_with_attrs(&self) -> Result<()> {
    Ok(())
}
"#;
        let names = extract_tool_names_from_source(content);
        assert_eq!(names.len(), 1);
        assert!(names.contains("tool_with_attrs"));
    }

    #[test]
    fn test_extract_tool_names_non_tool_attribute_before_async_fn() {
        // A non-#[tool] attribute before async fn should NOT collect the function
        let content = r#"
#[allow(dead_code)]
async fn not_a_tool(&self) -> Result<()> {
    Ok(())
}
"#;
        let names = extract_tool_names_from_source(content);
        assert!(
            names.is_empty(),
            "Non-tool attribute should not cause function to be collected"
        );
    }

    #[test]
    fn test_extract_tool_names_malformed_tool_no_closing() {
        // #[tool( without closing )] -- multi-line mode activated, never closed
        let content = r#"
#[tool(
    description = "Never closed
async fn orphan(&self) -> Result<()> {
    Ok(())
}
"#;
        let names = extract_tool_names_from_source(content);
        // The async fn line is consumed while in_tool_attr is true (looking for ")]")
        // Since ")]" is never found, it stays in multi-line mode
        assert!(
            names.is_empty(),
            "Unclosed tool attribute should not extract any tools"
        );
    }

    #[test]
    fn test_extract_tool_names_regular_code_resets_flag() {
        // A regular code line between #[tool] and async fn should reset the flag
        let content = r#"
#[tool(description = "Tool")]
let x = 42;
async fn not_preceded_by_tool(&self) -> Result<()> {
    Ok(())
}
"#;
        let names = extract_tool_names_from_source(content);
        assert!(
            names.is_empty(),
            "Regular code between #[tool] and async fn should reset detection"
        );
    }
}
