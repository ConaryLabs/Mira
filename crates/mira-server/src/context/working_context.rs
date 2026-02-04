// context/working_context.rs
// Detect which modules Claude is currently working in based on session activity.
//
// Uses session_behavior_log (file_access events from PostToolUse hook) to determine
// recently-touched files, then maps them to modules via module_conventions table.

use crate::utils::ResultExt;
use rusqlite::Connection;

/// A module Claude is currently working in
#[derive(Debug, Clone)]
pub struct WorkingModule {
    pub module_path: String,
    pub _module_id: String,
}

/// Detect working modules from recent file access events.
///
/// Strategy:
/// 1. Query session_behavior_log for recent file_access events
/// 2. Normalize absolute paths by stripping project_path prefix
/// 3. Map files to modules using longest-prefix match against module_conventions
/// 4. Return top 3 modules by recency
pub fn detect_working_modules(
    conn: &Connection,
    project_id: i64,
    session_id: &str,
    project_path: Option<&str>,
) -> Vec<WorkingModule> {
    // Get recent file access paths from session behavior log
    let recent_files = match get_recent_file_paths(conn, project_id, session_id) {
        Ok(files) => files,
        Err(e) => {
            tracing::debug!("Failed to get recent file paths: {}", e);
            return Vec::new();
        }
    };

    if recent_files.is_empty() {
        return Vec::new();
    }

    // Get all module paths for this project from module_conventions
    let module_map = match get_module_paths(conn, project_id) {
        Ok(modules) => modules,
        Err(e) => {
            tracing::debug!("Failed to get module paths: {}", e);
            return Vec::new();
        }
    };

    if module_map.is_empty() {
        return Vec::new();
    }

    // Map files to modules, preserving order (most recent first)
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();

    for file_path in &recent_files {
        // Normalize: strip project_path prefix to get relative path
        let relative = match project_path {
            Some(prefix) => file_path
                .strip_prefix(prefix)
                .unwrap_or(file_path)
                .trim_start_matches('/'),
            None => file_path.as_str(),
        };

        // Longest-prefix match against module paths
        if let Some((module_path, module_id)) = find_module_for_file(relative, &module_map)
            && seen.insert(module_path.clone()) {
                result.push(WorkingModule {
                    module_path,
                    _module_id: module_id,
                });
                if result.len() >= 3 {
                    break;
                }
            }
    }

    result
}

/// Get recent file paths from session_behavior_log
fn get_recent_file_paths(
    conn: &Connection,
    project_id: i64,
    session_id: &str,
) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT json_extract(event_data, '$.file_path') as fp
             FROM session_behavior_log
             WHERE project_id = ? AND session_id = ? AND event_type = 'file_access'
             AND fp IS NOT NULL
             ORDER BY created_at DESC
             LIMIT 10",
        )
        .str_err()?;

    let paths = stmt
        .query_map(rusqlite::params![project_id, session_id], |row| {
            row.get::<_, String>(0)
        })
        .str_err()?
        .filter_map(|r| r.ok())
        .collect();

    Ok(paths)
}

/// Get all module (module_id, module_path) pairs from module_conventions
fn get_module_paths(conn: &Connection, project_id: i64) -> Result<Vec<(String, String)>, String> {
    let mut stmt = conn
        .prepare("SELECT module_path, module_id FROM module_conventions WHERE project_id = ?")
        .str_err()?;

    let modules = stmt
        .query_map([project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .str_err()?
        .filter_map(|r| r.ok())
        .collect();

    Ok(modules)
}

/// Find the best module for a relative file path using longest-prefix match
fn find_module_for_file(
    relative_path: &str,
    modules: &[(String, String)],
) -> Option<(String, String)> {
    modules
        .iter()
        .filter(|(module_path, _)| relative_path.starts_with(module_path.as_str()))
        .max_by_key(|(module_path, _)| module_path.len())
        .map(|(path, id)| (path.clone(), id.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_module_for_file() {
        let modules = vec![
            (
                "crates/mira-server/src".to_string(),
                "mira-server".to_string(),
            ),
            (
                "crates/mira-server/src/context".to_string(),
                "context".to_string(),
            ),
            (
                "crates/mira-server/src/background".to_string(),
                "background".to_string(),
            ),
        ];

        // Should match most specific module
        let result = find_module_for_file("crates/mira-server/src/context/convention.rs", &modules);
        assert!(result.is_some());
        let (path, id) = result.unwrap();
        assert_eq!(id, "context");
        assert_eq!(path, "crates/mira-server/src/context");

        // Should match broader module
        let result = find_module_for_file("crates/mira-server/src/main.rs", &modules);
        assert!(result.is_some());
        assert_eq!(result.unwrap().1, "mira-server");

        // No match
        let result = find_module_for_file("some/other/path.rs", &modules);
        assert!(result.is_none());
    }

    #[test]
    fn test_path_normalization() {
        let abs_path = "/home/peter/Mira/crates/mira-server/src/context/mod.rs";
        let project_path = "/home/peter/Mira/";

        let relative = abs_path
            .strip_prefix(project_path)
            .unwrap_or(abs_path)
            .trim_start_matches('/');

        assert_eq!(relative, "crates/mira-server/src/context/mod.rs");
    }

    #[test]
    fn test_path_normalization_no_trailing_slash() {
        let abs_path = "/home/peter/Mira/crates/mira-server/src/context/mod.rs";
        let project_path = "/home/peter/Mira";

        let relative = abs_path
            .strip_prefix(project_path)
            .unwrap_or(abs_path)
            .trim_start_matches('/');

        assert_eq!(relative, "crates/mira-server/src/context/mod.rs");
    }

    #[test]
    fn test_detect_no_history() {
        // With an in-memory DB that has no data, should return empty
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session_behavior_log (
                id INTEGER PRIMARY KEY,
                project_id INTEGER,
                session_id TEXT,
                event_type TEXT,
                event_data TEXT,
                sequence_position INTEGER,
                time_since_last_event_ms INTEGER,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE module_conventions (
                id INTEGER PRIMARY KEY,
                project_id INTEGER,
                module_id TEXT,
                module_path TEXT,
                UNIQUE(project_id, module_path)
            );",
        )
        .unwrap();

        let result = detect_working_modules(&conn, 1, "test-session", Some("/project"));
        assert!(result.is_empty());
    }

    #[test]
    fn test_detect_from_behavior_log() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session_behavior_log (
                id INTEGER PRIMARY KEY,
                project_id INTEGER,
                session_id TEXT,
                event_type TEXT,
                event_data TEXT,
                sequence_position INTEGER,
                time_since_last_event_ms INTEGER,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE module_conventions (
                id INTEGER PRIMARY KEY,
                project_id INTEGER,
                module_id TEXT,
                module_path TEXT,
                UNIQUE(project_id, module_path)
            );",
        )
        .unwrap();

        // Insert module conventions
        conn.execute(
            "INSERT INTO module_conventions (project_id, module_id, module_path)
             VALUES (1, 'context', 'src/context')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO module_conventions (project_id, module_id, module_path)
             VALUES (1, 'background', 'src/background')",
            [],
        )
        .unwrap();

        // Insert file access events
        conn.execute(
            "INSERT INTO session_behavior_log (project_id, session_id, event_type, event_data)
             VALUES (1, 'sess1', 'file_access', '{\"file_path\": \"/project/src/context/mod.rs\"}')",
            [],
        )
        .unwrap();

        let result = detect_working_modules(&conn, 1, "sess1", Some("/project/"));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]._module_id, "context");
        assert_eq!(result[0].module_path, "src/context");
    }
}
