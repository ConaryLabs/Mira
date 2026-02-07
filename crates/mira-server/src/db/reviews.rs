// db/reviews.rs
// Database operations for code review findings and learned patterns

use rusqlite::{Connection, params};

// ============================================================================
// Sync functions for pool.interact() usage
// ============================================================================

/// Parameters for storing a review finding (owns data to avoid clone-heavy call sites)
pub struct ReviewFindingParams {
    pub project_id: Option<i64>,
    pub expert_role: String,
    pub file_path: Option<String>,
    pub finding_type: String,
    pub severity: String,
    pub content: String,
    pub code_snippet: Option<String>,
    pub suggestion: Option<String>,
    pub confidence: f64,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
}

/// Store a new review finding (sync version for pool.interact)
pub fn store_review_finding_sync(
    conn: &Connection,
    p: &ReviewFindingParams,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO review_findings (
            project_id, expert_role, file_path, finding_type, severity,
            content, code_snippet, suggestion, confidence, user_id, session_id
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            p.project_id,
            p.expert_role,
            p.file_path,
            p.finding_type,
            p.severity,
            p.content,
            p.code_snippet,
            p.suggestion,
            p.confidence,
            p.user_id,
            p.session_id,
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
        Some(_) => {
            "SELECT id, project_id, what_was_wrong, what_is_right, correction_type,
                           scope, confidence, occurrence_count, acceptance_rate, created_at
                    FROM corrections
                    WHERE (project_id = ? OR project_id IS NULL OR scope = 'global')
                          AND correction_type = ?
                          AND confidence >= 0.5
                    ORDER BY acceptance_rate DESC, occurrence_count DESC
                    LIMIT ?"
        }
        None => {
            "SELECT id, project_id, what_was_wrong, what_is_right, correction_type,
                        scope, confidence, occurrence_count, acceptance_rate, created_at
                 FROM corrections
                 WHERE (project_id = ? OR project_id IS NULL OR scope = 'global')
                       AND confidence >= 0.5
                 ORDER BY acceptance_rate DESC, occurrence_count DESC
                 LIMIT ?"
        }
    };

    let mut stmt = conn.prepare(sql)?;
    let rows = match correction_type {
        Some(ct) => stmt.query_map(params![project_id, ct, limit as i64], parse_correction_row)?,
        None => stmt.query_map(params![project_id, limit as i64], parse_correction_row)?,
    };
    rows.collect()
}

/// Get review findings with optional filters (sync version for pool.interact)
///
/// Uses UNION ALL instead of OR for index-friendly project scoping.
/// Positional params are shared across UNION ALL branches (?2-?4 reused in both).
pub fn get_findings_sync(
    conn: &Connection,
    project_id: Option<i64>,
    status: Option<&str>,
    expert_role: Option<&str>,
    file_path: Option<&str>,
    limit: usize,
) -> rusqlite::Result<Vec<ReviewFinding>> {
    let cols = "id, project_id, expert_role, file_path, finding_type, severity, \
                content, code_snippet, suggestion, status, feedback, confidence, \
                user_id, reviewed_by, session_id, created_at, reviewed_at";

    // Fixed param positions: ?1=project_id, ?2=status, ?3=role, ?4=path, ?5=limit
    let mut extra = Vec::new();
    if status.is_some() {
        extra.push("status = ?2");
    }
    if expert_role.is_some() {
        extra.push("expert_role = ?3");
    }
    if file_path.is_some() {
        extra.push("file_path = ?4");
    }
    let extra_clause = if extra.is_empty() {
        String::new()
    } else {
        format!(" AND {}", extra.join(" AND "))
    };

    let sql = match project_id {
        Some(_) => format!(
            "SELECT {cols} FROM review_findings WHERE project_id = ?1{extra_clause} \
             UNION ALL \
             SELECT {cols} FROM review_findings WHERE project_id IS NULL{extra_clause} \
             ORDER BY created_at DESC LIMIT ?5"
        ),
        None => format!(
            "SELECT {cols} FROM review_findings WHERE project_id IS NULL{extra_clause} \
             ORDER BY created_at DESC LIMIT ?5"
        ),
    };

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
pub fn get_finding_sync(
    conn: &Connection,
    finding_id: i64,
) -> rusqlite::Result<Option<ReviewFinding>> {
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
///
/// Uses UNION ALL subquery instead of OR for index-friendly project scoping.
pub fn get_finding_stats_sync(
    conn: &Connection,
    project_id: Option<i64>,
) -> rusqlite::Result<(i64, i64, i64, i64)> {
    let sql = match project_id {
        Some(_) => {
            "SELECT \
                 SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END), \
                 SUM(CASE WHEN status = 'accepted' THEN 1 ELSE 0 END), \
                 SUM(CASE WHEN status = 'rejected' THEN 1 ELSE 0 END), \
                 SUM(CASE WHEN status = 'fixed' THEN 1 ELSE 0 END) \
             FROM ( \
                 SELECT status FROM review_findings WHERE project_id = ?1 \
                 UNION ALL \
                 SELECT status FROM review_findings WHERE project_id IS NULL \
             )"
        }
        None => {
            "SELECT \
                 SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END), \
                 SUM(CASE WHEN status = 'accepted' THEN 1 ELSE 0 END), \
                 SUM(CASE WHEN status = 'rejected' THEN 1 ELSE 0 END), \
                 SUM(CASE WHEN status = 'fixed' THEN 1 ELSE 0 END) \
             FROM review_findings WHERE project_id IS NULL"
        }
    };

    conn.query_row(sql, params![project_id], |row| {
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
///
/// Uses UNION ALL instead of OR for index-friendly project scoping.
pub fn extract_patterns_from_findings_sync(
    conn: &Connection,
    project_id: Option<i64>,
) -> rusqlite::Result<usize> {
    // Find accepted findings that could become patterns
    let sql = match project_id {
        Some(_) => {
            "SELECT finding_type, content, MAX(suggestion), COUNT(*) as cnt FROM ( \
                 SELECT finding_type, content, suggestion FROM review_findings \
                     WHERE project_id = ?1 AND status = 'accepted' AND suggestion IS NOT NULL \
                 UNION ALL \
                 SELECT finding_type, content, suggestion FROM review_findings \
                     WHERE project_id IS NULL AND status = 'accepted' AND suggestion IS NOT NULL \
             ) GROUP BY finding_type, content HAVING cnt >= 2 ORDER BY cnt DESC LIMIT 50"
        }
        None => {
            "SELECT finding_type, content, MAX(suggestion), COUNT(*) as cnt \
             FROM review_findings \
             WHERE project_id IS NULL AND status = 'accepted' AND suggestion IS NOT NULL \
             GROUP BY finding_type, content HAVING cnt >= 2 ORDER BY cnt DESC LIMIT 50"
        }
    };

    let mut stmt = conn.prepare(sql)?;
    let patterns: Vec<(String, String, String, i64)> = stmt
        .query_map(params![project_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .filter_map(super::log_and_discard)
        .collect();

    let mut created = 0;
    // Exists check + update: use project_id = ?1 UNION ALL project_id IS NULL
    let exists_sql = match project_id {
        Some(_) => {
            "SELECT 1 FROM corrections WHERE project_id = ?1 AND what_was_wrong = ?2 \
             UNION ALL \
             SELECT 1 FROM corrections WHERE project_id IS NULL AND what_was_wrong = ?2 \
             LIMIT 1"
        }
        None => "SELECT 1 FROM corrections WHERE project_id IS NULL AND what_was_wrong = ?2 LIMIT 1",
    };
    let update_sql = match project_id {
        Some(_) => {
            "UPDATE corrections SET occurrence_count = occurrence_count + ?1 \
             WHERE (project_id = ?2 OR project_id IS NULL) AND what_was_wrong = ?3"
        }
        None => {
            "UPDATE corrections SET occurrence_count = occurrence_count + ?1 \
             WHERE project_id IS NULL AND what_was_wrong = ?3"
        }
    };

    for (finding_type, content, suggestion, count) in patterns {
        let exists: bool = conn
            .query_row(exists_sql, params![project_id, &content], |_| Ok(true))
            .unwrap_or(false);

        if !exists {
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
            conn.execute(update_sql, params![count, project_id, &content])?;
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
