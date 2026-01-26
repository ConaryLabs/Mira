// crates/mira-server/src/background/documentation/mod.rs
// Background worker for documentation tracking and generation

mod detection;
mod inventory;

use crate::db::{get_scan_info_sync, is_time_older_than_sync, delete_memory_by_key_sync, store_memory_sync, StoreMemoryParams};
use crate::db::pool::DatabasePool;
use std::process::Command;
use std::sync::Arc;

pub use detection::*;

/// Local struct for code symbol data (used in signature hash calculation)
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CodeSymbol {
    pub id: i64,
    pub project_id: i64,
    pub file_path: String,
    pub name: String,
    pub symbol_type: String,
    pub start_line: Option<i32>,
    pub end_line: Option<i32>,
    pub signature: Option<String>,
    pub indexed_at: String,
}

/// Calculate source signature hash (normalized hash of symbol signatures)
/// This is more stable than raw file checksum for detecting API changes
pub fn calculate_source_signature_hash(
    symbols: &[CodeSymbol],
) -> Option<String> {
    use sha2::Digest;

    if symbols.is_empty() {
        return None;
    }

    // Collect normalized signatures (name + type, not full signature with whitespace)
    let normalized: Vec<String> = symbols
        .iter()
        .filter_map(|s| s.signature.as_ref())
        .map(|sig| {
            // Normalize: remove extra whitespace, keep core signature
            sig.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();

    if normalized.is_empty() {
        return None;
    }

    // Sort for consistent hashing
    let mut sorted = normalized;
    sorted.sort();

    let mut hasher = sha2::Sha256::new();
    for sig in &sorted {
        hasher.update(sig.as_bytes());
        hasher.update(b"|");
    }

    Some(format!("{:x}", hasher.finalize()))
}

/// Process documentation detection for a single cycle
/// Called from BackgroundWorker::process_batch()
/// Only detects gaps - Claude decides when to write docs via write_documentation()
pub async fn process_documentation(
    pool: &Arc<DatabasePool>,
    _llm_factory: &Arc<crate::llm::ProviderFactory>,
) -> Result<usize, String> {
    // Scan for missing and stale documentation (detection only)
    let scan_count = scan_documentation_gaps(pool).await?;
    if scan_count > 0 {
        tracing::info!("Documentation scan found {} gaps", scan_count);
    }

    Ok(scan_count)
}

/// Get the current git HEAD commit hash
pub fn get_git_head(project_path: &str) -> Option<String> {
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

/// Check if a commit is an ancestor of HEAD (handles rebases, force pushes)
/// Copied from briefings.rs for documentation staleness detection
pub fn is_ancestor(project_path: &str, commit: &str) -> bool {
    Command::new("git")
        .args(["merge-base", "--is-ancestor", commit, "HEAD"])
        .current_dir(project_path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Calculate SHA256 checksum of file content
pub fn file_checksum(path: &std::path::Path) -> Option<String> {
    use sha2::Digest;
    use std::io::Read;

    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = Vec::new();

    file.read_to_end(&mut buffer).ok()?;
    hasher.update(&buffer);

    Some(format!("{:x}", hasher.finalize()))
}

/// Get a file's content as string
pub fn read_file_content(path: &std::path::Path) -> Result<String, std::io::Error> {
    std::fs::read_to_string(path)
}

/// Memory fact key for documentation scan marker
pub const DOC_SCAN_MARKER_KEY: &str = "documentation_last_scan";

/// Check if project needs documentation scan
pub fn needs_documentation_scan(
    conn: &rusqlite::Connection,
    project_id: i64,
    project_path: &str,
) -> Result<bool, String> {
    // Get last scan info from memory_facts
    let scan_info = get_scan_info_sync(conn, project_id, DOC_SCAN_MARKER_KEY);

    let (last_commit, last_scan_time) = match scan_info {
        Some((commit, time)) => (Some(commit), Some(time)),
        None => (None, None),
    };

    // Case 1: Never scanned
    if last_commit.is_none() {
        tracing::debug!("Project {} needs doc scan: never scanned", project_id);
        return Ok(true);
    }

    // Get current git HEAD
    let current_commit = get_git_head(project_path);

    // Case 2: Git changed AND rate limit passed (> 1 hour since last scan)
    if let (Some(last), Some(current)) = (&last_commit, &current_commit) {
        if last != current {
            if let Some(ref scan_time) = last_scan_time {
                if is_time_older_than_sync(conn, scan_time, "-1 hour") {
                    tracing::debug!(
                        "Project {} needs doc scan: git changed ({} -> {}) and rate limit passed",
                        project_id,
                        &last[..8.min(last.len())],
                        &current[..8.min(current.len())]
                    );
                    return Ok(true);
                }
            }
        }
    }

    // Case 3: Periodic refresh (> 24 hours since last scan)
    if let Some(ref scan_time) = last_scan_time {
        if is_time_older_than_sync(conn, scan_time, "-24 hours") {
            tracing::debug!("Project {} needs doc scan: periodic refresh", project_id);
            return Ok(true);
        }
    }

    Ok(false)
}

/// Mark that we've scanned a project's documentation (sync version)
pub fn mark_documentation_scanned_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
    project_path: &str,
) -> Result<(), String> {
    let commit = get_git_head(project_path).unwrap_or_else(|| "unknown".to_string());

    store_memory_sync(conn, StoreMemoryParams {
        project_id: Some(project_id),
        key: Some(DOC_SCAN_MARKER_KEY),
        content: &commit,
        fact_type: "system",
        category: Some("documentation"),
        confidence: 1.0,
        session_id: None,
        user_id: None,
        scope: "project",
    }).map_err(|e| e.to_string())?;
    Ok(())
}

/// Clear documentation scan marker to force new scan (sync version)
pub fn clear_documentation_scan_marker_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> Result<(), String> {
    delete_memory_by_key_sync(conn, project_id, DOC_SCAN_MARKER_KEY)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════════════════════════════════════════════
    // calculate_source_signature_hash Tests
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_calculate_source_signature_hash_empty() {
        let symbols: Vec<CodeSymbol> = vec![];
        assert!(calculate_source_signature_hash(&symbols).is_none());
    }

    #[test]
    fn test_calculate_source_signature_hash_no_signatures() {
        let symbols = vec![
            CodeSymbol {
                id: 1,
                project_id: 1,
                file_path: "test.rs".to_string(),
                name: "foo".to_string(),
                symbol_type: "function".to_string(),
                start_line: Some(1),
                end_line: Some(10),
                signature: None,
                indexed_at: "2024-01-01".to_string(),
            },
        ];
        assert!(calculate_source_signature_hash(&symbols).is_none());
    }

    #[test]
    fn test_calculate_source_signature_hash_with_signatures() {
        let symbols = vec![
            CodeSymbol {
                id: 1,
                project_id: 1,
                file_path: "test.rs".to_string(),
                name: "foo".to_string(),
                symbol_type: "function".to_string(),
                start_line: Some(1),
                end_line: Some(10),
                signature: Some("fn foo() -> bool".to_string()),
                indexed_at: "2024-01-01".to_string(),
            },
            CodeSymbol {
                id: 2,
                project_id: 1,
                file_path: "test.rs".to_string(),
                name: "bar".to_string(),
                symbol_type: "function".to_string(),
                start_line: Some(12),
                end_line: Some(20),
                signature: Some("fn bar(x: i32) -> String".to_string()),
                indexed_at: "2024-01-01".to_string(),
            },
        ];
        let hash = calculate_source_signature_hash(&symbols);
        assert!(hash.is_some());
        assert!(!hash.as_ref().unwrap().is_empty());
    }

    #[test]
    fn test_calculate_source_signature_hash_deterministic() {
        let symbols = vec![
            CodeSymbol {
                id: 1,
                project_id: 1,
                file_path: "test.rs".to_string(),
                name: "foo".to_string(),
                symbol_type: "function".to_string(),
                start_line: Some(1),
                end_line: Some(10),
                signature: Some("fn foo() -> bool".to_string()),
                indexed_at: "2024-01-01".to_string(),
            },
        ];
        let hash1 = calculate_source_signature_hash(&symbols);
        let hash2 = calculate_source_signature_hash(&symbols);
        assert_eq!(hash1, hash2, "Hash should be deterministic");
    }

    #[test]
    fn test_calculate_source_signature_hash_order_independent() {
        let symbols1 = vec![
            CodeSymbol {
                id: 1,
                project_id: 1,
                file_path: "test.rs".to_string(),
                name: "foo".to_string(),
                symbol_type: "function".to_string(),
                start_line: None,
                end_line: None,
                signature: Some("fn foo()".to_string()),
                indexed_at: "".to_string(),
            },
            CodeSymbol {
                id: 2,
                project_id: 1,
                file_path: "test.rs".to_string(),
                name: "bar".to_string(),
                symbol_type: "function".to_string(),
                start_line: None,
                end_line: None,
                signature: Some("fn bar()".to_string()),
                indexed_at: "".to_string(),
            },
        ];
        let symbols2 = vec![
            CodeSymbol {
                id: 2,
                project_id: 1,
                file_path: "test.rs".to_string(),
                name: "bar".to_string(),
                symbol_type: "function".to_string(),
                start_line: None,
                end_line: None,
                signature: Some("fn bar()".to_string()),
                indexed_at: "".to_string(),
            },
            CodeSymbol {
                id: 1,
                project_id: 1,
                file_path: "test.rs".to_string(),
                name: "foo".to_string(),
                symbol_type: "function".to_string(),
                start_line: None,
                end_line: None,
                signature: Some("fn foo()".to_string()),
                indexed_at: "".to_string(),
            },
        ];
        let hash1 = calculate_source_signature_hash(&symbols1);
        let hash2 = calculate_source_signature_hash(&symbols2);
        assert_eq!(hash1, hash2, "Hash should be order-independent (sorted internally)");
    }

    #[test]
    fn test_calculate_source_signature_hash_whitespace_normalization() {
        let symbols1 = vec![
            CodeSymbol {
                id: 1,
                project_id: 1,
                file_path: "test.rs".to_string(),
                name: "foo".to_string(),
                symbol_type: "function".to_string(),
                start_line: None,
                end_line: None,
                signature: Some("fn foo() -> bool".to_string()),
                indexed_at: "".to_string(),
            },
        ];
        let symbols2 = vec![
            CodeSymbol {
                id: 1,
                project_id: 1,
                file_path: "test.rs".to_string(),
                name: "foo".to_string(),
                symbol_type: "function".to_string(),
                start_line: None,
                end_line: None,
                signature: Some("fn   foo()   ->   bool".to_string()),
                indexed_at: "".to_string(),
            },
        ];
        let hash1 = calculate_source_signature_hash(&symbols1);
        let hash2 = calculate_source_signature_hash(&symbols2);
        assert_eq!(hash1, hash2, "Hash should normalize whitespace");
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // file_checksum Tests
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_file_checksum_nonexistent() {
        let path = std::path::Path::new("/nonexistent/path/file.txt");
        assert!(file_checksum(path).is_none());
    }

    #[test]
    fn test_file_checksum_temp_file() {
        use std::io::Write;

        // Create a temp file
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        temp_file.write_all(b"hello world").unwrap();
        temp_file.flush().unwrap();

        let checksum = file_checksum(temp_file.path());
        assert!(checksum.is_some());
        assert!(!checksum.as_ref().unwrap().is_empty());

        // Checksum should be deterministic
        let checksum2 = file_checksum(temp_file.path());
        assert_eq!(checksum, checksum2);
    }

    #[test]
    fn test_file_checksum_different_content() {
        use std::io::Write;

        let mut temp1 = tempfile::NamedTempFile::new().unwrap();
        temp1.write_all(b"content A").unwrap();
        temp1.flush().unwrap();

        let mut temp2 = tempfile::NamedTempFile::new().unwrap();
        temp2.write_all(b"content B").unwrap();
        temp2.flush().unwrap();

        let checksum1 = file_checksum(temp1.path());
        let checksum2 = file_checksum(temp2.path());

        assert_ne!(checksum1, checksum2, "Different content should have different checksums");
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // read_file_content Tests
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_read_file_content_nonexistent() {
        let path = std::path::Path::new("/nonexistent/path/file.txt");
        assert!(read_file_content(path).is_err());
    }

    #[test]
    fn test_read_file_content_temp_file() {
        use std::io::Write;

        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        temp_file.write_all(b"test content").unwrap();
        temp_file.flush().unwrap();

        let content = read_file_content(temp_file.path()).unwrap();
        assert_eq!(content, "test content");
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // CodeSymbol Tests
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_code_symbol_clone() {
        let symbol = CodeSymbol {
            id: 1,
            project_id: 42,
            file_path: "src/main.rs".to_string(),
            name: "main".to_string(),
            symbol_type: "function".to_string(),
            start_line: Some(1),
            end_line: Some(10),
            signature: Some("fn main()".to_string()),
            indexed_at: "2024-01-01T00:00:00Z".to_string(),
        };

        let cloned = symbol.clone();
        assert_eq!(symbol.id, cloned.id);
        assert_eq!(symbol.name, cloned.name);
        assert_eq!(symbol.signature, cloned.signature);
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // Constants Tests
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_doc_scan_marker_key_constant() {
        assert_eq!(DOC_SCAN_MARKER_KEY, "documentation_last_scan");
        assert!(!DOC_SCAN_MARKER_KEY.is_empty());
    }
}
