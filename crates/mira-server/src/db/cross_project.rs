// crates/mira-server/src/db/cross_project.rs
// Cross-project knowledge surfacing: query memories across project boundaries.
//
// Since Mira is single-user with a shared mira.db, all projects' memory_facts
// and vec_memory live in one database. Cross-project queries simply search
// with project_id != current_project_id.

use crate::utils::truncate_at_boundary;
use rusqlite::Connection;

/// Distance threshold for "You solved this" labeling.
/// Only memories within this threshold get the strong attribution.
const SOLVED_DISTANCE_THRESHOLD: f32 = 0.25;

/// A memory from another project with attribution.
#[derive(Debug, Clone)]
pub struct CrossProjectMemory {
    pub fact_id: i64,
    pub content: String,
    pub fact_type: String,
    pub category: Option<String>,
    pub project_name: String,
    pub project_id: i64,
    pub distance: f32,
}

/// A technology preference observed across multiple projects.
#[derive(Debug, Clone)]
pub struct CrossProjectPreference {
    pub content: String,
    pub projects: String,
    pub project_count: i64,
    pub max_confidence: f64,
}

/// Semantic search across OTHER projects' memories.
///
/// Queries vec_memory + memory_facts WHERE project_id != current_project_id.
/// Returns results attributed to their source project (with project name).
///
/// Only returns project-scoped, non-archived, non-suspicious user fact types.
pub fn recall_cross_project_sync(
    conn: &Connection,
    embedding_bytes: &[u8],
    current_project_id: i64,
    limit: usize,
) -> rusqlite::Result<Vec<CrossProjectMemory>> {
    let sql = r#"
        SELECT v.fact_id, f.content, f.fact_type, f.category,
               COALESCE(p.name, REPLACE(REPLACE(p.path, rtrim(p.path, REPLACE(p.path, '/', '')), ''), '/', '')) as project_name,
               f.project_id,
               vec_distance_cosine(v.embedding, ?1) as distance
        FROM vec_memory v
        JOIN memory_facts f ON v.fact_id = f.id
        JOIN projects p ON f.project_id = p.id
        WHERE f.project_id != ?2
          AND f.project_id IS NOT NULL
          AND f.fact_type IN ('general','preference','decision','pattern','context')
          AND f.status != 'archived'
          AND COALESCE(f.suspicious, 0) = 0
          AND f.scope = 'project'
        ORDER BY distance
        LIMIT ?3
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(
        rusqlite::params![embedding_bytes, current_project_id, limit as i64],
        |row| {
            Ok(CrossProjectMemory {
                fact_id: row.get(0)?,
                content: row.get(1)?,
                fact_type: row.get(2)?,
                category: row.get(3)?,
                project_name: row.get(4)?,
                project_id: row.get(5)?,
                distance: row.get(6)?,
            })
        },
    )?;

    rows.collect()
}

/// Find high-confidence solutions from other projects ("You solved this in Project X").
///
/// Tighter distance threshold than general cross-project recall,
/// restricted to decision/pattern fact types with high confidence.
pub fn find_solved_in_other_project_sync(
    conn: &Connection,
    embedding_bytes: &[u8],
    current_project_id: i64,
    distance_threshold: f32,
    limit: usize,
) -> rusqlite::Result<Vec<CrossProjectMemory>> {
    let sql = r#"
        SELECT v.fact_id, f.content, f.fact_type, f.category,
               COALESCE(p.name, REPLACE(REPLACE(p.path, rtrim(p.path, REPLACE(p.path, '/', '')), ''), '/', '')) as project_name,
               f.project_id,
               vec_distance_cosine(v.embedding, ?1) as distance
        FROM vec_memory v
        JOIN memory_facts f ON v.fact_id = f.id
        JOIN projects p ON f.project_id = p.id
        WHERE f.project_id != ?2
          AND f.project_id IS NOT NULL
          AND f.fact_type IN ('decision', 'pattern')
          AND f.confidence >= 0.7
          AND f.status != 'archived'
          AND COALESCE(f.suspicious, 0) = 0
          AND f.scope = 'project'
          AND vec_distance_cosine(v.embedding, ?1) < ?4
        ORDER BY distance
        LIMIT ?3
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(
        rusqlite::params![
            embedding_bytes,
            current_project_id,
            limit as i64,
            distance_threshold
        ],
        |row| {
            Ok(CrossProjectMemory {
                fact_id: row.get(0)?,
                content: row.get(1)?,
                fact_type: row.get(2)?,
                category: row.get(3)?,
                project_name: row.get(4)?,
                project_id: row.get(5)?,
                distance: row.get(6)?,
            })
        },
    )?;

    rows.collect()
}

/// Find technology preferences that appear across multiple projects.
///
/// Groups memory_facts by content where fact_type='preference' and the
/// same content appears in 2+ distinct projects. Returns with project
/// names for attribution.
///
/// No embeddings needed — pure SQL aggregation.
pub fn get_cross_project_preferences_sync(
    conn: &Connection,
    current_project_id: i64,
    limit: usize,
) -> rusqlite::Result<Vec<CrossProjectPreference>> {
    // Find preferences that exist in the current project AND at least one other project.
    // This ensures relevance: we only surface preferences the user has in THIS project
    // that are also confirmed in other projects.
    let sql = r#"
        SELECT f.content,
               GROUP_CONCAT(DISTINCT COALESCE(p.name, REPLACE(REPLACE(p.path, rtrim(p.path, REPLACE(p.path, '/', '')), ''), '/', ''))) as projects,
               COUNT(DISTINCT f.project_id) as project_count,
               MAX(f.confidence) as max_confidence
        FROM memory_facts f
        JOIN projects p ON f.project_id = p.id
        WHERE f.fact_type = 'preference'
          AND f.status != 'archived'
          AND f.scope = 'project'
          AND COALESCE(f.suspicious, 0) = 0
          AND f.project_id IS NOT NULL
          AND EXISTS (
              SELECT 1 FROM memory_facts f2
              WHERE f2.content = f.content
                AND f2.project_id = ?1
                AND f2.fact_type = 'preference'
                AND f2.status != 'archived'
                AND f2.scope = 'project'
                AND COALESCE(f2.suspicious, 0) = 0
          )
        GROUP BY f.content
        HAVING COUNT(DISTINCT f.project_id) >= 2
        ORDER BY project_count DESC, max_confidence DESC
        LIMIT ?2
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(
        rusqlite::params![current_project_id, limit as i64],
        |row| {
            Ok(CrossProjectPreference {
                content: row.get(0)?,
                projects: row.get(1)?,
                project_count: row.get(2)?,
                max_confidence: row.get(3)?,
            })
        },
    )?;

    rows.collect()
}

/// Format cross-project memories for display in session recap or hook context.
///
/// Uses "You solved this in Project X" format ONLY for tight matches
/// (distance < 0.25 AND decision/pattern type). All other matches use
/// the generic "From Project X" format to avoid overstating confidence.
pub fn format_cross_project_context(memories: &[CrossProjectMemory]) -> String {
    if memories.is_empty() {
        return String::new();
    }

    let lines: Vec<String> = memories
        .iter()
        .map(|m| {
            let truncated = truncate_at_boundary(&m.content, 200);
            let is_solved = m.distance < SOLVED_DISTANCE_THRESHOLD
                && (m.fact_type == "decision" || m.fact_type == "pattern");
            if is_solved {
                format!(
                    "[Mira/cross-project] You solved this in {}: \"{}\"",
                    m.project_name, truncated
                )
            } else {
                format!(
                    "[Mira/cross-project] From {}: {}",
                    m.project_name, truncated
                )
            }
        })
        .collect();

    lines.join("\n")
}

/// Format cross-project preferences for session recap.
pub fn format_cross_project_preferences(prefs: &[CrossProjectPreference]) -> String {
    if prefs.is_empty() {
        return String::new();
    }

    let lines: Vec<String> = prefs
        .iter()
        .map(|p| {
            let truncated = truncate_at_boundary(&p.content, 200);
            format!(
                "• {} (used in {} projects: {})",
                truncated, p.project_count, p.projects
            )
        })
        .collect();

    format!("Cross-project patterns:\n{}", lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_cross_project_context_empty() {
        assert_eq!(format_cross_project_context(&[]), "");
    }

    #[test]
    fn test_format_cross_project_context_tight_decision() {
        // Tight match (distance < 0.25) + decision → "You solved this"
        let memories = vec![CrossProjectMemory {
            fact_id: 1,
            content: "Use builder pattern for config".to_string(),
            fact_type: "decision".to_string(),
            category: Some("patterns".to_string()),
            project_name: "ProjectAlpha".to_string(),
            project_id: 42,
            distance: 0.2,
        }];
        let result = format_cross_project_context(&memories);
        assert!(result.contains("You solved this in ProjectAlpha"));
        assert!(result.contains("Use builder pattern for config"));
    }

    #[test]
    fn test_format_cross_project_context_loose_decision_no_solved_label() {
        // Loose match (distance 0.3) + decision → generic "From" format, NOT "You solved this"
        let memories = vec![CrossProjectMemory {
            fact_id: 1,
            content: "Use builder pattern for config".to_string(),
            fact_type: "decision".to_string(),
            category: Some("patterns".to_string()),
            project_name: "ProjectAlpha".to_string(),
            project_id: 42,
            distance: 0.3,
        }];
        let result = format_cross_project_context(&memories);
        assert!(
            !result.contains("You solved this"),
            "Loose match should not use 'You solved this' label"
        );
        assert!(result.contains("From ProjectAlpha"));
    }

    #[test]
    fn test_format_cross_project_context_general() {
        let memories = vec![CrossProjectMemory {
            fact_id: 2,
            content: "SQLite with connection pooling".to_string(),
            fact_type: "context".to_string(),
            category: None,
            project_name: "ProjectBeta".to_string(),
            project_id: 43,
            distance: 0.1,
        }];
        let result = format_cross_project_context(&memories);
        assert!(result.contains("From ProjectBeta"));
        assert!(result.contains("SQLite with connection pooling"));
    }

    #[test]
    fn test_format_cross_project_context_multibyte_utf8_truncation() {
        // Content > 200 bytes with multibyte chars forces truncation through
        // truncate_at_boundary. This must not panic on char boundaries.
        let long_unicode = "日本語".repeat(30); // 30 * 9 bytes = 270 bytes, exceeds 200
        assert!(long_unicode.len() > 200, "test content must exceed 200 bytes");
        let memories = vec![CrossProjectMemory {
            fact_id: 3,
            content: long_unicode,
            fact_type: "context".to_string(),
            category: None,
            project_name: "Unicode".to_string(),
            project_id: 44,
            distance: 0.2,
        }];
        // Should not panic and should produce valid output
        let result = format_cross_project_context(&memories);
        assert!(result.contains("From Unicode"));
        // Truncated content should be valid UTF-8 (implicit — String wouldn't compile otherwise)
    }

    #[test]
    fn test_format_cross_project_preferences_empty() {
        assert_eq!(format_cross_project_preferences(&[]), "");
    }

    #[test]
    fn test_format_cross_project_preferences() {
        let prefs = vec![CrossProjectPreference {
            content: "Use debug builds during development".to_string(),
            projects: "Mira,ProjectAlpha".to_string(),
            project_count: 2,
            max_confidence: 0.9,
        }];
        let result = format_cross_project_preferences(&prefs);
        assert!(result.contains("Cross-project patterns:"));
        assert!(result.contains("Use debug builds during development"));
        assert!(result.contains("2 projects"));
        assert!(result.contains("Mira,ProjectAlpha"));
    }
}
