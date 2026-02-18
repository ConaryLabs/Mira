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
        "SELECT module_id, name, path, purpose, exports, depends_on, symbol_count, line_count, detected_patterns
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
                detected_patterns: row.get(8)?,
            })
        })?
        .filter_map(super::log_and_discard)
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
         WHERE project_id = ? AND file_path LIKE ? ESCAPE '\\'
         ORDER BY name LIMIT ?",
    )?;

    let exports = stmt
        .query_map(params![project_id, path_pattern, limit as i64], |row| {
            row.get(0)
        })?
        .filter_map(super::log_and_discard)
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
        "SELECT COUNT(*) FROM code_symbols WHERE project_id = ? AND file_path LIKE ? ESCAPE '\\'",
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
         WHERE project_id = ? AND file_path LIKE ? ESCAPE '\\' AND is_external = 0",
    )?;

    let deps = stmt
        .query_map(params![project_id, path_pattern], |row| row.get(0))?
        .filter_map(super::log_and_discard)
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
        .filter_map(super::log_and_discard)
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
        .filter_map(super::log_and_discard)
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
    use crate::cartographer::Module;
    use crate::db::test_support::setup_test_connection;

    /// Set up test connection with code schema tables needed by cartographer
    fn setup_conn_with_code_schema() -> (Connection, i64) {
        let conn = setup_test_connection();
        // Create code tables that normally live in the code DB
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS code_symbols (
                id INTEGER PRIMARY KEY,
                project_id INTEGER NOT NULL,
                file_path TEXT NOT NULL,
                name TEXT NOT NULL,
                symbol_type TEXT NOT NULL,
                start_line INTEGER,
                end_line INTEGER,
                signature TEXT,
                indexed_at TEXT DEFAULT CURRENT_TIMESTAMP
            );
            CREATE INDEX IF NOT EXISTS idx_symbols_name ON code_symbols(name);
            CREATE TABLE IF NOT EXISTS imports (
                id INTEGER PRIMARY KEY,
                project_id INTEGER NOT NULL,
                file_path TEXT NOT NULL,
                import_path TEXT NOT NULL,
                is_external INTEGER DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS codebase_modules (
                id INTEGER PRIMARY KEY,
                project_id INTEGER NOT NULL,
                module_id TEXT NOT NULL,
                name TEXT NOT NULL,
                path TEXT NOT NULL,
                purpose TEXT,
                exports TEXT,
                depends_on TEXT,
                symbol_count INTEGER DEFAULT 0,
                line_count INTEGER DEFAULT 0,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                detected_patterns TEXT,
                UNIQUE(project_id, module_id)
            );",
        )
        .unwrap();
        let (pid, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();
        (conn, pid)
    }

    fn make_module(id: &str, name: &str) -> Module {
        Module {
            id: id.to_string(),
            name: name.to_string(),
            path: format!("src/{}", name),
            purpose: Some("Test module".to_string()),
            exports: vec!["export_a".to_string(), "export_b".to_string()],
            depends_on: vec!["other_mod".to_string()],
            symbol_count: 15,
            line_count: 200,
            detected_patterns: None,
        }
    }

    #[test]
    fn test_upsert_module_and_get_cached() {
        let (conn, pid) = setup_conn_with_code_schema();
        let module = make_module("db", "database");

        upsert_module_sync(&conn, pid, &module).unwrap();

        let cached = get_cached_modules_sync(&conn, pid).unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].id, "db");
        assert_eq!(cached[0].name, "database");
        assert_eq!(cached[0].path, "src/database");
        assert_eq!(cached[0].purpose.as_deref(), Some("Test module"));
        assert_eq!(cached[0].exports, vec!["export_a", "export_b"]);
        assert_eq!(cached[0].depends_on, vec!["other_mod"]);
        assert_eq!(cached[0].symbol_count, 15);
        assert_eq!(cached[0].line_count, 200);
    }

    #[test]
    fn test_upsert_module_replaces_on_conflict() {
        let (conn, pid) = setup_conn_with_code_schema();

        let mut module = make_module("db", "database");
        upsert_module_sync(&conn, pid, &module).unwrap();

        module.purpose = Some("Updated purpose".to_string());
        module.symbol_count = 30;
        upsert_module_sync(&conn, pid, &module).unwrap();

        let cached = get_cached_modules_sync(&conn, pid).unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].purpose.as_deref(), Some("Updated purpose"));
        assert_eq!(cached[0].symbol_count, 30);
    }

    #[test]
    fn test_count_cached_modules() {
        let (conn, pid) = setup_conn_with_code_schema();

        assert_eq!(count_cached_modules_sync(&conn, pid).unwrap(), 0);

        upsert_module_sync(&conn, pid, &make_module("a", "mod_a")).unwrap();
        upsert_module_sync(&conn, pid, &make_module("b", "mod_b")).unwrap();

        assert_eq!(count_cached_modules_sync(&conn, pid).unwrap(), 2);
    }

    #[test]
    fn test_get_module_exports() {
        let (conn, pid) = setup_conn_with_code_schema();

        // Seed code_symbols for the path
        conn.execute(
            "INSERT INTO code_symbols (project_id, file_path, name, symbol_type, start_line, end_line)
             VALUES (?1, 'src/db/pool.rs', 'DatabasePool', 'struct', 1, 50)",
            params![pid],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO code_symbols (project_id, file_path, name, symbol_type, start_line, end_line)
             VALUES (?1, 'src/db/pool.rs', 'open', 'function', 10, 30)",
            params![pid],
        )
        .unwrap();

        let exports = get_module_exports_sync(&conn, pid, "src/db/%", 10).unwrap();
        assert_eq!(exports.len(), 2);
        assert!(exports.contains(&"DatabasePool".to_string()));
        assert!(exports.contains(&"open".to_string()));
    }

    #[test]
    fn test_count_symbols_in_path() {
        let (conn, pid) = setup_conn_with_code_schema();

        conn.execute(
            "INSERT INTO code_symbols (project_id, file_path, name, symbol_type, start_line, end_line)
             VALUES (?1, 'src/db/pool.rs', 'fn_a', 'function', 1, 10)",
            params![pid],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO code_symbols (project_id, file_path, name, symbol_type, start_line, end_line)
             VALUES (?1, 'src/db/types.rs', 'fn_b', 'function', 1, 10)",
            params![pid],
        )
        .unwrap();

        let count = count_symbols_in_path_sync(&conn, pid, "src/db/%").unwrap();
        assert_eq!(count, 2);

        let count_other = count_symbols_in_path_sync(&conn, pid, "src/other/%").unwrap();
        assert_eq!(count_other, 0);
    }

    #[test]
    fn test_get_module_dependencies() {
        let (conn, pid) = setup_conn_with_code_schema();

        conn.execute(
            "INSERT INTO imports (project_id, file_path, import_path, is_external)
             VALUES (?1, 'src/db/pool.rs', 'crate::config', 0)",
            params![pid],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO imports (project_id, file_path, import_path, is_external)
             VALUES (?1, 'src/db/pool.rs', 'tokio', 1)",
            params![pid],
        )
        .unwrap();

        let deps = get_module_dependencies_sync(&conn, pid, "src/db/%").unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0], "crate::config");
    }

    #[test]
    fn test_get_external_deps() {
        let (conn, pid) = setup_conn_with_code_schema();

        conn.execute(
            "INSERT INTO imports (project_id, file_path, import_path, is_external)
             VALUES (?1, 'src/main.rs', 'tokio', 1)",
            params![pid],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO imports (project_id, file_path, import_path, is_external)
             VALUES (?1, 'src/main.rs', 'serde', 1)",
            params![pid],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO imports (project_id, file_path, import_path, is_external)
             VALUES (?1, 'src/main.rs', 'crate::db', 0)",
            params![pid],
        )
        .unwrap();

        let deps = get_external_deps_sync(&conn, pid).unwrap();
        assert_eq!(deps.len(), 2);
        assert!(deps.contains(&"tokio".to_string()));
        assert!(deps.contains(&"serde".to_string()));
    }

    #[test]
    fn test_get_modules_needing_summaries() {
        let (conn, pid) = setup_conn_with_code_schema();

        // Module with purpose
        upsert_module_sync(&conn, pid, &make_module("a", "has_purpose")).unwrap();

        // Module without purpose
        let mut no_purpose = make_module("b", "no_purpose");
        no_purpose.purpose = None;
        upsert_module_sync(&conn, pid, &no_purpose).unwrap();

        // Module with heuristic purpose
        let mut heuristic = make_module("c", "heuristic_purpose");
        heuristic.purpose = Some("[heuristic] Some guess".to_string());
        upsert_module_sync(&conn, pid, &heuristic).unwrap();

        let needing = get_modules_needing_summaries_sync(&conn, pid).unwrap();
        assert_eq!(needing.len(), 2);
        let ids: Vec<&str> = needing.iter().map(|m| m.module_id.as_str()).collect();
        assert!(ids.contains(&"b"));
        assert!(ids.contains(&"c"));
    }

    #[test]
    fn test_update_module_purposes() {
        let (conn, pid) = setup_conn_with_code_schema();

        let mut no_purpose = make_module("a", "mod_a");
        no_purpose.purpose = None;
        upsert_module_sync(&conn, pid, &no_purpose).unwrap();

        let mut summaries = HashMap::new();
        summaries.insert("a".to_string(), "Handles authentication".to_string());

        let updated = update_module_purposes_sync(&conn, pid, &summaries).unwrap();
        assert_eq!(updated, 1);

        let cached = get_cached_modules_sync(&conn, pid).unwrap();
        assert_eq!(cached[0].purpose.as_deref(), Some("Handles authentication"));
    }
}
