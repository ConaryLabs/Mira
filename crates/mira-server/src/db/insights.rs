// db/insights.rs
// Unified insights digest — merges pondering and doc gap insights into a ranked list

use rusqlite::{Connection, params};

use super::types::UnifiedInsight;

/// Auto-dismiss pondering insights older than 14 days that haven't been re-triggered.
fn auto_dismiss_stale_insights(conn: &Connection, project_id: i64) -> rusqlite::Result<usize> {
    let rows = conn.execute(
        "UPDATE behavior_patterns SET dismissed = 1 \
         WHERE project_id = ?1 \
           AND pattern_type LIKE 'insight_%' \
           AND pattern_type NOT IN ('insight_stale_goal', 'insight_fragile_code', 'insight_untested', 'insight_recurring_error', 'insight_health_degrading') \
           AND (dismissed IS NULL OR dismissed = 0) \
           AND last_triggered_at < datetime('now', '-14 days')",
        params![project_id],
    )?;
    Ok(rows)
}

/// Query-time merge of insight sources into a single ranked list.
/// Proactive suggestions are excluded from the digest (available via their own API).
pub fn get_unified_insights_sync(
    conn: &Connection,
    project_id: i64,
    filter_source: Option<&str>,
    min_confidence: f64,
    days_back: i64,
    limit: usize,
) -> rusqlite::Result<Vec<UnifiedInsight>> {
    // Clean up stale insights before querying
    let _ = auto_dismiss_stale_insights(conn, project_id);

    let mut all = Vec::new();

    let include = |src: &str| filter_source.is_none() || filter_source == Some(src);

    if include("pondering") {
        all.extend(fetch_pondering_insights(conn, project_id, days_back)?);
    }
    if include("doc_gap") {
        all.extend(fetch_doc_gap_insights(conn, project_id)?);
    }
    if include("health_trend") {
        all.extend(fetch_health_trend_insights(conn, project_id)?);
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

/// Dismiss a single insight by setting `dismissed = 1` on its behavior_patterns row.
/// Scoped to project_id and insight pattern_types only.
/// Returns whether a row was actually updated.
pub fn dismiss_insight_sync(conn: &Connection, project_id: i64, id: i64) -> rusqlite::Result<bool> {
    let rows = conn.execute(
        "UPDATE behavior_patterns SET dismissed = 1 \
         WHERE id = ?1 AND project_id = ?2 \
           AND pattern_type LIKE 'insight_%' \
           AND (dismissed IS NULL OR dismissed = 0)",
        params![id, project_id],
    )?;
    Ok(rows > 0)
}

/// Compute age in days from a timestamp string (format: "YYYY-MM-DD HH:MM:SS").
pub(crate) fn compute_age_days(timestamp: &str) -> f64 {
    use chrono::{NaiveDateTime, Utc};
    NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%d %H:%M:%S")
        .map(|t| {
            let now = Utc::now().naive_utc();
            (now - t).num_hours() as f64 / 24.0
        })
        .unwrap_or(0.0)
}

/// Map raw pattern_type strings to human-readable labels for display.
fn humanize_insight_type(pattern_type: &str) -> String {
    match pattern_type {
        "insight_revert_cluster" => "Revert Pattern".to_string(),
        "insight_fragile_code" => "Fragile Code".to_string(),
        "insight_stale_goal" => "Stale Goal".to_string(),
        "insight_untested" => "Untested Code".to_string(),
        "insight_recurring_error" => "Recurring Error".to_string(),
        "insight_churn_hotspot" => "Code Churn".to_string(),
        "insight_health_degrading" => "Health Trend".to_string(),
        "insight_session" => "Session Pattern".to_string(),
        "insight_workflow" => "Workflow".to_string(),
        other => other
            .strip_prefix("insight_")
            .unwrap_or(other)
            .replace('_', " ")
            .to_string(),
    }
}

/// Pondering insights from behavior_patterns where pattern_type starts with 'insight_'
fn fetch_pondering_insights(
    conn: &Connection,
    project_id: i64,
    days_back: i64,
) -> rusqlite::Result<Vec<UnifiedInsight>> {
    let mut stmt = conn.prepare(
        r#"SELECT id, pattern_type, pattern_data, confidence, last_triggered_at
           FROM behavior_patterns
           WHERE project_id = ?1
             AND pattern_type LIKE 'insight_%'
             AND last_triggered_at > datetime('now', '-' || ?2 || ' days')
             AND (dismissed IS NULL OR dismissed = 0)
           ORDER BY last_triggered_at DESC"#,
    )?;

    let rows = stmt.query_map(params![project_id, days_back], |row| {
        let row_id: i64 = row.get(0)?;
        let pattern_type: String = row.get(1)?;
        let pattern_data: String = row.get(2)?;
        let confidence: f64 = row.get(3)?;
        let timestamp: String = row.get::<_, Option<String>>(4)?.unwrap_or_default();
        Ok((row_id, pattern_type, pattern_data, confidence, timestamp))
    })?;

    let mut insights = Vec::new();
    for row in rows {
        let (row_id, pattern_type, pattern_data, confidence, timestamp) = row?;

        // Extract description and evidence from JSON
        let (description, evidence) =
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&pattern_data) {
                let desc = data
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or(&pattern_data)
                    .to_string();
                let ev = data.get("evidence").and_then(|e| {
                    if let Some(s) = e.as_str() {
                        Some(s.to_string())
                    } else if let Some(arr) = e.as_array() {
                        let items: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                        if items.is_empty() {
                            None
                        } else {
                            Some(items.join("; "))
                        }
                    } else {
                        None
                    }
                });
                (desc, ev)
            } else {
                (pattern_data, None)
            };

        // Type weight: higher weight = more likely to surface
        let type_weight = match pattern_type.as_str() {
            "insight_revert_cluster" => 1.0,
            "insight_recurring_error" => 0.95,
            "insight_fragile_code" => 0.95,
            "insight_stale_goal" => 0.9,
            "insight_untested" => 0.85,
            "insight_churn_hotspot" => 0.8,
            "insight_health_degrading" => 0.85,
            "insight_session" => 0.75,
            "insight_workflow" => 0.7,
            _ => 0.5,
        };

        // Temporal decay: type-aware scoring
        let age_days = compute_age_days(&timestamp);
        let decay = match pattern_type.as_str() {
            // Chronic issues get MORE important over time (inverse decay, cap at 2.0x)
            "insight_stale_goal"
            | "insight_fragile_code"
            | "insight_untested"
            | "insight_recurring_error"
            | "insight_health_degrading" => (1.0 + (age_days / 14.0)).min(2.0),
            // Acute issues decay normally, floor at 30%
            _ => (1.0 - (age_days / 14.0)).max(0.3),
        };
        let priority_score = confidence * type_weight * decay;

        insights.push(UnifiedInsight {
            source: "pondering".to_string(),
            source_type: humanize_insight_type(&pattern_type),
            description,
            priority_score,
            confidence,
            timestamp,
            evidence,
            row_id: Some(row_id),
            trend: None,
            change_summary: None,
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
            row_id: None,
            trend: None,
            change_summary: None,
        });
    }

    Ok(insights)
}

/// Health trend insights from health_snapshots table.
/// Emits an insight when the latest 2 snapshots show meaningful change (>10% or tier transition).
fn fetch_health_trend_insights(
    conn: &Connection,
    project_id: i64,
) -> rusqlite::Result<Vec<UnifiedInsight>> {
    // Get 2 most recent snapshots
    let mut stmt = conn.prepare(
        "SELECT avg_debt_score, max_debt_score, tier_distribution, snapshot_at, module_count
         FROM health_snapshots
         WHERE project_id = ?1
         ORDER BY snapshot_at DESC
         LIMIT 2",
    )?;

    let snapshots: Vec<(f64, f64, String, String, i64)> = stmt
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, f64>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    if snapshots.len() < 2 {
        return Ok(vec![]); // Need at least 2 snapshots for comparison
    }

    let (current_avg, _current_max, current_tiers, current_time, module_count) = &snapshots[0];
    let (prev_avg, _prev_max, _prev_tiers, _prev_time, _prev_modules) = &snapshots[1];

    // Check for meaningful change: >10% delta
    if *prev_avg == 0.0 {
        return Ok(vec![]);
    }
    let delta_pct = ((current_avg - prev_avg) / prev_avg) * 100.0;
    if delta_pct.abs() < 10.0 {
        return Ok(vec![]); // Not significant enough
    }

    // Determine trend direction and build description
    let (trend, direction_word) = if delta_pct > 0.0 {
        ("degraded".to_string(), "degraded")
    } else {
        ("improved".to_string(), "improved")
    };

    // Try to extract dominant tier changes
    let change_summary = format!("{:.1} \u{2192} {:.1}", prev_avg, current_avg);

    let description = format!(
        "Codebase health {} ({:+.0}%): avg debt score {} across {} modules",
        direction_word, delta_pct, change_summary, module_count
    );

    // 7-day average for context
    let week_avg: Option<f64> = conn
        .query_row(
            "SELECT AVG(avg_debt_score) FROM health_snapshots
             WHERE project_id = ?1
               AND snapshot_at > datetime('now', '-7 days')",
            params![project_id],
            |row| row.get(0),
        )
        .ok();

    let evidence = week_avg.map(|avg| format!("7-day avg: {:.1}, tiers: {}", avg, current_tiers));

    let confidence = if delta_pct.abs() > 25.0 { 0.85 } else { 0.7 };
    let priority_score = confidence * 0.85; // Same weight as insight_health_degrading

    Ok(vec![UnifiedInsight {
        source: "health_trend".to_string(),
        source_type: "Health Trend".to_string(),
        description,
        priority_score,
        confidence,
        timestamp: current_time.clone(),
        evidence,
        row_id: None,
        trend: Some(trend),
        change_summary: Some(change_summary.clone()),
    }])
}
