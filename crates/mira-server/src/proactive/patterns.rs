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

    conn.execute(sql, rusqlite::params![
        pattern.project_id,
        pattern.pattern_type.as_str(),
        &pattern.pattern_key,
        pattern.pattern_data.to_json(),
        pattern.confidence,
        pattern.occurrence_count,
    ])?;

    Ok(conn.last_insert_rowid())
}

/// Get patterns for a project by type
pub fn get_patterns_by_type(conn: &Connection, project_id: i64, pattern_type: &PatternType, limit: i64) -> Result<Vec<BehaviorPattern>> {
    let sql = r#"
        SELECT id, project_id, pattern_type, pattern_key, pattern_data, confidence, occurrence_count
        FROM behavior_patterns
        WHERE project_id = ? AND pattern_type = ?
        ORDER BY confidence DESC, occurrence_count DESC
        LIMIT ?
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(rusqlite::params![project_id, pattern_type.as_str(), limit], |row| {
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
        let (id, project_id, pattern_type_str, pattern_key, pattern_data_str, confidence, occurrence_count) = row;

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
pub fn get_high_confidence_patterns(conn: &Connection, project_id: i64, min_confidence: f64) -> Result<Vec<BehaviorPattern>> {
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
        let (id, project_id, pattern_type_str, pattern_key, pattern_data_str, confidence, occurrence_count) = row;

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

/// Mine file sequence patterns from behavior logs
pub fn mine_file_sequence_patterns(conn: &Connection, project_id: i64, min_occurrences: i64) -> Result<Vec<BehaviorPattern>> {
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
pub fn mine_tool_chain_patterns(conn: &Connection, project_id: i64, min_occurrences: i64) -> Result<Vec<BehaviorPattern>> {
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

        let pattern_key = format!("{}->{}",  &tool_a, &tool_b);
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
pub fn update_pattern_confidence(conn: &Connection, pattern_id: i64, feedback_multiplier: f64) -> Result<()> {
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

    Ok(patterns_stored)
}
