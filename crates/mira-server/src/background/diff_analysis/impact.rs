// background/diff_analysis/impact.rs
// Impact analysis via call graph traversal and historical risk computation

use super::types::{HistoricalRisk, ImpactAnalysis, MatchedPattern};
use crate::db::map_files_to_symbols_sync;
use crate::proactive::PatternType;
use crate::proactive::patterns::{PatternData, get_patterns_by_type};
use crate::search::find_callers;
use std::collections::HashSet;

/// Map changed files to affected symbols in the database
pub fn map_to_symbols(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    changed_files: &[String],
) -> Vec<(String, String, String)> {
    map_files_to_symbols_sync(conn, project_id, changed_files)
}

/// Build impact analysis by traversing call graph
pub fn build_impact_graph(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    changed_symbols: &[(String, String, String)],
    max_depth: u32,
) -> ImpactAnalysis {
    let mut affected_functions: Vec<(String, String, u32)> = Vec::new();
    let mut affected_files: HashSet<String> = HashSet::new();
    let mut seen: HashSet<String> = HashSet::new();

    // Start with the changed functions
    let function_names: Vec<&str> = changed_symbols
        .iter()
        .filter(|(_, sym_type, _)| sym_type == "function" || sym_type == "method")
        .map(|(name, _, _)| name.as_str())
        .collect();

    for func_name in function_names {
        if seen.contains(func_name) {
            continue;
        }
        seen.insert(func_name.to_string());

        // Find callers at each depth level
        let mut current_level = vec![func_name.to_string()];

        for depth in 1..=max_depth {
            let mut next_level = Vec::new();

            for name in &current_level {
                let callers = find_callers(conn, project_id, name, 20);
                for caller in callers {
                    if !seen.contains(&caller.symbol_name) {
                        seen.insert(caller.symbol_name.clone());
                        affected_functions.push((
                            caller.symbol_name.clone(),
                            caller.file_path.clone(),
                            depth,
                        ));
                        affected_files.insert(caller.file_path);
                        next_level.push(caller.symbol_name);
                    }
                }
            }

            if next_level.is_empty() {
                break;
            }
            current_level = next_level;
        }
    }

    ImpactAnalysis {
        affected_functions,
        affected_files: affected_files.into_iter().collect(),
    }
}

/// Compute historical risk by matching current diff files against mined ChangePattern patterns.
///
/// This is computed LIVE at query time (never cached) so it always reflects
/// the latest mined patterns.
pub fn compute_historical_risk(
    conn: &rusqlite::Connection,
    project_id: i64,
    files: &[String],
    files_changed: i64,
) -> Option<HistoricalRisk> {
    let patterns = get_patterns_by_type(conn, project_id, &PatternType::ChangePattern, 50).ok()?;

    if patterns.is_empty() {
        return None;
    }

    let file_set: HashSet<&str> = files.iter().map(|f| f.as_str()).collect();

    // Extract top-level module for each file (same logic as mining)
    let file_modules: HashSet<&str> = files
        .iter()
        .map(|f| match f.find('/') {
            Some(idx) => &f[..idx],
            None => f.as_str(),
        })
        .collect();

    // Determine size bucket (same buckets as mining)
    let size_bucket = if files_changed <= 3 {
        "small"
    } else if files_changed <= 10 {
        "medium"
    } else {
        "large"
    };

    let mut matches = Vec::new();

    for pattern in &patterns {
        if let PatternData::ChangePattern {
            ref files,
            ref module,
            ref pattern_subtype,
            ref outcome_stats,
            ..
        } = pattern.pattern_data
        {
            let bad_rate = if outcome_stats.total > 0 {
                (outcome_stats.reverted + outcome_stats.follow_up_fix) as f64
                    / outcome_stats.total as f64
            } else {
                0.0
            };

            match pattern_subtype.as_str() {
                "module_hotspot" => {
                    if let Some(m) = module
                        && file_modules.contains(m.as_str())
                    {
                        matches.push(MatchedPattern {
                            pattern_subtype: pattern_subtype.clone(),
                            description: format!(
                                "Module '{}' has {:.0}% bad outcome rate ({} of {} changes)",
                                m,
                                bad_rate * 100.0,
                                outcome_stats.reverted + outcome_stats.follow_up_fix,
                                outcome_stats.total
                            ),
                            confidence: pattern.confidence,
                            bad_rate,
                        });
                    }
                }
                "co_change_gap" => {
                    if files.len() >= 2 {
                        let file_a = &files[0];
                        let file_b = &files[1];
                        // Flag if file_a is in diff but file_b is NOT
                        if file_set.contains(file_a.as_str()) && !file_set.contains(file_b.as_str())
                        {
                            matches.push(MatchedPattern {
                                pattern_subtype: pattern_subtype.clone(),
                                description: format!(
                                    "'{}' changed without '{}' — historically {:.0}% bad outcome rate",
                                    file_a,
                                    file_b,
                                    bad_rate * 100.0
                                ),
                                confidence: pattern.confidence,
                                bad_rate,
                            });
                        }
                        // Also check the reverse: file_b without file_a
                        if file_set.contains(file_b.as_str()) && !file_set.contains(file_a.as_str())
                        {
                            matches.push(MatchedPattern {
                                pattern_subtype: pattern_subtype.clone(),
                                description: format!(
                                    "'{}' changed without '{}' — historically {:.0}% bad outcome rate",
                                    file_b,
                                    file_a,
                                    bad_rate * 100.0
                                ),
                                confidence: pattern.confidence,
                                bad_rate,
                            });
                        }
                    }
                }
                "size_risk" => {
                    // Extract bucket from pattern_key: "size_risk:small"
                    let pattern_bucket =
                        pattern.pattern_key.strip_prefix("size_risk:").unwrap_or("");
                    if pattern_bucket == size_bucket {
                        matches.push(MatchedPattern {
                            pattern_subtype: pattern_subtype.clone(),
                            description: format!(
                                "{} changes ({} files) have {:.0}% bad outcome rate historically",
                                size_bucket.to_uppercase(),
                                files_changed,
                                bad_rate * 100.0
                            ),
                            confidence: pattern.confidence,
                            bad_rate,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    if matches.is_empty() {
        return None;
    }

    let total_confidence: f64 = matches.iter().map(|m| m.confidence).sum();
    let overall_confidence = total_confidence / matches.len() as f64;

    let risk_delta = if matches.iter().any(|m| m.confidence > 0.5) {
        "elevated".to_string()
    } else {
        "normal".to_string()
    };

    Some(HistoricalRisk {
        risk_delta,
        matching_patterns: matches,
        overall_confidence,
    })
}
