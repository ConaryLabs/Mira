// db/dependencies.rs
// Database operations for module dependency analysis

use rusqlite::{Connection, params};

/// A module dependency edge
pub struct ModuleDependency {
    pub source_module_id: String,
    pub target_module_id: String,
    pub dependency_type: String,
    pub call_count: i64,
    pub import_count: i64,
    pub is_circular: bool,
}

/// Upsert a module dependency record
pub fn upsert_module_dependency_sync(
    conn: &Connection,
    project_id: i64,
    dep: &ModuleDependency,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO module_dependencies
         (project_id, source_module_id, target_module_id, dependency_type, call_count, import_count, is_circular, computed_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, datetime('now'))
         ON CONFLICT(project_id, source_module_id, target_module_id) DO UPDATE SET
           dependency_type = excluded.dependency_type,
           call_count = excluded.call_count,
           import_count = excluded.import_count,
           is_circular = excluded.is_circular,
           computed_at = datetime('now')",
        params![
            project_id,
            dep.source_module_id,
            dep.target_module_id,
            dep.dependency_type,
            dep.call_count,
            dep.import_count,
            dep.is_circular as i32,
        ],
    )?;
    Ok(())
}

/// Get all module dependencies for a project
pub fn get_module_deps_sync(
    conn: &Connection,
    project_id: i64,
) -> rusqlite::Result<Vec<ModuleDependency>> {
    let mut stmt = conn.prepare(
        "SELECT source_module_id, target_module_id, dependency_type, call_count, import_count, is_circular
         FROM module_dependencies
         WHERE project_id = ?
         ORDER BY call_count + import_count DESC",
    )?;

    let deps = stmt
        .query_map(params![project_id], |row| {
            Ok(ModuleDependency {
                source_module_id: row.get(0)?,
                target_module_id: row.get(1)?,
                dependency_type: row.get(2)?,
                call_count: row.get(3)?,
                import_count: row.get(4)?,
                is_circular: row.get::<_, i32>(5)? != 0,
            })
        })?
        .filter_map(super::log_and_discard)
        .collect();

    Ok(deps)
}

/// Get circular dependencies for a project
pub fn get_circular_dependencies_sync(
    conn: &Connection,
    project_id: i64,
) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT source_module_id, target_module_id
         FROM module_dependencies
         WHERE project_id = ? AND is_circular = 1
         ORDER BY source_module_id",
    )?;

    let circulars = stmt
        .query_map(params![project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(super::log_and_discard)
        .collect();

    Ok(circulars)
}

/// Clear stale dependencies before recomputing
pub fn clear_module_dependencies_sync(conn: &Connection, project_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM module_dependencies WHERE project_id = ?",
        params![project_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::setup_test_connection;

    fn setup_deps_db() -> (Connection, i64) {
        let conn = setup_test_connection();
        // module_dependencies lives in the code DB schema, not main migrations.
        // Create it manually for tests (same pattern as crossref tests).
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS module_dependencies (
                id INTEGER PRIMARY KEY,
                project_id INTEGER NOT NULL,
                source_module_id TEXT NOT NULL,
                target_module_id TEXT NOT NULL,
                dependency_type TEXT NOT NULL,
                call_count INTEGER DEFAULT 0,
                import_count INTEGER DEFAULT 0,
                is_circular INTEGER DEFAULT 0,
                computed_at TEXT DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(project_id, source_module_id, target_module_id)
            );
            CREATE INDEX IF NOT EXISTS idx_module_deps_project ON module_dependencies(project_id);
            CREATE INDEX IF NOT EXISTS idx_module_deps_circular ON module_dependencies(project_id, is_circular) WHERE is_circular = 1;",
        )
        .unwrap();
        let (pid, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();
        (conn, pid)
    }

    fn make_dep(source: &str, target: &str, circular: bool) -> ModuleDependency {
        ModuleDependency {
            source_module_id: source.to_string(),
            target_module_id: target.to_string(),
            dependency_type: "import".to_string(),
            call_count: 5,
            import_count: 2,
            is_circular: circular,
        }
    }

    // ========================================================================
    // Circular dependency detection
    // ========================================================================

    #[test]
    fn test_get_circular_dependencies_none() {
        let (conn, pid) = setup_deps_db();

        // Insert non-circular deps
        upsert_module_dependency_sync(&conn, pid, &make_dep("mod_a", "mod_b", false))
            .expect("upsert should succeed");

        let circulars =
            get_circular_dependencies_sync(&conn, pid).expect("get circular should succeed");
        assert!(circulars.is_empty(), "no circular deps should be found");
    }

    #[test]
    fn test_get_circular_dependencies_found() {
        let (conn, pid) = setup_deps_db();

        // Insert a circular dep
        upsert_module_dependency_sync(&conn, pid, &make_dep("mod_a", "mod_b", true))
            .expect("upsert should succeed");
        upsert_module_dependency_sync(&conn, pid, &make_dep("mod_b", "mod_a", true))
            .expect("upsert should succeed");

        let circulars =
            get_circular_dependencies_sync(&conn, pid).expect("get circular should succeed");
        assert_eq!(circulars.len(), 2, "should find both circular edges");

        let pairs: Vec<(&str, &str)> = circulars
            .iter()
            .map(|(s, t)| (s.as_str(), t.as_str()))
            .collect();
        assert!(pairs.contains(&("mod_a", "mod_b")));
        assert!(pairs.contains(&("mod_b", "mod_a")));
    }

    #[test]
    fn test_get_circular_dependencies_empty_table() {
        let (conn, pid) = setup_deps_db();

        let circulars =
            get_circular_dependencies_sync(&conn, pid).expect("get circular should succeed");
        assert!(circulars.is_empty());
    }

    // ========================================================================
    // Clearing deps for nonexistent project
    // ========================================================================

    #[test]
    fn test_clear_deps_nonexistent_project() {
        let (conn, _pid) = setup_deps_db();

        // Clearing deps for a project that has no deps should succeed
        let result = clear_module_dependencies_sync(&conn, 99999);
        assert!(
            result.is_ok(),
            "clearing nonexistent project deps should not error"
        );
    }

    // ========================================================================
    // Upsert idempotency
    // ========================================================================

    #[test]
    fn test_upsert_idempotency() {
        let (conn, pid) = setup_deps_db();

        let dep = make_dep("mod_x", "mod_y", false);

        // Insert first time
        upsert_module_dependency_sync(&conn, pid, &dep).expect("first upsert should succeed");

        // Upsert again with same key
        upsert_module_dependency_sync(&conn, pid, &dep).expect("second upsert should succeed");

        // Should only have one row
        let deps = get_module_deps_sync(&conn, pid).expect("get deps should succeed");
        assert_eq!(deps.len(), 1, "upsert should not create duplicate rows");
    }

    #[test]
    fn test_upsert_updates_values() {
        let (conn, pid) = setup_deps_db();

        let dep1 = ModuleDependency {
            source_module_id: "mod_x".to_string(),
            target_module_id: "mod_y".to_string(),
            dependency_type: "import".to_string(),
            call_count: 5,
            import_count: 2,
            is_circular: false,
        };
        upsert_module_dependency_sync(&conn, pid, &dep1).unwrap();

        // Upsert with updated values
        let dep2 = ModuleDependency {
            source_module_id: "mod_x".to_string(),
            target_module_id: "mod_y".to_string(),
            dependency_type: "re-export".to_string(),
            call_count: 10,
            import_count: 3,
            is_circular: true,
        };
        upsert_module_dependency_sync(&conn, pid, &dep2).unwrap();

        let deps = get_module_deps_sync(&conn, pid).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].dependency_type, "re-export");
        assert_eq!(deps[0].call_count, 10);
        assert_eq!(deps[0].import_count, 3);
        assert!(deps[0].is_circular);
    }

    // ========================================================================
    // get_module_deps_sync: empty
    // ========================================================================

    #[test]
    fn test_get_module_deps_empty() {
        let (conn, pid) = setup_deps_db();

        let deps = get_module_deps_sync(&conn, pid).expect("get deps should succeed");
        assert!(deps.is_empty());
    }
}
