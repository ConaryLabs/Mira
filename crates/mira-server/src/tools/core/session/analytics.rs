// crates/mira-server/src/tools/core/session/analytics.rs
//! Analytics queries: error patterns, health trends, session lineage, capabilities.

use crate::db::{get_error_patterns_sync, get_health_history_sync, get_session_lineage_sync};
use crate::error::MiraError;
use crate::mcp::responses::Json;
use crate::mcp::responses::{
    CapabilitiesData, CapabilityStatus, ErrorPatternItem, ErrorPatternsData, HealthSnapshotItem,
    HealthTrendsData, LineageSession, SessionData, SessionLineageData, SessionOutput,
};
use crate::tools::core::{NO_ACTIVE_PROJECT_ERROR, ToolContext};
use crate::utils::truncate_at_boundary;

/// Query error patterns for the active project.
pub(super) async fn get_error_patterns<C: ToolContext>(
    ctx: &C,
    limit: Option<i64>,
) -> Result<Json<SessionOutput>, MiraError> {
    let project = ctx.get_project().await;
    let project_id = project
        .as_ref()
        .map(|p| p.id)
        .ok_or_else(|| MiraError::InvalidInput(NO_ACTIVE_PROJECT_ERROR.to_string()))?;

    let limit = limit.unwrap_or(20).clamp(1, 100) as usize;

    let rows = ctx
        .pool()
        .run(move |conn| Ok::<_, String>(get_error_patterns_sync(conn, project_id, limit)))
        .await?;

    if rows.is_empty() {
        return Ok(Json(SessionOutput {
            action: "error_patterns".into(),
            message: "No error patterns recorded yet.".to_string(),
            data: Some(SessionData::ErrorPatterns(ErrorPatternsData {
                patterns: vec![],
                total: 0,
            })),
        }));
    }

    let total = rows.len();
    let mut output = format!("Learned error patterns ({} total):\n\n", total);
    let items: Vec<ErrorPatternItem> = rows
        .into_iter()
        .map(|row| {
            output.push_str(&format!(
                "  [{}] (seen {}x) {}\n",
                row.tool_name, row.occurrence_count, row.error_fingerprint
            ));
            if let Some(ref fix) = row.fix_description {
                output.push_str(&format!("    Fix: {}\n", fix));
            }
            output.push('\n');
            ErrorPatternItem {
                tool_name: row.tool_name,
                error_fingerprint: row.error_fingerprint,
                fix_description: row.fix_description,
                occurrence_count: row.occurrence_count,
                last_seen: row.last_seen,
            }
        })
        .collect();

    Ok(Json(SessionOutput {
        action: "error_patterns".into(),
        message: output,
        data: Some(SessionData::ErrorPatterns(ErrorPatternsData {
            patterns: items,
            total,
        })),
    }))
}

/// Query health snapshot trends for the active project.
pub(super) async fn get_health_trends<C: ToolContext>(
    ctx: &C,
    limit: Option<i64>,
) -> Result<Json<SessionOutput>, MiraError> {
    let project = ctx.get_project().await;
    let project_id = project
        .as_ref()
        .map(|p| p.id)
        .ok_or_else(|| MiraError::InvalidInput(NO_ACTIVE_PROJECT_ERROR.to_string()))?;

    let limit = limit.unwrap_or(10).clamp(1, 50) as usize;

    let snapshots = ctx
        .pool()
        .run(move |conn| get_health_history_sync(conn, project_id, limit))
        .await?;

    if snapshots.is_empty() {
        return Ok(Json(SessionOutput {
            action: "health_trends".into(),
            message: "No health snapshots found.\n\nRun a health scan first: `mira tool index '{\"action\":\"health\"}'`".to_string(),
            data: Some(SessionData::HealthTrends(HealthTrendsData {
                snapshots: vec![],
                trend: None,
            })),
        }));
    }

    // Snapshots are newest-first from DB; reverse for chronological display
    let chronological: Vec<_> = snapshots.iter().rev().collect();

    // Calculate trend by comparing first and last snapshot avg_debt_score
    let trend = if chronological.len() >= 2 {
        let Some(first_snap) = chronological.first() else {
            unreachable!()
        };
        let Some(last_snap) = chronological.last() else {
            unreachable!()
        };
        let first = first_snap.avg_debt_score;
        let last = last_snap.avg_debt_score;
        if first == 0.0 {
            Some("stable".to_string())
        } else {
            let delta_pct = ((last - first) / first) * 100.0;
            if delta_pct < -5.0 {
                Some("improving".to_string())
            } else if delta_pct > 5.0 {
                Some("degrading".to_string())
            } else {
                Some("stable".to_string())
            }
        }
    } else {
        None
    };

    // Build human-readable message
    let mut output = format!("## Health Trends ({} snapshots)\n\n", chronological.len());

    if let Some(ref t) = trend {
        output.push_str(&format!("Overall trend: **{}**\n\n", t));
    }

    output.push_str("| Date | Modules | Avg Debt | Max Debt | Warnings | TODOs | Findings |\n");
    output.push_str("|------|---------|----------|----------|----------|-------|----------|\n");

    for snap in &chronological {
        output.push_str(&format!(
            "| {} | {} | {:.1} | {:.1} | {} | {} | {} |\n",
            snap.snapshot_at,
            snap.module_count,
            snap.avg_debt_score,
            snap.max_debt_score,
            snap.warning_count,
            snap.todo_count,
            snap.total_finding_count,
        ));
    }

    // Show delta summary if we have multiple snapshots
    if chronological.len() >= 2 {
        let Some(first) = chronological.first() else {
            unreachable!()
        };
        let Some(last) = chronological.last() else {
            unreachable!()
        };
        output.push_str(&format!(
            "\nDelta: avg debt {:.1} \u{2192} {:.1}, modules {} \u{2192} {}, findings {} \u{2192} {}\n",
            first.avg_debt_score,
            last.avg_debt_score,
            first.module_count,
            last.module_count,
            first.total_finding_count,
            last.total_finding_count,
        ));
    }

    let items: Vec<HealthSnapshotItem> = chronological
        .iter()
        .map(|snap| HealthSnapshotItem {
            snapshot_at: snap.snapshot_at.clone(),
            module_count: snap.module_count,
            avg_debt_score: snap.avg_debt_score,
            max_debt_score: snap.max_debt_score,
            tier_distribution: snap.tier_distribution.clone(),
            warning_count: snap.warning_count,
            todo_count: snap.todo_count,
            total_finding_count: snap.total_finding_count,
        })
        .collect();

    Ok(Json(SessionOutput {
        action: "health_trends".into(),
        message: output,
        data: Some(SessionData::HealthTrends(HealthTrendsData {
            snapshots: items,
            trend,
        })),
    }))
}

/// Query session lineage (resume chains) for the active project.
pub(super) async fn get_session_lineage<C: ToolContext>(
    ctx: &C,
    limit: Option<i64>,
) -> Result<Json<SessionOutput>, MiraError> {
    let project = ctx.get_project().await;
    let project_id = project
        .as_ref()
        .map(|p| p.id)
        .ok_or_else(|| MiraError::InvalidInput(NO_ACTIVE_PROJECT_ERROR.to_string()))?;

    let limit = limit.unwrap_or(20).clamp(1, 100) as usize;

    let rows = ctx
        .pool()
        .run(move |conn| get_session_lineage_sync(conn, project_id, limit))
        .await?;

    if rows.is_empty() {
        return Ok(Json(SessionOutput {
            action: "session_lineage".into(),
            message: "No sessions found for this project.".to_string(),
            data: Some(SessionData::SessionLineage(SessionLineageData {
                sessions: vec![],
                total: 0,
            })),
        }));
    }

    // Build a set of session IDs for quick lookup when determining indentation
    let session_ids: std::collections::HashSet<&str> = rows.iter().map(|r| r.id.as_str()).collect();

    // Format human-readable output with lineage indentation
    let mut output = format!("## Session Lineage ({} sessions)\n\n", rows.len());

    for row in &rows {
        let short_id = truncate_at_boundary(&row.id, 8);
        let source_tag = row.source.as_deref().unwrap_or("startup");
        let branch_info = row
            .branch
            .as_ref()
            .map(|b| format!(" (branch: {})", b))
            .unwrap_or_default();
        let age = crate::tools::core::insights::format_age(&row.last_activity);
        let goal_info = match row.goal_count {
            Some(n) if n > 0 => format!(" -- {} goal{}", n, if n == 1 { "" } else { "s" }),
            _ => String::new(),
        };

        // Indent resumed sessions that resume from a session in our result set
        let is_resume_child = row
            .resumed_from
            .as_ref()
            .is_some_and(|rf| session_ids.contains(rf.as_str()));

        if is_resume_child {
            output.push_str(&format!(
                "  <- [{}] {}{}{}{}\n",
                source_tag, short_id, branch_info, age, goal_info
            ));
        } else {
            output.push_str(&format!(
                "[{}] {}{}{}{}\n",
                source_tag, short_id, branch_info, age, goal_info
            ));
        }
    }

    let items: Vec<LineageSession> = rows
        .into_iter()
        .map(|row| LineageSession {
            id: row.id,
            source: row.source,
            resumed_from: row.resumed_from,
            branch: row.branch,
            started_at: row.started_at,
            last_activity: row.last_activity,
            status: row.status,
            goal_count: row.goal_count,
        })
        .collect();

    let total = items.len();
    Ok(Json(SessionOutput {
        action: "session_lineage".into(),
        message: output,
        data: Some(SessionData::SessionLineage(SessionLineageData {
            sessions: items,
            total,
        })),
    }))
}

/// Report which features are available, degraded, or unavailable.
///
/// Checks embeddings, LLM provider, fuzzy cache, and code index status.
/// CLI-only action -- not exposed via MCP schema.
pub(super) async fn get_capabilities<C: ToolContext>(
    ctx: &C,
) -> Result<Json<SessionOutput>, MiraError> {
    let mut caps = Vec::new();

    // Semantic search (requires embeddings)
    let has_embeddings = ctx.embeddings().is_some();
    caps.push(CapabilityStatus {
        name: "semantic_search".into(),
        status: if has_embeddings {
            "available"
        } else {
            "unavailable"
        }
        .into(),
        detail: if !has_embeddings {
            Some("keyword + fuzzy search active | add OPENAI_API_KEY for semantic search".into())
        } else {
            None
        },
    });

    // Background analysis (local heuristics, no LLM required)
    caps.push(CapabilityStatus {
        name: "background_analysis".into(),
        status: "available".into(),
        detail: Some("Local heuristic analysis".into()),
    });

    // Fuzzy search (requires cache)
    let has_fuzzy = ctx.fuzzy_cache().is_some();
    caps.push(CapabilityStatus {
        name: "fuzzy_search".into(),
        status: if has_fuzzy {
            "available"
        } else {
            "unavailable"
        }
        .into(),
        detail: None,
    });

    // Code index (requires indexed symbols in code DB for this project)
    let project_id = ctx.project_id().await;
    let code_indexed = ctx
        .code_pool()
        .run(move |conn| {
            let count = conn
                .query_row(
                    "SELECT COUNT(*) FROM code_symbols WHERE project_id IS ?1",
                    rusqlite::params![project_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0);
            Ok::<_, MiraError>(count > 0)
        })
        .await
        .unwrap_or(false);
    caps.push(CapabilityStatus {
        name: "code_index".into(),
        status: if code_indexed {
            "available"
        } else {
            "unavailable"
        }
        .into(),
        detail: if !code_indexed {
            Some("Run index(action='project') to enable code intelligence".into())
        } else {
            None
        },
    });

    // MCP sampling (client supports createMessage)
    let has_sampling = ctx.has_sampling();
    caps.push(CapabilityStatus {
        name: "mcp_sampling".into(),
        status: if has_sampling {
            "available"
        } else {
            "unavailable"
        }
        .into(),
        detail: if !has_sampling {
            Some("MCP client does not support sampling/createMessage".into())
        } else {
            None
        },
    });

    // Format message
    let mut msg = String::from("Capability status:\n");
    for cap in &caps {
        let icon = match cap.status.as_str() {
            "available" => "\u{2713}",
            "degraded" => "~",
            _ => "\u{2717}",
        };
        msg.push_str(&format!("  {} {} ({})", icon, cap.name, cap.status));
        if let Some(ref detail) = cap.detail {
            msg.push_str(&format!(" \u{2014} {}", detail));
        }
        msg.push('\n');
    }

    Ok(Json(SessionOutput {
        action: "capabilities".into(),
        message: msg,
        data: Some(SessionData::Capabilities(CapabilitiesData {
            capabilities: caps,
        })),
    }))
}
