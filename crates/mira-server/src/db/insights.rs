// db/insights.rs
// Unified insights digest — merges pondering, proactive suggestions, and doc gaps

use rusqlite::{Connection, params};

use super::types::UnifiedInsight;

/// Query-time merge of all insight sources into a single ranked list.
pub fn get_unified_insights_sync(
    conn: &Connection,
    project_id: i64,
    filter_source: Option<&str>,
    min_confidence: f64,
    days_back: i64,
    limit: usize,
) -> rusqlite::Result<Vec<UnifiedInsight>> {
    let mut all = Vec::new();

    let include = |src: &str| filter_source.is_none() || filter_source == Some(src);

    if include("pondering") {
        all.extend(fetch_pondering_insights(conn, project_id, days_back)?);
    }
    if include("proactive") {
        all.extend(fetch_proactive_insights(conn, project_id, days_back)?);
    }
    if include("doc_gap") {
        all.extend(fetch_doc_gap_insights(conn, project_id)?);
    }

    // Filter by min_confidence, sort by priority_score desc then timestamp desc
    all.retain(|i| i.priority_score >= min_confidence);
    all.sort_by(|a, b| {
        b.priority_score
            .partial_cmp(&a.priority_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.timestamp.cmp(&a.timestamp))
    });
    all.truncate(limit);

    Ok(all)
}

/// Pondering insights from behavior_patterns where pattern_type starts with 'insight_'
fn fetch_pondering_insights(
    conn: &Connection,
    project_id: i64,
    days_back: i64,
) -> rusqlite::Result<Vec<UnifiedInsight>> {
    let mut stmt = conn.prepare(
        r#"SELECT pattern_type, pattern_data, confidence, last_triggered_at
           FROM behavior_patterns
           WHERE project_id = ?1
             AND pattern_type LIKE 'insight_%'
             AND last_triggered_at > datetime('now', '-' || ?2 || ' days')
           ORDER BY last_triggered_at DESC"#,
    )?;

    let rows = stmt.query_map(params![project_id, days_back], |row| {
        let pattern_type: String = row.get(0)?;
        let pattern_data: String = row.get(1)?;
        let confidence: f64 = row.get(2)?;
        let timestamp: String = row.get::<_, Option<String>>(3)?.unwrap_or_default();
        Ok((pattern_type, pattern_data, confidence, timestamp))
    })?;

    let mut insights = Vec::new();
    for row in rows {
        let (pattern_type, pattern_data, confidence, timestamp) = row?;

        // Extract description and evidence from JSON
        let (description, evidence) =
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&pattern_data) {
                let desc = data
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or(&pattern_data)
                    .to_string();
                let ev = data
                    .get("evidence")
                    .and_then(|e| e.as_str())
                    .map(String::from);
                (desc, ev)
            } else {
                (pattern_data, None)
            };

        // Type weight: friction=1.0, tool_chain=0.8, workflow=0.7, focus_area=0.6
        let type_weight = match pattern_type.as_str() {
            "insight_friction" => 1.0,
            "insight_tool_chain" => 0.8,
            "insight_workflow" => 0.7,
            "insight_focus_area" => 0.6,
            _ => 0.5,
        };

        insights.push(UnifiedInsight {
            source: "pondering".to_string(),
            source_type: pattern_type,
            description,
            priority_score: confidence * type_weight,
            confidence,
            timestamp,
            evidence,
        });
    }

    Ok(insights)
}

/// Proactive suggestions that haven't expired
fn fetch_proactive_insights(
    conn: &Connection,
    project_id: i64,
    days_back: i64,
) -> rusqlite::Result<Vec<UnifiedInsight>> {
    let mut stmt = conn.prepare(
        r#"SELECT trigger_key, suggestion_text, confidence, created_at
           FROM proactive_suggestions
           WHERE project_id = ?1
             AND (expires_at IS NULL OR expires_at > datetime('now'))
             AND created_at > datetime('now', '-' || ?2 || ' days')
           ORDER BY confidence DESC"#,
    )?;

    let rows = stmt.query_map(params![project_id, days_back], |row| {
        let trigger_key: String = row.get(0)?;
        let suggestion_text: String = row.get(1)?;
        let confidence: f64 = row.get::<_, Option<f64>>(2)?.unwrap_or(0.5);
        let timestamp: String = row.get::<_, Option<String>>(3)?.unwrap_or_default();
        Ok((trigger_key, suggestion_text, confidence, timestamp))
    })?;

    let mut insights = Vec::new();
    for row in rows {
        let (trigger_key, suggestion_text, confidence, timestamp) = row?;
        insights.push(UnifiedInsight {
            source: "proactive".to_string(),
            source_type: trigger_key,
            description: suggestion_text,
            priority_score: confidence * 0.9,
            confidence,
            timestamp,
            evidence: None,
        });
    }

    Ok(insights)
}

/// Documentation gaps (pending tasks)
fn fetch_doc_gap_insights(
    conn: &Connection,
    project_id: i64,
) -> rusqlite::Result<Vec<UnifiedInsight>> {
    let sql = format!(
        "SELECT doc_type, doc_category, target_doc_path, priority, reason, created_at
           FROM documentation_tasks
           WHERE project_id = ?1
             AND status = 'pending'
           ORDER BY {}",
        super::PRIORITY_ORDER_SQL
    );
    let mut stmt = conn.prepare(&sql)?;

    let rows = stmt.query_map(params![project_id], |row| {
        let doc_type: String = row.get(0)?;
        let doc_category: String = row.get(1)?;
        let target_doc_path: String = row.get(2)?;
        let priority: String = row.get::<_, Option<String>>(3)?.unwrap_or("medium".into());
        let reason: Option<String> = row.get(4)?;
        let timestamp: String = row.get::<_, Option<String>>(5)?.unwrap_or_default();
        Ok((
            doc_type,
            doc_category,
            target_doc_path,
            priority,
            reason,
            timestamp,
        ))
    })?;

    let mut insights = Vec::new();
    for row in rows {
        let (doc_type, doc_category, target_doc_path, priority, reason, timestamp) = row?;

        let priority_score = super::priority_score(&priority);

        let description = format!(
            "Missing {} documentation: {} ({})",
            doc_category, target_doc_path, doc_type
        );

        insights.push(UnifiedInsight {
            source: "doc_gap".to_string(),
            source_type: format!("{}:{}", doc_type, doc_category),
            description,
            priority_score,
            confidence: priority_score,
            timestamp,
            evidence: reason,
        });
    }

    Ok(insights)
}
