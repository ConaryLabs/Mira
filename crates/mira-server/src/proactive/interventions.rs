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
    let last_intervention: Option<String> = match conn.query_row(
        r#"SELECT created_at FROM proactive_interventions
               WHERE project_id = ?
               ORDER BY created_at DESC LIMIT 1"#,
        params![project_id],
        |row| row.get(0),
    ) {
        Ok(v) => Some(v),
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(e) => {
            tracing::debug!("Failed to record intervention: {e}");
            None
        }
    };

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
    for row in rows.filter_map(crate::db::log_and_discard) {
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

    for row in stale_rows.filter_map(crate::db::log_and_discard) {
        let (doc_path, summary) = row;

        // Skip if the doc file no longer exists on disk
        if let Some(base) = project_path
            && !Path::new(base).join(&doc_path).exists()
        {
            tracing::debug!("Skipping stale doc intervention: file gone: {}", doc_path);
            continue;
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

    for row in pending_rows.filter_map(crate::db::log_and_discard) {
        let (target_path, source_path, category) = row;

        // Skip if the source file no longer exists on disk
        if let Some(base) = project_path
            && let Some(ref src) = source_path
            && !Path::new(base).join(src).exists()
        {
            tracing::debug!("Skipping missing doc intervention: source gone: {}", src);
            continue;
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

    // Also mark the underlying insight as shown so the status line's "new" count
    // stays consistent with what was surfaced via hooks.
    if let Some(pid) = intervention.pattern_id {
        let _ = conn.execute(
            "UPDATE behavior_patterns SET shown_count = COALESCE(shown_count, 0) + 1 WHERE id = ?",
            params![pid],
        );
    }

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

/// Extract description from pattern_data JSON.
/// Tries explicit "description" field first (pondering insights), then falls back
/// to generating a human-readable summary from the structured PatternData.
fn extract_description(pattern_data: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(pattern_data).ok()?;

    // Pondering insights have an explicit description
    if let Some(desc) = value.get("description").and_then(|d| d.as_str()) {
        return Some(desc.to_string());
    }

    // Mined patterns — generate a summary from the structured data
    use super::patterns::PatternData;
    let data = PatternData::from_json(pattern_data)?;
    Some(summarize_pattern(&data))
}

/// Generate a concise human-readable summary from structured pattern data.
fn summarize_pattern(data: &super::patterns::PatternData) -> String {
    use super::patterns::PatternData;
    match data {
        PatternData::FileSequence { files, .. } => {
            let short: Vec<&str> = files.iter().map(|f| short_path(f)).collect();
            match short.len() {
                0 => "File sequence pattern detected".to_string(),
                1 => format!("Recurring file access: {}", short[0]),
                2 => format!("Files often edited together: {} and {}", short[0], short[1]),
                _ => format!(
                    "Files often edited together: {}, {}, +{} more",
                    short[0],
                    short[1],
                    short.len() - 2
                ),
            }
        }
        PatternData::ToolChain { tools, .. } => {
            if tools.len() >= 2 {
                format!("Common tool sequence: {} -> {}", tools[0], tools[1])
            } else {
                "Tool chain pattern detected".to_string()
            }
        }
        PatternData::SessionFlow { stages, .. } => {
            let preview: Vec<&str> = stages.iter().take(3).map(|s| s.as_str()).collect();
            format!("Session flow: {}", preview.join(" -> "))
        }
        PatternData::QueryPattern {
            keywords,
            query_type,
            ..
        } => {
            if keywords.is_empty() {
                format!("Recurring {} pattern", query_type)
            } else {
                format!("Recurring {} for: {}", query_type, keywords.join(", "))
            }
        }
        PatternData::ChangePattern {
            module,
            pattern_subtype,
            outcome_stats,
            ..
        } => {
            let location = module.as_deref().unwrap_or("unknown module");
            let bad = outcome_stats.reverted + outcome_stats.follow_up_fix;
            match pattern_subtype.as_str() {
                "module_hotspot" => format!(
                    "Change hotspot in {}: {}/{} changes needed follow-up",
                    location, bad, outcome_stats.total
                ),
                "co_change_gap" => format!(
                    "Co-change gap in {}: files often need coordinated edits",
                    location
                ),
                "size_risk" => format!(
                    "Large changes in {} are risky: {}/{} had issues",
                    location, bad, outcome_stats.total
                ),
                _ => format!(
                    "Change pattern in {}: {}/{} changes had issues",
                    location, bad, outcome_stats.total
                ),
            }
        }
    }
}

/// Extract the filename (or last two path components) for brevity.
fn short_path(path: &str) -> &str {
    // Show "dir/file.rs" instead of full path
    let mut parts = path.rsplitn(3, '/');
    let file = parts.next().unwrap_or(path);
    match parts.next() {
        Some(dir) => {
            // Find the start of "dir/file" in the original string
            let offset = path.len() - dir.len() - 1 - file.len();
            &path[offset..]
        }
        None => file,
    }
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
        // Pondering insight with explicit description
        let json = r#"{"description": "Test description", "evidence": []}"#;
        assert_eq!(
            extract_description(json),
            Some("Test description".to_string())
        );

        // Invalid JSON returns None
        assert_eq!(extract_description("not json"), None);

        // Empty object with no description and no valid PatternData returns None
        assert_eq!(extract_description(r#"{"evidence": []}"#), None);
    }

    #[test]
    fn test_extract_description_file_sequence() {
        let json = r#"{"type":"file_sequence","files":["src/mcp/router.rs","src/tools/core/session.rs"],"transitions":[["src/mcp/router.rs","src/tools/core/session.rs"]]}"#;
        let desc = extract_description(json).unwrap();
        assert!(desc.contains("often edited together"), "got: {desc}");
        assert!(desc.contains("router.rs"), "got: {desc}");
        assert!(desc.contains("session.rs"), "got: {desc}");
    }

    #[test]
    fn test_extract_description_tool_chain() {
        let json = r#"{"type":"tool_chain","tools":["memory","code"],"typical_args":{}}"#;
        let desc = extract_description(json).unwrap();
        assert!(desc.contains("memory"), "got: {desc}");
        assert!(desc.contains("code"), "got: {desc}");
        assert!(desc.contains("->"), "got: {desc}");
    }

    #[test]
    fn test_extract_description_change_pattern() {
        let json = r#"{"type":"change_pattern","files":["src/auth.rs"],"module":"src/auth","pattern_subtype":"module_hotspot","outcome_stats":{"total":10,"clean":6,"reverted":2,"follow_up_fix":2},"sample_commits":[]}"#;
        let desc = extract_description(json).unwrap();
        assert!(desc.contains("hotspot"), "got: {desc}");
        assert!(desc.contains("src/auth"), "got: {desc}");
        assert!(desc.contains("4/10"), "got: {desc}");
    }

    #[test]
    fn test_short_path() {
        assert_eq!(short_path("src/mcp/router.rs"), "mcp/router.rs");
        assert_eq!(short_path("router.rs"), "router.rs");
        assert_eq!(short_path("a/b/c/d.rs"), "c/d.rs");
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
