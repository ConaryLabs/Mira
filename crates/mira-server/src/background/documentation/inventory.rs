// crates/mira-server/src/background/documentation/inventory.rs
// Documentation inventory scanning and tracking

use crate::db::documentation::upsert_doc_inventory;
use crate::db::get_symbols_for_file_sync;
use crate::db::pool::DatabasePool;
use crate::utils::ResultExt;
use std::path::Path;
use std::sync::Arc;

use super::{get_git_head, read_file_content};

/// Scan existing documentation and update inventory
pub async fn scan_existing_docs(
    pool: &Arc<DatabasePool>,
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
    for file in [
        "README.md",
        "CONTRIBUTING.md",
        "CONFIGURATION.md",
        "ARCHITECTURE.md",
    ] {
        let file_path = project_path.join(file);
        if file_path.exists() {
            doc_files.push(file_path);
        }
    }

    // Process all files
    for file_path in doc_files {
        if inventory_file(
            pool,
            project_id,
            project_path,
            &file_path,
            current_commit.as_deref(),
        )
        .await
        .is_ok()
        {
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
    pool: &Arc<DatabasePool>,
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
        get_source_signature(pool, project_id, source).await?
    } else {
        None
    };

    let source_symbols = source_signature_hash
        .as_ref()
        .map(|_| "from_source".to_string());

    // Convert git_commit reference to owned String
    let git_commit_owned = git_commit.map(|s| s.to_string());

    // Upsert inventory
    pool.interact(move |conn| {
        upsert_doc_inventory(
            conn,
            project_id,
            &doc_path,
            &doc_type,
            doc_category.as_deref(),
            title.as_deref(),
            source_signature_hash.as_deref(),
            source_symbols.as_deref(),
            git_commit_owned.as_deref(),
        )
        .map_err(|e| anyhow::anyhow!("{}", e))
    })
    .await
    .str_err()?;

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
    let content = tokio::task::spawn_blocking(move || read_file_content(&file_path_buf).ok())
        .await
        .ok()
        .flatten()?;

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(title) = trimmed.strip_prefix("# ") {
            return Some(title.trim().to_string());
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
    pool: &Arc<DatabasePool>,
    project_id: i64,
    source_path: &str,
) -> Result<Option<String>, String> {
    use sha2::Digest;

    let source_path = source_path.to_string();

    // Get symbols from db - returns (id, name, symbol_type, start_line, end_line, signature)
    let symbols = pool
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
