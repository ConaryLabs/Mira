// crates/mira-server/src/db/diff_outcomes.rs
// Database operations for diff outcome tracking (change intelligence)

use rusqlite::{Connection, params};

/// (id, to_commit, from_commit, files_json)
type UnscannedDiffRow = (i64, String, String, Option<String>);

/// A stored diff outcome
#[derive(Debug, Clone)]
pub struct DiffOutcome {
    pub id: i64,
    pub diff_analysis_id: i64,
    pub project_id: Option<i64>,
    pub outcome_type: String,
    pub evidence_commit: Option<String>,
    pub evidence_message: Option<String>,
    pub time_to_outcome_seconds: Option<i64>,
    pub detected_by: String,
    pub created_at: String,
}

fn parse_diff_outcome_row(row: &rusqlite::Row) -> rusqlite::Result<DiffOutcome> {
    Ok(DiffOutcome {
        id: row.get(0)?,
        diff_analysis_id: row.get(1)?,
        project_id: row.get(2)?,
        outcome_type: row.get(3)?,
        evidence_commit: row.get(4)?,
        evidence_message: row.get(5)?,
        time_to_outcome_seconds: row.get(6)?,
        detected_by: row.get(7)?,
        created_at: row.get(8)?,
    })
}

// ============================================================================
// Sync functions for pool.interact() usage
// ============================================================================

/// Store a new diff outcome (UPSERT — ignores duplicates)
/// Parameters for storing a diff outcome
pub struct StoreDiffOutcomeParams<'a> {
    pub diff_analysis_id: i64,
    pub project_id: Option<i64>,
    pub outcome_type: &'a str,
    pub evidence_commit: Option<&'a str>,
    pub evidence_message: Option<&'a str>,
    pub time_to_outcome_seconds: Option<i64>,
    pub detected_by: &'a str,
}

pub fn store_diff_outcome_sync(
    conn: &Connection,
    p: &StoreDiffOutcomeParams,
) -> rusqlite::Result<Option<i64>> {
    conn.execute(
        "INSERT INTO diff_outcomes (
            diff_analysis_id, project_id, outcome_type,
            evidence_commit, evidence_message,
            time_to_outcome_seconds, detected_by
        ) VALUES (?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(diff_analysis_id, outcome_type, evidence_commit) DO NOTHING",
        params![
            p.diff_analysis_id,
            p.project_id,
            p.outcome_type,
            p.evidence_commit,
            p.evidence_message,
            p.time_to_outcome_seconds,
            p.detected_by,
        ],
    )?;
    if conn.changes() > 0 {
        Ok(Some(conn.last_insert_rowid()))
    } else {
        Ok(None)
    }
}

/// Get all outcomes for a specific diff analysis
pub fn get_outcomes_for_diff_sync(
    conn: &Connection,
    diff_analysis_id: i64,
) -> rusqlite::Result<Vec<DiffOutcome>> {
    let sql = "SELECT id, diff_analysis_id, project_id, outcome_type,
                      evidence_commit, evidence_message,
                      time_to_outcome_seconds, detected_by, created_at
               FROM diff_outcomes
               WHERE diff_analysis_id = ?
               ORDER BY created_at DESC";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![diff_analysis_id], parse_diff_outcome_row)?;
    rows.collect()
}

/// Get outcomes by project, optionally filtered by outcome type
pub fn get_outcomes_by_project_sync(
    conn: &Connection,
    project_id: i64,
    outcome_type: Option<&str>,
    limit: usize,
) -> rusqlite::Result<Vec<DiffOutcome>> {
    let sql = "SELECT id, diff_analysis_id, project_id, outcome_type,
                      evidence_commit, evidence_message,
                      time_to_outcome_seconds, detected_by, created_at
               FROM diff_outcomes
               WHERE project_id = ?1 AND (?2 IS NULL OR outcome_type = ?2)
               ORDER BY created_at DESC
               LIMIT ?3";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(
        params![project_id, outcome_type, limit as i64],
        parse_diff_outcome_row,
    )?;
    rows.collect()
}

/// Mark aged diff analyses as having clean outcomes (no issues detected).
/// Only marks diffs that have no existing outcomes and are older than `age_days`.
pub fn mark_clean_outcomes_sync(
    conn: &Connection,
    project_id: i64,
    age_days: i64,
) -> rusqlite::Result<usize> {
    debug_assert!(age_days > 0, "age_days must be positive");
    let sql = "INSERT INTO diff_outcomes (diff_analysis_id, project_id, outcome_type, evidence_commit, detected_by)
               SELECT da.id, da.project_id, 'clean', '', 'aging'
               FROM diff_analyses da
               WHERE da.project_id = ?
                 AND da.created_at < datetime('now', ? || ' days')
                 AND length(da.to_commit) = 40
                 AND NOT EXISTS (
                     SELECT 1 FROM diff_outcomes do2
                     WHERE do2.diff_analysis_id = da.id
                 )
               ON CONFLICT(diff_analysis_id, outcome_type, evidence_commit) DO NOTHING";
    conn.execute(sql, params![project_id, -age_days])
}

/// Get diff analyses that have no outcomes yet (candidates for scanning).
/// Only returns analyses with full 40-char commit SHAs and non-null project_id.
pub fn get_unscanned_diffs_sync(
    conn: &Connection,
    project_id: i64,
    limit: usize,
) -> rusqlite::Result<Vec<UnscannedDiffRow>> {
    let sql = "SELECT da.id, da.to_commit, da.from_commit, da.files_json
               FROM diff_analyses da
               WHERE da.project_id = ?
                 AND length(da.to_commit) = 40
                 AND length(da.from_commit) = 40
                 AND NOT EXISTS (
                     SELECT 1 FROM diff_outcomes do2
                     WHERE do2.diff_analysis_id = da.id
                 )
               ORDER BY da.created_at ASC
               LIMIT ?";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![project_id, limit as i64], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })?;
    rows.collect()
}

/// Get outcome statistics for a project (for pattern mining)
pub fn get_outcome_stats_sync(
    conn: &Connection,
    project_id: i64,
) -> rusqlite::Result<Vec<(String, i64)>> {
    let sql = "SELECT outcome_type, COUNT(*) as cnt
               FROM diff_outcomes
               WHERE project_id = ?
               GROUP BY outcome_type
               ORDER BY cnt DESC";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![project_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::setup_test_connection;

    fn setup_diff_db() -> (Connection, i64) {
        let conn = setup_test_connection();
        let (pid, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();
        (conn, pid)
    }

    /// Insert a diff_analysis row and return its id
    fn insert_diff_analysis(
        conn: &Connection,
        project_id: i64,
        to_commit: &str,
        from_commit: &str,
    ) -> i64 {
        conn.execute(
            "INSERT INTO diff_analyses (project_id, to_commit, from_commit, summary)
             VALUES (?1, ?2, ?3, 'test summary')",
            params![project_id, to_commit, from_commit],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    // ========================================================================
    // Happy-path: store, retrieve, query outcomes
    // ========================================================================

    #[test]
    fn test_store_and_get_outcomes_for_diff() {
        let (conn, pid) = setup_diff_db();
        let sha = "d".repeat(40);
        let da_id = insert_diff_analysis(&conn, pid, &sha, &sha);

        let id = store_diff_outcome_sync(
            &conn,
            &StoreDiffOutcomeParams {
                diff_analysis_id: da_id,
                project_id: Some(pid),
                outcome_type: "revert",
                evidence_commit: Some("abc123"),
                evidence_message: Some("Reverted due to regression"),
                time_to_outcome_seconds: Some(3600),
                detected_by: "git_scan",
            },
        )
        .unwrap();
        assert!(id.is_some());

        let outcomes = get_outcomes_for_diff_sync(&conn, da_id).unwrap();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].outcome_type, "revert");
        assert_eq!(outcomes[0].evidence_commit.as_deref(), Some("abc123"));
        assert_eq!(
            outcomes[0].evidence_message.as_deref(),
            Some("Reverted due to regression")
        );
        assert_eq!(outcomes[0].time_to_outcome_seconds, Some(3600));
        assert_eq!(outcomes[0].detected_by, "git_scan");
        assert_eq!(outcomes[0].project_id, Some(pid));
    }

    #[test]
    fn test_get_outcomes_by_project_all() {
        let (conn, pid) = setup_diff_db();
        let sha = "e".repeat(40);
        let da_id = insert_diff_analysis(&conn, pid, &sha, &sha);

        store_diff_outcome_sync(
            &conn,
            &StoreDiffOutcomeParams {
                diff_analysis_id: da_id,
                project_id: Some(pid),
                outcome_type: "clean",
                evidence_commit: Some(""),
                evidence_message: None,
                time_to_outcome_seconds: None,
                detected_by: "aging",
            },
        )
        .unwrap();

        store_diff_outcome_sync(
            &conn,
            &StoreDiffOutcomeParams {
                diff_analysis_id: da_id,
                project_id: Some(pid),
                outcome_type: "revert",
                evidence_commit: Some("xyz"),
                evidence_message: None,
                time_to_outcome_seconds: None,
                detected_by: "git_scan",
            },
        )
        .unwrap();

        let all = get_outcomes_by_project_sync(&conn, pid, None, 10).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_get_outcomes_by_project_filtered_by_type() {
        let (conn, pid) = setup_diff_db();
        let sha = "f".repeat(40);
        let da_id = insert_diff_analysis(&conn, pid, &sha, &sha);

        store_diff_outcome_sync(
            &conn,
            &StoreDiffOutcomeParams {
                diff_analysis_id: da_id,
                project_id: Some(pid),
                outcome_type: "clean",
                evidence_commit: Some("ev1"),
                evidence_message: None,
                time_to_outcome_seconds: None,
                detected_by: "aging",
            },
        )
        .unwrap();

        store_diff_outcome_sync(
            &conn,
            &StoreDiffOutcomeParams {
                diff_analysis_id: da_id,
                project_id: Some(pid),
                outcome_type: "revert",
                evidence_commit: Some("ev2"),
                evidence_message: None,
                time_to_outcome_seconds: None,
                detected_by: "git_scan",
            },
        )
        .unwrap();

        let clean_only = get_outcomes_by_project_sync(&conn, pid, Some("clean"), 10).unwrap();
        assert_eq!(clean_only.len(), 1);
        assert_eq!(clean_only[0].outcome_type, "clean");
    }

    #[test]
    fn test_get_outcome_stats_with_data() {
        let (conn, pid) = setup_diff_db();
        let sha = "1".repeat(40);
        let da_id = insert_diff_analysis(&conn, pid, &sha, &sha);

        // Use unique evidence_commits to avoid UPSERT DO NOTHING
        let outcomes = [("clean", "ev1"), ("clean", "ev2"), ("revert", "ev3")];
        for (outcome, ev) in &outcomes {
            store_diff_outcome_sync(
                &conn,
                &StoreDiffOutcomeParams {
                    diff_analysis_id: da_id,
                    project_id: Some(pid),
                    outcome_type: outcome,
                    evidence_commit: Some(ev),
                    evidence_message: None,
                    time_to_outcome_seconds: None,
                    detected_by: "test",
                },
            )
            .unwrap();
        }

        let stats = get_outcome_stats_sync(&conn, pid).unwrap();
        assert_eq!(stats.len(), 2);

        let clean_stat = stats.iter().find(|(t, _)| t == "clean").unwrap();
        assert_eq!(clean_stat.1, 2);

        let revert_stat = stats.iter().find(|(t, _)| t == "revert").unwrap();
        assert_eq!(revert_stat.1, 1);
    }

    #[test]
    fn test_get_unscanned_diffs_returns_full_sha_diffs() {
        let (conn, pid) = setup_diff_db();

        let from_sha = "a".repeat(40);
        let to_sha = "b".repeat(40);
        insert_diff_analysis(&conn, pid, &to_sha, &from_sha);

        let unscanned = get_unscanned_diffs_sync(&conn, pid, 10).unwrap();
        assert_eq!(unscanned.len(), 1);
        assert_eq!(unscanned[0].1, to_sha); // to_commit
        assert_eq!(unscanned[0].2, from_sha); // from_commit
    }

    // ========================================================================
    // mark_clean_outcomes_sync: no matching outcomes
    // ========================================================================

    #[test]
    fn test_mark_clean_outcomes_no_matching_diffs() {
        let (conn, pid) = setup_diff_db();

        // No diff_analyses exist at all
        let marked = mark_clean_outcomes_sync(&conn, pid, 7).expect("mark clean should succeed");
        assert_eq!(marked, 0, "no diffs to mark as clean");
    }

    #[test]
    fn test_mark_clean_outcomes_recent_diffs_not_marked() {
        let (conn, pid) = setup_diff_db();

        // Insert a recent diff with a full 40-char commit SHA
        let sha = "a".repeat(40);
        insert_diff_analysis(&conn, pid, &sha, &sha);

        // age_days=7 means only diffs older than 7 days
        let marked = mark_clean_outcomes_sync(&conn, pid, 7).expect("mark clean should succeed");
        assert_eq!(marked, 0, "recent diffs should not be marked clean");
    }

    #[test]
    fn test_mark_clean_outcomes_already_has_outcome() {
        let (conn, pid) = setup_diff_db();

        let sha = "b".repeat(40);
        let da_id = insert_diff_analysis(&conn, pid, &sha, &sha);

        // Manually backdate the diff_analysis
        conn.execute(
            "UPDATE diff_analyses SET created_at = datetime('now', '-30 days') WHERE id = ?",
            [da_id],
        )
        .unwrap();

        // Store an outcome for this diff
        store_diff_outcome_sync(
            &conn,
            &StoreDiffOutcomeParams {
                diff_analysis_id: da_id,
                project_id: Some(pid),
                outcome_type: "revert",
                evidence_commit: Some("abc"),
                evidence_message: Some("reverted"),
                time_to_outcome_seconds: None,
                detected_by: "test",
            },
        )
        .unwrap();

        // mark_clean should skip diffs that already have outcomes
        let marked = mark_clean_outcomes_sync(&conn, pid, 7).expect("mark clean should succeed");
        assert_eq!(
            marked, 0,
            "diff with existing outcome should not be double-marked"
        );
    }

    // ========================================================================
    // get_unscanned_diffs_sync: empty results
    // ========================================================================

    #[test]
    fn test_get_unscanned_diffs_empty() {
        let (conn, pid) = setup_diff_db();

        let unscanned =
            get_unscanned_diffs_sync(&conn, pid, 10).expect("get unscanned should succeed");
        assert!(unscanned.is_empty(), "no diffs means no unscanned");
    }

    #[test]
    fn test_get_unscanned_diffs_excludes_short_commits() {
        let (conn, pid) = setup_diff_db();

        // Insert a diff with short (non-full-SHA) commits
        insert_diff_analysis(&conn, pid, "short", "also_short");

        let unscanned =
            get_unscanned_diffs_sync(&conn, pid, 10).expect("get unscanned should succeed");
        assert!(
            unscanned.is_empty(),
            "diffs with short commits should be excluded"
        );
    }

    // ========================================================================
    // get_outcomes_for_diff_sync: nonexistent diff
    // ========================================================================

    #[test]
    fn test_get_outcomes_for_nonexistent_diff() {
        let (conn, _pid) = setup_diff_db();

        let outcomes =
            get_outcomes_for_diff_sync(&conn, 99999).expect("get outcomes should succeed");
        assert!(outcomes.is_empty());
    }

    // ========================================================================
    // get_outcome_stats_sync: empty
    // ========================================================================

    #[test]
    fn test_get_outcome_stats_empty() {
        let (conn, pid) = setup_diff_db();

        let stats = get_outcome_stats_sync(&conn, pid).expect("stats should succeed");
        assert!(stats.is_empty());
    }

    // ========================================================================
    // store_diff_outcome_sync: UPSERT duplicate returns None
    // ========================================================================

    #[test]
    fn test_store_diff_outcome_duplicate_returns_none() {
        let (conn, pid) = setup_diff_db();
        let sha = "c".repeat(40);
        let da_id = insert_diff_analysis(&conn, pid, &sha, &sha);

        let p = StoreDiffOutcomeParams {
            diff_analysis_id: da_id,
            project_id: Some(pid),
            outcome_type: "clean",
            evidence_commit: Some(""),
            evidence_message: None,
            time_to_outcome_seconds: None,
            detected_by: "test",
        };

        let first = store_diff_outcome_sync(&conn, &p).expect("first store should succeed");
        assert!(first.is_some(), "first insert should return Some(id)");

        let second = store_diff_outcome_sync(&conn, &p).expect("second store should succeed");
        assert!(
            second.is_none(),
            "duplicate should return None (DO NOTHING)"
        );
    }
}
