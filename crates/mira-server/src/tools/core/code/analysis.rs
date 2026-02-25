// crates/mira-server/src/tools/core/code/analysis.rs
// Dependencies and dead code analysis (patterns, tech debt, conventions removed)

use crate::error::MiraError;
use crate::mcp::responses::Json;
use crate::mcp::responses::{
    CodeData, CodeOutput, DeadCodeData, DependenciesData, DependencyEdge, UnreferencedSymbol,
};
use crate::tools::core::{NO_ACTIVE_PROJECT_ERROR, ToolContext};

/// Try to queue a health scan for a cold-start project (never scanned before).
/// Returns a user-facing message appropriate for the outcome.
async fn maybe_queue_health_scan<C: ToolContext>(
    ctx: &C,
    project_id: i64,
    data_kind: &str,
) -> String {
    let pid = project_id;
    let already_scanned = ctx
        .pool()
        .run(move |conn| {
            Ok::<_, MiraError>(
                crate::db::get_scan_info_sync(conn, pid, "health_scan_time").is_some(),
            )
        })
        .await
        .unwrap_or(false);

    if already_scanned {
        return format!("No {} found for this project.", data_kind);
    }

    // Cold start — queue a health scan
    let pid = project_id;
    let pool = ctx.pool().clone();
    let queued = pool
        .run(move |conn| crate::background::code_health::mark_health_scan_needed_sync(conn, pid))
        .await;

    if queued.is_ok() {
        format!(
            "No {data_kind} yet — a health scan has been queued and will complete shortly. Try again in a moment."
        )
    } else {
        format!("No {data_kind} yet. Run index(action=\"health\") to generate it.")
    }
}

/// Get module dependencies and circular dependency warnings
pub async fn get_dependencies<C: ToolContext>(ctx: &C) -> Result<Json<CodeOutput>, MiraError> {
    let project_id = ctx
        .project_id()
        .await
        .ok_or_else(|| MiraError::InvalidInput(NO_ACTIVE_PROJECT_ERROR.to_string()))?;

    let deps = ctx
        .code_pool()
        .run(move |conn| crate::db::dependencies::get_module_deps_sync(conn, project_id))
        .await?;

    if deps.is_empty() {
        let message = maybe_queue_health_scan(ctx, project_id, "module dependencies").await;
        return Ok(Json(CodeOutput {
            action: "dependencies".into(),
            message,
            data: Some(CodeData::Dependencies(DependenciesData {
                edges: vec![],
                circular_count: 0,
                total: 0,
            })),
        }));
    }

    let circular: Vec<_> = deps.iter().filter(|d| d.is_circular).collect();
    let circular_count = circular.len();

    let mut response = format!("Module dependencies ({} edges):\n\n", deps.len());

    // Show circular warnings first
    if !circular.is_empty() {
        response.push_str(&format!(
            "WARNING: {} circular dependencies detected:\n",
            circular.len()
        ));
        for dep in &circular {
            response.push_str(&format!(
                "  {} <-> {} ({} calls, {} imports)\n",
                dep.source_module_id, dep.target_module_id, dep.call_count, dep.import_count
            ));
        }
        response.push('\n');
    }

    // Show top dependencies by weight
    response.push_str("Top dependencies (by call+import count):\n");
    for dep in deps.iter().take(30) {
        let circular_marker = if dep.is_circular { " WARNING" } else { "" };
        response.push_str(&format!(
            "  {} -> {} [{}] calls:{} imports:{}{}\n",
            dep.source_module_id,
            dep.target_module_id,
            dep.dependency_type,
            dep.call_count,
            dep.import_count,
            circular_marker,
        ));
    }

    if deps.len() > 30 {
        response.push_str(&format!("  ... and {} more\n", deps.len() - 30));
    }

    let total = deps.len();
    let edges: Vec<DependencyEdge> = deps
        .iter()
        .map(|d| DependencyEdge {
            source: d.source_module_id.clone(),
            target: d.target_module_id.clone(),
            dependency_type: d.dependency_type.clone(),
            call_count: d.call_count,
            import_count: d.import_count,
            is_circular: d.is_circular,
        })
        .collect();

    Ok(Json(CodeOutput {
        action: "dependencies".into(),
        message: response,
        data: Some(CodeData::Dependencies(DependenciesData {
            edges,
            circular_count,
            total,
        })),
    }))
}

/// Find unreferenced symbols (dead code candidates)
pub async fn get_dead_code<C: ToolContext>(
    ctx: &C,
    limit: Option<i64>,
) -> Result<Json<CodeOutput>, MiraError> {
    let project_id = ctx
        .project_id()
        .await
        .ok_or_else(|| MiraError::InvalidInput(NO_ACTIVE_PROJECT_ERROR.to_string()))?;

    let limit = limit.unwrap_or(50).clamp(1, 200) as usize;

    let symbols = ctx
        .code_pool()
        .run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT s.name, s.symbol_type, s.file_path, s.start_line
                 FROM code_symbols s
                 WHERE s.project_id = ?1
                   AND s.symbol_type IN ('function', 'method')
                   AND s.name NOT IN ('main', 'new', 'default', 'from', 'into', 'drop', 'fmt', 'clone', 'eq', 'hash', 'deref')
                   AND NOT EXISTS (
                     SELECT 1 FROM call_graph cg
                     JOIN code_symbols cs ON cg.caller_id = cs.id
                     WHERE cg.callee_name = s.name AND cs.project_id = ?1
                   )
                 ORDER BY s.file_path, s.start_line
                 LIMIT ?2",
            )?;

            let rows = stmt
                .query_map(rusqlite::params![project_id, limit], |row| {
                    Ok(UnreferencedSymbol {
                        name: row.get(0)?,
                        symbol_type: row.get(1)?,
                        file_path: row.get(2)?,
                        start_line: row.get(3)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            Ok::<_, MiraError>(rows)
        })
        .await?;

    if symbols.is_empty() {
        let message =
            maybe_queue_health_scan(ctx, project_id, "unreferenced symbols (dead code)").await;
        return Ok(Json(CodeOutput {
            action: "dead_code".into(),
            message,
            data: Some(CodeData::DeadCode(DeadCodeData {
                unreferenced: vec![],
                total: 0,
            })),
        }));
    }

    let mut response = format!(
        "Dead code candidates ({} unreferenced symbols):\n\n",
        symbols.len()
    );

    for sym in &symbols {
        response.push_str(&format!(
            "  {} [{}] {}:{}\n",
            sym.name, sym.symbol_type, sym.file_path, sym.start_line,
        ));
    }

    if symbols.len() == limit {
        response.push_str(&format!(
            "\n(Showing first {} results -- increase limit for more)\n",
            limit
        ));
    }

    let total = symbols.len();
    Ok(Json(CodeOutput {
        action: "dead_code".into(),
        message: response,
        data: Some(CodeData::DeadCode(DeadCodeData {
            unreferenced: symbols,
            total,
        })),
    }))
}
