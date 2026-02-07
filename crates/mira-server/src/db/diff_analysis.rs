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
