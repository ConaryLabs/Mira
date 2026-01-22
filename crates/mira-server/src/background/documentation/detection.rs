// crates/mira-server/src/background/documentation/detection.rs
// Documentation gap and staleness detection

use crate::db::Database;
use crate::db::documentation::{create_doc_task, mark_doc_stale, DocGap};
use rusqlite::params;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use super::{calculate_source_signature_hash, get_git_head, is_ancestor};

/// Scan for documentation gaps and stale docs
/// Returns number of new tasks created
pub async fn scan_documentation_gaps(db: &Arc<Database>) -> Result<usize, String> {
    let db_clone = db.clone();

    // Get all indexed projects
    let projects = tokio::task::spawn_blocking(move || {
        let conn = db_clone.conn();
        conn.prepare(
            "SELECT DISTINCT p.id, p.path
             FROM projects p
             JOIN codebase_modules m ON m.project_id = p.id"
        )
        .map_err(|e| e.to_string())?
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))??;

    let mut total_created = 0;

    for (project_id, project_path) in projects {
        // Check if project needs scan
        let needs_scan = tokio::task::spawn_blocking({
            let db_clone = db.clone();
            let project_path = project_path.clone();
            move || {
                let conn = db_clone.conn();
                super::needs_documentation_scan(&conn, project_id, &project_path)
            }
        })
        .await
        .map_err(|e| format!("spawn_blocking panicked: {}", e))??;

        if !needs_scan {
            continue;
        }

        let project_path = Path::new(&project_path);
        if !project_path.exists() {
            continue;
        }

        // Scan for gaps
        let created = detect_gaps_for_project(db, project_id, project_path).await?;
        total_created += created;

        // Scan for stale docs
        let stale = detect_stale_docs_for_project(db, project_id, project_path).await?;

        // Mark as scanned
        super::mark_documentation_scanned(db, project_id, project_path.to_str().unwrap_or(""))?;
    }

    Ok(total_created)
}

/// Detect missing documentation for a project
async fn detect_gaps_for_project(
    db: &Arc<Database>,
    project_id: i64,
    project_path: &Path,
) -> Result<usize, String> {
    let mut gaps = Vec::new();
    let project_str = project_path.to_str().unwrap_or("");

    // Get current git commit
    let git_commit = get_git_head(project_str);

    // Detect MCP tool documentation gaps
    gaps.extend(detect_mcp_tool_gaps(db, project_id, project_path).await?);

    // Detect CLI command documentation gaps
    gaps.extend(detect_cli_gaps(db, project_id, project_path).await?);

    // Detect public API documentation gaps
    gaps.extend(detect_public_api_gaps(db, project_id, project_path).await?);

    // Detect module documentation gaps
    gaps.extend(detect_module_doc_gaps(db, project_id, project_path).await?);

    // Create tasks for all gaps
    let db_clone = db.clone();
    let gaps_to_create = gaps.clone();

    let created = tokio::task::spawn_blocking(move || {
        let conn = db_clone.conn();
        let mut count = 0;
        for gap in gaps_to_create {
            match create_doc_task(&conn, &gap, git_commit.as_deref()) {
                Ok(_) => count += 1,
                Err(e) => {
                    // Likely a uniqueness constraint violation - task already exists
                    tracing::debug!("Documentation task already exists for {:?}: {}", gap.target_doc_path, e);
                }
            }
        }
        Ok::<usize, String>(count)
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))??;

    if created > 0 {
        tracing::info!("Documentation: created {} tasks for project {}", created, project_id);
    }

    Ok(created)
}

/// Detect undocumented MCP tools by parsing mcp/mod.rs
async fn detect_mcp_tool_gaps(
    db: &Arc<Database>,
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
        std::fs::read_to_string(&mcp_mod_path)
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
    let documented = tokio::task::spawn_blocking({
        let db_clone = db.clone();
        move || {
            let conn = db_clone.conn();
            let mut stmt = conn
                .prepare(
                    "SELECT doc_path FROM documentation_inventory
                     WHERE project_id = ? AND doc_category = 'mcp_tool'"
                )
                .map_err(|e| e.to_string())?;

            stmt.query_map(params![project_id], |row| row.get::<_, String>("doc_path"))
                .map_err(|e| e.to_string())?
                .collect::<Result<HashSet<_>, _>>()
                .map_err(|e| e.to_string())
        }
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))??;

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
    db: &Arc<Database>,
    project_id: i64,
    project_path: &Path,
) -> Result<Vec<DocGap>, String> {
    // For now, return empty - CLI commands are documented in README
    // This could be extended to parse main.rs clap Command definitions
    Ok(Vec::new())
}

/// Detect undocumented public APIs
async fn detect_public_api_gaps(
    db: &Arc<Database>,
    project_id: i64,
    _project_path: &Path,
) -> Result<Vec<DocGap>, String> {
    let mut gaps = Vec::new();

    // Find public symbols in lib.rs
    let db_clone = db.clone();
    let lib_symbols = tokio::task::spawn_blocking(move || {
        let conn = db_clone.conn();
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT name, signature
                 FROM code_symbols
                 WHERE project_id = ? AND file_path LIKE '%lib.rs'
                 AND symbol_type IN ('function', 'struct', 'enum', 'type')
                 ORDER BY name"
            )
            .map_err(|e| e.to_string())?;

        stmt.query_map(params![project_id], |row| {
            Ok((row.get::<_, String>("name")?, row.get::<_, Option<String>>("signature")?))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))??;

    // Get documented public APIs
    let documented = tokio::task::spawn_blocking({
        let db_clone = db.clone();
        move || {
            let conn = db_clone.conn();
            let mut stmt = conn
                .prepare(
                    "SELECT doc_path FROM documentation_inventory
                     WHERE project_id = ? AND doc_category = 'public_api'"
                )
                .map_err(|e| e.to_string())?;

            stmt.query_map(params![project_id], |row| row.get::<_, String>("doc_path"))
                .map_err(|e| e.to_string())?
                .collect::<Result<HashSet<_>, _>>()
                .map_err(|e| e.to_string())
        }
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))??;

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
    db: &Arc<Database>,
    project_id: i64,
    _project_path: &Path,
) -> Result<Vec<DocGap>, String> {
    let mut gaps = Vec::new();

    // Get all modules from codebase_modules
    let db_clone = db.clone();
    let modules = tokio::task::spawn_blocking(move || {
        let conn = db_clone.conn();
        let mut stmt = conn
            .prepare(
                "SELECT module_id, path, purpose
                 FROM codebase_modules
                 WHERE project_id = ?
                 ORDER BY module_id"
            )
            .map_err(|e| e.to_string())?;

        stmt.query_map(params![project_id], |row| {
            Ok((
                row.get::<_, String>("module_id")?,
                row.get::<_, String>("path")?,
                row.get::<_, Option<String>>("purpose")?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))??;

    // Get documented modules
    let documented = tokio::task::spawn_blocking({
        let db_clone = db.clone();
        move || {
            let conn = db_clone.conn();
            let mut stmt = conn
                .prepare(
                    "SELECT doc_path FROM documentation_inventory
                     WHERE project_id = ? AND doc_category = 'module'"
                )
                .map_err(|e| e.to_string())?;

            stmt.query_map(params![project_id], |row| row.get::<_, String>("doc_path"))
                .map_err(|e| e.to_string())?
                .collect::<Result<HashSet<_>, _>>()
                .map_err(|e| e.to_string())
        }
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))??;

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
    db: &Arc<Database>,
    project_id: i64,
    project_path: &Path,
) -> Result<usize, String> {
    let mut stale_count = 0;
    let project_str = project_path.to_str().unwrap_or("");
    let current_commit = get_git_head(project_str);

    // Get all documented items with source info
    let db_clone = db.clone();
    let inventory = tokio::task::spawn_blocking(move || {
        let conn = db_clone.conn();
        let mut stmt = conn
            .prepare(
                "SELECT * FROM documentation_inventory
                 WHERE project_id = ? AND source_signature_hash IS NOT NULL
                 AND is_stale = 0"
            )
            .map_err(|e| e.to_string())?;

        stmt.query_map(params![project_id], |row| {
            Ok(crate::db::documentation::parse_doc_inventory(row)?)
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))??;

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
            if let Some(new_hash) = check_source_signature_changed(db, project_id, &source_file).await? {
                if item.source_signature_hash.as_ref() != Some(&new_hash) {
                    is_stale = true;
                    if reason.is_empty() {
                        reason = format!("Source signatures changed");
                    }
                }
            }
        }

        if is_stale {
            // Mark as stale (synchronous DB operation)
            {
                let conn = db.conn();
                mark_doc_stale(&conn, project_id, &item.doc_path, &reason)?;
            }
            stale_count += 1;
            tracing::debug!("Stale documentation: {} - {}", item.doc_path, reason);
        }
    }

    Ok(stale_count)
}

/// Check if source signatures have changed
async fn check_source_signature_changed(
    db: &Arc<Database>,
    project_id: i64,
    source_path: &str,
) -> Result<Option<String>, String> {
    let db_clone = db.clone();
    let source_path = source_path.to_string();

    let symbols = tokio::task::spawn_blocking(move || {
        let conn = db_clone.conn();
        let mut stmt = conn
            .prepare(
                "SELECT * FROM code_symbols
                 WHERE project_id = ? AND file_path = ?
                 ORDER BY name"
            )
            .map_err(|e| e.to_string())?;

        stmt.query_map(params![project_id, source_path], |row| {
            Ok(super::CodeSymbol {
                id: row.get("id")?,
                project_id: row.get("project_id")?,
                file_path: row.get("file_path")?,
                name: row.get("name")?,
                symbol_type: row.get("symbol_type")?,
                start_line: row.get("start_line")?,
                end_line: row.get("end_line")?,
                signature: row.get("signature")?,
                indexed_at: row.get("indexed_at")?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))??;

    Ok(calculate_source_signature_hash(&symbols))
}
