// crates/mira-server/src/db/diff_analysis.rs
// Database operations for semantic diff analysis

use rusqlite::{Connection, params};

/// A stored diff analysis result
#[derive(Debug, Clone)]
pub struct DiffAnalysis {
    pub id: i64,
    pub project_id: Option<i64>,
    pub from_commit: String,
    pub to_commit: String,
    pub analysis_type: String,
    pub changes_json: Option<String>,
    pub impact_json: Option<String>,
    pub risk_json: Option<String>,
    pub summary: Option<String>,
    pub files_changed: Option<i64>,
    pub lines_added: Option<i64>,
    pub lines_removed: Option<i64>,
    pub status: String,
    pub created_at: String,
    pub files_json: Option<String>,
}

/// Parse DiffAnalysis from a rusqlite Row
pub fn parse_diff_analysis_row(row: &rusqlite::Row) -> rusqlite::Result<DiffAnalysis> {
    Ok(DiffAnalysis {
        id: row.get(0)?,
        project_id: row.get(1)?,
        from_commit: row.get(2)?,
        to_commit: row.get(3)?,
        analysis_type: row.get(4)?,
        changes_json: row.get(5)?,
        impact_json: row.get(6)?,
        risk_json: row.get(7)?,
        summary: row.get(8)?,
        files_changed: row.get(9)?,
        lines_added: row.get(10)?,
        lines_removed: row.get(11)?,
        status: row.get(12)?,
        created_at: row.get(13)?,
        files_json: row.get(14)?,
    })
}

// ============================================================================
// Sync functions for pool.interact() usage
// ============================================================================

/// Parameters for storing a diff analysis
pub struct StoreDiffAnalysisParams<'a> {
    pub project_id: Option<i64>,
    pub from_commit: &'a str,
    pub to_commit: &'a str,
    pub analysis_type: &'a str,
    pub changes_json: Option<&'a str>,
    pub impact_json: Option<&'a str>,
    pub risk_json: Option<&'a str>,
    pub summary: Option<&'a str>,
    pub files_changed: Option<i64>,
    pub lines_added: Option<i64>,
    pub lines_removed: Option<i64>,
    pub files_json: Option<&'a str>,
}

/// Store a new diff analysis (sync version for pool.interact())
pub fn store_diff_analysis_sync(
    conn: &Connection,
    p: &StoreDiffAnalysisParams,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO diff_analyses (
            project_id, from_commit, to_commit, analysis_type,
            changes_json, impact_json, risk_json, summary,
            files_changed, lines_added, lines_removed, files_json
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            p.project_id,
            p.from_commit,
            p.to_commit,
            p.analysis_type,
            p.changes_json,
            p.impact_json,
            p.risk_json,
            p.summary,
            p.files_changed,
            p.lines_added,
            p.lines_removed,
            p.files_json,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Get a cached diff analysis if it exists (sync version for pool.interact())
pub fn get_cached_diff_analysis_sync(
    conn: &Connection,
    project_id: Option<i64>,
    from_commit: &str,
    to_commit: &str,
) -> rusqlite::Result<Option<DiffAnalysis>> {
    let sql = "SELECT id, project_id, from_commit, to_commit, analysis_type,
                      changes_json, impact_json, risk_json, summary,
                      files_changed, lines_added, lines_removed, status, created_at,
                      files_json
               FROM diff_analyses
               WHERE (project_id = ? OR (project_id IS NULL AND ? IS NULL))
                     AND from_commit = ? AND to_commit = ?
               ORDER BY created_at DESC
               LIMIT 1";
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query_map(
        params![project_id, project_id, from_commit, to_commit],
        parse_diff_analysis_row,
    )?;
    match rows.next() {
        Some(Ok(analysis)) => Ok(Some(analysis)),
        Some(Err(e)) => Err(e),
        None => Ok(None),
    }
}

/// Get recent diff analyses for a project (sync version for pool.interact())
///
/// Uses UNION ALL instead of OR for index-friendly project scoping.
pub fn get_recent_diff_analyses_sync(
    conn: &Connection,
    project_id: Option<i64>,
    limit: usize,
) -> rusqlite::Result<Vec<DiffAnalysis>> {
    let cols = "id, project_id, from_commit, to_commit, analysis_type, \
                changes_json, impact_json, risk_json, summary, \
                files_changed, lines_added, lines_removed, status, created_at, \
                files_json";
    let lim = limit as i64;
    match project_id {
        Some(pid) => {
            let sql = format!(
                "SELECT {cols} FROM diff_analyses WHERE project_id = ?1 \
                 UNION ALL \
                 SELECT {cols} FROM diff_analyses WHERE project_id IS NULL \
                 ORDER BY created_at DESC LIMIT ?2"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![pid, lim], parse_diff_analysis_row)?;
            rows.collect()
        }
        None => {
            let sql = format!(
                "SELECT {cols} FROM diff_analyses WHERE project_id IS NULL \
                 ORDER BY created_at DESC LIMIT ?1"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![lim], parse_diff_analysis_row)?;
            rows.collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::setup_test_connection;

    fn setup_conn_with_project() -> (Connection, i64) {
        let conn = setup_test_connection();
        let (pid, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/path", Some("test")).unwrap();
        (conn, pid)
    }

    fn make_params(project_id: Option<i64>) -> StoreDiffAnalysisParams<'static> {
        StoreDiffAnalysisParams {
            project_id,
            from_commit: "aaaa1111aaaa1111aaaa1111aaaa1111aaaa1111",
            to_commit: "bbbb2222bbbb2222bbbb2222bbbb2222bbbb2222",
            analysis_type: "semantic",
            changes_json: Some(r#"[{"file":"src/main.rs"}]"#),
            impact_json: Some(r#"{"risk":"low"}"#),
            risk_json: Some(r#"{"score":2}"#),
            summary: Some("Minor refactoring"),
            files_changed: Some(3),
            lines_added: Some(25),
            lines_removed: Some(10),
            files_json: Some(r#"["src/main.rs","src/lib.rs"]"#),
        }
    }

    #[test]
    fn test_store_diff_analysis_returns_id() {
        let (conn, pid) = setup_conn_with_project();
        let params = make_params(Some(pid));

        let id = store_diff_analysis_sync(&conn, &params).unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_get_cached_diff_analysis_found() {
        let (conn, pid) = setup_conn_with_project();
        let params = make_params(Some(pid));
        store_diff_analysis_sync(&conn, &params).unwrap();

        let cached = get_cached_diff_analysis_sync(
            &conn,
            Some(pid),
            "aaaa1111aaaa1111aaaa1111aaaa1111aaaa1111",
            "bbbb2222bbbb2222bbbb2222bbbb2222bbbb2222",
        )
        .unwrap();

        assert!(cached.is_some());
        let analysis = cached.unwrap();
        assert_eq!(
            analysis.from_commit,
            "aaaa1111aaaa1111aaaa1111aaaa1111aaaa1111"
        );
        assert_eq!(
            analysis.to_commit,
            "bbbb2222bbbb2222bbbb2222bbbb2222bbbb2222"
        );
        assert_eq!(analysis.analysis_type, "semantic");
        assert_eq!(analysis.summary.as_deref(), Some("Minor refactoring"));
        assert_eq!(analysis.files_changed, Some(3));
        assert_eq!(analysis.lines_added, Some(25));
        assert_eq!(analysis.lines_removed, Some(10));
    }

    #[test]
    fn test_get_cached_diff_analysis_not_found() {
        let (conn, pid) = setup_conn_with_project();

        let cached =
            get_cached_diff_analysis_sync(&conn, Some(pid), "nonexistent_from", "nonexistent_to")
                .unwrap();

        assert!(cached.is_none());
    }

    #[test]
    fn test_get_recent_diff_analyses_with_project() {
        let (conn, pid) = setup_conn_with_project();

        store_diff_analysis_sync(&conn, &make_params(Some(pid))).unwrap();

        let mut p2 = make_params(Some(pid));
        p2.to_commit = "cccc3333cccc3333cccc3333cccc3333cccc3333";
        p2.summary = Some("Second analysis");
        store_diff_analysis_sync(&conn, &p2).unwrap();

        let recent = get_recent_diff_analyses_sync(&conn, Some(pid), 10).unwrap();
        assert_eq!(recent.len(), 2);
    }

    #[test]
    fn test_get_recent_diff_analyses_respects_limit() {
        let (conn, pid) = setup_conn_with_project();

        let commits: Vec<String> = (0..5).map(|i| format!("{:0>40}", i)).collect();
        for commit in &commits {
            let mut p = make_params(Some(pid));
            p.to_commit = commit.as_str();
            store_diff_analysis_sync(&conn, &p).unwrap();
        }

        let recent = get_recent_diff_analyses_sync(&conn, Some(pid), 2).unwrap();
        assert_eq!(recent.len(), 2);
    }

    #[test]
    fn test_get_recent_diff_analyses_no_project_filter() {
        let conn = setup_test_connection();

        store_diff_analysis_sync(&conn, &make_params(None)).unwrap();

        let recent = get_recent_diff_analyses_sync(&conn, None, 10).unwrap();
        assert_eq!(recent.len(), 1);
        assert!(recent[0].project_id.is_none());
    }

    #[test]
    fn test_store_diff_analysis_all_optional_fields_none() {
        let (conn, pid) = setup_conn_with_project();
        let params = StoreDiffAnalysisParams {
            project_id: Some(pid),
            from_commit: "aaaa1111aaaa1111aaaa1111aaaa1111aaaa1111",
            to_commit: "bbbb2222bbbb2222bbbb2222bbbb2222bbbb2222",
            analysis_type: "quick",
            changes_json: None,
            impact_json: None,
            risk_json: None,
            summary: None,
            files_changed: None,
            lines_added: None,
            lines_removed: None,
            files_json: None,
        };

        let id = store_diff_analysis_sync(&conn, &params).unwrap();
        let cached = get_cached_diff_analysis_sync(
            &conn,
            Some(pid),
            "aaaa1111aaaa1111aaaa1111aaaa1111aaaa1111",
            "bbbb2222bbbb2222bbbb2222bbbb2222bbbb2222",
        )
        .unwrap()
        .unwrap();

        assert_eq!(cached.id, id);
        assert!(cached.changes_json.is_none());
        assert!(cached.summary.is_none());
        assert!(cached.files_changed.is_none());
    }
}
