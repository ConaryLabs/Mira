// background/code_health/scoring.rs
// Per-module tech debt scoring: composite score (0-100) from all health signals

use crate::db::pool::DatabasePool;
use crate::db::tech_debt::{TechDebtScore, store_debt_score_sync};
use crate::utils::ResultExt;
use std::collections::HashMap;
use std::sync::Arc;

/// Scoring factor weights (must sum to 1.0)
const W_COMPLEXITY: f64 = 0.25;
const W_ERROR_HANDLING: f64 = 0.15;
const W_UNWRAP: f64 = 0.15;
const W_TODO: f64 = 0.10;
const W_DOC_GAPS: f64 = 0.15;
const W_UNUSED: f64 = 0.10;
const W_PATTERN_VIOLATIONS: f64 = 0.10;

/// Score tier thresholds
fn score_to_tier(score: f64) -> &'static str {
    match score as u32 {
        0..=20 => "A",
        21..=40 => "B",
        41..=60 => "C",
        61..=80 => "D",
        _ => "F",
    }
}

pub fn tier_label(tier: &str) -> &'static str {
    match tier {
        "A" => "Low debt",
        "B" => "Moderate",
        "C" => "Significant",
        "D" => "High",
        "F" => "Critical",
        _ => "Unknown",
    }
}

/// Compute tech debt scores for all modules in a project.
/// Returns the number of modules scored.
pub async fn compute_tech_debt_scores(
    main_pool: &Arc<DatabasePool>,
    code_pool: &Arc<DatabasePool>,
    project_id: i64,
) -> Result<usize, String> {
    // Step 1: Get module list with line counts from code DB
    let modules: Vec<ModuleLineCount> = code_pool
        .run(move |conn| get_module_line_counts(conn, project_id))
        .await?;

    if modules.is_empty() {
        return Ok(0);
    }

    // Step 2: Gather findings from main DB (memory_facts)
    let module_ids: Vec<String> = modules.iter().map(|m| m.module_id.clone()).collect();
    let module_paths: Vec<String> = modules.iter().map(|m| m.path.clone()).collect();
    let findings = main_pool
        .run(move |conn| gather_findings(conn, project_id, &module_paths))
        .await?;

    // Step 3: Gather doc gaps from main DB
    let paths_for_docs: Vec<String> = modules.iter().map(|m| m.path.clone()).collect();
    let doc_gaps = main_pool
        .run(move |conn| gather_doc_gaps(conn, project_id, &paths_for_docs))
        .await?;

    // Step 4: Gather pattern violations from code DB
    let module_ids_for_patterns = module_ids.clone();
    let pattern_scores: HashMap<String, f64> = code_pool
        .run(move |conn| gather_pattern_violations(conn, project_id, &module_ids_for_patterns))
        .await?;

    // Step 5: Compute scores for each module
    let mut scores = Vec::new();
    for module in &modules {
        let line_count = module.line_count.max(1) as f64;
        let per_1k = 1000.0 / line_count;

        let empty = ModuleFindings::default();
        let f = findings.get(&module.path).unwrap_or(&empty);
        let doc_gap_count = doc_gaps.get(&module.path).copied().unwrap_or(0) as f64;
        let pattern_score = pattern_scores
            .get(&module.module_id)
            .copied()
            .unwrap_or(0.0);

        // Normalize each factor to 0-100 based on finding density per 1K lines
        let complexity_score = normalize(f.complexity as f64 * per_1k, 2.0);
        let error_score = normalize(f.error_handling as f64 * per_1k, 3.0);
        let unwrap_score = normalize(f.unwrap as f64 * per_1k, 5.0);
        let todo_score = normalize(f.todo as f64 * per_1k, 4.0);
        let doc_score = normalize(doc_gap_count * per_1k, 2.0);
        let unused_score = normalize(f.unused as f64 * per_1k, 3.0);

        let overall = W_COMPLEXITY * complexity_score
            + W_ERROR_HANDLING * error_score
            + W_UNWRAP * unwrap_score
            + W_TODO * todo_score
            + W_DOC_GAPS * doc_score
            + W_UNUSED * unused_score
            + W_PATTERN_VIOLATIONS * pattern_score;

        let overall = overall.min(100.0);
        let tier = score_to_tier(overall);

        let factor_scores = serde_json::json!({
            "complexity": { "score": round2(complexity_score), "count": f.complexity, "weight": W_COMPLEXITY },
            "error_handling": { "score": round2(error_score), "count": f.error_handling, "weight": W_ERROR_HANDLING },
            "unwrap_risk": { "score": round2(unwrap_score), "count": f.unwrap, "weight": W_UNWRAP },
            "todos": { "score": round2(todo_score), "count": f.todo, "weight": W_TODO },
            "doc_gaps": { "score": round2(doc_score), "count": doc_gap_count as i64, "weight": W_DOC_GAPS },
            "unused_code": { "score": round2(unused_score), "count": f.unused, "weight": W_UNUSED },
            "pattern_violations": { "score": round2(pattern_score), "weight": W_PATTERN_VIOLATIONS },
        })
        .to_string();

        let total_findings = f.complexity + f.error_handling + f.unwrap + f.todo + f.unused;

        scores.push(TechDebtScore {
            module_id: module.module_id.clone(),
            module_path: module.path.clone(),
            overall_score: round2(overall),
            tier: tier.to_string(),
            factor_scores,
            line_count: Some(module.line_count),
            finding_count: Some(total_findings),
        });
    }

    // Step 6: Store scores in main DB
    let count = scores.len();
    main_pool
        .interact(move |conn| {
            for score in &scores {
                store_debt_score_sync(conn, project_id, score)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await
        .str_err()?;

    Ok(count)
}

/// Normalize a density value to 0-100 score.
/// `threshold` is the density at which score reaches 100.
fn normalize(density: f64, threshold: f64) -> f64 {
    ((density / threshold) * 100.0).clamp(0.0, 100.0)
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

// ============================================================================
// Data Types
// ============================================================================

struct ModuleLineCount {
    module_id: String,
    path: String,
    line_count: i64,
}

#[derive(Default)]
struct ModuleFindings {
    complexity: i64,
    error_handling: i64,
    unwrap: i64,
    todo: i64,
    unused: i64,
}

// ============================================================================
// Database Queries
// ============================================================================

fn get_module_line_counts(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> Result<Vec<ModuleLineCount>, String> {
    let mut stmt = conn
        .prepare("SELECT module_id, path, line_count FROM codebase_modules WHERE project_id = ?")
        .str_err()?;

    let modules = stmt
        .query_map([project_id], |row| {
            Ok(ModuleLineCount {
                module_id: row.get(0)?,
                path: row.get(1)?,
                line_count: row.get::<_, i64>(2).unwrap_or(100),
            })
        })
        .str_err()?
        .filter_map(|r| r.ok())
        .collect();

    Ok(modules)
}

/// Gather health findings from memory_facts grouped by module path.
/// Categories: complexity, error_handling, error_quality, unwrap, todo, unimplemented, unused
fn gather_findings(
    conn: &rusqlite::Connection,
    project_id: i64,
    module_paths: &[String],
) -> Result<HashMap<String, ModuleFindings>, String> {
    let mut result: HashMap<String, ModuleFindings> = HashMap::new();

    // Query all health findings for this project
    let mut stmt = conn
        .prepare(
            "SELECT content, category FROM memory_facts
             WHERE project_id = ? AND fact_type = 'health'
             AND category IN ('complexity', 'error_handling', 'error_quality', 'unwrap', 'todo', 'unimplemented', 'unused')",
        )
        .str_err()?;

    let rows = stmt
        .query_map([project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .str_err()?;

    for row in rows {
        let (content, category) = row.str_err()?;

        // Match finding to module by checking if content mentions a file in the module path
        let module_path = module_paths
            .iter()
            .find(|p| content.contains(p.as_str()))
            .cloned();

        // Fall back: try to extract file path from content and match
        let module_path = module_path.or_else(|| {
            // Content often has format: "[type] ... at path/file.rs:line"
            module_paths
                .iter()
                .find(|p| {
                    // Check if any part of the content matches
                    content
                        .split_whitespace()
                        .any(|word| word.starts_with(p.as_str()))
                })
                .cloned()
        });

        if let Some(path) = module_path {
            let entry = result.entry(path).or_default();
            match category.as_str() {
                "complexity" => entry.complexity += 1,
                "error_handling" | "error_quality" => entry.error_handling += 1,
                "unwrap" => entry.unwrap += 1,
                "todo" | "unimplemented" => entry.todo += 1,
                "unused" => entry.unused += 1,
                _ => {}
            }
        }
    }

    Ok(result)
}

/// Count pending documentation tasks per module path
fn gather_doc_gaps(
    conn: &rusqlite::Connection,
    project_id: i64,
    module_paths: &[String],
) -> Result<HashMap<String, i64>, String> {
    let mut result: HashMap<String, i64> = HashMap::new();

    let mut stmt = conn
        .prepare(
            "SELECT source_file_path FROM documentation_tasks
             WHERE project_id = ? AND status = 'pending'",
        )
        .str_err()?;

    let rows = stmt
        .query_map([project_id], |row| row.get::<_, String>(0))
        .str_err()?;

    for row in rows {
        let file_path = row.str_err()?;
        if let Some(module_path) = module_paths
            .iter()
            .find(|p| file_path.starts_with(p.as_str()))
        {
            *result.entry(module_path.clone()).or_default() += 1;
        }
    }

    Ok(result)
}

/// Score pattern violations: low-confidence patterns indicate unclear architecture
fn gather_pattern_violations(
    conn: &rusqlite::Connection,
    project_id: i64,
    module_ids: &[String],
) -> Result<HashMap<String, f64>, String> {
    let mut result: HashMap<String, f64> = HashMap::new();

    for module_id in module_ids {
        let patterns_json: Option<String> = conn
            .query_row(
                "SELECT detected_patterns FROM codebase_modules
                 WHERE project_id = ? AND module_id = ?",
                rusqlite::params![project_id, module_id],
                |row| row.get(0),
            )
            .ok()
            .flatten();

        if let Some(json) = patterns_json
            && let Ok(patterns) = serde_json::from_str::<Vec<serde_json::Value>>(&json)
        {
            // Low-confidence patterns contribute to debt score
            let violation_score: f64 = patterns
                .iter()
                .filter_map(|p| p.get("confidence").and_then(|c| c.as_f64()))
                .filter(|&c| c < 0.6) // Low confidence = unclear pattern
                .map(|c| (1.0 - c) * 100.0) // Invert: lower confidence = higher score
                .sum::<f64>()
                / patterns.len().max(1) as f64;

            if violation_score > 0.0 {
                result.insert(module_id.clone(), violation_score.min(100.0));
            }
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_score_to_tier() {
        assert_eq!(score_to_tier(0.0), "A");
        assert_eq!(score_to_tier(10.0), "A");
        assert_eq!(score_to_tier(20.0), "A");
        assert_eq!(score_to_tier(21.0), "B");
        assert_eq!(score_to_tier(40.0), "B");
        assert_eq!(score_to_tier(41.0), "C");
        assert_eq!(score_to_tier(60.0), "C");
        assert_eq!(score_to_tier(61.0), "D");
        assert_eq!(score_to_tier(80.0), "D");
        assert_eq!(score_to_tier(81.0), "F");
        assert_eq!(score_to_tier(100.0), "F");
    }

    #[test]
    fn test_normalize() {
        assert_eq!(normalize(0.0, 2.0), 0.0);
        assert_eq!(normalize(1.0, 2.0), 50.0);
        assert_eq!(normalize(2.0, 2.0), 100.0);
        assert_eq!(normalize(4.0, 2.0), 100.0); // capped at 100
    }

    #[test]
    fn test_round2() {
        assert_eq!(round2(1.23456), 1.23);
        assert_eq!(round2(0.0), 0.0);
        assert_eq!(round2(100.0), 100.0);
    }

    #[test]
    fn test_tier_labels() {
        assert_eq!(tier_label("A"), "Low debt");
        assert_eq!(tier_label("B"), "Moderate");
        assert_eq!(tier_label("C"), "Significant");
        assert_eq!(tier_label("D"), "High");
        assert_eq!(tier_label("F"), "Critical");
    }

    #[test]
    fn test_weighted_score_calculation() {
        // All factors at 50 should produce weighted average of 50
        let score = W_COMPLEXITY * 50.0
            + W_ERROR_HANDLING * 50.0
            + W_UNWRAP * 50.0
            + W_TODO * 50.0
            + W_DOC_GAPS * 50.0
            + W_UNUSED * 50.0
            + W_PATTERN_VIOLATIONS * 50.0;
        assert!((score - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_weights_sum_to_one() {
        let sum = W_COMPLEXITY
            + W_ERROR_HANDLING
            + W_UNWRAP
            + W_TODO
            + W_DOC_GAPS
            + W_UNUSED
            + W_PATTERN_VIOLATIONS;
        assert!(
            (sum - 1.0).abs() < 0.001,
            "Weights must sum to 1.0, got {}",
            sum
        );
    }
}
