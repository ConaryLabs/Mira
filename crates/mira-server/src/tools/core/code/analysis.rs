// crates/mira-server/src/tools/core/code/analysis.rs
// Dependencies, patterns, and tech debt analysis

use crate::mcp::responses::Json;
use crate::mcp::responses::{
    CodeData, CodeOutput, DebtFactor, DependenciesData, DependencyEdge, ModulePatterns,
    PatternEntry, PatternsData, TechDebtData, TechDebtModule, TechDebtTier,
};
use crate::tools::core::{NO_ACTIVE_PROJECT_ERROR, ToolContext};
use crate::utils::ResultExt;

/// Get module dependencies and circular dependency warnings
pub async fn get_dependencies<C: ToolContext>(ctx: &C) -> Result<Json<CodeOutput>, String> {
    let project_id = ctx.project_id().await.ok_or(NO_ACTIVE_PROJECT_ERROR)?;

    let deps = ctx
        .code_pool()
        .run(move |conn| crate::db::dependencies::get_module_deps_sync(conn, project_id).str_err())
        .await?;

    if deps.is_empty() {
        return Ok(Json(CodeOutput {
            action: "dependencies".into(),
            message: "No module dependencies found. Run a health scan or index the project first."
                .to_string(),
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
pub async fn get_patterns<C: ToolContext>(ctx: &C) -> Result<Json<CodeOutput>, String> {
    let project_id = ctx.project_id().await.ok_or(NO_ACTIVE_PROJECT_ERROR)?;

    let patterns = ctx
        .code_pool()
        .run(move |conn| {
            crate::background::code_health::patterns::get_all_module_patterns(conn, project_id)
        })
        .await?;

    if patterns.is_empty() {
        return Ok(Json(CodeOutput {
            action: "patterns".into(),
            message: "No architectural patterns detected yet. Run a health scan first.".to_string(),
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
pub async fn get_tech_debt<C: ToolContext>(ctx: &C) -> Result<Json<CodeOutput>, String> {
    use crate::background::code_health::scoring::tier_label;

    let project_id = ctx.project_id().await.ok_or(NO_ACTIVE_PROJECT_ERROR)?;

    let scores = ctx
        .pool()
        .run(move |conn| crate::db::tech_debt::get_debt_scores_sync(conn, project_id).str_err())
        .await?;

    if scores.is_empty() {
        return Ok(Json(CodeOutput {
            action: "tech_debt".into(),
            message: "No tech debt scores computed yet. Run a health scan first.".to_string(),
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
        .run(move |conn| crate::db::tech_debt::get_debt_summary_sync(conn, project_id).str_err())
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
