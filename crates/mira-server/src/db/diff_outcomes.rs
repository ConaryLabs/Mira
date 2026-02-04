// crates/mira-server/src/db/diff_outcomes.rs
// Database operations for diff outcome tracking (change intelligence)

use rusqlite::{Connection, params};

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
pub fn store_diff_outcome_sync(
    conn: &Connection,
    diff_analysis_id: i64,
    project_id: Option<i64>,
    outcome_type: &str,
    evidence_commit: Option<&str>,
    evidence_message: Option<&str>,
    time_to_outcome_seconds: Option<i64>,
    detected_by: &str,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO diff_outcomes (
            diff_analysis_id, project_id, outcome_type,
            evidence_commit, evidence_message,
            time_to_outcome_seconds, detected_by
        ) VALUES (?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(diff_analysis_id, outcome_type, evidence_commit) DO NOTHING",
        params![
            diff_analysis_id,
            project_id,
            outcome_type,
            evidence_commit,
            evidence_message,
            time_to_outcome_seconds,
            detected_by,
        ],
    )?;
    Ok(conn.last_insert_rowid())
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
    match outcome_type {
        Some(otype) => {
            let sql = "SELECT id, diff_analysis_id, project_id, outcome_type,
                              evidence_commit, evidence_message,
                              time_to_outcome_seconds, detected_by, created_at
                       FROM diff_outcomes
                       WHERE project_id = ? AND outcome_type = ?
                       ORDER BY created_at DESC
                       LIMIT ?";
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map(
                params![project_id, otype, limit as i64],
                parse_diff_outcome_row,
            )?;
            rows.collect()
        }
        None => {
            let sql = "SELECT id, diff_analysis_id, project_id, outcome_type,
                              evidence_commit, evidence_message,
                              time_to_outcome_seconds, detected_by, created_at
                       FROM diff_outcomes
                       WHERE project_id = ?
                       ORDER BY created_at DESC
                       LIMIT ?";
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map(params![project_id, limit as i64], parse_diff_outcome_row)?;
            rows.collect()
        }
    }
}

/// Mark aged diff analyses as having clean outcomes (no issues detected).
/// Only marks diffs that have no existing outcomes and are older than `age_days`.
pub fn mark_clean_outcomes_sync(
    conn: &Connection,
    project_id: i64,
    age_days: i64,
) -> rusqlite::Result<usize> {
    let sql = "INSERT INTO diff_outcomes (diff_analysis_id, project_id, outcome_type, detected_by)
               SELECT da.id, da.project_id, 'clean', 'aging'
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
) -> rusqlite::Result<Vec<(i64, String, String, Option<String>)>> {
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
