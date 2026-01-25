// crates/mira-server/src/experts/patterns.rs
// Problem pattern detection and learning

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::ExpertRole;

/// A learned problem pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProblemPattern {
    pub id: Option<i64>,
    pub expert_role: ExpertRole,
    pub pattern_signature: String,
    pub pattern_description: Option<String>,
    pub common_context_elements: Vec<String>,
    pub successful_approaches: Vec<String>,
    pub recommended_tools: Vec<String>,
    pub success_rate: f64,
    pub occurrence_count: i64,
    pub avg_confidence: f64,
    pub avg_acceptance_rate: f64,
}

/// Store or update a problem pattern
pub fn upsert_pattern(conn: &Connection, pattern: &ProblemPattern) -> Result<i64> {
    let sql = r#"
        INSERT INTO problem_patterns
        (expert_role, pattern_signature, pattern_description, common_context_elements,
         successful_approaches, recommended_tools, success_rate, occurrence_count,
         avg_confidence, avg_acceptance_rate, last_seen_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))
        ON CONFLICT(expert_role, pattern_signature) DO UPDATE SET
            pattern_description = COALESCE(excluded.pattern_description, pattern_description),
            common_context_elements = excluded.common_context_elements,
            successful_approaches = excluded.successful_approaches,
            recommended_tools = excluded.recommended_tools,
            success_rate = (success_rate * occurrence_count + excluded.success_rate) / (occurrence_count + 1),
            occurrence_count = occurrence_count + 1,
            avg_confidence = (avg_confidence * occurrence_count + excluded.avg_confidence) / (occurrence_count + 1),
            avg_acceptance_rate = (avg_acceptance_rate * occurrence_count + excluded.avg_acceptance_rate) / (occurrence_count + 1),
            last_seen_at = datetime('now')
    "#;

    let context_json = serde_json::to_string(&pattern.common_context_elements).unwrap_or_default();
    let approaches_json = serde_json::to_string(&pattern.successful_approaches).unwrap_or_default();
    let tools_json = serde_json::to_string(&pattern.recommended_tools).unwrap_or_default();

    conn.execute(sql, rusqlite::params![
        pattern.expert_role.as_str(),
        pattern.pattern_signature,
        pattern.pattern_description,
        context_json,
        approaches_json,
        tools_json,
        pattern.success_rate,
        pattern.occurrence_count,
        pattern.avg_confidence,
        pattern.avg_acceptance_rate,
    ])?;

    Ok(conn.last_insert_rowid())
}

/// Get patterns for an expert sorted by success rate
pub fn get_top_patterns(conn: &Connection, expert_role: ExpertRole, limit: i64) -> Result<Vec<ProblemPattern>> {
    let sql = r#"
        SELECT id, expert_role, pattern_signature, pattern_description,
               common_context_elements, successful_approaches, recommended_tools,
               success_rate, occurrence_count, avg_confidence, avg_acceptance_rate
        FROM problem_patterns
        WHERE expert_role = ?
        ORDER BY success_rate DESC, occurrence_count DESC
        LIMIT ?
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([expert_role.as_str(), &limit.to_string()], |row| {
        let role_str: String = row.get(1)?;
        let context_json: String = row.get(4)?;
        let approaches_json: String = row.get(5)?;
        let tools_json: String = row.get(6)?;

        Ok(ProblemPattern {
            id: Some(row.get(0)?),
            expert_role: ExpertRole::from_str(&role_str).unwrap_or(ExpertRole::Architect),
            pattern_signature: row.get(2)?,
            pattern_description: row.get(3)?,
            common_context_elements: serde_json::from_str(&context_json).unwrap_or_default(),
            successful_approaches: serde_json::from_str(&approaches_json).unwrap_or_default(),
            recommended_tools: serde_json::from_str(&tools_json).unwrap_or_default(),
            success_rate: row.get(7)?,
            occurrence_count: row.get(8)?,
            avg_confidence: row.get(9)?,
            avg_acceptance_rate: row.get(10)?,
        })
    })?;

    let patterns: Vec<ProblemPattern> = rows.flatten().collect();
    Ok(patterns)
}

/// Find matching patterns for a given context
pub fn find_matching_patterns(
    conn: &Connection,
    expert_role: ExpertRole,
    context: &str,
    min_success_rate: f64,
) -> Result<Vec<ProblemPattern>> {
    // Extract keywords from context for matching
    let keywords = extract_keywords(context);

    if keywords.is_empty() {
        return Ok(vec![]);
    }

    // Get all patterns for this expert with good success rate
    let patterns = get_top_patterns(conn, expert_role, 20)?;

    // Filter to those that match the context keywords
    let matching: Vec<ProblemPattern> = patterns
        .into_iter()
        .filter(|p| {
            p.success_rate >= min_success_rate && pattern_matches_context(p, &keywords)
        })
        .collect();

    Ok(matching)
}

/// Extract keywords from context for pattern matching
fn extract_keywords(context: &str) -> Vec<String> {
    let lower = context.to_lowercase();

    // Common technical keywords to look for
    let technical_keywords = [
        "security", "auth", "password", "token", "api", "database", "query",
        "performance", "cache", "memory", "cpu", "async", "thread",
        "error", "exception", "bug", "fix", "crash",
        "refactor", "clean", "simplify", "extract",
        "test", "coverage", "mock", "stub",
        "design", "pattern", "architecture", "module",
        "config", "env", "setting", "option",
    ];

    technical_keywords
        .iter()
        .filter(|kw| lower.contains(*kw))
        .map(|s| s.to_string())
        .collect()
}

/// Check if a pattern matches the context keywords
fn pattern_matches_context(pattern: &ProblemPattern, keywords: &[String]) -> bool {
    // Check if any of the pattern's context elements match the keywords
    for element in &pattern.common_context_elements {
        let lower = element.to_lowercase();
        for kw in keywords {
            if lower.contains(kw) {
                return true;
            }
        }
    }

    // Also check pattern description
    if let Some(desc) = &pattern.pattern_description {
        let lower = desc.to_lowercase();
        for kw in keywords {
            if lower.contains(kw) {
                return true;
            }
        }
    }

    false
}

/// Mine patterns from consultation history
pub fn mine_patterns_from_history(conn: &Connection, expert_role: ExpertRole, project_id: i64) -> Result<usize> {
    // Find consultations with accepted findings
    let sql = r#"
        SELECT ec.problem_category, ec.tools_used, ec.context_summary,
               AVG(CASE WHEN rf.status IN ('accepted', 'fixed') THEN 1.0 ELSE 0.0 END) as success_rate,
               COUNT(*) as count,
               AVG(ec.initial_confidence) as avg_confidence
        FROM expert_consultations ec
        LEFT JOIN review_findings rf ON rf.expert_role = ec.expert_role
            AND rf.project_id = ec.project_id
            AND rf.session_id = ec.session_id
        WHERE ec.expert_role = ? AND ec.project_id = ?
        GROUP BY ec.problem_category
        HAVING COUNT(*) >= 3
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([expert_role.as_str(), &project_id.to_string()], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, f64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, Option<f64>>(5)?,
        ))
    })?;

    let mut patterns_stored = 0;

    for row in rows.flatten() {
        let (category, tools_json, _context_summary, success_rate, count, avg_confidence) = row;

        if let Some(category) = category {
            let tools: Vec<String> = serde_json::from_str(&tools_json).unwrap_or_default();

            // Create pattern from this category
            let pattern = ProblemPattern {
                id: None,
                expert_role,
                pattern_signature: format!("{}:{}", expert_role.as_str(), category),
                pattern_description: Some(format!("{} problems", category)),
                common_context_elements: vec![category.clone()],
                successful_approaches: vec![format!("Standard {} approach", category)],
                recommended_tools: tools,
                success_rate,
                occurrence_count: count,
                avg_confidence: avg_confidence.unwrap_or(0.5),
                avg_acceptance_rate: success_rate,
            };

            upsert_pattern(conn, &pattern)?;
            patterns_stored += 1;
        }
    }

    Ok(patterns_stored)
}

/// Update pattern success rate based on new outcome
pub fn update_pattern_success(conn: &Connection, pattern_id: i64, outcome_success: bool) -> Result<()> {
    let success_delta = if outcome_success { 1.0 } else { 0.0 };

    let sql = r#"
        UPDATE problem_patterns
        SET success_rate = (success_rate * occurrence_count + ?) / (occurrence_count + 1),
            occurrence_count = occurrence_count + 1,
            last_seen_at = datetime('now')
        WHERE id = ?
    "#;

    conn.execute(sql, rusqlite::params![success_delta, pattern_id])?;
    Ok(())
}
