// crates/mira-server/src/cartographer/map.rs
// Codebase map generation, enrichment, and caching

use super::detection::{count_lines_in_module, detect_modules, find_entry_points, resolve_import_to_module};
use super::types::{CodebaseMap, Module};
use crate::db::Database;
use anyhow::Result;
use rusqlite::params;
use std::collections::HashSet;
use std::path::Path;

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
        conn.query_row(
            "SELECT COUNT(*) FROM codebase_modules WHERE project_id = ?",
            params![project_id],
            |row| row.get(0),
        )?
    }; // conn dropped here

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
        let mut stmt = conn.prepare(
            "SELECT module_id, name, path, purpose, exports, depends_on, symbol_count, line_count
             FROM codebase_modules WHERE project_id = ? ORDER BY module_id",
        )?;

        stmt.query_map(params![project_id], |row| {
            let exports_json: Option<String> = row.get(4)?;
            let depends_json: Option<String> = row.get(5)?;

            Ok(Module {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                purpose: row.get(3)?,
                exports: exports_json
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default(),
                depends_on: depends_json
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default(),
                symbol_count: row.get(6)?,
                line_count: row.get(7)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect()
    }; // conn dropped here

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
        let mut stmt = conn.prepare(
            "SELECT DISTINCT name FROM code_symbols
             WHERE project_id = ? AND file_path LIKE ?
             ORDER BY name LIMIT 20",
        )?;

        module.exports = stmt
            .query_map(params![project_id, pattern], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        tracing::debug!("  found {} exports", module.exports.len());

        // Get symbol count
        module.symbol_count = conn.query_row(
            "SELECT COUNT(*) FROM code_symbols WHERE project_id = ? AND file_path LIKE ?",
            params![project_id, pattern],
            |row| row.get(0),
        )?;
        tracing::debug!("  symbol_count: {}", module.symbol_count);

        // Get dependencies from imports
        let mut deps_stmt = conn.prepare(
            "SELECT DISTINCT import_path FROM imports
             WHERE project_id = ? AND file_path LIKE ? AND is_external = 0",
        )?;

        let raw_deps: Vec<String> = deps_stmt
            .query_map(params![project_id, pattern], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
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
        let exports_json = serde_json::to_string(&module.exports)?;
        let depends_json = serde_json::to_string(&module.depends_on)?;

        conn.execute(
            "INSERT OR REPLACE INTO codebase_modules
             (project_id, module_id, name, path, purpose, exports, depends_on, symbol_count, line_count, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))",
            params![
                project_id,
                module.id,
                module.name,
                module.path,
                module.purpose,
                exports_json,
                depends_json,
                module.symbol_count,
                module.line_count
            ],
        )?;
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
    let mut stmt = conn.prepare(
        "SELECT DISTINCT import_path FROM imports
         WHERE project_id = ? AND is_external = 1
         ORDER BY import_path LIMIT 30",
    )?;

    let deps: Vec<String> = stmt
        .query_map(params![project_id], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(deps)
}

/// Get all modules with their purposes for capabilities scanning
pub fn get_modules_with_purposes(db: &Database, project_id: i64) -> Result<Vec<Module>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT module_id, name, path, purpose, exports, depends_on, symbol_count, line_count
         FROM codebase_modules WHERE project_id = ? ORDER BY module_id",
    )?;

    let modules = stmt
        .query_map(params![project_id], |row| {
            let exports_json: Option<String> = row.get(4)?;
            let depends_json: Option<String> = row.get(5)?;

            Ok(Module {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                purpose: row.get(3)?,
                exports: exports_json
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default(),
                depends_on: depends_json
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default(),
                symbol_count: row.get(6)?,
                line_count: row.get(7)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(modules)
}
