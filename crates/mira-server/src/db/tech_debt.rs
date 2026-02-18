// db/tech_debt.rs
// Database operations for tech debt scoring

use rusqlite::{Connection, params};

use super::log_and_discard;

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
        .filter_map(log_and_discard)
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
        .filter_map(log_and_discard)
        .collect();

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::setup_test_connection;

    fn setup_debt_db() -> (Connection, i64) {
        let conn = setup_test_connection();
        let (pid, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();
        (conn, pid)
    }

    fn make_score(module_id: &str, score: f64, tier: &str) -> TechDebtScore {
        TechDebtScore {
            module_id: module_id.to_string(),
            module_path: format!("src/{}.rs", module_id),
            overall_score: score,
            tier: tier.to_string(),
            factor_scores: "{}".to_string(),
            line_count: Some(100),
            finding_count: Some(5),
        }
    }

    // ========================================================================
    // Various score ranges: 0.0, 1.0, large float
    // ========================================================================

    #[test]
    fn test_store_debt_score_zero() {
        let (conn, pid) = setup_debt_db();

        let score = make_score("clean_module", 0.0, "A");
        store_debt_score_sync(&conn, pid, &score).expect("store score=0.0 should succeed");

        let scores = get_debt_scores_sync(&conn, pid).unwrap();
        assert_eq!(scores.len(), 1);
        assert!((scores[0].overall_score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_store_debt_score_one() {
        let (conn, pid) = setup_debt_db();

        let score = make_score("max_debt", 1.0, "F");
        store_debt_score_sync(&conn, pid, &score).expect("store score=1.0 should succeed");

        let scores = get_debt_scores_sync(&conn, pid).unwrap();
        assert_eq!(scores.len(), 1);
        assert!((scores[0].overall_score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_store_debt_score_large_float() {
        let (conn, pid) = setup_debt_db();

        let score = make_score("huge_debt", f64::MAX / 2.0, "F");
        store_debt_score_sync(&conn, pid, &score).expect("store very large score should succeed");

        let scores = get_debt_scores_sync(&conn, pid).unwrap();
        assert_eq!(scores.len(), 1);
        assert!(scores[0].overall_score > 1.0);
    }

    // ========================================================================
    // Querying with no data
    // ========================================================================

    #[test]
    fn test_get_debt_scores_empty() {
        let (conn, pid) = setup_debt_db();

        let scores =
            get_debt_scores_sync(&conn, pid).expect("get scores on empty table should succeed");
        assert!(scores.is_empty());
    }

    #[test]
    fn test_get_debt_summary_empty() {
        let (conn, pid) = setup_debt_db();

        let summary =
            get_debt_summary_sync(&conn, pid).expect("get summary on empty table should succeed");
        assert!(summary.is_empty());
    }

    // ========================================================================
    // UPSERT behavior (store_debt_score_sync)
    // ========================================================================

    #[test]
    fn test_store_debt_score_upsert() {
        let (conn, pid) = setup_debt_db();

        let score1 = make_score("mod_a", 0.5, "C");
        store_debt_score_sync(&conn, pid, &score1).unwrap();

        // Upsert with same module_id but different score
        let score2 = make_score("mod_a", 0.9, "F");
        store_debt_score_sync(&conn, pid, &score2).unwrap();

        let scores = get_debt_scores_sync(&conn, pid).unwrap();
        assert_eq!(scores.len(), 1, "upsert should not create duplicate");
        assert!((scores[0].overall_score - 0.9).abs() < f64::EPSILON);
        assert_eq!(scores[0].tier, "F");
    }

    // ========================================================================
    // Scores sorted worst-first
    // ========================================================================

    #[test]
    fn test_get_debt_scores_sorted_descending() {
        let (conn, pid) = setup_debt_db();

        store_debt_score_sync(&conn, pid, &make_score("low", 0.1, "A")).unwrap();
        store_debt_score_sync(&conn, pid, &make_score("high", 0.9, "F")).unwrap();
        store_debt_score_sync(&conn, pid, &make_score("mid", 0.5, "C")).unwrap();

        let scores = get_debt_scores_sync(&conn, pid).unwrap();
        assert_eq!(scores.len(), 3);
        assert_eq!(scores[0].module_id, "high");
        assert_eq!(scores[1].module_id, "mid");
        assert_eq!(scores[2].module_id, "low");
    }

    // ========================================================================
    // Nil values for optional fields
    // ========================================================================

    #[test]
    fn test_store_debt_score_with_none_optional_fields() {
        let (conn, pid) = setup_debt_db();

        let score = TechDebtScore {
            module_id: "sparse".to_string(),
            module_path: "src/sparse.rs".to_string(),
            overall_score: 0.3,
            tier: "B".to_string(),
            factor_scores: "{}".to_string(),
            line_count: None,
            finding_count: None,
        };
        store_debt_score_sync(&conn, pid, &score).expect("store with None fields should succeed");

        let scores = get_debt_scores_sync(&conn, pid).unwrap();
        assert_eq!(scores.len(), 1);
        assert!(scores[0].line_count.is_none());
        assert!(scores[0].finding_count.is_none());
    }
}
