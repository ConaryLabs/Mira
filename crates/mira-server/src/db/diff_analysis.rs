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
    })
}

// ============================================================================
// Sync functions for pool.interact() usage
// ============================================================================

/// Store a new diff analysis (sync version for pool.interact())
pub fn store_diff_analysis_sync(
    conn: &Connection,
    project_id: Option<i64>,
    from_commit: &str,
    to_commit: &str,
    analysis_type: &str,
    changes_json: Option<&str>,
    impact_json: Option<&str>,
    risk_json: Option<&str>,
    summary: Option<&str>,
    files_changed: Option<i64>,
    lines_added: Option<i64>,
    lines_removed: Option<i64>,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO diff_analyses (
            project_id, from_commit, to_commit, analysis_type,
            changes_json, impact_json, risk_json, summary,
            files_changed, lines_added, lines_removed
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            project_id,
            from_commit,
            to_commit,
            analysis_type,
            changes_json,
            impact_json,
            risk_json,
            summary,
            files_changed,
            lines_added,
            lines_removed
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
                      files_changed, lines_added, lines_removed, status, created_at
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
pub fn get_recent_diff_analyses_sync(
    conn: &Connection,
    project_id: Option<i64>,
    limit: usize,
) -> rusqlite::Result<Vec<DiffAnalysis>> {
    let sql = "SELECT id, project_id, from_commit, to_commit, analysis_type,
                      changes_json, impact_json, risk_json, summary,
                      files_changed, lines_added, lines_removed, status, created_at
               FROM diff_analyses
               WHERE project_id = ? OR project_id IS NULL
               ORDER BY created_at DESC
               LIMIT ?";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![project_id, limit as i64], parse_diff_analysis_row)?;
    rows.collect()
}

