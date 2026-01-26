// db/reviews.rs
// Database operations for code review findings and learned patterns

use anyhow::Result;
use rusqlite::{params, Connection};

use super::Database;

// ============================================================================
// Sync functions for pool.interact() usage
// ============================================================================

/// Store a new review finding (sync version for pool.interact)
#[allow(clippy::too_many_arguments)]
pub fn store_review_finding_sync(
    conn: &Connection,
    project_id: Option<i64>,
    expert_role: &str,
    file_path: Option<&str>,
    finding_type: &str,
    severity: &str,
    content: &str,
    code_snippet: Option<&str>,
    suggestion: Option<&str>,
    confidence: f64,
    user_id: Option<&str>,
    session_id: Option<&str>,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO review_findings (
            project_id, expert_role, file_path, finding_type, severity,
            content, code_snippet, suggestion, confidence, user_id, session_id
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            project_id,
            expert_role,
            file_path,
            finding_type,
            severity,
            content,
            code_snippet,
            suggestion,
            confidence,
            user_id,
            session_id
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Get relevant corrections for context injection (sync version for pool.interact)
pub fn get_relevant_corrections_sync(
    conn: &Connection,
    project_id: Option<i64>,
    correction_type: Option<&str>,
    limit: usize,
) -> rusqlite::Result<Vec<Correction>> {
    let sql = match correction_type {
        Some(_) => "SELECT id, project_id, what_was_wrong, what_is_right, correction_type,
                           scope, confidence, occurrence_count, acceptance_rate, created_at
                    FROM corrections
                    WHERE (project_id = ? OR project_id IS NULL OR scope = 'global')
                          AND correction_type = ?
                          AND confidence >= 0.5
                    ORDER BY acceptance_rate DESC, occurrence_count DESC
                    LIMIT ?",
        None => "SELECT id, project_id, what_was_wrong, what_is_right, correction_type,
                        scope, confidence, occurrence_count, acceptance_rate, created_at
                 FROM corrections
                 WHERE (project_id = ? OR project_id IS NULL OR scope = 'global')
                       AND confidence >= 0.5
                 ORDER BY acceptance_rate DESC, occurrence_count DESC
                 LIMIT ?",
    };

    let mut stmt = conn.prepare(sql)?;
    let rows = match correction_type {
        Some(ct) => stmt.query_map(params![project_id, ct, limit as i64], parse_correction_row)?,
        None => stmt.query_map(params![project_id, limit as i64], parse_correction_row)?,
    };
    rows.collect()
}

/// Get review findings with optional filters (sync version for pool.interact)
pub fn get_findings_sync(
    conn: &Connection,
    project_id: Option<i64>,
    status: Option<&str>,
    expert_role: Option<&str>,
    file_path: Option<&str>,
    limit: usize,
) -> rusqlite::Result<Vec<ReviewFinding>> {
    // Build dynamic query
    let mut conditions = vec!["(project_id = ?1 OR project_id IS NULL)"];
    if status.is_some() {
        conditions.push("status = ?2");
    }
    if expert_role.is_some() {
        conditions.push("expert_role = ?3");
    }
    if file_path.is_some() {
        conditions.push("file_path = ?4");
    }

    let sql = format!(
        "SELECT id, project_id, expert_role, file_path, finding_type, severity,
                content, code_snippet, suggestion, status, feedback, confidence,
                user_id, reviewed_by, session_id, created_at, reviewed_at
         FROM review_findings
         WHERE {}
         ORDER BY created_at DESC
         LIMIT ?5",
        conditions.join(" AND ")
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        params![
            project_id,
            status.unwrap_or(""),
            expert_role.unwrap_or(""),
            file_path.unwrap_or(""),
            limit as i64
        ],
        parse_review_finding_row,
    )?;
    rows.collect()
}

/// Get a single finding by ID (sync version for pool.interact)
pub fn get_finding_sync(conn: &Connection, finding_id: i64) -> rusqlite::Result<Option<ReviewFinding>> {
    let sql = "SELECT id, project_id, expert_role, file_path, finding_type, severity,
                      content, code_snippet, suggestion, status, feedback, confidence,
                      user_id, reviewed_by, session_id, created_at, reviewed_at
               FROM review_findings
               WHERE id = ?";
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query_map([finding_id], parse_review_finding_row)?;
    match rows.next() {
        Some(Ok(finding)) => Ok(Some(finding)),
        Some(Err(e)) => Err(e),
        None => Ok(None),
    }
}

/// Get statistics about review findings (sync version for pool.interact)
pub fn get_finding_stats_sync(conn: &Connection, project_id: Option<i64>) -> rusqlite::Result<(i64, i64, i64, i64)> {
    let sql = "SELECT
                   SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END) as pending,
                   SUM(CASE WHEN status = 'accepted' THEN 1 ELSE 0 END) as accepted,
                   SUM(CASE WHEN status = 'rejected' THEN 1 ELSE 0 END) as rejected,
                   SUM(CASE WHEN status = 'fixed' THEN 1 ELSE 0 END) as fixed
               FROM review_findings
               WHERE project_id = ? OR project_id IS NULL";

    conn.query_row(sql, [project_id], |row| {
        Ok((
            row.get::<_, Option<i64>>(0)?.unwrap_or(0),
            row.get::<_, Option<i64>>(1)?.unwrap_or(0),
            row.get::<_, Option<i64>>(2)?.unwrap_or(0),
            row.get::<_, Option<i64>>(3)?.unwrap_or(0),
        ))
    })
}

/// Update a finding's status (sync version for pool.interact)
pub fn update_finding_status_sync(
    conn: &Connection,
    finding_id: i64,
    status: &str,
    feedback: Option<&str>,
    reviewed_by: Option<&str>,
) -> rusqlite::Result<bool> {
    let rows = conn.execute(
        "UPDATE review_findings
         SET status = ?, feedback = ?, reviewed_by = ?, reviewed_at = CURRENT_TIMESTAMP
         WHERE id = ?",
        params![status, feedback, reviewed_by, finding_id],
    )?;
    Ok(rows > 0)
}

/// Bulk update finding statuses (sync version for pool.interact)
pub fn bulk_update_finding_status_sync(
    conn: &Connection,
    finding_ids: &[i64],
    status: &str,
    reviewed_by: Option<&str>,
) -> rusqlite::Result<usize> {
    if finding_ids.is_empty() {
        return Ok(0);
    }

    let placeholders: Vec<&str> = finding_ids.iter().map(|_| "?").collect();
    let sql = format!(
        "UPDATE review_findings
         SET status = ?1, reviewed_by = ?2, reviewed_at = CURRENT_TIMESTAMP
         WHERE id IN ({})",
        placeholders.join(",")
    );

    // Build params dynamically
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![
        Box::new(status.to_string()),
        Box::new(reviewed_by.map(|s| s.to_string())),
    ];
    for id in finding_ids {
        params_vec.push(Box::new(*id));
    }

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    conn.execute(&sql, params_refs.as_slice())
}

/// Extract patterns from accepted findings (sync version for pool.interact)
pub fn extract_patterns_from_findings_sync(conn: &Connection, project_id: Option<i64>) -> rusqlite::Result<usize> {
    // Find accepted findings that could become patterns
    let sql = "SELECT finding_type, content, suggestion, COUNT(*) as cnt
               FROM review_findings
               WHERE (project_id = ? OR project_id IS NULL)
                     AND status = 'accepted'
                     AND suggestion IS NOT NULL
               GROUP BY finding_type, content
               HAVING cnt >= 2
               ORDER BY cnt DESC
               LIMIT 50";

    let mut stmt = conn.prepare(sql)?;
    let patterns: Vec<(String, String, String, i64)> = stmt
        .query_map([project_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut created = 0;
    for (finding_type, content, suggestion, count) in patterns {
        // Check if this pattern already exists
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM corrections
                 WHERE (project_id = ? OR project_id IS NULL)
                       AND what_was_wrong = ?",
                params![project_id, &content],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if !exists {
            // Create new correction
            let confidence = (count as f64 / 10.0).min(1.0);
            conn.execute(
                "INSERT INTO corrections (
                    project_id, what_was_wrong, what_is_right, correction_type,
                    scope, confidence, occurrence_count, acceptance_rate
                ) VALUES (?, ?, ?, ?, 'project', ?, ?, 1.0)",
                params![project_id, &content, &suggestion, &finding_type, confidence, count],
            )?;
            created += 1;
        } else {
            conn.execute(
                "UPDATE corrections
                 SET occurrence_count = occurrence_count + ?
                 WHERE (project_id = ? OR project_id IS NULL) AND what_was_wrong = ?",
                params![count, project_id, &content],
            )?;
        }
    }

    Ok(created)
}

// ============================================================================
// Types and Database impl methods
// ============================================================================

/// A code review finding from an expert consultation
#[derive(Debug, Clone)]
pub struct ReviewFinding {
    pub id: i64,
    pub project_id: Option<i64>,
    pub expert_role: String,
    pub file_path: Option<String>,
    pub finding_type: String,
    pub severity: String,
    pub content: String,
    pub code_snippet: Option<String>,
    pub suggestion: Option<String>,
    pub status: String,
    pub feedback: Option<String>,
    pub confidence: f64,
    pub user_id: Option<String>,
    pub reviewed_by: Option<String>,
    pub session_id: Option<String>,
    pub created_at: String,
    pub reviewed_at: Option<String>,
}

/// A learned correction pattern
#[derive(Debug, Clone)]
pub struct Correction {
    pub id: i64,
    pub project_id: Option<i64>,
    pub what_was_wrong: String,
    pub what_is_right: String,
    pub correction_type: String,
    pub scope: String,
    pub confidence: f64,
    pub occurrence_count: i64,
    pub acceptance_rate: f64,
    pub created_at: String,
}

/// Parse ReviewFinding from a rusqlite Row
pub fn parse_review_finding_row(row: &rusqlite::Row) -> rusqlite::Result<ReviewFinding> {
    Ok(ReviewFinding {
        id: row.get(0)?,
        project_id: row.get(1)?,
        expert_role: row.get(2)?,
        file_path: row.get(3)?,
        finding_type: row.get(4)?,
        severity: row.get(5)?,
        content: row.get(6)?,
        code_snippet: row.get(7)?,
        suggestion: row.get(8)?,
        status: row.get(9)?,
        feedback: row.get(10)?,
        confidence: row.get(11)?,
        user_id: row.get(12)?,
        reviewed_by: row.get(13)?,
        session_id: row.get(14)?,
        created_at: row.get(15)?,
        reviewed_at: row.get(16)?,
    })
}

/// Parse Correction from a rusqlite Row
pub fn parse_correction_row(row: &rusqlite::Row) -> rusqlite::Result<Correction> {
    Ok(Correction {
        id: row.get(0)?,
        project_id: row.get(1)?,
        what_was_wrong: row.get(2)?,
        what_is_right: row.get(3)?,
        correction_type: row.get(4)?,
        scope: row.get(5)?,
        confidence: row.get(6)?,
        occurrence_count: row.get::<_, Option<i64>>(7)?.unwrap_or(1),
        acceptance_rate: row.get::<_, Option<f64>>(8)?.unwrap_or(1.0),
        created_at: row.get(9)?,
    })
}

impl Database {
    /// Store a new review finding
    pub fn store_review_finding(
        &self,
        project_id: Option<i64>,
        expert_role: &str,
        file_path: Option<&str>,
        finding_type: &str,
        severity: &str,
        content: &str,
        code_snippet: Option<&str>,
        suggestion: Option<&str>,
        confidence: f64,
        user_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<i64> {
        store_review_finding_sync(
            &self.conn(), project_id, expert_role, file_path, finding_type, severity,
            content, code_snippet, suggestion, confidence, user_id, session_id,
        ).map_err(Into::into)
    }

    /// Get pending review findings for a project
    pub fn get_pending_findings(
        &self,
        project_id: Option<i64>,
        limit: usize,
    ) -> Result<Vec<ReviewFinding>> {
        let conn = self.conn();
        let sql = "SELECT id, project_id, expert_role, file_path, finding_type, severity,
                          content, code_snippet, suggestion, status, feedback, confidence,
                          user_id, reviewed_by, session_id, created_at, reviewed_at
                   FROM review_findings
                   WHERE (project_id = ? OR project_id IS NULL) AND status = 'pending'
                   ORDER BY
                       CASE severity
                           WHEN 'critical' THEN 1
                           WHEN 'major' THEN 2
                           WHEN 'minor' THEN 3
                           ELSE 4
                       END,
                       created_at DESC
                   LIMIT ?";
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![project_id, limit as i64], parse_review_finding_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Get review findings with optional filters
    pub fn get_findings(
        &self,
        project_id: Option<i64>,
        status: Option<&str>,
        expert_role: Option<&str>,
        file_path: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ReviewFinding>> {
        get_findings_sync(&self.conn(), project_id, status, expert_role, file_path, limit)
            .map_err(Into::into)
    }

    /// Get a single finding by ID
    pub fn get_finding(&self, finding_id: i64) -> Result<Option<ReviewFinding>> {
        get_finding_sync(&self.conn(), finding_id).map_err(Into::into)
    }

    /// Update a finding's status (accept/reject/fixed)
    pub fn update_finding_status(
        &self,
        finding_id: i64,
        status: &str,
        feedback: Option<&str>,
        reviewed_by: Option<&str>,
    ) -> Result<bool> {
        update_finding_status_sync(&self.conn(), finding_id, status, feedback, reviewed_by)
            .map_err(Into::into)
    }

    /// Bulk update finding statuses
    pub fn bulk_update_finding_status(
        &self,
        finding_ids: &[i64],
        status: &str,
        reviewed_by: Option<&str>,
    ) -> Result<usize> {
        bulk_update_finding_status_sync(&self.conn(), finding_ids, status, reviewed_by)
            .map_err(Into::into)
    }

    /// Delete a finding
    pub fn delete_finding(&self, finding_id: i64) -> Result<bool> {
        let conn = self.conn();
        let rows = conn.execute("DELETE FROM review_findings WHERE id = ?", [finding_id])?;
        Ok(rows > 0)
    }

    /// Get findings for a specific file
    pub fn get_findings_for_file(
        &self,
        project_id: Option<i64>,
        file_path: &str,
        limit: usize,
    ) -> Result<Vec<ReviewFinding>> {
        let conn = self.conn();
        let sql = "SELECT id, project_id, expert_role, file_path, finding_type, severity,
                          content, code_snippet, suggestion, status, feedback, confidence,
                          user_id, reviewed_by, session_id, created_at, reviewed_at
                   FROM review_findings
                   WHERE (project_id = ? OR project_id IS NULL) AND file_path = ?
                   ORDER BY created_at DESC
                   LIMIT ?";
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![project_id, file_path, limit as i64], parse_review_finding_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    // === Corrections / Learned Patterns ===

    /// Store a learned correction pattern
    pub fn store_correction(
        &self,
        project_id: Option<i64>,
        what_was_wrong: &str,
        what_is_right: &str,
        correction_type: &str,
        scope: &str,
        confidence: f64,
    ) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO corrections (
                project_id, what_was_wrong, what_is_right, correction_type, scope, confidence
            ) VALUES (?, ?, ?, ?, ?, ?)",
            params![project_id, what_was_wrong, what_is_right, correction_type, scope, confidence],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get relevant corrections for context injection
    pub fn get_relevant_corrections(
        &self,
        project_id: Option<i64>,
        correction_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Correction>> {
        get_relevant_corrections_sync(&self.conn(), project_id, correction_type, limit)
            .map_err(Into::into)
    }

    /// Update correction statistics after a finding is reviewed
    pub fn update_correction_stats(
        &self,
        correction_id: i64,
        was_accepted: bool,
    ) -> Result<()> {
        let conn = self.conn();

        // Get current stats
        let (occurrence_count, acceptance_rate): (i64, f64) = conn.query_row(
            "SELECT COALESCE(occurrence_count, 1), COALESCE(acceptance_rate, 1.0)
             FROM corrections WHERE id = ?",
            [correction_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        // Calculate new acceptance rate
        let new_count = occurrence_count + 1;
        let accepted_count = (acceptance_rate * occurrence_count as f64) as i64;
        let new_accepted = if was_accepted { accepted_count + 1 } else { accepted_count };
        let new_rate = new_accepted as f64 / new_count as f64;

        conn.execute(
            "UPDATE corrections SET occurrence_count = ?, acceptance_rate = ? WHERE id = ?",
            params![new_count, new_rate, correction_id],
        )?;

        Ok(())
    }

    /// Extract patterns from accepted findings and create/update corrections
    /// This is called periodically to learn from reviewed findings
    pub fn extract_patterns_from_findings(&self, project_id: Option<i64>) -> Result<usize> {
        extract_patterns_from_findings_sync(&self.conn(), project_id).map_err(Into::into)
    }

    /// Get statistics about review findings
    pub fn get_finding_stats(&self, project_id: Option<i64>) -> Result<(i64, i64, i64, i64)> {
        get_finding_stats_sync(&self.conn(), project_id).map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_and_retrieve_finding() {
        let db = Database::open_in_memory().unwrap();
        let (project_id, _) = db.get_or_create_project("/test", None).unwrap();

        let id = db.store_review_finding(
            Some(project_id),
            "code_reviewer",
            Some("src/main.rs"),
            "bug",
            "major",
            "Potential null pointer dereference",
            Some("let x = foo.unwrap();"),
            Some("Use .unwrap_or_default() or handle the None case"),
            0.8,
            Some("user@example.com"),
            Some("session-123"),
        ).unwrap();

        assert!(id > 0);

        let finding = db.get_finding(id).unwrap().unwrap();
        assert_eq!(finding.expert_role, "code_reviewer");
        assert_eq!(finding.finding_type, "bug");
        assert_eq!(finding.status, "pending");
    }

    #[test]
    fn test_update_finding_status() {
        let db = Database::open_in_memory().unwrap();
        let (project_id, _) = db.get_or_create_project("/test", None).unwrap();

        let id = db.store_review_finding(
            Some(project_id),
            "security",
            Some("src/auth.rs"),
            "security",
            "critical",
            "SQL injection vulnerability",
            None,
            Some("Use parameterized queries"),
            0.9,
            None,
            None,
        ).unwrap();

        let updated = db.update_finding_status(
            id,
            "accepted",
            Some("Good catch, will fix"),
            Some("reviewer@example.com"),
        ).unwrap();

        assert!(updated);

        let finding = db.get_finding(id).unwrap().unwrap();
        assert_eq!(finding.status, "accepted");
        assert_eq!(finding.feedback, Some("Good catch, will fix".to_string()));
    }

    #[test]
    fn test_get_pending_findings() {
        let db = Database::open_in_memory().unwrap();
        let (project_id, _) = db.get_or_create_project("/test", None).unwrap();

        // Create findings with different severities
        db.store_review_finding(Some(project_id), "code_reviewer", None, "bug", "minor", "Minor issue", None, None, 0.5, None, None).unwrap();
        db.store_review_finding(Some(project_id), "code_reviewer", None, "bug", "critical", "Critical issue", None, None, 0.9, None, None).unwrap();
        db.store_review_finding(Some(project_id), "code_reviewer", None, "bug", "major", "Major issue", None, None, 0.7, None, None).unwrap();

        let findings = db.get_pending_findings(Some(project_id), 10).unwrap();
        assert_eq!(findings.len(), 3);
        // Should be ordered by severity: critical, major, minor
        assert_eq!(findings[0].severity, "critical");
        assert_eq!(findings[1].severity, "major");
        assert_eq!(findings[2].severity, "minor");
    }
}
