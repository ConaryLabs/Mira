// db/patterns.rs
// Behavior pattern types and DB operations for change intelligence.
//
// Moved from proactive/patterns.rs during proactive system removal.
// These types are used by: diff_analysis impact, pre_tool hook warnings,
// insights system, and pondering.

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// Pattern types for behavior analysis
#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, strum::IntoStaticStr, strum::EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PatternType {
    ChangePattern, // Recurring code change patterns correlated with outcomes
}

impl PatternType {
    pub fn as_str(&self) -> &'static str {
        self.into()
    }
}

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
            occurrence_count = excluded.occurrence_count,
            last_triggered_at = datetime('now'),
            updated_at = datetime('now')
        RETURNING id
    "#;

    let id = conn.query_row(
        sql,
        rusqlite::params![
            pattern.project_id,
            pattern.pattern_type.as_str(),
            &pattern.pattern_key,
            pattern.pattern_data.to_json(),
            pattern.confidence,
            pattern.occurrence_count,
        ],
        |row| row.get(0),
    )?;

    Ok(id)
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
    for row in rows.filter_map(crate::db::log_and_discard) {
        let (
            id,
            project_id,
            pattern_type_str,
            pattern_key,
            pattern_data_str,
            confidence,
            occurrence_count,
        ) = row;

        if let (Ok(pattern_type), Some(pattern_data)) = (
            pattern_type_str.parse::<PatternType>(),
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

#[cfg(test)]
mod tests {
    use super::*;

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

        let parsed = PatternData::from_json(&json).unwrap();
        let PatternData::ChangePattern {
            files,
            module,
            pattern_subtype,
            outcome_stats,
            sample_commits,
        } = parsed;
        assert_eq!(files.len(), 2);
        assert_eq!(module, Some("src".to_string()));
        assert_eq!(pattern_subtype, "co_change_gap");
        assert_eq!(outcome_stats.total, 10);
        assert_eq!(outcome_stats.reverted, 2);
        assert_eq!(outcome_stats.follow_up_fix, 4);
        assert_eq!(sample_commits.len(), 1);
    }

    #[test]
    fn test_pattern_data_from_json_invalid() {
        assert!(PatternData::from_json("not valid json").is_none());
        assert!(PatternData::from_json("{}").is_none());
        assert!(PatternData::from_json("").is_none());
    }

    #[test]
    fn test_pattern_type_roundtrip() {
        let pattern = PatternType::ChangePattern;
        let s = pattern.as_str();
        let parsed: PatternType = s.parse().unwrap();
        assert_eq!(parsed, pattern);
    }
}
