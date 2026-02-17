// crates/mira-server/src/tools/core/code/analysis.rs
// Dependencies, patterns, and tech debt analysis

use rusqlite::OptionalExtension;

use crate::error::MiraError;
use crate::mcp::responses::Json;
use crate::mcp::responses::{
    CodeData, CodeOutput, ConventionsData, DeadCodeData, DebtFactor, DependenciesData,
    DependencyEdge, ModulePatterns, PatternEntry, PatternsData, TechDebtData, TechDebtModule,
    TechDebtTier, UnreferencedSymbol,
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
            "⚠ {} circular dependencies detected:\n",
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
        let circular_marker = if dep.is_circular { " ⚠" } else { "" };
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

/// Get detected architectural patterns
pub async fn get_patterns<C: ToolContext>(ctx: &C) -> Result<Json<CodeOutput>, MiraError> {
    let project_id = ctx
        .project_id()
        .await
        .ok_or_else(|| MiraError::InvalidInput(NO_ACTIVE_PROJECT_ERROR.to_string()))?;

    let patterns = ctx
        .code_pool()
        .run(move |conn| {
            crate::background::code_health::patterns::get_all_module_patterns(conn, project_id)
        })
        .await?;

    if patterns.is_empty() {
        let message = maybe_queue_health_scan(ctx, project_id, "architectural patterns").await;
        return Ok(Json(CodeOutput {
            action: "patterns".into(),
            message,
            data: Some(CodeData::Patterns(PatternsData {
                modules: vec![],
                total: 0,
            })),
        }));
    }

    let mut response = format!("Architectural patterns ({} modules):\n\n", patterns.len());

    let mut module_patterns_list: Vec<ModulePatterns> = Vec::new();

    for (module_id, name, patterns_json) in &patterns {
        response.push_str(&format!("━━━ {} ({}) ━━━\n", module_id, name));

        let mut pattern_entries: Vec<PatternEntry> = Vec::new();

        if let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(patterns_json) {
            for p in &parsed {
                let pattern = p.get("pattern").and_then(|v| v.as_str()).unwrap_or("?");
                let confidence = p.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let evidence_list = p.get("evidence").and_then(|v| v.as_array()).map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect::<Vec<_>>()
                });
                let evidence_str = evidence_list
                    .as_ref()
                    .map(|v| v.join(", "))
                    .unwrap_or_default();

                response.push_str(&format!(
                    "  [{}] {:.0}% — {}\n",
                    pattern,
                    confidence * 100.0,
                    evidence_str
                ));

                pattern_entries.push(PatternEntry {
                    pattern: pattern.to_string(),
                    confidence,
                    evidence: evidence_list,
                });
            }
        }
        response.push('\n');

        module_patterns_list.push(ModulePatterns {
            module_id: module_id.clone(),
            module_name: name.clone(),
            patterns: pattern_entries,
        });
    }

    let total = module_patterns_list.len();
    Ok(Json(CodeOutput {
        action: "patterns".into(),
        message: response,
        data: Some(CodeData::Patterns(PatternsData {
            modules: module_patterns_list,
            total,
        })),
    }))
}

/// Get tech debt scores for all modules
pub async fn get_tech_debt<C: ToolContext>(ctx: &C) -> Result<Json<CodeOutput>, MiraError> {
    use crate::background::code_health::scoring::tier_label;

    let project_id = ctx
        .project_id()
        .await
        .ok_or_else(|| MiraError::InvalidInput(NO_ACTIVE_PROJECT_ERROR.to_string()))?;

    let scores = ctx
        .pool()
        .run(move |conn| crate::db::tech_debt::get_debt_scores_sync(conn, project_id))
        .await?;

    if scores.is_empty() {
        let message = maybe_queue_health_scan(ctx, project_id, "tech debt scores").await;
        return Ok(Json(CodeOutput {
            action: "tech_debt".into(),
            message,
            data: Some(CodeData::TechDebt(TechDebtData {
                modules: vec![],
                summary: vec![],
                total: 0,
            })),
        }));
    }

    // Summary
    let summary = ctx
        .pool()
        .run(move |conn| crate::db::tech_debt::get_debt_summary_sync(conn, project_id))
        .await?;

    let mut response = format!("Tech Debt Report ({} modules):\n\n", scores.len());

    // Tier summary
    response.push_str("Summary by tier:\n");
    for (tier, count) in &summary {
        response.push_str(&format!(
            "  {} ({}): {} modules\n",
            tier,
            tier_label(tier),
            count
        ));
    }
    response.push('\n');

    // Per-module scores (worst first)
    response.push_str("Modules (worst first):\n\n");

    let mut module_items: Vec<TechDebtModule> = Vec::new();

    for score in &scores {
        let line_info = score
            .line_count
            .map(|l| format!(" {}L", l))
            .unwrap_or_default();
        let finding_info = score
            .finding_count
            .map(|f| format!(" {}findings", f))
            .unwrap_or_default();

        response.push_str(&format!(
            "  {} [{} {}] score:{:.0}{}{}\n",
            score.module_path,
            score.tier,
            tier_label(&score.tier),
            score.overall_score,
            line_info,
            finding_info,
        ));

        // Show top factors for D/F tier
        let mut top_factors: Option<Vec<DebtFactor>> = None;
        if (score.tier == "D" || score.tier == "F")
            && let Ok(factors) = serde_json::from_str::<serde_json::Value>(&score.factor_scores)
        {
            let mut factor_list: Vec<(String, f64)> = Vec::new();
            if let Some(obj) = factors.as_object() {
                for (name, val) in obj {
                    if let Some(s) = val.get("score").and_then(|v| v.as_f64())
                        && s > 20.0
                    {
                        factor_list.push((name.clone(), s));
                    }
                }
            }
            factor_list.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            for (name, s) in factor_list.iter().take(3) {
                response.push_str(&format!("    ↳ {}: {:.0}\n", name, s));
            }
            if !factor_list.is_empty() {
                top_factors = Some(
                    factor_list
                        .into_iter()
                        .take(3)
                        .map(|(name, s)| DebtFactor { name, score: s })
                        .collect(),
                );
            }
        }

        module_items.push(TechDebtModule {
            module_path: score.module_path.clone(),
            tier: score.tier.clone(),
            overall_score: score.overall_score,
            line_count: score.line_count,
            finding_count: score.finding_count,
            top_factors,
        });
    }

    let summary_items: Vec<TechDebtTier> = summary
        .iter()
        .map(|(tier, count)| TechDebtTier {
            tier: tier.clone(),
            label: tier_label(tier).to_string(),
            count: *count as usize,
        })
        .collect();

    let total = module_items.len();
    Ok(Json(CodeOutput {
        action: "tech_debt".into(),
        message: response,
        data: Some(CodeData::TechDebt(TechDebtData {
            modules: module_items,
            summary: summary_items,
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

    let limit = limit.unwrap_or(50).min(200) as usize;

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
                     WHERE cg.callee = s.name AND cg.project_id = ?1
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

/// Show detected conventions for the module containing a file
pub async fn get_conventions<C: ToolContext>(
    ctx: &C,
    file_path: String,
) -> Result<Json<CodeOutput>, MiraError> {
    let project_id = ctx
        .project_id()
        .await
        .ok_or_else(|| MiraError::InvalidInput(NO_ACTIVE_PROJECT_ERROR.to_string()))?;

    let fp = file_path.clone();
    let result = ctx
        .code_pool()
        .run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT cm.module_id, m.name, cm.error_handling, cm.test_pattern,
                        cm.naming, cm.key_imports, cm.detected_patterns
                 FROM module_conventions cm
                 JOIN codebase_modules m ON cm.module_id = m.module_id AND cm.project_id = m.project_id
                 WHERE cm.project_id = ?1
                   AND ?2 LIKE m.module_id || '%'
                 ORDER BY LENGTH(m.module_id) DESC
                 LIMIT 1",
            )?;

            let row = stmt
                .query_row(rusqlite::params![project_id, fp], |row| {
                    Ok(ConventionsData {
                        module_id: row.get(0)?,
                        module_name: row.get(1)?,
                        error_handling: row.get(2)?,
                        test_pattern: row.get(3)?,
                        naming: row.get(4)?,
                        key_imports: row.get(5)?,
                        detected_patterns: row.get(6)?,
                    })
                })
                .optional()?;

            Ok::<_, MiraError>(row)
        })
        .await?;

    match result {
        Some(conv) => {
            let mut response = format!(
                "Conventions for module {} ({}):\n\n",
                conv.module_id, conv.module_name
            );

            if let Some(ref eh) = conv.error_handling {
                response.push_str(&format!("  Error handling: {}\n", eh));
            }
            if let Some(ref tp) = conv.test_pattern {
                response.push_str(&format!("  Test pattern: {}\n", tp));
            }
            if let Some(ref n) = conv.naming {
                response.push_str(&format!("  Naming: {}\n", n));
            }
            if let Some(ref ki) = conv.key_imports {
                response.push_str(&format!("  Key imports: {}\n", ki));
            }
            if let Some(ref dp) = conv.detected_patterns {
                response.push_str(&format!("  Detected patterns: {}\n", dp));
            }

            Ok(Json(CodeOutput {
                action: "conventions".into(),
                message: response,
                data: Some(CodeData::Conventions(conv)),
            }))
        }
        None => {
            let message =
                maybe_queue_health_scan(ctx, project_id, "module conventions").await;
            Ok(Json(CodeOutput {
                action: "conventions".into(),
                message: format!(
                    "No conventions found for file '{}'. {}",
                    file_path, message
                ),
                data: Some(CodeData::Conventions(ConventionsData {
                    module_id: String::new(),
                    module_name: String::new(),
                    error_handling: None,
                    test_pattern: None,
                    naming: None,
                    key_imports: None,
                    detected_patterns: None,
                })),
            }))
        }
    }
}
