// crates/mira-server/src/background/documentation/mod.rs
// Background worker for documentation tracking and generation

mod detection;
mod inventory;

use crate::db::pool::DatabasePool;
use crate::db::{
    StoreObservationParams, get_scan_info_sync, is_time_older_than_sync, store_observation_sync,
};
use crate::utils::{ResultExt, truncate_at_boundary};
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
    let sigs: Vec<&str> = symbols
        .iter()
        .filter_map(|s| s.signature.as_deref())
        .collect();
    hash_normalized_signatures(&sigs)
}

/// Compute a SHA256 hash from a slice of raw signature strings.
/// Normalises whitespace and sorts for order-independence.
/// Shared by `calculate_source_signature_hash`, `get_source_signature` (inventory),
/// and `check_source_signature_changed` (detection).
pub fn hash_normalized_signatures(signatures: &[&str]) -> Option<String> {
    use sha2::Digest;

    if signatures.is_empty() {
        return None;
    }

    let mut normalized: Vec<String> = signatures
        .iter()
        .map(|sig| sig.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();

    if normalized.is_empty() {
        return None;
    }

    // Sort for consistent hashing
    normalized.sort();

    let mut hasher = sha2::Sha256::new();
    for sig in &normalized {
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
/// - `client`: optional LLM client for semantic analysis; heuristic fallback when absent
///
/// Only detects gaps - Claude Code writes docs directly via documentation(action="get/complete")
pub async fn process_documentation(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
) -> Result<usize, String> {
    // Scan for missing and stale documentation (detection only)
    let scan_count = scan_documentation_gaps(main_pool, code_pool).await?;
    if scan_count > 0 {
        tracing::info!("Documentation scan found {} gaps", scan_count);
    }

    // Analyze impact of stale docs (heuristic comparison)
    let analyzed = analyze_stale_doc_impacts(main_pool, code_pool).await?;
    if analyzed > 0 {
        tracing::info!("Analyzed impact for {} stale docs", analyzed);
    }

    Ok(scan_count + analyzed)
}

/// Analyze the impact of changes for stale documentation.
/// Uses heuristic comparison of signature hashes.
///
/// - `main_pool`: for documentation_inventory
/// - `code_pool`: for code_symbols (current signatures)
async fn analyze_stale_doc_impacts(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
) -> Result<usize, String> {
    use crate::db::documentation::{get_stale_docs_needing_analysis, update_doc_impact_analysis};

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
        .await?;

    let mut total_analyzed = 0;

    for (project_id, _project_path) in projects {
        // Get stale docs for this project (limit to avoid overwhelming)
        let stale_docs = main_pool
            .run(move |conn| get_stale_docs_needing_analysis(conn, project_id, 3))
            .await?;

        for doc in stale_docs {
            // Extract source file path from source_symbols (expects "source_file:<path>" format)
            let source_file = doc
                .source_symbols
                .as_deref()
                .and_then(|s| s.strip_prefix("source_file:"))
                .unwrap_or_else(|| doc.source_symbols.as_deref().unwrap_or("unknown"));
            let _staleness_reason = doc.staleness_reason.as_deref().unwrap_or("source changed");

            let (impact, summary) = analyze_impact_heuristic(
                code_pool,
                project_id,
                source_file,
                doc.source_signature_hash.as_deref(),
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
    }

    Ok(total_analyzed)
}

/// Heuristic doc impact analysis: compare old signature hash with current signatures.
/// - If signature count changed -> "significant"
/// - If signatures changed but count is same -> "moderate"
/// - If only hash changed with no visible diff -> "minor"
/// - Default to "significant" when ambiguous
async fn analyze_impact_heuristic(
    code_pool: &Arc<DatabasePool>,
    project_id: i64,
    source_file: &str,
    old_hash: Option<&str>,
) -> (String, String) {
    use super::HEURISTIC_PREFIX;

    let source_path = source_file.to_string();

    // Get current symbols from code DB
    let current_symbols = code_pool
        .interact(move |conn| {
            crate::db::get_symbols_for_file_sync(conn, project_id, &source_path)
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .ok()
        .unwrap_or_default();

    let current_sigs: Vec<&str> = current_symbols
        .iter()
        .filter_map(|(_, _, _, _, _, sig)| sig.as_deref())
        .collect();

    let current_hash = hash_normalized_signatures(&current_sigs);
    let current_count = current_sigs.len();

    match (old_hash, current_hash.as_deref()) {
        // No old hash: can't compare, assume significant
        (None, _) => (
            "significant".to_string(),
            format!(
                "{}No previous signature hash to compare, assuming significant",
                HEURISTIC_PREFIX
            ),
        ),
        // Hash unchanged: shouldn't be stale, but mark as minor
        (Some(old), Some(new)) if old == new => (
            "minor".to_string(),
            format!(
                "{}Signature hash unchanged, likely internal changes only",
                HEURISTIC_PREFIX
            ),
        ),
        // Hash changed: check signature count for severity
        (Some(_), Some(_)) => {
            // Get old signature count from stored hash (we can't recover it, but we
            // can check if current symbols are empty vs non-empty)
            if current_count == 0 {
                (
                    "significant".to_string(),
                    format!(
                        "{}Source file signatures no longer found (file removed or renamed?)",
                        HEURISTIC_PREFIX
                    ),
                )
            } else {
                // Hash changed with signatures present: classify as moderate
                // (can't determine count difference without storing old count)
                (
                    "moderate".to_string(),
                    format!(
                        "{}Source signatures changed ({} current signatures)",
                        HEURISTIC_PREFIX, current_count
                    ),
                )
            }
        }
        // No current hash but had old one: source likely removed
        (Some(_), None) => (
            "significant".to_string(),
            format!(
                "{}Source signatures no longer available (file removed or emptied?)",
                HEURISTIC_PREFIX
            ),
        ),
    }
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
        && is_time_older_than_sync(conn, scan_time, "-1 hour")
    {
        tracing::debug!(
            "Project {} needs doc scan: git changed ({} -> {}) and rate limit passed",
            project_id,
            truncate_at_boundary(last, 8),
            truncate_at_boundary(current, 8)
        );
        return Ok(true);
    }

    // Case 3: Periodic refresh (> 24 hours since last scan)
    if let Some(ref scan_time) = last_scan_time
        && is_time_older_than_sync(conn, scan_time, "-24 hours")
    {
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

    store_observation_sync(
        conn,
        StoreObservationParams {
            project_id: Some(project_id),
            key: Some(DOC_SCAN_MARKER_KEY),
            content: &commit,
            observation_type: "system",
            category: Some("documentation"),
            confidence: 1.0,
            source: "documentation",
            session_id: None,
            team_id: None,
            scope: "project",
            expires_at: None,
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
    crate::db::delete_observation_by_key_sync(conn, project_id, DOC_SCAN_MARKER_KEY)
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

    // =========================================================================
    // parse_impact_response Tests
    // =========================================================================

    #[test]
    fn test_parse_impact_response_valid_minor() {
        let response = "IMPACT: minor\nSUMMARY: Internal refactoring only, no API changes";
        let (impact, summary) = parse_impact_response(response);

        assert_eq!(impact, "minor");
        assert_eq!(summary, "Internal refactoring only, no API changes");
    }

    #[test]
    fn test_parse_impact_response_valid_significant() {
        let response = "IMPACT: significant\nSUMMARY: Public function signature changed";
        let (impact, summary) = parse_impact_response(response);

        assert_eq!(impact, "significant");
        assert_eq!(summary, "Public function signature changed");
    }

    #[test]
    fn test_parse_impact_response_missing_impact_defaults_significant() {
        let response = "SUMMARY: Some changes were made";
        let (impact, summary) = parse_impact_response(response);

        assert_eq!(
            impact, "significant",
            "Missing IMPACT line should default to significant"
        );
        assert_eq!(summary, "Some changes were made");
    }

    #[test]
    fn test_parse_impact_response_missing_summary_keeps_default() {
        let response = "IMPACT: minor";
        let (impact, summary) = parse_impact_response(response);

        assert_eq!(impact, "minor");
        assert_eq!(
            summary, "Unable to determine change impact",
            "Missing SUMMARY should keep default message"
        );
    }

    #[test]
    fn test_parse_impact_response_empty_string() {
        let (impact, summary) = parse_impact_response("");

        assert_eq!(
            impact, "significant",
            "Empty response should default to significant"
        );
        assert_eq!(summary, "Unable to determine change impact");
    }

    #[test]
    fn test_parse_impact_response_extra_whitespace() {
        let response = "IMPACT:   minor  \nSUMMARY:   Whitespace around values   ";
        let (impact, summary) = parse_impact_response(response);

        assert_eq!(
            impact, "minor",
            "Should trim whitespace around impact value"
        );
        assert_eq!(summary, "Whitespace around values");
    }

    #[test]
    fn test_parse_impact_response_mixed_case_impact() {
        let response = "IMPACT: Minor\nSUMMARY: Case test";
        let (impact, summary) = parse_impact_response(response);

        assert_eq!(
            impact, "minor",
            "Impact classification should be case-insensitive"
        );
        assert_eq!(summary, "Case test");
    }

    #[test]
    fn test_parse_impact_response_minor_with_qualifier() {
        // The code checks starts_with("minor"), so "minor change" should still be minor
        let response = "IMPACT: minor change detected\nSUMMARY: Qualified minor";
        let (impact, _summary) = parse_impact_response(response);

        assert_eq!(
            impact, "minor",
            "Impact starting with 'minor' should classify as minor"
        );
    }

    #[test]
    fn test_parse_impact_response_unknown_impact_defaults_significant() {
        let response = "IMPACT: moderate\nSUMMARY: Unknown impact level";
        let (impact, _summary) = parse_impact_response(response);

        assert_eq!(
            impact, "significant",
            "Unknown impact values should default to significant"
        );
    }

    #[test]
    fn test_parse_impact_response_reversed_order() {
        // SUMMARY before IMPACT -- both should still be parsed
        let response = "SUMMARY: Reversed order test\nIMPACT: minor";
        let (impact, summary) = parse_impact_response(response);

        assert_eq!(impact, "minor");
        assert_eq!(summary, "Reversed order test");
    }

    // =========================================================================
    // needs_documentation_scan Tests
    // =========================================================================

    #[test]
    fn test_needs_documentation_scan_never_scanned() {
        let conn = crate::db::test_support::setup_test_connection();
        let project_id =
            crate::db::get_or_create_project_sync(&conn, "/tmp/doc-test", Some("test"))
                .expect("create project")
                .0;

        let result = needs_documentation_scan(&conn, project_id, "/tmp/doc-test");
        assert!(result.is_ok());
        assert!(
            result.unwrap(),
            "Never-scanned project should need documentation scan"
        );
    }

    #[test]
    fn test_needs_documentation_scan_recently_scanned_same_commit() {
        let conn = crate::db::test_support::setup_test_connection();
        let project_id =
            crate::db::get_or_create_project_sync(&conn, "/tmp/doc-test2", Some("test"))
                .expect("create project")
                .0;

        // Simulate a recent scan by storing an observation with current timestamp
        // We use a fake commit hash -- since get_git_head on a non-git path returns None,
        // and the stored commit is not None, it will compare Some("fakehash") != None.
        // But the key test is: when the project has been scanned, the function does not
        // unconditionally return true.
        store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(project_id),
                key: Some(DOC_SCAN_MARKER_KEY),
                content: "fakehash1234567890abcdef1234567890abcdef",
                observation_type: "system",
                category: Some("documentation"),
                confidence: 1.0,
                source: "documentation",
                session_id: None,
                team_id: None,
                scope: "project",
                expires_at: None,
            },
        )
        .expect("store observation");

        // The scan_info now exists, so last_commit is Some.
        // get_git_head("/tmp/doc-test2") returns None (no git repo).
        // Case 2 requires both last and current to be Some, so it falls through.
        // Case 3 checks if scan_time is older than 24 hours -- it was just created, so false.
        // Result: Ok(false) -- no scan needed.
        let result = needs_documentation_scan(&conn, project_id, "/tmp/doc-test2");
        assert!(result.is_ok());
        assert!(
            !result.unwrap(),
            "Recently scanned project with no git changes should not need scan"
        );
    }

    #[test]
    fn test_needs_documentation_scan_periodic_refresh_after_24h() {
        let conn = crate::db::test_support::setup_test_connection();
        let project_id =
            crate::db::get_or_create_project_sync(&conn, "/tmp/doc-test3", Some("test"))
                .expect("create project")
                .0;

        // Store a scan marker using the proper API, then backdate it
        store_observation_sync(
            &conn,
            StoreObservationParams {
                project_id: Some(project_id),
                key: Some(DOC_SCAN_MARKER_KEY),
                content: "fakehash",
                observation_type: "system",
                category: Some("documentation"),
                confidence: 1.0,
                source: "documentation",
                session_id: None,
                team_id: None,
                scope: "project",
                expires_at: None,
            },
        )
        .expect("store observation");

        // Backdate the timestamps so the scan looks old (>24 hours ago)
        conn.execute(
            "UPDATE system_observations SET created_at = datetime('now', '-48 hours'), \
             updated_at = datetime('now', '-48 hours') \
             WHERE project_id = ?1 AND key = ?2",
            rusqlite::params![project_id, DOC_SCAN_MARKER_KEY],
        )
        .expect("backdate scan marker");

        let result = needs_documentation_scan(&conn, project_id, "/tmp/doc-test3");
        assert!(result.is_ok());
        assert!(
            result.unwrap(),
            "Project not scanned in >24 hours should need periodic refresh"
        );
    }
}
