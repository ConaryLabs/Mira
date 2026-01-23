// crates/mira-server/src/cartographer/map.rs
// Codebase map generation, enrichment, and caching

use super::detection::{count_lines_in_module, detect_modules, find_entry_points, resolve_import_to_module};
use super::types::{CodebaseMap, Module};
use crate::db::{
    count_cached_modules_sync, get_cached_modules_sync, get_module_exports_sync,
    count_symbols_in_path_sync, get_module_dependencies_sync, upsert_module_sync,
    get_external_deps_sync, Database,
};
use anyhow::{Result, anyhow};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use tokio::task::spawn_blocking;

/// Get or generate codebase map
pub fn get_or_generate_map(
    db: &Database,
    project_id: i64,
    project_path: &str,
    project_name: &str,
    project_type: &str,
) -> Result<CodebaseMap> {
    tracing::info!(
        "get_or_generate_map: project_id={}, path={}",
        project_id,
        project_path
    );

    // Check if we have cached modules
    let cached_count: i64 = {
        let conn = db.conn();
        count_cached_modules_sync(&conn, project_id)?
    };

    tracing::info!("Cached modules: {}", cached_count);

    if cached_count == 0 {
        // Generate fresh using polyglot detection
        let path = Path::new(project_path);
        let modules = detect_modules(path, project_type);

        // Enrich with database data and store
        let enriched = enrich_and_store_modules(db, project_id, modules, path, project_type)?;

        return Ok(CodebaseMap {
            name: project_name.to_string(),
            project_type: project_type.to_string(),
            modules: enriched,
            entry_points: find_entry_points(path, project_type),
            external_deps: get_external_deps(db, project_id)?,
            updated_at: chrono::Utc::now().to_rfc3339(),
        });
    }

    // Load from cache
    let modules: Vec<Module> = {
        let conn = db.conn();
        get_cached_modules_sync(&conn, project_id)?
    };

    Ok(CodebaseMap {
        name: project_name.to_string(),
        project_type: project_type.to_string(),
        modules,
        entry_points: find_entry_points(Path::new(project_path), project_type),
        external_deps: get_external_deps(db, project_id)?,
        updated_at: chrono::Utc::now().to_rfc3339(),
    })
}

fn enrich_and_store_modules(
    db: &Database,
    project_id: i64,
    mut modules: Vec<Module>,
    project_path: &Path,
    project_type: &str,
) -> Result<Vec<Module>> {
    tracing::info!(
        "enrich_and_store_modules: starting with {} modules",
        modules.len()
    );
    let conn = db.conn();

    // First pass: collect exports, symbol counts, line counts, raw deps
    let mut raw_deps_per_module: Vec<Vec<String>> = Vec::with_capacity(modules.len());
    let total_modules = modules.len();

    for (i, module) in modules.iter_mut().enumerate() {
        tracing::debug!(
            "Module {}/{}: {} (path={})",
            i + 1,
            total_modules,
            module.id,
            module.path
        );

        // Get exports (pub symbols in this module's path)
        let pattern = format!("{}%", module.path);
        module.exports = get_module_exports_sync(&conn, project_id, &pattern, 20)?;
        tracing::debug!("  found {} exports", module.exports.len());

        // Get symbol count
        module.symbol_count = count_symbols_in_path_sync(&conn, project_id, &pattern)?;
        tracing::debug!("  symbol_count: {}", module.symbol_count);

        // Get dependencies from imports
        let raw_deps = get_module_dependencies_sync(&conn, project_id, &pattern)?;
        tracing::debug!("  found {} deps", raw_deps.len());
        raw_deps_per_module.push(raw_deps);

        // Get line count from files (polyglot)
        tracing::debug!("  counting lines...");
        module.line_count = count_lines_in_module(project_path, &module.path, project_type);
        tracing::debug!("  line_count: {}", module.line_count);

        // Generate purpose heuristic
        if module.purpose.is_none() {
            module.purpose = generate_purpose_heuristic(&module.name, &module.exports);
        }
        tracing::debug!("  done with module");
    }

    // Second pass: resolve dependencies (needs immutable access to modules)
    // Create a snapshot of module IDs for dependency resolution
    let module_ids: Vec<(String, String)> = modules
        .iter()
        .map(|m| (m.id.clone(), m.name.clone()))
        .collect();

    for (i, module) in modules.iter_mut().enumerate() {
        module.depends_on = raw_deps_per_module[i]
            .iter()
            .filter_map(|import| resolve_import_to_module(import, &module_ids, project_type))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        // Store in database
        upsert_module_sync(&conn, project_id, module)?;
    }

    Ok(modules)
}

fn generate_purpose_heuristic(name: &str, exports: &[String]) -> Option<String> {
    // Check common module names
    let lower = name.to_lowercase();
    let purpose = match lower.as_str() {
        "db" | "database" => "Database operations and queries",
        "api" => "API endpoint handlers",
        "web" => "HTTP server and routes",
        "mcp" => "MCP protocol implementation",
        "models" | "types" => "Data type definitions",
        "utils" | "helpers" => "Utility functions",
        "auth" | "authentication" => "Authentication and authorization",
        "config" => "Configuration management",
        "handlers" => "Request/event handlers",
        "middleware" => "Middleware components",
        "routes" | "routing" => "Route definitions",
        "indexer" => "Code indexing and analysis",
        "embeddings" => "Vector embeddings",
        "cartographer" => "Codebase structure mapping",
        "hooks" => "Event hooks and callbacks",
        "tools" => "Tool implementations",
        "parsers" => "Code parsing",
        "tests" | "test" => "Test suites",
        _ => {
            // Try to infer from exports
            if exports
                .iter()
                .any(|e| e.contains("Test") || e.contains("test"))
            {
                "Test utilities"
            } else if exports.iter().any(|e| e.contains("Error")) {
                "Error types and handling"
            } else if exports.iter().any(|e| e.contains("Config")) {
                "Configuration"
            } else {
                return None;
            }
        }
    };
    Some(purpose.to_string())
}

fn get_external_deps(db: &Database, project_id: i64) -> Result<Vec<String>> {
    let conn = db.conn();
    Ok(get_external_deps_sync(&conn, project_id)?)
}

/// Get all modules with their purposes for capabilities scanning
pub fn get_modules_with_purposes(db: &Database, project_id: i64) -> Result<Vec<Module>> {
    let conn = db.conn();
    Ok(get_cached_modules_sync(&conn, project_id)?)
}

/// Async version of get_or_generate_map that runs on a blocking thread
pub async fn get_or_generate_map_async(
    db: Arc<Database>,
    project_id: i64,
    project_path: &str,
    project_name: &str,
    project_type: &str,
) -> Result<CodebaseMap> {
    let project_path = project_path.to_string();
    let project_name = project_name.to_string();
    let project_type = project_type.to_string();

    spawn_blocking(move || {
        get_or_generate_map(&db, project_id, &project_path, &project_name, &project_type)
    })
    .await
    .map_err(|e| anyhow!("spawn_blocking panicked: {}", e))?
}

/// Async version of get_modules_with_purposes that runs on a blocking thread
pub async fn get_modules_with_purposes_async(
    db: Arc<Database>,
    project_id: i64,
) -> Result<Vec<Module>> {
    spawn_blocking(move || {
        get_modules_with_purposes(&db, project_id)
    })
    .await
    .map_err(|e| anyhow!("spawn_blocking panicked: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // generate_purpose_heuristic tests
    // ============================================================================

    #[test]
    fn test_purpose_heuristic_database() {
        assert_eq!(
            generate_purpose_heuristic("db", &[]),
            Some("Database operations and queries".to_string())
        );
        assert_eq!(
            generate_purpose_heuristic("database", &[]),
            Some("Database operations and queries".to_string())
        );
    }

    #[test]
    fn test_purpose_heuristic_api_web() {
        assert_eq!(
            generate_purpose_heuristic("api", &[]),
            Some("API endpoint handlers".to_string())
        );
        assert_eq!(
            generate_purpose_heuristic("web", &[]),
            Some("HTTP server and routes".to_string())
        );
    }

    #[test]
    fn test_purpose_heuristic_mcp() {
        assert_eq!(
            generate_purpose_heuristic("mcp", &[]),
            Some("MCP protocol implementation".to_string())
        );
    }

    #[test]
    fn test_purpose_heuristic_types() {
        assert_eq!(
            generate_purpose_heuristic("models", &[]),
            Some("Data type definitions".to_string())
        );
        assert_eq!(
            generate_purpose_heuristic("types", &[]),
            Some("Data type definitions".to_string())
        );
    }

    #[test]
    fn test_purpose_heuristic_utils() {
        assert_eq!(
            generate_purpose_heuristic("utils", &[]),
            Some("Utility functions".to_string())
        );
        assert_eq!(
            generate_purpose_heuristic("helpers", &[]),
            Some("Utility functions".to_string())
        );
    }

    #[test]
    fn test_purpose_heuristic_auth() {
        assert_eq!(
            generate_purpose_heuristic("auth", &[]),
            Some("Authentication and authorization".to_string())
        );
        assert_eq!(
            generate_purpose_heuristic("authentication", &[]),
            Some("Authentication and authorization".to_string())
        );
    }

    #[test]
    fn test_purpose_heuristic_config() {
        assert_eq!(
            generate_purpose_heuristic("config", &[]),
            Some("Configuration management".to_string())
        );
    }

    #[test]
    fn test_purpose_heuristic_mira_modules() {
        assert_eq!(
            generate_purpose_heuristic("indexer", &[]),
            Some("Code indexing and analysis".to_string())
        );
        assert_eq!(
            generate_purpose_heuristic("embeddings", &[]),
            Some("Vector embeddings".to_string())
        );
        assert_eq!(
            generate_purpose_heuristic("cartographer", &[]),
            Some("Codebase structure mapping".to_string())
        );
        assert_eq!(
            generate_purpose_heuristic("hooks", &[]),
            Some("Event hooks and callbacks".to_string())
        );
    }

    #[test]
    fn test_purpose_heuristic_test_modules() {
        assert_eq!(
            generate_purpose_heuristic("tests", &[]),
            Some("Test suites".to_string())
        );
        assert_eq!(
            generate_purpose_heuristic("test", &[]),
            Some("Test suites".to_string())
        );
    }

    #[test]
    fn test_purpose_heuristic_from_exports_test() {
        let exports = vec!["TestHelper".to_string(), "MockDb".to_string()];
        assert_eq!(
            generate_purpose_heuristic("unknown_module", &exports),
            Some("Test utilities".to_string())
        );
    }

    #[test]
    fn test_purpose_heuristic_from_exports_error() {
        let exports = vec!["DatabaseError".to_string(), "NetworkError".to_string()];
        assert_eq!(
            generate_purpose_heuristic("errors", &exports),
            Some("Error types and handling".to_string())
        );
    }

    #[test]
    fn test_purpose_heuristic_from_exports_config() {
        let exports = vec!["AppConfig".to_string(), "Settings".to_string()];
        assert_eq!(
            generate_purpose_heuristic("settings", &exports),
            Some("Configuration".to_string())
        );
    }

    #[test]
    fn test_purpose_heuristic_unknown() {
        let exports = vec!["SomeFunction".to_string(), "AnotherStruct".to_string()];
        assert_eq!(generate_purpose_heuristic("random_module", &exports), None);
    }

    #[test]
    fn test_purpose_heuristic_case_insensitive() {
        assert_eq!(
            generate_purpose_heuristic("DB", &[]),
            Some("Database operations and queries".to_string())
        );
        assert_eq!(
            generate_purpose_heuristic("Api", &[]),
            Some("API endpoint handlers".to_string())
        );
        assert_eq!(
            generate_purpose_heuristic("CONFIG", &[]),
            Some("Configuration management".to_string())
        );
    }

    // ============================================================================
    // Module type tests
    // ============================================================================

    #[test]
    fn test_module_default_values() {
        let module = Module {
            id: "test".to_string(),
            name: "test".to_string(),
            path: "src/test".to_string(),
            purpose: None,
            exports: vec![],
            depends_on: vec![],
            symbol_count: 0,
            line_count: 0,
        };
        assert!(module.purpose.is_none());
        assert!(module.exports.is_empty());
        assert!(module.depends_on.is_empty());
    }

    #[test]
    fn test_module_with_data() {
        let module = Module {
            id: "mira/db".to_string(),
            name: "db".to_string(),
            path: "crates/mira-server/src/db".to_string(),
            purpose: Some("Database operations".to_string()),
            exports: vec!["Database".to_string(), "Pool".to_string()],
            depends_on: vec!["mira/config".to_string()],
            symbol_count: 50,
            line_count: 1000,
        };
        assert_eq!(module.id, "mira/db");
        assert_eq!(module.exports.len(), 2);
        assert_eq!(module.depends_on.len(), 1);
    }
}
