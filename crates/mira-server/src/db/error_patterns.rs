// crates/mira-server/src/db/error_patterns.rs
// Error pattern storage for cross-session error learning

use rusqlite::{Connection, params};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Parameters for storing/upserting an error pattern
pub struct StoreErrorPatternParams<'a> {
    pub project_id: i64,
    pub tool_name: &'a str,
    pub error_fingerprint: &'a str,
    pub error_template: &'a str,
    pub raw_error_sample: &'a str,
    pub session_id: &'a str,
}

/// A resolved error pattern with fix information
pub struct ResolvedErrorPattern {
    pub id: i64,
    pub tool_name: String,
    pub error_template: String,
    pub fix_description: String,
    pub occurrence_count: i64,
}

/// Store or update an error pattern (UPSERT on fingerprint).
/// Increments occurrence_count, updates last_seen_session_id.
pub fn store_error_pattern_sync(
    conn: &Connection,
    params: StoreErrorPatternParams,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO error_patterns (project_id, tool_name, error_fingerprint, error_template, raw_error_sample, first_seen_session_id, last_seen_session_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
         ON CONFLICT(project_id, tool_name, error_fingerprint) DO UPDATE SET
            occurrence_count = occurrence_count + 1,
            last_seen_session_id = ?6,
            raw_error_sample = ?5,
            updated_at = CURRENT_TIMESTAMP",
        params![
            params.project_id,
            params.tool_name,
            params.error_fingerprint,
            params.error_template,
            params.raw_error_sample,
            params.session_id,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Look up resolved error patterns matching a fingerprint.
/// Returns patterns that have a fix_description (were resolved in a past session).
pub fn lookup_resolved_pattern_sync(
    conn: &Connection,
    project_id: i64,
    tool_name: &str,
    error_fingerprint: &str,
) -> Option<ResolvedErrorPattern> {
    conn.query_row(
        "SELECT id, tool_name, error_template, fix_description, occurrence_count
         FROM error_patterns
         WHERE project_id = ?1
           AND tool_name = ?2
           AND error_fingerprint = ?3
           AND fix_description IS NOT NULL
           AND resolved_at IS NOT NULL",
        params![project_id, tool_name, error_fingerprint],
        |row| {
            Ok(ResolvedErrorPattern {
                id: row.get(0)?,
                tool_name: row.get(1)?,
                error_template: row.get(2)?,
                fix_description: row.get(3)?,
                occurrence_count: row.get(4)?,
            })
        },
    )
    .ok()
}

/// Mark an error pattern as resolved.
/// Called when the same tool succeeds after repeated failures.
pub fn resolve_error_pattern_sync(
    conn: &Connection,
    project_id: i64,
    tool_name: &str,
    error_fingerprint: &str,
    fix_session_id: &str,
    fix_description: &str,
) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE error_patterns
         SET fix_description = ?1,
             fix_session_id = ?2,
             resolved_at = CURRENT_TIMESTAMP,
             updated_at = CURRENT_TIMESTAMP
         WHERE project_id = ?3
           AND tool_name = ?4
           AND error_fingerprint = ?5
           AND resolved_at IS NULL",
        params![
            fix_description,
            fix_session_id,
            project_id,
            tool_name,
            error_fingerprint
        ],
    )
}

/// Look up the most recent unresolved error pattern for a tool in a project.
///
/// Only returns a pattern if it has `occurrence_count >= 3`, ensuring we don't
/// auto-resolve one-off errors. Returns at most one pattern (the most recently
/// updated) to avoid incorrectly resolving unrelated fingerprints when a tool
/// has multiple distinct failure modes.
pub fn get_unresolved_patterns_for_tool_sync(
    conn: &Connection,
    project_id: i64,
    tool_name: &str,
    session_id: &str,
) -> Vec<(i64, String)> {
    let mut stmt = match conn.prepare(
        "SELECT id, error_fingerprint FROM error_patterns
         WHERE project_id = ?1
           AND tool_name = ?2
           AND last_seen_session_id = ?3
           AND resolved_at IS NULL
           AND occurrence_count >= 3
         ORDER BY updated_at DESC
         LIMIT 1",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    stmt.query_map(params![project_id, tool_name, session_id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

/// Generate a fingerprint for an error message by normalizing dynamic content.
///
/// Strips: absolute paths, line:col numbers, UUIDs/hex hashes, long quoted strings, timestamps.
/// Then hashes the normalized form for O(1) lookup.
///
/// Returns (fingerprint_hash, normalized_template)
pub fn error_fingerprint(tool_name: &str, raw_error: &str) -> (String, String) {
    use regex::Regex;
    use std::sync::LazyLock;

    #[allow(clippy::expect_used)]
    static RE_PATH: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(/[\w./-]+)+\.\w+").expect("valid regex"));
    #[allow(clippy::expect_used)]
    static RE_LINE_COL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r":\d+:\d+").expect("valid regex"));
    #[allow(clippy::expect_used)]
    static RE_NUMBERS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\b\d{2,}\b").expect("valid regex"));
    #[allow(clippy::expect_used)]
    static RE_HEX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[0-9a-f]{8,}").expect("valid regex"));
    #[allow(clippy::expect_used)]
    static RE_DQUOTE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#""[^"]{20,}""#).expect("valid regex"));
    #[allow(clippy::expect_used)]
    static RE_BTICK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"`[^`]{20,}`").expect("valid regex"));

    let normalized = raw_error.to_lowercase();
    let normalized = RE_PATH.replace_all(&normalized, "<PATH>");
    let normalized = RE_LINE_COL.replace_all(&normalized, ":<N>:<N>");
    let normalized = RE_NUMBERS.replace_all(&normalized, "<N>");
    let normalized = RE_HEX.replace_all(&normalized, "<ID>");
    let normalized = RE_DQUOTE.replace_all(&normalized, "<STR>");
    let normalized = RE_BTICK.replace_all(&normalized, "<STR>");

    let template = normalized.trim().to_string();

    let mut hasher = DefaultHasher::new();
    format!("{}:{}", tool_name, &template).hash(&mut hasher);
    let hash = format!("{:016x}", hasher.finish());

    (hash, template)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS projects (
                id INTEGER PRIMARY KEY,
                path TEXT UNIQUE NOT NULL,
                name TEXT,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            );
            INSERT INTO projects (id, path, name) VALUES (1, '/test', 'test');
            CREATE TABLE IF NOT EXISTS error_patterns (
                id INTEGER PRIMARY KEY,
                project_id INTEGER NOT NULL REFERENCES projects(id),
                tool_name TEXT NOT NULL,
                error_fingerprint TEXT NOT NULL,
                error_template TEXT NOT NULL,
                raw_error_sample TEXT,
                fix_description TEXT,
                fix_session_id TEXT,
                occurrence_count INTEGER DEFAULT 1,
                first_seen_session_id TEXT,
                last_seen_session_id TEXT,
                resolved_at TEXT,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(project_id, tool_name, error_fingerprint)
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_error_fingerprint_normalizes_paths() {
        let (fp1, _) = error_fingerprint("Bash", "error at /home/user/project/src/main.rs:10:5");
        let (fp2, _) = error_fingerprint("Bash", "error at /tmp/other/path/lib.rs:42:12");
        assert_eq!(
            fp1, fp2,
            "different paths should produce the same fingerprint"
        );
    }

    #[test]
    fn test_error_fingerprint_normalizes_hex() {
        let (fp1, _) = error_fingerprint("Bash", "commit abc12345def was bad");
        let (fp2, _) = error_fingerprint("Bash", "commit 99887766aa was bad");
        assert_eq!(
            fp1, fp2,
            "different hex IDs should produce the same fingerprint"
        );
    }

    #[test]
    fn test_error_fingerprint_different_errors_differ() {
        let (fp1, _) = error_fingerprint("Bash", "connection refused");
        let (fp2, _) = error_fingerprint("Bash", "permission denied");
        assert_ne!(
            fp1, fp2,
            "different errors should produce different fingerprints"
        );
    }

    #[test]
    fn test_error_fingerprint_different_tools_differ() {
        let (fp1, _) = error_fingerprint("Bash", "error: not found");
        let (fp2, _) = error_fingerprint("Read", "error: not found");
        assert_ne!(fp1, fp2, "same error for different tools should differ");
    }

    #[test]
    fn test_store_error_pattern_upsert_increments_count() {
        let conn = setup_test_db();
        let (fp, tmpl) = error_fingerprint("Bash", "error: something failed");

        // First insert
        store_error_pattern_sync(
            &conn,
            StoreErrorPatternParams {
                project_id: 1,
                tool_name: "Bash",
                error_fingerprint: &fp,
                error_template: &tmpl,
                raw_error_sample: "error: something failed",
                session_id: "s1",
            },
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT occurrence_count FROM error_patterns WHERE error_fingerprint = ?1",
                params![fp],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Second insert (UPSERT)
        store_error_pattern_sync(
            &conn,
            StoreErrorPatternParams {
                project_id: 1,
                tool_name: "Bash",
                error_fingerprint: &fp,
                error_template: &tmpl,
                raw_error_sample: "error: something failed again",
                session_id: "s2",
            },
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT occurrence_count FROM error_patterns WHERE error_fingerprint = ?1",
                params![fp],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);

        // Verify last_seen_session_id was updated
        let last_session: String = conn
            .query_row(
                "SELECT last_seen_session_id FROM error_patterns WHERE error_fingerprint = ?1",
                params![fp],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(last_session, "s2");
    }

    #[test]
    fn test_lookup_resolved_pattern_only_returns_resolved() {
        let conn = setup_test_db();
        let (fp, tmpl) = error_fingerprint("Bash", "error: test");

        // Store an unresolved pattern
        store_error_pattern_sync(
            &conn,
            StoreErrorPatternParams {
                project_id: 1,
                tool_name: "Bash",
                error_fingerprint: &fp,
                error_template: &tmpl,
                raw_error_sample: "error: test",
                session_id: "s1",
            },
        )
        .unwrap();

        // Should not find it (unresolved)
        assert!(lookup_resolved_pattern_sync(&conn, 1, "Bash", &fp).is_none());

        // Resolve it
        resolve_error_pattern_sync(&conn, 1, "Bash", &fp, "s2", "Fixed by doing X").unwrap();

        // Now should find it
        let resolved = lookup_resolved_pattern_sync(&conn, 1, "Bash", &fp);
        assert!(resolved.is_some());
        let resolved = resolved.unwrap();
        assert_eq!(resolved.fix_description, "Fixed by doing X");
        assert_eq!(resolved.occurrence_count, 1);
    }

    #[test]
    fn test_resolve_error_pattern_marks_resolved() {
        let conn = setup_test_db();
        let (fp, tmpl) = error_fingerprint("Bash", "error: resolve me");

        store_error_pattern_sync(
            &conn,
            StoreErrorPatternParams {
                project_id: 1,
                tool_name: "Bash",
                error_fingerprint: &fp,
                error_template: &tmpl,
                raw_error_sample: "error: resolve me",
                session_id: "s1",
            },
        )
        .unwrap();

        let updated =
            resolve_error_pattern_sync(&conn, 1, "Bash", &fp, "s2", "Applied fix Y").unwrap();
        assert_eq!(updated, 1);

        // Resolving again should be a no-op (already resolved)
        let updated_again =
            resolve_error_pattern_sync(&conn, 1, "Bash", &fp, "s3", "Another fix").unwrap();
        assert_eq!(updated_again, 0);
    }

    #[test]
    fn test_get_unresolved_patterns_requires_3_occurrences() {
        let conn = setup_test_db();
        let (fp1, tmpl1) = error_fingerprint("Bash", "error: alpha");

        // Store pattern once — should NOT be eligible for auto-resolution
        store_error_pattern_sync(
            &conn,
            StoreErrorPatternParams {
                project_id: 1,
                tool_name: "Bash",
                error_fingerprint: &fp1,
                error_template: &tmpl1,
                raw_error_sample: "error: alpha",
                session_id: "s1",
            },
        )
        .unwrap();

        let unresolved = get_unresolved_patterns_for_tool_sync(&conn, 1, "Bash", "s1");
        assert_eq!(unresolved.len(), 0, "1 occurrence should not be eligible");

        // Bump to 3 occurrences via UPSERT
        store_error_pattern_sync(
            &conn,
            StoreErrorPatternParams {
                project_id: 1,
                tool_name: "Bash",
                error_fingerprint: &fp1,
                error_template: &tmpl1,
                raw_error_sample: "error: alpha",
                session_id: "s1",
            },
        )
        .unwrap();
        store_error_pattern_sync(
            &conn,
            StoreErrorPatternParams {
                project_id: 1,
                tool_name: "Bash",
                error_fingerprint: &fp1,
                error_template: &tmpl1,
                raw_error_sample: "error: alpha",
                session_id: "s1",
            },
        )
        .unwrap();

        let unresolved = get_unresolved_patterns_for_tool_sync(&conn, 1, "Bash", "s1");
        assert_eq!(unresolved.len(), 1, "3 occurrences should be eligible");
    }

    #[test]
    fn test_get_unresolved_patterns_returns_at_most_one() {
        let conn = setup_test_db();
        let (fp1, tmpl1) = error_fingerprint("Bash", "error: alpha");
        let (fp2, tmpl2) = error_fingerprint("Bash", "error: beta");

        // Store two patterns, each with 3+ occurrences
        for _ in 0..3 {
            store_error_pattern_sync(
                &conn,
                StoreErrorPatternParams {
                    project_id: 1,
                    tool_name: "Bash",
                    error_fingerprint: &fp1,
                    error_template: &tmpl1,
                    raw_error_sample: "error: alpha",
                    session_id: "s1",
                },
            )
            .unwrap();
            store_error_pattern_sync(
                &conn,
                StoreErrorPatternParams {
                    project_id: 1,
                    tool_name: "Bash",
                    error_fingerprint: &fp2,
                    error_template: &tmpl2,
                    raw_error_sample: "error: beta",
                    session_id: "s1",
                },
            )
            .unwrap();
        }

        // Should return at most 1 (most recently updated) to avoid
        // resolving unrelated patterns on a single success
        let unresolved = get_unresolved_patterns_for_tool_sync(&conn, 1, "Bash", "s1");
        assert_eq!(
            unresolved.len(),
            1,
            "should return at most 1 pattern to avoid resolving unrelated errors"
        );
    }
}
