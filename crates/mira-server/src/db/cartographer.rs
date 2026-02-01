// crates/mira-server/src/db/cartographer.rs
// Database operations for codebase module mapping
//
// Consolidates cartographer-related SQL from:
// - cartographer/map.rs
// - cartographer/summaries.rs

use crate::cartographer::{Module, ModuleSummaryContext};
use rusqlite::{Connection, params};
use std::collections::HashMap;

/// Count cached modules for a project
pub fn count_cached_modules_sync(conn: &Connection, project_id: i64) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM codebase_modules WHERE project_id = ?",
        params![project_id],
        |row| row.get(0),
    )
}

/// Get all cached modules for a project
pub fn get_cached_modules_sync(
    conn: &Connection,
    project_id: i64,
) -> rusqlite::Result<Vec<Module>> {
    let mut stmt = conn.prepare(
        "SELECT module_id, name, path, purpose, exports, depends_on, symbol_count, line_count
         FROM codebase_modules WHERE project_id = ? ORDER BY module_id",
    )?;

    let modules = stmt
        .query_map(params![project_id], |row| {
            let exports_json: String = row.get(4)?;
            let depends_json: String = row.get(5)?;
            Ok(Module {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                purpose: row.get(3)?,
                exports: serde_json::from_str(&exports_json).unwrap_or_default(),
                depends_on: serde_json::from_str(&depends_json).unwrap_or_default(),
                symbol_count: row.get(6)?,
                line_count: row.get(7)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(modules)
}

/// Get export symbols for files in a module path
pub fn get_module_exports_sync(
    conn: &Connection,
    project_id: i64,
    path_pattern: &str,
    limit: usize,
) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT name FROM code_symbols
         WHERE project_id = ? AND file_path LIKE ?
         ORDER BY name LIMIT ?",
    )?;

    let exports = stmt
        .query_map(params![project_id, path_pattern, limit as i64], |row| {
            row.get(0)
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(exports)
}

/// Count symbols in files matching a path pattern
pub fn count_symbols_in_path_sync(
    conn: &Connection,
    project_id: i64,
    path_pattern: &str,
) -> rusqlite::Result<u32> {
    conn.query_row(
        "SELECT COUNT(*) FROM code_symbols WHERE project_id = ? AND file_path LIKE ?",
        params![project_id, path_pattern],
        |row| row.get(0),
    )
}

/// Get internal dependencies for files in a module path
pub fn get_module_dependencies_sync(
    conn: &Connection,
    project_id: i64,
    path_pattern: &str,
) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT import_path FROM imports
         WHERE project_id = ? AND file_path LIKE ? AND is_external = 0",
    )?;

    let deps = stmt
        .query_map(params![project_id, path_pattern], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(deps)
}

/// Insert or update a module in the cache
pub fn upsert_module_sync(
    conn: &Connection,
    project_id: i64,
    module: &Module,
) -> rusqlite::Result<()> {
    let exports_json = serde_json::to_string(&module.exports).unwrap_or_else(|_| "[]".to_string());
    let depends_json =
        serde_json::to_string(&module.depends_on).unwrap_or_else(|_| "[]".to_string());

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
            module.line_count,
        ],
    )?;

    Ok(())
}

/// Get external dependencies for a project
pub fn get_external_deps_sync(conn: &Connection, project_id: i64) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT import_path FROM imports
         WHERE project_id = ? AND is_external = 1
         ORDER BY import_path LIMIT 30",
    )?;

    let deps = stmt
        .query_map(params![project_id], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(deps)
}

/// Get modules that need summaries (no purpose or empty purpose)
pub fn get_modules_needing_summaries_sync(
    conn: &Connection,
    project_id: i64,
) -> rusqlite::Result<Vec<ModuleSummaryContext>> {
    let mut stmt = conn.prepare(
        "SELECT module_id, name, path, exports, line_count
         FROM codebase_modules
         WHERE project_id = ? AND (purpose IS NULL OR purpose = '' OR purpose LIKE '[heuristic] %')",
    )?;

    let modules = stmt
        .query_map(params![project_id], |row| {
            let exports_json: String = row.get(3)?;
            Ok(ModuleSummaryContext {
                module_id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                exports: serde_json::from_str(&exports_json).unwrap_or_default(),
                code_preview: String::new(), // Filled in by caller
                line_count: row.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(modules)
}

/// Update module purposes from a map of module_id -> purpose
pub fn update_module_purposes_sync(
    conn: &Connection,
    project_id: i64,
    summaries: &HashMap<String, String>,
) -> rusqlite::Result<usize> {
    let mut updated = 0;

    for (module_id, purpose) in summaries {
        let rows = conn.execute(
            "UPDATE codebase_modules SET purpose = ?, updated_at = datetime('now')
             WHERE project_id = ? AND module_id = ?",
            params![purpose, project_id, module_id],
        )?;
        updated += rows;
    }

    Ok(updated)
}
