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
