// crates/mira-server/src/background/documentation/inventory.rs
// Documentation inventory scanning and tracking

use crate::db::Database;
use crate::db::documentation::upsert_doc_inventory;
use rusqlite::params;
use std::path::Path;
use std::sync::Arc;

use super::{calculate_source_signature_hash, get_git_head};

/// Scan existing documentation and update inventory
pub async fn scan_existing_docs(
    db: &Arc<Database>,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    let project_path = Path::new(project_path);

    // Get current git commit
    let current_commit = get_git_head(project_path.to_str().unwrap_or(""));

    // Scan for docs/ directory
    let docs_dir = project_path.join("docs");
    if !docs_dir.exists() {
        return Ok(0);
    }

    let mut scanned = 0;

    // Collect all markdown files using simple recursive scan
    let mut doc_files = collect_markdown_files(&docs_dir);

    // Also add root-level documentation files
    for file in ["README.md", "CONTRIBUTING.md", "CONFIGURATION.md", "ARCHITECTURE.md"] {
        let file_path = project_path.join(file);
        if file_path.exists() {
            doc_files.push(file_path);
        }
    }

    // Process all files
    for file_path in doc_files {
        if inventory_file(
            db,
            project_id,
            project_path,
            &file_path,
            current_commit.as_deref(),
        ).await.is_ok() {
            scanned += 1;
        }
    }

    tracing::info!(
        "Documentation inventory: scanned {} docs for project {}",
        scanned,
        project_id
    );

    Ok(scanned)
}

/// Collect all markdown files recursively
fn collect_markdown_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_markdown_files(&path));
            } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
                files.push(path);
            }
        }
    }

    files
}

/// Inventory a single documentation file
async fn inventory_file(
    db: &Arc<Database>,
    project_id: i64,
    project_root: &Path,
    file_path: &Path,
    git_commit: Option<&str>,
) -> Result<(), String> {
    let rel_path = file_path
        .strip_prefix(project_root)
        .map_err(|e| format!("Failed to get relative path: {}", e))?;

    let doc_path = rel_path
        .to_str()
        .ok_or("Invalid UTF-8 in path")?
        .to_string();

    // Determine doc type and category from path
    let (doc_type, doc_category) = classify_document(&doc_path);

    // Extract title from first heading
    let title = extract_title(file_path).await;

    // Get source file path (code that this doc documents)
    let source_file_path = find_source_for_doc(&doc_path);

    // Calculate source signature hash if we have a source file
    let source_signature_hash = if let Some(ref source) = source_file_path {
        get_source_signature(db, project_id, source).await?
    } else {
        None
    };

    let source_symbols = source_signature_hash.as_ref().map(|_| "from_source".to_string());

    // Clone db for spawn_blocking (Arc clone is cheap)
    let db_clone = db.clone();
    // Convert git_commit reference to owned String
    let git_commit_owned = git_commit.map(|s| s.to_string());

    // Upsert inventory
    tokio::task::spawn_blocking(move || {
        let conn = db_clone.conn();
        upsert_doc_inventory(
            &conn,
            project_id,
            &doc_path,
            &doc_type,
            doc_category.as_deref(),
            title.as_deref(),
            source_signature_hash.as_deref(),
            source_symbols.as_deref(),
            git_commit_owned.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("spawn_blocking panicked: {}", e))??;

    Ok(())
}

/// Classify document type and category from path
fn classify_document(path: &str) -> (String, Option<String>) {
    let path_lower = path.to_lowercase();

    let (doc_type, doc_category) = if path_lower.contains("tools/") {
        ("api".to_string(), Some("mcp_tool".to_string()))
    } else if path_lower.contains("api/") {
        ("api".to_string(), Some("public_api".to_string()))
    } else if path_lower.contains("modules/") {
        ("architecture".to_string(), Some("module".to_string()))
    } else if path_lower.contains("guides/") || path_lower.contains("guide/") {
        ("guide".to_string(), None)
    } else if path_lower.contains("contributing") {
        ("guide".to_string(), Some("contributing".to_string()))
    } else if path_lower.contains("readme") {
        ("guide".to_string(), Some("readme".to_string()))
    } else if path_lower.contains("configuration") || path_lower.contains("config") {
        ("guide".to_string(), Some("config".to_string()))
    } else if path_lower.contains("architecture") {
        ("architecture".to_string(), None)
    } else if path_lower.contains("testing") || path_lower.contains("test") {
        ("guide".to_string(), Some("testing".to_string()))
    } else {
        ("guide".to_string(), None)
    };

    (doc_type, doc_category)
}

/// Extract title from markdown file (first # heading)
async fn extract_title(file_path: &Path) -> Option<String> {
    let file_path_buf = file_path.to_path_buf();
    let content = tokio::task::spawn_blocking(move || {
        std::fs::read_to_string(&file_path_buf).ok()
    })
    .await
    .ok()
    .flatten()?;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            return Some(trimmed[2..].trim().to_string());
        }
    }

    None
}

/// Find the source file that a document documents
fn find_source_for_doc(doc_path: &str) -> Option<String> {
    // Common mappings from docs to source
    if doc_path.contains("tools/") {
        // Extract tool name from path like docs/tools/my_tool.md
        if let Some(name) = doc_path
            .split("tools/")
            .nth(1)
            .and_then(|s| s.strip_suffix(".md"))
        {
            return Some(format!("src/tools/core/{}.rs", name));
        }
    }

    if doc_path.contains("modules/") {
        // Extract module ID from path like docs/modules/cartographer.md
        if let Some(name) = doc_path
            .split("modules/")
            .nth(1)
            .and_then(|s| s.strip_suffix(".md"))
        {
            return Some(format!("src/{}.rs", name));
        }
    }

    None
}

/// Get source signature hash for a source file
async fn get_source_signature(
    db: &Arc<Database>,
    project_id: i64,
    source_path: &str,
) -> Result<Option<String>, String> {
    let db_clone = db.clone();
    let source_path = source_path.to_string();

    let symbols = tokio::task::spawn_blocking(move || {
        let conn = db_clone.conn();
        conn.prepare(
            "SELECT * FROM code_symbols
             WHERE project_id = ? AND file_path = ?
             ORDER BY name"
        )
        .map_err(|e| e.to_string())?
        .query_map(params![project_id, source_path], |row| {
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
