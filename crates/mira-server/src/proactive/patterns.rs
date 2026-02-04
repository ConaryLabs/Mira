// crates/mira-server/src/proactive/patterns.rs
// Pattern recognition - mines behavior logs for recurring patterns

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::PatternType;

/// A recognized behavior pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorPattern {
    pub id: Option<i64>,
    pub project_id: i64,
    pub pattern_type: PatternType,
    pub pattern_key: String,
    pub pattern_data: PatternData,
    pub confidence: f64,
    pub occurrence_count: i64,
}

/// Pattern-specific data structures
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PatternData {
    FileSequence {
        files: Vec<String>,
        transitions: Vec<(String, String)>,
    },
    ToolChain {
        tools: Vec<String>,
        typical_args: HashMap<String, String>,
    },
    SessionFlow {
        stages: Vec<String>,
        typical_duration_ms: Option<i64>,
    },
    QueryPattern {
        keywords: Vec<String>,
        query_type: String,
        typical_context: Option<String>,
    },
    ChangePattern {
        /// Files involved in this change pattern
        files: Vec<String>,
        /// Module/directory this pattern applies to
        module: Option<String>,
        /// Pattern subtype: "module_hotspot", "co_change_gap", "size_risk"
        pattern_subtype: String,
        /// Outcome statistics
        outcome_stats: OutcomeStats,
        /// Sample commit hashes for reference
        sample_commits: Vec<String>,
    },
}

/// Outcome statistics for change patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeStats {
    pub total: i64,
    pub clean: i64,
    pub reverted: i64,
    pub follow_up_fix: i64,
}

impl PatternData {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from_json(s: &str) -> Option<Self> {
        serde_json::from_str(s).ok()
    }
}

/// Store or update a pattern
pub fn upsert_pattern(conn: &Connection, pattern: &BehaviorPattern) -> Result<i64> {
    let sql = r#"
        INSERT INTO behavior_patterns
        (project_id, pattern_type, pattern_key, pattern_data, confidence, occurrence_count, last_triggered_at)
        VALUES (?, ?, ?, ?, ?, ?, datetime('now'))
        ON CONFLICT(project_id, pattern_type, pattern_key) DO UPDATE SET
            pattern_data = excluded.pattern_data,
            confidence = excluded.confidence,
            occurrence_count = occurrence_count + 1,
            last_triggered_at = datetime('now'),
            updated_at = datetime('now')
    "#;

    conn.execute(
        sql,
        rusqlite::params![
            pattern.project_id,
            pattern.pattern_type.as_str(),
            &pattern.pattern_key,
            pattern.pattern_data.to_json(),
            pattern.confidence,
            pattern.occurrence_count,
        ],
    )?;

    Ok(conn.last_insert_rowid())
}

/// Get patterns for a project by type
pub fn get_patterns_by_type(
    conn: &Connection,
    project_id: i64,
    pattern_type: &PatternType,
    limit: i64,
) -> Result<Vec<BehaviorPattern>> {
    let sql = r#"
        SELECT id, project_id, pattern_type, pattern_key, pattern_data, confidence, occurrence_count
        FROM behavior_patterns
        WHERE project_id = ? AND pattern_type = ?
        ORDER BY confidence DESC, occurrence_count DESC
        LIMIT ?
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(
        rusqlite::params![project_id, pattern_type.as_str(), limit],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, f64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        },
    )?;

    let mut patterns = Vec::new();
    for row in rows.flatten() {
        let (
            id,
            project_id,
            pattern_type_str,
            pattern_key,
            pattern_data_str,
            confidence,
            occurrence_count,
        ) = row;

        if let (Some(pattern_type), Some(pattern_data)) = (
            PatternType::from_str(&pattern_type_str),
            PatternData::from_json(&pattern_data_str),
        ) {
            patterns.push(BehaviorPattern {
                id: Some(id),
                project_id,
                pattern_type,
                pattern_key,
                pattern_data,
                confidence,
                occurrence_count,
            });
        }
    }

    Ok(patterns)
}

/// Get high-confidence patterns for predictions
pub fn get_high_confidence_patterns(
    conn: &Connection,
    project_id: i64,
    min_confidence: f64,
) -> Result<Vec<BehaviorPattern>> {
    let sql = r#"
        SELECT id, project_id, pattern_type, pattern_key, pattern_data, confidence, occurrence_count
        FROM behavior_patterns
        WHERE project_id = ? AND confidence >= ?
        ORDER BY confidence DESC, occurrence_count DESC
        LIMIT 50
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(rusqlite::params![project_id, min_confidence], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, f64>(5)?,
            row.get::<_, i64>(6)?,
        ))
    })?;

    let mut patterns = Vec::new();
    for row in rows.flatten() {
        let (
            id,
            project_id,
            pattern_type_str,
            pattern_key,
            pattern_data_str,
            confidence,
            occurrence_count,
        ) = row;

        let pattern_type = PatternType::from_str(&pattern_type_str);
        let pattern_data = PatternData::from_json(&pattern_data_str);

        match (pattern_type, pattern_data) {
            (Some(pattern_type), Some(pattern_data)) => {
                patterns.push(BehaviorPattern {
                    id: Some(id),
                    project_id,
                    pattern_type,
                    pattern_key,
                    pattern_data,
                    confidence,
                    occurrence_count,
                });
            }
            (None, _) => {
                // Skip insight patterns (insight_*) silently - they're from pondering, not mining
                if !pattern_type_str.starts_with("insight_") {
                    tracing::warn!(
                        "Skipping pattern with unknown type: {} (key: {})",
                        pattern_type_str,
                        pattern_key
                    );
                }
            }
            (_, None) => {
                tracing::warn!(
                    "Skipping pattern with invalid data: type={}, data={}...",
                    pattern_type_str,
                    &pattern_data_str[..pattern_data_str.len().min(100)]
                );
            }
        }
    }

    Ok(patterns)
}

/// Mine file sequence patterns from behavior logs
pub fn mine_file_sequence_patterns(
    conn: &Connection,
    project_id: i64,
    min_occurrences: i64,
) -> Result<Vec<BehaviorPattern>> {
    // Find files that are frequently accessed together (within 5 minutes of each other)
    let sql = r#"
        WITH file_pairs AS (
            SELECT
                a.session_id,
                json_extract(a.event_data, '$.file_path') as file_a,
                json_extract(b.event_data, '$.file_path') as file_b
            FROM session_behavior_log a
            JOIN session_behavior_log b ON a.session_id = b.session_id
                AND b.sequence_position > a.sequence_position
                AND b.sequence_position <= a.sequence_position + 5
            WHERE a.project_id = ?
              AND a.event_type = 'file_access'
              AND b.event_type = 'file_access'
              AND json_extract(a.event_data, '$.file_path') IS NOT NULL
              AND json_extract(b.event_data, '$.file_path') IS NOT NULL
              AND json_extract(a.event_data, '$.file_path') != json_extract(b.event_data, '$.file_path')
        )
        SELECT file_a, file_b, COUNT(*) as pair_count
        FROM file_pairs
        GROUP BY file_a, file_b
        HAVING COUNT(*) >= ?
        ORDER BY pair_count DESC
        LIMIT 100
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([project_id, min_occurrences], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;

    let mut patterns = Vec::new();
    for row in rows.flatten() {
        let (file_a, file_b, count) = row;

        // Generate a unique key for this pair
        let pattern_key = format!("{}|{}", &file_a, &file_b);

        // Confidence based on occurrence count (normalized)
        let confidence = (count as f64 / 10.0).min(1.0);

        patterns.push(BehaviorPattern {
            id: None,
            project_id,
            pattern_type: PatternType::FileSequence,
            pattern_key,
            pattern_data: PatternData::FileSequence {
                files: vec![file_a.clone(), file_b.clone()],
                transitions: vec![(file_a, file_b)],
            },
            confidence,
            occurrence_count: count,
        });
    }

    Ok(patterns)
}

/// Mine tool chain patterns from behavior logs
pub fn mine_tool_chain_patterns(
    conn: &Connection,
    project_id: i64,
    min_occurrences: i64,
) -> Result<Vec<BehaviorPattern>> {
    // Find tools that are frequently used in sequence
    let sql = r#"
        WITH tool_pairs AS (
            SELECT
                a.session_id,
                json_extract(a.event_data, '$.tool_name') as tool_a,
                json_extract(b.event_data, '$.tool_name') as tool_b
            FROM session_behavior_log a
            JOIN session_behavior_log b ON a.session_id = b.session_id
                AND b.sequence_position = a.sequence_position + 1
            WHERE a.project_id = ?
              AND a.event_type = 'tool_use'
              AND b.event_type = 'tool_use'
              AND json_extract(a.event_data, '$.tool_name') IS NOT NULL
              AND json_extract(b.event_data, '$.tool_name') IS NOT NULL
        )
        SELECT tool_a, tool_b, COUNT(*) as pair_count
        FROM tool_pairs
        GROUP BY tool_a, tool_b
        HAVING COUNT(*) >= ?
        ORDER BY pair_count DESC
        LIMIT 50
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([project_id, min_occurrences], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;

    let mut patterns = Vec::new();
    for row in rows.flatten() {
        let (tool_a, tool_b, count) = row;

        let pattern_key = format!("{}->{}", &tool_a, &tool_b);
        let confidence = (count as f64 / 5.0).min(1.0);

        patterns.push(BehaviorPattern {
            id: None,
            project_id,
            pattern_type: PatternType::ToolChain,
            pattern_key,
            pattern_data: PatternData::ToolChain {
                tools: vec![tool_a, tool_b],
                typical_args: HashMap::new(),
            },
            confidence,
            occurrence_count: count,
        });
    }

    Ok(patterns)
}

/// Update pattern confidence based on intervention feedback
pub fn update_pattern_confidence(
    conn: &Connection,
    pattern_id: i64,
    feedback_multiplier: f64,
) -> Result<()> {
    // Adjust confidence using exponential moving average
    let sql = r#"
        UPDATE behavior_patterns
        SET confidence = confidence * 0.9 + ? * 0.1,
            updated_at = datetime('now')
        WHERE id = ?
    "#;

    conn.execute(sql, rusqlite::params![feedback_multiplier, pattern_id])?;
    Ok(())
}

/// Run pattern mining and update stored patterns
pub fn run_pattern_mining(conn: &Connection, project_id: i64) -> Result<usize> {
    let min_occurrences = 3;
    let mut patterns_stored = 0;

    // Mine file sequences
    let file_patterns = mine_file_sequence_patterns(conn, project_id, min_occurrences)?;
    for pattern in file_patterns {
        upsert_pattern(conn, &pattern)?;
        patterns_stored += 1;
    }

    // Mine tool chains
    let tool_patterns = mine_tool_chain_patterns(conn, project_id, min_occurrences)?;
    for pattern in tool_patterns {
        upsert_pattern(conn, &pattern)?;
        patterns_stored += 1;
    }

    // Mine change patterns from diff outcomes (change intelligence)
    match crate::background::change_patterns::mine_change_patterns(conn, project_id) {
        Ok(count) => {
            if count > 0 {
                tracing::debug!("Mined {} change patterns for project {}", count, project_id);
            }
            patterns_stored += count;
        }
        Err(e) => {
            tracing::warn!(
                "Change pattern mining failed for project {}: {}",
                project_id,
                e
            );
        }
    }

    Ok(patterns_stored)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════════════════════════════════════════════
    // PatternData Tests
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_pattern_data_file_sequence_json_roundtrip() {
        let data = PatternData::FileSequence {
            files: vec!["src/main.rs".to_string(), "src/lib.rs".to_string()],
            transitions: vec![("src/main.rs".to_string(), "src/lib.rs".to_string())],
        };

        let json = data.to_json();
        assert!(json.contains("file_sequence"));
        assert!(json.contains("src/main.rs"));

        let parsed = PatternData::from_json(&json).unwrap();
        if let PatternData::FileSequence { files, transitions } = parsed {
            assert_eq!(files.len(), 2);
            assert_eq!(transitions.len(), 1);
        } else {
            panic!("Wrong variant");
        }
    }

    #[test]
    fn test_pattern_data_tool_chain_json_roundtrip() {
        let mut typical_args = HashMap::new();
        typical_args.insert("verbose".to_string(), "true".to_string());

        let data = PatternData::ToolChain {
            tools: vec!["cargo".to_string(), "rustfmt".to_string()],
            typical_args,
        };

        let json = data.to_json();
        assert!(json.contains("tool_chain"));
        assert!(json.contains("cargo"));

        let parsed = PatternData::from_json(&json).unwrap();
        if let PatternData::ToolChain {
            tools,
            typical_args,
        } = parsed
        {
            assert_eq!(tools.len(), 2);
            assert_eq!(typical_args.get("verbose"), Some(&"true".to_string()));
        } else {
            panic!("Wrong variant");
        }
    }

    #[test]
    fn test_pattern_data_session_flow_json_roundtrip() {
        let data = PatternData::SessionFlow {
            stages: vec!["init".to_string(), "work".to_string(), "commit".to_string()],
            typical_duration_ms: Some(3600000),
        };

        let json = data.to_json();
        assert!(json.contains("session_flow"));

        let parsed = PatternData::from_json(&json).unwrap();
        if let PatternData::SessionFlow {
            stages,
            typical_duration_ms,
        } = parsed
        {
            assert_eq!(stages.len(), 3);
            assert_eq!(typical_duration_ms, Some(3600000));
        } else {
            panic!("Wrong variant");
        }
    }

    #[test]
    fn test_pattern_data_query_pattern_json_roundtrip() {
        let data = PatternData::QueryPattern {
            keywords: vec!["error".to_string(), "handling".to_string()],
            query_type: "search".to_string(),
            typical_context: Some("debugging session".to_string()),
        };

        let json = data.to_json();
        assert!(json.contains("query_pattern"));

        let parsed = PatternData::from_json(&json).unwrap();
        if let PatternData::QueryPattern {
            keywords,
            query_type,
            typical_context,
        } = parsed
        {
            assert_eq!(keywords.len(), 2);
            assert_eq!(query_type, "search");
            assert_eq!(typical_context, Some("debugging session".to_string()));
        } else {
            panic!("Wrong variant");
        }
    }

    #[test]
    fn test_pattern_data_change_pattern_json_roundtrip() {
        let data = PatternData::ChangePattern {
            files: vec!["src/auth.rs".to_string(), "src/middleware.rs".to_string()],
            module: Some("src".to_string()),
            pattern_subtype: "co_change_gap".to_string(),
            outcome_stats: OutcomeStats {
                total: 10,
                clean: 4,
                reverted: 2,
                follow_up_fix: 4,
            },
            sample_commits: vec!["abc123".to_string()],
        };

        let json = data.to_json();
        assert!(json.contains("change_pattern"));
        assert!(json.contains("co_change_gap"));
        assert!(json.contains("outcome_stats"));

        let parsed = PatternData::from_json(&json).unwrap();
        if let PatternData::ChangePattern {
            files,
            module,
            pattern_subtype,
            outcome_stats,
            sample_commits,
        } = parsed
        {
            assert_eq!(files.len(), 2);
            assert_eq!(module, Some("src".to_string()));
            assert_eq!(pattern_subtype, "co_change_gap");
            assert_eq!(outcome_stats.total, 10);
            assert_eq!(outcome_stats.reverted, 2);
            assert_eq!(outcome_stats.follow_up_fix, 4);
            assert_eq!(sample_commits.len(), 1);
        } else {
            panic!("Wrong variant");
        }
    }

    #[test]
    fn test_pattern_data_from_json_invalid() {
        assert!(PatternData::from_json("not valid json").is_none());
        assert!(PatternData::from_json("{}").is_none()); // Missing type tag
        assert!(PatternData::from_json("").is_none());
    }

    #[test]
    fn test_pattern_data_to_json_empty() {
        let data = PatternData::FileSequence {
            files: vec![],
            transitions: vec![],
        };
        let json = data.to_json();
        assert!(!json.is_empty());
        assert!(json.contains("file_sequence"));
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // BehaviorPattern Tests
    // ═══════════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_behavior_pattern_serialization() {
        let pattern = BehaviorPattern {
            id: Some(1),
            project_id: 42,
            pattern_type: PatternType::FileSequence,
            pattern_key: "test_key".to_string(),
            pattern_data: PatternData::FileSequence {
                files: vec!["a.rs".to_string()],
                transitions: vec![],
            },
            confidence: 0.85,
            occurrence_count: 10,
        };

        let json = serde_json::to_string(&pattern).unwrap();
        let parsed: BehaviorPattern = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.id, Some(1));
        assert_eq!(parsed.project_id, 42);
        assert_eq!(parsed.confidence, 0.85);
        assert_eq!(parsed.occurrence_count, 10);
    }
}
