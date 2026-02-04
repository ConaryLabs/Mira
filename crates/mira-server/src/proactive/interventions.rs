// crates/mira-server/src/proactive/interventions.rs
// Intervention generation from pondering insights

use std::path::Path;

use anyhow::Result;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use super::{InterventionType, ProactiveConfig};

/// A pending intervention to show the user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingIntervention {
    pub id: Option<i64>,
    pub intervention_type: InterventionType,
    pub content: String,
    pub confidence: f64,
    pub pattern_id: Option<i64>,
    pub pattern_type: String,
}

impl PendingIntervention {
    /// Format for display to user
    pub fn format(&self) -> String {
        let icon = match self.pattern_type.as_str() {
            "friction" => "!",
            "workflow" => "*",
            "focus_area" => "@",
            "stale_doc" => "~",
            "missing_doc" => "+",
            _ => ">",
        };
        // Documentation interventions don't need confidence display
        if self.pattern_type == "stale_doc" || self.pattern_type == "missing_doc" {
            format!("[{}] {}", icon, self.content)
        } else {
            let confidence_pct = (self.confidence * 100.0) as i32;
            format!(
                "[{}] {} ({}% confidence)",
                icon, self.content, confidence_pct
            )
        }
    }
}

/// Get pending interventions from recent pondering insights
pub fn get_pending_interventions_sync(
    conn: &Connection,
    project_id: i64,
    config: &ProactiveConfig,
) -> Result<Vec<PendingIntervention>> {
    if !config.enabled {
        return Ok(vec![]);
    }

    // Check cooldown - don't show interventions too frequently
    let last_intervention: Option<String> = conn
        .query_row(
            r#"SELECT created_at FROM proactive_interventions
               WHERE project_id = ?
               ORDER BY created_at DESC LIMIT 1"#,
            params![project_id],
            |row| row.get(0),
        )
        .ok();

    if let Some(last_time) = last_intervention {
        let too_recent: bool = conn
            .query_row(
                "SELECT datetime(?) > datetime('now', '-' || ? || ' seconds')",
                params![last_time, config.cooldown_seconds],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if too_recent {
            return Ok(vec![]);
        }
    }

    // Check hourly limit
    let hourly_count: i64 = conn.query_row(
        r#"SELECT COUNT(*) FROM proactive_interventions
           WHERE project_id = ?
             AND created_at > datetime('now', '-1 hour')"#,
        params![project_id],
        |row| row.get(0),
    )?;

    if hourly_count >= config.max_interventions_per_hour as i64 {
        return Ok(vec![]);
    }

    // Get recent high-confidence pondering insights that haven't been shown
    let mut stmt = conn.prepare(
        r#"SELECT bp.id, bp.pattern_type, bp.pattern_data, bp.confidence
           FROM behavior_patterns bp
           WHERE bp.project_id = ?
             AND bp.confidence >= ?
             AND bp.last_triggered_at > datetime('now', '-7 days')
             AND NOT EXISTS (
                 SELECT 1 FROM proactive_interventions pi
                 WHERE pi.trigger_pattern_id = bp.id
                   AND pi.created_at > datetime('now', '-24 hours')
             )
           ORDER BY bp.confidence DESC, bp.last_triggered_at DESC
           LIMIT 3"#,
    )?;

    let rows = stmt.query_map(params![project_id, config.min_confidence], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, f64>(3)?,
        ))
    })?;

    let mut interventions = Vec::new();
    for row in rows.flatten() {
        let (pattern_id, pattern_type, pattern_data, confidence) = row;

        // Extract description from pattern_data JSON
        let description = extract_description(&pattern_data)
            .unwrap_or_else(|| format!("Pattern detected: {}", pattern_type));

        // Map pattern type to intervention type
        let intervention_type = match pattern_type.as_str() {
            "friction" => InterventionType::BugWarning,
            "workflow" | "tool_chain" => InterventionType::ContextPrediction,
            "focus_area" => InterventionType::ResourceSuggestion,
            _ => InterventionType::ContextPrediction,
        };

        interventions.push(PendingIntervention {
            id: None,
            intervention_type,
            content: description,
            confidence,
            pattern_id: Some(pattern_id),
            pattern_type,
        });
    }

    // Resolve project path for file existence checks
    let project_path = crate::db::get_project_info_sync(conn, project_id)
        .ok()
        .flatten()
        .map(|(_, path)| path);

    // Also get documentation interventions (stale/missing docs)
    let doc_interventions =
        get_documentation_interventions_sync(conn, project_id, project_path.as_deref())?;
    interventions.extend(doc_interventions);

    // Limit total interventions
    interventions.truncate(5);

    Ok(interventions)
}

/// Get interventions for stale or missing documentation
fn get_documentation_interventions_sync(
    conn: &Connection,
    project_id: i64,
    project_path: Option<&str>,
) -> Result<Vec<PendingIntervention>> {
    let mut interventions = Vec::new();

    // Get stale docs with SIGNIFICANT impact (LLM analyzed)
    // Only surface docs where the change actually matters
    let mut stale_stmt = conn.prepare(
        r#"SELECT doc_path, change_summary
           FROM documentation_inventory
           WHERE project_id = ?
             AND is_stale = 1
             AND change_impact = 'significant'
             AND impact_analyzed_at > datetime('now', '-2 hours')
           ORDER BY impact_analyzed_at DESC
           LIMIT 2"#,
    )?;

    let stale_rows = stale_stmt.query_map(params![project_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    })?;

    for row in stale_rows.flatten() {
        let (doc_path, summary) = row;

        // Skip if the doc file no longer exists on disk
        if let Some(base) = project_path {
            if !Path::new(base).join(&doc_path).exists() {
                tracing::debug!("Skipping stale doc intervention: file gone: {}", doc_path);
                continue;
            }
        }

        let content = if let Some(s) = summary {
            format!("`{}`: {}", doc_path, s)
        } else {
            format!("`{}` needs updating (significant API changes)", doc_path)
        };

        interventions.push(PendingIntervention {
            id: None,
            intervention_type: InterventionType::ResourceSuggestion,
            content,
            confidence: 0.95, // High confidence - LLM confirmed significant
            pattern_id: None,
            pattern_type: "stale_doc".to_string(),
        });
    }

    // Get high-priority pending doc tasks (missing docs)
    let pending_sql = format!(
        "SELECT target_doc_path, source_file_path, doc_category
           FROM documentation_tasks
           WHERE project_id = ?
             AND status = 'pending'
             AND priority IN ('high', 'urgent')
           ORDER BY {}, created_at DESC
           LIMIT 2",
        crate::db::PRIORITY_ORDER_SQL
    );
    let mut pending_stmt = conn.prepare(&pending_sql)?;

    let pending_rows = pending_stmt.query_map(params![project_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    for row in pending_rows.flatten() {
        let (target_path, source_path, category) = row;

        // Skip if the source file no longer exists on disk
        if let Some(base) = project_path {
            if let Some(ref src) = source_path {
                if !Path::new(base).join(src).exists() {
                    tracing::debug!("Skipping missing doc intervention: source gone: {}", src);
                    continue;
                }
            }
        }

        let content = if let Some(src) = source_path {
            format!("`{}` needs documentation ({})", src, category)
        } else {
            format!("`{}` needs to be written", target_path)
        };

        interventions.push(PendingIntervention {
            id: None,
            intervention_type: InterventionType::ResourceSuggestion,
            content,
            confidence: 0.85,
            pattern_id: None,
            pattern_type: "missing_doc".to_string(),
        });
    }

    Ok(interventions)
}

/// Record that an intervention was shown
pub fn record_intervention_sync(
    conn: &Connection,
    project_id: i64,
    session_id: Option<&str>,
    intervention: &PendingIntervention,
) -> Result<i64> {
    conn.execute(
        r#"INSERT INTO proactive_interventions
           (project_id, session_id, intervention_type, trigger_pattern_id,
            trigger_context, suggestion_content, confidence, created_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, datetime('now'))"#,
        params![
            project_id,
            session_id,
            intervention.intervention_type.as_str(),
            intervention.pattern_id,
            intervention.pattern_type,
            intervention.content,
            intervention.confidence,
        ],
    )?;

    Ok(conn.last_insert_rowid())
}

/// Record user response to an intervention
pub fn record_intervention_response_sync(
    conn: &Connection,
    intervention_id: i64,
    response: &str,
) -> Result<()> {
    conn.execute(
        r#"UPDATE proactive_interventions
           SET user_response = ?,
               responded_at = datetime('now')
           WHERE id = ?"#,
        params![response, intervention_id],
    )?;

    // If pattern was associated, update its confidence
    let pattern_id: Option<i64> = conn
        .query_row(
            "SELECT trigger_pattern_id FROM proactive_interventions WHERE id = ?",
            params![intervention_id],
            |row| row.get(0),
        )
        .ok()
        .flatten();

    if let Some(pid) = pattern_id {
        let multiplier = match response {
            "accepted" => 1.1,
            "acted_upon" => 1.05,
            "ignored" => 0.95,
            "dismissed" => 0.8,
            _ => 1.0,
        };

        conn.execute(
            r#"UPDATE behavior_patterns
               SET confidence = MIN(1.0, MAX(0.1, confidence * ?)),
                   updated_at = datetime('now')
               WHERE id = ?"#,
            params![multiplier, pid],
        )?;
    }

    Ok(())
}

/// Extract description from pattern_data JSON
fn extract_description(pattern_data: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(pattern_data).ok()?;
    value.get("description")?.as_str().map(|s| s.to_string())
}

/// Get interventions shown in current session
pub fn get_session_interventions_sync(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<(i64, String, Option<String>)>> {
    let mut stmt = conn.prepare(
        r#"SELECT id, suggestion_content, user_response
           FROM proactive_interventions
           WHERE session_id = ?
           ORDER BY created_at DESC"#,
    )?;

    let rows = stmt.query_map(params![session_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pending_intervention_format() {
        let intervention = PendingIntervention {
            id: None,
            intervention_type: InterventionType::BugWarning,
            content: "You often forget to run tests after editing handlers".to_string(),
            confidence: 0.85,
            pattern_id: Some(1),
            pattern_type: "friction".to_string(),
        };

        let formatted = intervention.format();
        assert!(formatted.contains("!"));
        assert!(formatted.contains("85%"));
        assert!(formatted.contains("forget to run tests"));
    }

    #[test]
    fn test_extract_description() {
        let json = r#"{"description": "Test description", "evidence": []}"#;
        assert_eq!(
            extract_description(json),
            Some("Test description".to_string())
        );

        let no_desc = r#"{"evidence": []}"#;
        assert_eq!(extract_description(no_desc), None);

        let invalid = "not json";
        assert_eq!(extract_description(invalid), None);
    }

    #[test]
    fn test_documentation_intervention_format() {
        // Stale doc - uses ~ icon, no confidence
        let stale = PendingIntervention {
            id: None,
            intervention_type: InterventionType::ResourceSuggestion,
            content: "`docs/api.md` is stale: source signatures changed".to_string(),
            confidence: 0.9,
            pattern_id: None,
            pattern_type: "stale_doc".to_string(),
        };
        let formatted = stale.format();
        assert!(formatted.contains("[~]"));
        assert!(!formatted.contains("confidence")); // No confidence display for docs

        // Missing doc - uses + icon
        let missing = PendingIntervention {
            id: None,
            intervention_type: InterventionType::ResourceSuggestion,
            content: "`src/auth.rs` needs documentation".to_string(),
            confidence: 0.85,
            pattern_id: None,
            pattern_type: "missing_doc".to_string(),
        };
        let formatted = missing.format();
        assert!(formatted.contains("[+]"));
        assert!(formatted.contains("needs documentation"));
    }
}
