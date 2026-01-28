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
         WHERE project_id = ? AND (purpose IS NULL OR purpose = '')",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn test_count_cached_modules_empty() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let count = count_cached_modules_sync(&conn, 1).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_get_cached_modules_empty() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let modules = get_cached_modules_sync(&conn, 1).unwrap();
        assert!(modules.is_empty());
    }

    #[test]
    fn test_upsert_and_get_module() {
        let db = Database::open_in_memory().unwrap();

        // Create a project first
        let (project_id, _) = db.get_or_create_project("/test", Some("test")).unwrap();

        let conn = db.conn();
        let module = Module {
            id: "test/mod".to_string(),
            name: "mod".to_string(),
            path: "src/mod".to_string(),
            purpose: Some("Test module".to_string()),
            exports: vec!["foo".to_string(), "bar".to_string()],
            depends_on: vec!["other/mod".to_string()],
            symbol_count: 10,
            line_count: 100,
        };

        upsert_module_sync(&conn, project_id, &module).unwrap();

        let modules = get_cached_modules_sync(&conn, project_id).unwrap();
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].id, "test/mod");
        assert_eq!(modules[0].purpose, Some("Test module".to_string()));
        assert_eq!(modules[0].exports, vec!["foo", "bar"]);
    }

    #[test]
    fn test_get_external_deps_empty() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let deps = get_external_deps_sync(&conn, 1).unwrap();
        assert!(deps.is_empty());
    }

    #[test]
    fn test_get_modules_needing_summaries_empty() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let modules = get_modules_needing_summaries_sync(&conn, 1).unwrap();
        assert!(modules.is_empty());
    }

    #[test]
    fn test_update_module_purposes_empty() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let summaries = HashMap::new();
        let updated = update_module_purposes_sync(&conn, 1, &summaries).unwrap();
        assert_eq!(updated, 0);
    }
}
