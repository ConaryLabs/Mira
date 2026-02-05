// crates/mira-server/src/cross_project/preferences.rs
// Opt-in sharing configuration for cross-project intelligence

use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::{AnonymizationLevel, CrossPatternType};

/// Sharing preferences for a project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharingPreferences {
    pub project_id: i64,
    /// Master switch for cross-project sharing
    pub sharing_enabled: bool,
    /// Allow exporting patterns from this project
    pub export_patterns: bool,
    /// Allow importing patterns to this project
    pub import_patterns: bool,
    /// Minimum anonymization level required for exports
    pub min_anonymization_level: AnonymizationLevel,
    /// Pattern types allowed for sharing (None = all)
    pub allowed_pattern_types: Option<Vec<CrossPatternType>>,
    /// Total differential privacy budget
    pub privacy_epsilon_budget: f64,
    /// Privacy budget already consumed
    pub privacy_epsilon_used: f64,
}

impl Default for SharingPreferences {
    fn default() -> Self {
        Self {
            project_id: 0,
            sharing_enabled: false, // Opt-in by default
            export_patterns: false,
            import_patterns: true, // Can receive but not send by default
            min_anonymization_level: AnonymizationLevel::Full,
            allowed_pattern_types: None,
            privacy_epsilon_budget: 1.0,
            privacy_epsilon_used: 0.0,
        }
    }
}

impl SharingPreferences {
    /// Check if a specific pattern type is allowed
    pub fn is_pattern_type_allowed(&self, pattern_type: CrossPatternType) -> bool {
        match &self.allowed_pattern_types {
            Some(types) => types.contains(&pattern_type),
            None => true, // All types allowed if not specified
        }
    }

    /// Check if there's remaining privacy budget
    pub fn has_privacy_budget(&self, required: f64) -> bool {
        self.privacy_epsilon_used + required <= self.privacy_epsilon_budget
    }

    /// Get remaining privacy budget
    pub fn remaining_privacy_budget(&self) -> f64 {
        (self.privacy_epsilon_budget - self.privacy_epsilon_used).max(0.0)
    }

    /// Check if export is allowed for a pattern type
    pub fn can_export(&self, pattern_type: CrossPatternType, epsilon_cost: f64) -> bool {
        self.sharing_enabled
            && self.export_patterns
            && self.is_pattern_type_allowed(pattern_type)
            && self.has_privacy_budget(epsilon_cost)
    }

    /// Check if import is allowed for a pattern type
    pub fn can_import(&self, pattern_type: CrossPatternType) -> bool {
        self.sharing_enabled && self.import_patterns && self.is_pattern_type_allowed(pattern_type)
    }
}

/// Get sharing preferences for a project
pub fn get_preferences(conn: &Connection, project_id: i64) -> Result<SharingPreferences> {
    let sql = r#"
        SELECT sharing_enabled, export_patterns, import_patterns,
               min_anonymization_level, allowed_pattern_types,
               privacy_epsilon_budget, privacy_epsilon_used
        FROM cross_project_preferences
        WHERE project_id = ?
    "#;

    let result = conn.query_row(sql, [project_id], |row| {
        let level_str: String = row.get(3)?;
        let types_json: Option<String> = row.get(4)?;

        let allowed_types = types_json
            .and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok())
            .map(|types| {
                types
                    .iter()
                    .filter_map(|s| s.parse::<CrossPatternType>().ok())
                    .collect()
            });

        Ok(SharingPreferences {
            project_id,
            sharing_enabled: row.get::<_, i32>(0)? != 0,
            export_patterns: row.get::<_, i32>(1)? != 0,
            import_patterns: row.get::<_, i32>(2)? != 0,
            min_anonymization_level: level_str.parse::<AnonymizationLevel>()
                .unwrap_or(AnonymizationLevel::Full),
            allowed_pattern_types: allowed_types,
            privacy_epsilon_budget: row.get(5)?,
            privacy_epsilon_used: row.get(6)?,
        })
    });

    match result {
        Ok(prefs) => Ok(prefs),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            // Return default preferences if not configured
            Ok(SharingPreferences {
                project_id,
                ..Default::default()
            })
        }
        Err(e) => Err(e.into()),
    }
}

/// Update sharing preferences for a project
pub fn update_preferences(conn: &Connection, prefs: &SharingPreferences) -> Result<()> {
    let types_json = prefs.allowed_pattern_types.as_ref().map(|types| {
        serde_json::to_string(&types.iter().map(|t| t.as_str()).collect::<Vec<_>>())
            .unwrap_or_default()
    });

    let sql = r#"
        INSERT INTO cross_project_preferences
        (project_id, sharing_enabled, export_patterns, import_patterns,
         min_anonymization_level, allowed_pattern_types,
         privacy_epsilon_budget, privacy_epsilon_used, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))
        ON CONFLICT(project_id) DO UPDATE SET
            sharing_enabled = excluded.sharing_enabled,
            export_patterns = excluded.export_patterns,
            import_patterns = excluded.import_patterns,
            min_anonymization_level = excluded.min_anonymization_level,
            allowed_pattern_types = excluded.allowed_pattern_types,
            privacy_epsilon_budget = excluded.privacy_epsilon_budget,
            privacy_epsilon_used = excluded.privacy_epsilon_used,
            updated_at = datetime('now')
    "#;

    conn.execute(
        sql,
        rusqlite::params![
            prefs.project_id,
            prefs.sharing_enabled as i32,
            prefs.export_patterns as i32,
            prefs.import_patterns as i32,
            prefs.min_anonymization_level.as_str(),
            types_json,
            prefs.privacy_epsilon_budget,
            prefs.privacy_epsilon_used,
        ],
    )?;

    Ok(())
}

/// Consume privacy budget for an export
pub fn consume_privacy_budget(conn: &Connection, project_id: i64, epsilon: f64) -> Result<bool> {
    let prefs = get_preferences(conn, project_id)?;

    if !prefs.has_privacy_budget(epsilon) {
        return Ok(false);
    }

    let sql = r#"
        UPDATE cross_project_preferences
        SET privacy_epsilon_used = privacy_epsilon_used + ?,
            updated_at = datetime('now')
        WHERE project_id = ?
    "#;

    conn.execute(sql, rusqlite::params![epsilon, project_id])?;
    Ok(true)
}

/// Reset privacy budget (e.g., at the start of a new period)
pub fn reset_privacy_budget(conn: &Connection, project_id: i64) -> Result<()> {
    let sql = r#"
        UPDATE cross_project_preferences
        SET privacy_epsilon_used = 0.0,
            updated_at = datetime('now')
        WHERE project_id = ?
    "#;

    conn.execute(sql, [project_id])?;
    Ok(())
}

/// Enable sharing for a project (convenience function)
pub fn enable_sharing(
    conn: &Connection,
    project_id: i64,
    export: bool,
    import: bool,
) -> Result<()> {
    let mut prefs = get_preferences(conn, project_id)?;
    prefs.sharing_enabled = true;
    prefs.export_patterns = export;
    prefs.import_patterns = import;
    update_preferences(conn, &prefs)
}

/// Disable sharing for a project
pub fn disable_sharing(conn: &Connection, project_id: i64) -> Result<()> {
    let mut prefs = get_preferences(conn, project_id)?;
    prefs.sharing_enabled = false;
    prefs.export_patterns = false;
    update_preferences(conn, &prefs)
}
