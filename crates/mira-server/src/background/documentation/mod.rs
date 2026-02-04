// crates/mira-server/src/background/documentation/mod.rs
// Background worker for documentation tracking and generation

mod detection;
mod inventory;

use crate::db::pool::DatabasePool;
use crate::db::{
    StoreMemoryParams, delete_memory_by_key_sync, get_scan_info_sync, is_time_older_than_sync,
    store_memory_sync,
};
use crate::utils::ResultExt;
use std::sync::Arc;

pub use detection::*;

/// Local struct for code symbol data (used in signature hash calculation)
#[derive(Debug, Clone)]
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
pub fn calculate_source_signature_hash(symbols: &[CodeSymbol]) -> Option<String> {
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
            sig.split_whitespace().collect::<Vec<_>>().join(" ")
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

/// Process documentation detection for a single cycle.
/// Called from SlowLaneWorker.
///
/// - `main_pool`: for documentation_inventory, memory_facts, doc tasks, LLM usage
/// - `code_pool`: for code_symbols, codebase_modules
///
/// Only detects gaps - Claude Code writes docs directly via documentation(action="get/complete")
pub async fn process_documentation(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
    llm_factory: &Arc<crate::llm::ProviderFactory>,
) -> Result<usize, String> {
    // Scan for missing and stale documentation (detection only)
    let scan_count = scan_documentation_gaps(main_pool, code_pool).await?;
    if scan_count > 0 {
        tracing::info!("Documentation scan found {} gaps", scan_count);
    }

    // Analyze impact of stale docs using LLM
    let analyzed = analyze_stale_doc_impacts(main_pool, code_pool, llm_factory).await?;
    if analyzed > 0 {
        tracing::info!("Analyzed impact for {} stale docs", analyzed);
    }

    Ok(scan_count + analyzed)
}

/// Analyze the impact of changes for stale documentation using LLM.
///
/// - `main_pool`: for documentation_inventory, LLM usage
/// - `code_pool`: for code_symbols (current signatures)
async fn analyze_stale_doc_impacts(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
    llm_factory: &Arc<crate::llm::ProviderFactory>,
) -> Result<usize, String> {
    use crate::db::documentation::{get_stale_docs_needing_analysis, update_doc_impact_analysis};
    use crate::llm::{PromptBuilder, record_llm_usage};

    // Get LLM client for background work
    let client = match llm_factory.client_for_background() {
        Some(c) => c,
        None => {
            tracing::debug!("No LLM client available for doc impact analysis");
            return Ok(0);
        }
    };

    // Get all projects with stale docs needing analysis
    let projects: Vec<(i64, String)> = main_pool
        .run(|conn| {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT p.id, p.path FROM projects p
                 JOIN documentation_inventory di ON di.project_id = p.id
                 WHERE di.is_stale = 1 AND di.change_impact IS NULL
                 LIMIT 5",
            )?;
            let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<Result<Vec<_>, _>>()
        })
        .await
        .str_err()?;

    let mut total_analyzed = 0;

    for (project_id, _project_path) in projects {
        // Get stale docs for this project (limit to avoid overwhelming)
        let stale_docs = main_pool
            .run(move |conn| get_stale_docs_needing_analysis(conn, project_id, 3))
            .await?;

        for doc in stale_docs {
            // Build context for LLM
            let source_file = doc.source_symbols.as_deref().unwrap_or("unknown");
            let staleness_reason = doc.staleness_reason.as_deref().unwrap_or("source changed");

            // Try to get current source signatures for comparison
            let current_signatures =
                get_current_signatures(code_pool, project_id, source_file).await;

            let prompt = format!(
                r#"Analyze the impact of source code changes on documentation.

Documentation file: {}
Source file: {}
Change detected: {}

Current source signatures:
{}

Classify the change impact as either "significant" or "minor":

SIGNIFICANT changes (documentation MUST be updated):
- Public function signatures changed (parameters, return types)
- New public functions/methods added
- Functions removed or renamed
- Behavior changes that affect usage
- New error conditions or edge cases

MINOR changes (documentation update optional):
- Internal refactoring (variable renames, code reorganization)
- Performance optimizations without API changes
- Comments or formatting changes
- Private/internal function changes

Respond in this exact format:
IMPACT: [significant/minor]
SUMMARY: [One sentence explaining what changed and why it matters or doesn't]"#,
                doc.doc_path,
                source_file,
                staleness_reason,
                current_signatures.unwrap_or_else(|| "Unable to retrieve".to_string())
            );

            let messages = PromptBuilder::for_background().build_messages(prompt);

            match client.chat(messages, None).await {
                Ok(result) => {
                    // Parse the response
                    let content = result.content.as_deref().unwrap_or("");
                    let (impact, summary) = parse_impact_response(content);

                    // Record LLM usage
                    let _ = record_llm_usage(
                        main_pool,
                        client.provider_type(),
                        &client.model_name(),
                        "background:doc_impact_analysis",
                        &result,
                        Some(project_id),
                        None,
                    )
                    .await;

                    // Update the database
                    let doc_id = doc.id;
                    let impact_clone = impact.clone();
                    let summary_clone = summary.clone();
                    main_pool
                        .run(move |conn| {
                            update_doc_impact_analysis(conn, doc_id, &impact_clone, &summary_clone)
                        })
                        .await?;

                    tracing::debug!(
                        "Doc impact analysis for {}: {} - {}",
                        doc.doc_path,
                        impact,
                        summary
                    );
                    total_analyzed += 1;
                }
                Err(e) => {
                    tracing::warn!("Failed to analyze doc impact for {}: {}", doc.doc_path, e);
                }
            }
        }
    }

    Ok(total_analyzed)
}

/// Get current source signatures for a file (reads from code_symbols in code DB)
async fn get_current_signatures(
    code_pool: &Arc<DatabasePool>,
    project_id: i64,
    source_file: &str,
) -> Option<String> {
    let source_file = source_file.to_string();
    code_pool
        .run(move |conn| -> Result<Option<String>, rusqlite::Error> {
            let mut stmt = conn.prepare(
                "SELECT name, symbol_type, signature FROM code_symbols
             WHERE project_id = ? AND file_path = ?
             AND symbol_type IN ('function', 'method', 'struct', 'enum', 'trait')
             ORDER BY start_line",
            )?;

            let rows: Vec<String> = stmt
                .query_map(rusqlite::params![project_id, source_file], |row| {
                    let name: String = row.get(0)?;
                    let sym_type: String = row.get(1)?;
                    let sig: Option<String> = row.get(2)?;
                    Ok(format!(
                        "- {} ({}): {}",
                        name,
                        sym_type,
                        sig.unwrap_or_else(|| "no signature".to_string())
                    ))
                })?
                .filter_map(|r| r.ok())
                .collect();

            if rows.is_empty() {
                Ok(None)
            } else {
                Ok(Some(rows.join("\n")))
            }
        })
        .await
        .ok()
        .flatten()
}

/// Parse the LLM response to extract impact and summary
fn parse_impact_response(response: &str) -> (String, String) {
    let mut impact = "significant".to_string(); // Default to significant if parsing fails
    let mut summary = "Unable to determine change impact".to_string();

    for line in response.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("IMPACT:") {
            let value = rest.trim().to_lowercase();
            if value == "minor" || value.starts_with("minor") {
                impact = "minor".to_string();
            } else {
                impact = "significant".to_string();
            }
        } else if let Some(rest) = line.strip_prefix("SUMMARY:") {
            summary = rest.trim().to_string();
        }
    }

    (impact, summary)
}

pub use crate::git::{get_git_head, is_ancestor};

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
    if let (Some(last), Some(current)) = (&last_commit, &current_commit)
        && last != current
            && let Some(ref scan_time) = last_scan_time
                && is_time_older_than_sync(conn, scan_time, "-1 hour") {
                    tracing::debug!(
                        "Project {} needs doc scan: git changed ({} -> {}) and rate limit passed",
                        project_id,
                        &last[..8.min(last.len())],
                        &current[..8.min(current.len())]
                    );
                    return Ok(true);
                }

    // Case 3: Periodic refresh (> 24 hours since last scan)
    if let Some(ref scan_time) = last_scan_time
        && is_time_older_than_sync(conn, scan_time, "-24 hours") {
            tracing::debug!("Project {} needs doc scan: periodic refresh", project_id);
            return Ok(true);
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

    store_memory_sync(
        conn,
        StoreMemoryParams {
            project_id: Some(project_id),
            key: Some(DOC_SCAN_MARKER_KEY),
            content: &commit,
            fact_type: "system",
            category: Some("documentation"),
            confidence: 1.0,
            session_id: None,
            user_id: None,
            scope: "project",
            branch: None,
        },
    )
    .str_err()?;
    Ok(())
}

/// Clear documentation scan marker to force new scan (sync version)
pub fn clear_documentation_scan_marker_sync(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> Result<(), String> {
    delete_memory_by_key_sync(conn, project_id, DOC_SCAN_MARKER_KEY)
        .map(|_| ())
        .str_err()
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
        let symbols = vec![CodeSymbol {
            id: 1,
            project_id: 1,
            file_path: "test.rs".to_string(),
            name: "foo".to_string(),
            symbol_type: "function".to_string(),
            start_line: Some(1),
            end_line: Some(10),
            signature: None,
            indexed_at: "2024-01-01".to_string(),
        }];
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
        let symbols = vec![CodeSymbol {
            id: 1,
            project_id: 1,
            file_path: "test.rs".to_string(),
            name: "foo".to_string(),
            symbol_type: "function".to_string(),
            start_line: Some(1),
            end_line: Some(10),
            signature: Some("fn foo() -> bool".to_string()),
            indexed_at: "2024-01-01".to_string(),
        }];
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
        assert_eq!(
            hash1, hash2,
            "Hash should be order-independent (sorted internally)"
        );
    }

    #[test]
    fn test_calculate_source_signature_hash_whitespace_normalization() {
        let symbols1 = vec![CodeSymbol {
            id: 1,
            project_id: 1,
            file_path: "test.rs".to_string(),
            name: "foo".to_string(),
            symbol_type: "function".to_string(),
            start_line: None,
            end_line: None,
            signature: Some("fn foo() -> bool".to_string()),
            indexed_at: "".to_string(),
        }];
        let symbols2 = vec![CodeSymbol {
            id: 1,
            project_id: 1,
            file_path: "test.rs".to_string(),
            name: "foo".to_string(),
            symbol_type: "function".to_string(),
            start_line: None,
            end_line: None,
            signature: Some("fn   foo()   ->   bool".to_string()),
            indexed_at: "".to_string(),
        }];
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

        assert_ne!(
            checksum1, checksum2,
            "Different content should have different checksums"
        );
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
}
