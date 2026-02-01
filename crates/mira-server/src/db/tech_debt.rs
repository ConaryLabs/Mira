// db/tech_debt.rs
// Database operations for tech debt scoring

use rusqlite::{Connection, params};

/// A tech debt score for a module
pub struct TechDebtScore {
    pub module_id: String,
    pub module_path: String,
    pub overall_score: f64,
    pub tier: String,
    pub factor_scores: String, // JSON
    pub line_count: Option<i64>,
    pub finding_count: Option<i64>,
}

/// Store or update a tech debt score
pub fn store_debt_score_sync(
    conn: &Connection,
    project_id: i64,
    score: &TechDebtScore,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO tech_debt_scores
         (project_id, module_id, module_path, overall_score, tier, factor_scores, line_count, finding_count, computed_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))
         ON CONFLICT(project_id, module_id) DO UPDATE SET
           module_path = excluded.module_path,
           overall_score = excluded.overall_score,
           tier = excluded.tier,
           factor_scores = excluded.factor_scores,
           line_count = excluded.line_count,
           finding_count = excluded.finding_count,
           computed_at = datetime('now')",
        params![
            project_id,
            score.module_id,
            score.module_path,
            score.overall_score,
            score.tier,
            score.factor_scores,
            score.line_count,
            score.finding_count,
        ],
    )?;
    Ok(())
}

/// Get all tech debt scores for a project, sorted worst-first
pub fn get_debt_scores_sync(
    conn: &Connection,
    project_id: i64,
) -> rusqlite::Result<Vec<TechDebtScore>> {
    let mut stmt = conn.prepare(
        "SELECT module_id, module_path, overall_score, tier, factor_scores, line_count, finding_count
         FROM tech_debt_scores
         WHERE project_id = ?
         ORDER BY overall_score DESC",
    )?;

    let scores = stmt
        .query_map(params![project_id], |row| {
            Ok(TechDebtScore {
                module_id: row.get(0)?,
                module_path: row.get(1)?,
                overall_score: row.get(2)?,
                tier: row.get(3)?,
                factor_scores: row.get(4)?,
                line_count: row.get(5)?,
                finding_count: row.get(6)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(scores)
}

/// Get a summary: count per tier
pub fn get_debt_summary_sync(
    conn: &Connection,
    project_id: i64,
) -> rusqlite::Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT tier, COUNT(*) FROM tech_debt_scores
         WHERE project_id = ?
         GROUP BY tier ORDER BY tier",
    )?;

    let summary = stmt
        .query_map(params![project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(summary)
}
