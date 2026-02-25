// crates/mira-server/src/tools/core/insights.rs
// Insights tool implementation (extracted from session.rs)

use crate::db::{compute_age_days, dismiss_insight_sync, get_unified_insights_sync};
use crate::error::MiraError;
use crate::mcp::responses::Json;
use crate::mcp::responses::{InsightItem, InsightsData, SessionData, SessionOutput};
use crate::tools::core::{NO_ACTIVE_PROJECT_ERROR, ToolContext};

/// Category display order and human-readable labels.
pub(crate) const CATEGORY_ORDER: &[(&str, &str)] = &[
    ("attention", "Attention Required"),
    ("quality", "Code Quality"),
    ("testing", "Testing & Reliability"),
    ("workflow", "Workflow"),
    ("documentation", "Documentation"),
    ("health", "Health Trend"),
    ("other", "Other"),
];

/// Query unified insights digest, formatted as a categorized Health Dashboard.
pub async fn query_insights<C: ToolContext>(
    ctx: &C,
    insight_source: Option<String>,
    min_confidence: Option<f64>,
    since_days: Option<u32>,
    limit: Option<i64>,
) -> Result<Json<SessionOutput>, MiraError> {
    use std::collections::BTreeMap;

    let project = ctx.get_project().await;
    let project_id = project
        .as_ref()
        .map(|p| p.id)
        .ok_or_else(|| MiraError::InvalidInput(NO_ACTIVE_PROJECT_ERROR.to_string()))?;

    let filter_source = insight_source.clone();
    let min_conf = min_confidence.unwrap_or(0.5);
    let days_back = since_days.unwrap_or(30) as i64;
    let lim = limit.unwrap_or(20).max(0) as usize;

    let insights = ctx
        .pool()
        .run(move |conn| {
            get_unified_insights_sync(
                conn,
                project_id,
                filter_source.as_deref(),
                min_conf,
                days_back,
                lim,
            )
        })
        .await?;

    // Handle empty state
    if insights.is_empty() {
        let has_filters = insight_source.is_some()
            || min_confidence.is_some()
            || since_days.is_some()
            || limit.is_some();
        let empty_msg = if has_filters {
            "No insights match the current filters.".to_string()
        } else {
            "No insights found.\n\nTo generate insights:\n  \
             1. Index your project: index(action=\"project\")\n  \
             2. CLI health scan: `mira tool index '{\"action\":\"health\"}'`\n  \
             3. Then check insights again"
                .to_string()
        };
        return Ok(Json(SessionOutput {
            action: "insights".into(),
            message: empty_msg,
            data: Some(SessionData::Insights(InsightsData {
                insights: vec![],
                total: 0,
            })),
        }));
    }

    // ── Build categorized dashboard ──

    // Separate high-priority items into "attention" bucket; track their indices
    // so they don't also appear under their original category.
    let mut attention_indices: Vec<usize> = Vec::new();
    for (i, insight) in insights.iter().enumerate() {
        if insight.priority_score >= 0.75 {
            attention_indices.push(i);
        }
    }

    // Group remaining insights by category
    let mut by_category: BTreeMap<String, Vec<(usize, &crate::db::UnifiedInsight)>> =
        BTreeMap::new();
    for (i, insight) in insights.iter().enumerate() {
        if attention_indices.contains(&i) {
            by_category
                .entry("attention".to_string())
                .or_default()
                .push((i, insight));
        } else {
            let cat = insight.category.as_deref().unwrap_or("other").to_string();
            by_category.entry(cat).or_default().push((i, insight));
        }
    }

    // ── Dashboard header ──
    let mut output = String::from("## Project Health Dashboard\n\n");

    // ── Category sections (skip empty) ──
    for &(cat_key, cat_label) in CATEGORY_ORDER {
        let entries = by_category.get(cat_key);
        let count = entries.map_or(0, |v| v.len());
        if count == 0 {
            continue;
        }
        output.push_str(&format!("### {} ({})\n\n", cat_label, count));

        if let Some(entries) = entries {
            for (_idx, insight) in entries {
                let indicator = if insight.priority_score >= 0.75 {
                    "[!!]"
                } else if insight.priority_score >= 0.5 {
                    "[!]"
                } else {
                    "[ ]"
                };
                let age = format_age(&insight.timestamp);
                output.push_str(&format!("  {} {}{}\n", indicator, insight.description, age));
                if let Some(ref trend) = insight.trend
                    && let Some(ref summary) = insight.change_summary
                {
                    output.push_str(&format!("       Trend: {} ({})\n", trend, summary));
                }
                if let Some(ref evidence) = insight.evidence {
                    output.push_str(&format!("       {}\n", evidence));
                }
                output.push('\n');
            }
        }
    }

    // ── Footer ──
    let dismissable = insights.iter().filter(|i| i.row_id.is_some()).count();
    if dismissable > 0 {
        output.push_str("---\n");
        output.push_str(&format!(
            "{} dismissable (use insights action=dismiss_insight insight_id=<row_id> insight_source=<source>)\n",
            dismissable
        ));
    }

    // ── Build InsightItem vec ──
    let items: Vec<InsightItem> = insights
        .iter()
        .enumerate()
        .map(|(i, insight)| {
            let item_category = if attention_indices.contains(&i) {
                Some("attention".to_string())
            } else {
                insight.category.clone()
            };
            InsightItem {
                row_id: insight.row_id,
                source: insight.source.clone(),
                source_type: insight.source_type.clone(),
                description: insight.description.clone(),
                priority_score: insight.priority_score,
                confidence: insight.confidence,
                evidence: insight.evidence.clone(),
                trend: insight.trend.clone(),
                change_summary: insight.change_summary.clone(),
                category: item_category,
            }
        })
        .collect();

    // Fire-and-forget: mark all returned pondering insights as shown by row ID
    // Only pondering insights live in behavior_patterns — filter to avoid cross-table ID collision
    let row_ids: Vec<i64> = insights
        .iter()
        .filter(|i| i.source == "pondering")
        .filter_map(|i| i.row_id)
        .collect();
    if !row_ids.is_empty() {
        let pool = ctx.pool().clone();
        tokio::spawn(async move {
            let _ = pool
                .run(move |conn| {
                    let placeholders: String =
                        row_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                    let sql = format!(
                        "UPDATE behavior_patterns \
                         SET shown_count = COALESCE(shown_count, 0) + 1 \
                         WHERE id IN ({})",
                        placeholders
                    );
                    if let Err(e) = conn.execute(&sql, rusqlite::params_from_iter(row_ids.iter())) {
                        tracing::warn!("Failed to update insight shown_count: {}", e);
                    }
                    Ok::<_, String>(())
                })
                .await;
        });
    }

    let total = items.len();
    Ok(Json(SessionOutput {
        action: "insights".into(),
        message: output,
        data: Some(SessionData::Insights(InsightsData {
            insights: items,
            total,
        })),
    }))
}

/// Dismiss a single insight by row ID, routed by source.
/// - `"pondering"` -> dismisses behavior_patterns row
/// - `"doc_gap"` -> marks documentation_tasks row as 'skipped'
///
/// `insight_source` is required to prevent cross-table ID collisions.
pub async fn dismiss_insight<C: ToolContext>(
    ctx: &C,
    insight_id: Option<i64>,
    insight_source: Option<String>,
) -> Result<Json<SessionOutput>, MiraError> {
    let id = insight_id.ok_or_else(|| {
        MiraError::InvalidInput("insight_id is required for dismiss_insight action".to_string())
    })?;
    let source = insight_source.ok_or_else(|| {
        MiraError::InvalidInput(
            "insight_source is required for dismiss_insight (use 'pondering' or 'doc_gap')"
                .to_string(),
        )
    })?;

    let project = ctx.get_project().await;
    let project_id = project
        .as_ref()
        .map(|p| p.id)
        .ok_or_else(|| MiraError::InvalidInput(NO_ACTIVE_PROJECT_ERROR.to_string()))?;

    let source_clone = source.clone();
    let updated = ctx
        .pool()
        .run(move |conn| {
            dismiss_insight_sync(conn, project_id, id, Some(&source_clone))
                .map_err(|e| format!("Failed to dismiss insight: {}. Verify insight_id and insight_source (\"pondering\" or \"doc_gap\") with insights(action=\"insights\").", e))
        })
        .await?;

    let message = if updated {
        format!("Insight {} ({}) dismissed.", id, source)
    } else {
        format!(
            "Insight {} ({}) not found or already dismissed. Use insights(action=\"insights\") to see active insights.",
            id, source
        )
    };

    Ok(Json(SessionOutput {
        action: "dismiss_insight".into(),
        message,
        data: None,
    }))
}

/// Format an insight timestamp as a human-readable age suffix.
pub(crate) fn format_age(timestamp: &str) -> String {
    let age_days = compute_age_days(timestamp);
    if age_days < 1.0 {
        " (today)".to_string()
    } else if age_days < 2.0 {
        " (yesterday)".to_string()
    } else if age_days < 7.0 {
        format!(" ({} days ago)", age_days as i64)
    } else if age_days < 14.0 {
        " (last week)".to_string()
    } else {
        let weeks = (age_days / 7.0) as i64;
        if weeks == 1 {
            " (1 week ago)".to_string()
        } else {
            format!(" ({} weeks ago)", weeks)
        }
    }
}
