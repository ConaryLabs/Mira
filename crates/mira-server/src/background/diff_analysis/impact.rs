// background/diff_analysis/impact.rs
// Impact analysis via call graph traversal

use super::types::ImpactAnalysis;
use crate::db::map_files_to_symbols_sync;
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
                let callers = find_callers(conn, project_id, name, 20).unwrap_or_else(|e| {
                    tracing::warn!(function = %name, error = %e, "impact analysis: failed to query callers");
                    Vec::new()
                });
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
