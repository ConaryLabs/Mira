// crates/mira-server/src/background/summaries.rs
// Heuristic module summary generation

use super::HEURISTIC_PREFIX;
use crate::cartographer;
use crate::db::pool::DatabasePool;
use crate::db::{
    get_modules_needing_summaries_sync, get_project_ids_needing_summaries_sync,
    get_project_paths_by_ids_sync, update_module_purposes_sync,
};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Maximum summaries to process per batch
const BATCH_SIZE: usize = 5;

/// Max exports to list in heuristic summary
const FALLBACK_MAX_EXPORTS: usize = 10;

/// Process pending summaries.
///
/// - `code_pool`: for reading/writing codebase_modules
/// - `main_pool`: for project path lookup
pub async fn process_queue(
    code_pool: &Arc<DatabasePool>,
    main_pool: &Arc<DatabasePool>,
) -> Result<usize, String> {
    // Step 1: Get project IDs with pending summaries (from code DB)
    let project_ids = code_pool
        .run(get_project_ids_needing_summaries_sync)
        .await?;

    if project_ids.is_empty() {
        return Ok(0);
    }

    // Step 2: Get project paths from main DB
    let ids_clone = project_ids.clone();
    let projects = main_pool
        .run(move |conn| get_project_paths_by_ids_sync(conn, &ids_clone))
        .await?;

    if projects.is_empty() {
        return Ok(0);
    }

    let mut total_processed = 0;

    for (project_id, project_path) in projects {
        // Get modules needing summaries for this project (from code DB)
        let mut modules = code_pool
            .run(move |conn| get_modules_needing_summaries_sync(conn, project_id))
            .await?;

        if modules.is_empty() {
            continue;
        }

        // Limit to batch size
        modules.truncate(BATCH_SIZE);

        tracing::info!(
            "Found {} modules needing summaries for project {}",
            modules.len(),
            project_id
        );

        // Fill in code previews
        let path = Path::new(&project_path);
        for module in &mut modules {
            module.code_preview = cartographer::get_module_code_preview(path, &module.path);
        }

        // Heuristic fallback
        let summaries = generate_heuristic_summaries(&modules);
        if !summaries.is_empty() {
            match code_pool
                .interact(move |conn| {
                    update_module_purposes_sync(conn, project_id, &summaries)
                        .map_err(|e| anyhow::anyhow!("Failed to update: {}", e))
                })
                .await
            {
                Ok(count) => {
                    tracing::info!(
                        "Updated {} heuristic module summaries for project {}",
                        count,
                        project_id
                    );
                    total_processed += count;
                }
                Err(e) => {
                    tracing::warn!("Failed to update heuristic summaries: {}", e);
                }
            }
        }
    }

    Ok(total_processed)
}

/// Generate heuristic summaries from module metadata (no LLM required)
pub(crate) fn generate_heuristic_summaries(
    modules: &[cartographer::ModuleSummaryContext],
) -> HashMap<String, String> {
    let mut summaries = HashMap::new();

    for module in modules {
        let exports_display: Vec<&str> = module
            .exports
            .iter()
            .take(FALLBACK_MAX_EXPORTS)
            .map(|s| s.as_str())
            .collect();

        let mut summary = format!(
            "{}{} module ({} lines)",
            HEURISTIC_PREFIX, module.name, module.line_count,
        );

        if !exports_display.is_empty() {
            summary.push_str(&format!(". Exports: {}", exports_display.join(", ")));
            if module.exports.len() > FALLBACK_MAX_EXPORTS {
                summary.push_str(&format!(
                    " (+{} more)",
                    module.exports.len() - FALLBACK_MAX_EXPORTS
                ));
            }
        }

        summaries.insert(module.module_id.clone(), summary);
    }

    summaries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartographer::ModuleSummaryContext;

    #[test]
    fn test_heuristic_summary_with_exports() {
        let modules = vec![ModuleSummaryContext {
            module_id: "background/fuzzy".to_string(),
            name: "fuzzy".to_string(),
            path: "src/background/fuzzy".to_string(),
            exports: vec![
                "FuzzyCache".to_string(),
                "FuzzyCodeResult".to_string(),
                "FuzzyMemoryResult".to_string(),
            ],
            code_preview: String::new(),
            line_count: 329,
        }];

        let summaries = generate_heuristic_summaries(&modules);
        assert_eq!(summaries.len(), 1);

        let summary = summaries.get("background/fuzzy").unwrap();
        assert!(summary.starts_with(HEURISTIC_PREFIX));
        assert!(summary.contains("fuzzy module"));
        assert!(summary.contains("329 lines"));
        assert!(summary.contains("FuzzyCache"));
        assert!(summary.contains("FuzzyCodeResult"));
    }

    #[test]
    fn test_heuristic_summary_no_exports() {
        let modules = vec![ModuleSummaryContext {
            module_id: "main".to_string(),
            name: "main".to_string(),
            path: "src/main.rs".to_string(),
            exports: vec![],
            code_preview: String::new(),
            line_count: 50,
        }];

        let summaries = generate_heuristic_summaries(&modules);
        let summary = summaries.get("main").unwrap();
        assert!(summary.starts_with(HEURISTIC_PREFIX));
        assert!(summary.contains("50 lines"));
        assert!(!summary.contains("Exports"));
    }

    #[test]
    fn test_heuristic_summary_many_exports_truncated() {
        let exports: Vec<String> = (0..15).map(|i| format!("Export{}", i)).collect();
        let modules = vec![ModuleSummaryContext {
            module_id: "big".to_string(),
            name: "big".to_string(),
            path: "src/big".to_string(),
            exports,
            code_preview: String::new(),
            line_count: 1000,
        }];

        let summaries = generate_heuristic_summaries(&modules);
        let summary = summaries.get("big").unwrap();
        assert!(summary.contains("(+5 more)"));
    }
}
