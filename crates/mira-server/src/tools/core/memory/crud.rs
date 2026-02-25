// crates/mira-server/src/tools/core/memory/crud.rs
//! Memory CRUD operations: list, forget, archive, export, purge, entities.

use crate::error::MiraError;
use crate::mcp::responses::Json;
use crate::mcp::responses::{
    EntitiesData, EntityItem, ExportData, ExportMemoryItem, ListData, ListMemoryItem, MemoryData,
    MemoryOutput, PurgeData,
};
use crate::tools::core::{ToolContext, get_project_info};
use crate::utils::truncate;

/// Verify the caller has access to a memory based on scope rules.
///
/// Same logic as `fuzzy::memory_visible`: project-scoped memories require matching
/// project_id, personal requires matching user_id, team requires matching team_id.
pub(super) fn verify_memory_access(
    scope_info: &crate::db::MemoryScopeInfo,
    caller_project_id: Option<i64>,
    caller_user_id: Option<&str>,
    caller_team_id: Option<i64>,
) -> Result<(), MiraError> {
    let (mem_project_id, ref scope, ref mem_user_id, mem_team_id) = *scope_info;

    // Project-scoped memories require matching project_id (global memories are always accessible)
    if mem_project_id.is_some() && mem_project_id != caller_project_id {
        return Err(MiraError::InvalidInput(
            "Access denied: memory belongs to a different project".to_string(),
        ));
    }

    match scope.as_str() {
        "personal" => {
            if mem_user_id.as_deref() != caller_user_id {
                return Err(MiraError::InvalidInput(
                    "Access denied: personal memory belongs to a different user".to_string(),
                ));
            }
        }
        "team" => {
            if caller_team_id.is_none() || mem_team_id != caller_team_id {
                return Err(MiraError::InvalidInput(
                    "Access denied: team memory belongs to a different team".to_string(),
                ));
            }
        }
        _ => {} // project / NULL scope — accessible if project check passed
    }

    Ok(())
}

/// List all memories for the current project with pagination and optional filtering
pub async fn list_memories<C: ToolContext>(
    ctx: &C,
    limit: Option<i64>,
    offset: Option<i64>,
    category: Option<String>,
    fact_type: Option<String>,
) -> Result<Json<MemoryOutput>, MiraError> {
    let project_id = ctx.project_id().await;
    let user_id = ctx.get_user_identity();
    let team_id: Option<i64> = ctx.get_team_membership().map(|m| m.team_id);

    let limit = (limit.unwrap_or(20).clamp(1, 100)) as usize;
    let offset = (offset.unwrap_or(0).max(0)) as usize;

    let cat = category.clone();
    let ft = fact_type.clone();
    let uid = user_id.clone();

    let (rows, total): (Vec<ListMemoryItem>, usize) = ctx
        .pool()
        .run(move |conn| {
            // Exclude system/automated fact types from user-facing list.
            // These belong in system_observations, not user memories.
            let system_filter = "AND fact_type NOT IN ('health', 'persona', 'system', 'session_event', 'extracted', 'tool_outcome', 'convergence_alert', 'distilled')";

            // Count total matching rows
            let count_sql = format!(
                "SELECT COUNT(*) FROM memory_facts
                 WHERE (project_id IS ?1 OR project_id IS NULL)
                   AND COALESCE(suspicious, 0) = 0
                   AND COALESCE(status, 'active') != 'archived'
                   AND (
                     COALESCE(scope, 'project') = 'project'
                     OR (scope = 'personal' AND COALESCE(user_id, '') = COALESCE(?2, ''))
                     OR (scope = 'team' AND COALESCE(team_id, 0) = COALESCE(?3, 0))
                   )
                   AND (?4 IS NULL OR category = ?4)
                   AND (?5 IS NULL OR fact_type = ?5)
                   {system_filter}",
            );
            let total: usize = conn.query_row(
                &count_sql,
                rusqlite::params![project_id, uid.as_deref(), team_id, cat, ft],
                |row| row.get::<_, usize>(0),
            )?;

            // Fetch paginated results
            let list_sql = format!(
                "SELECT id, content, fact_type, category,
                        COALESCE(scope, 'project') as scope, key, created_at
                 FROM memory_facts
                 WHERE (project_id IS ?1 OR project_id IS NULL)
                   AND COALESCE(suspicious, 0) = 0
                   AND COALESCE(status, 'active') != 'archived'
                   AND (
                     COALESCE(scope, 'project') = 'project'
                     OR (scope = 'personal' AND COALESCE(user_id, '') = COALESCE(?2, ''))
                     OR (scope = 'team' AND COALESCE(team_id, 0) = COALESCE(?3, 0))
                   )
                   AND (?4 IS NULL OR category = ?4)
                   AND (?5 IS NULL OR fact_type = ?5)
                   {system_filter}
                 ORDER BY created_at DESC
                 LIMIT ?6 OFFSET ?7",
            );
            let mut stmt = conn.prepare(&list_sql)?;

            let rows = stmt
                .query_map(
                    rusqlite::params![project_id, uid.as_deref(), team_id, cat, ft, limit, offset],
                    |row| {
                        Ok(ListMemoryItem {
                            id: row.get(0)?,
                            content: row.get(1)?,
                            fact_type: row.get(2)?,
                            category: row.get(3)?,
                            scope: row.get(4)?,
                            key: row.get(5)?,
                            created_at: row.get(6)?,
                        })
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;

            Ok::<(Vec<ListMemoryItem>, usize), rusqlite::Error>((rows, total))
        })
        .await?;

    let shown = rows.len();
    let has_more = offset + shown < total;

    let mut response = format!(
        "Found {} memories (showing {}-{}):\n",
        total,
        offset + 1,
        offset + shown
    );
    for item in &rows {
        let preview = truncate(&item.content, 80);
        let cat_tag = item
            .category
            .as_ref()
            .map(|c| format!(" [{}]", c))
            .unwrap_or_default();
        response.push_str(&format!(
            "  [{}] ({}){} {}\n",
            item.id,
            item.fact_type.as_deref().unwrap_or("general"),
            cat_tag,
            preview
        ));
    }
    if has_more {
        response.push_str(&format!(
            "\n{} more -- use offset={} to see next page",
            total - offset - shown,
            offset + shown
        ));
    }

    Ok(Json(MemoryOutput {
        action: "list".into(),
        message: response,
        data: Some(MemoryData::List(ListData {
            memories: rows,
            total,
            offset,
            has_more,
        })),
    }))
}

/// Delete a memory
pub async fn forget<C: ToolContext>(ctx: &C, id: i64) -> Result<Json<MemoryOutput>, MiraError> {
    use crate::db::{delete_memory_sync, get_memory_scope_sync};

    if id <= 0 {
        return Err(MiraError::InvalidInput(
            "Invalid memory ID: must be positive".to_string(),
        ));
    }

    // Verify scope/ownership before deleting
    let scope_info = ctx
        .pool()
        .run(move |conn| get_memory_scope_sync(conn, id))
        .await?;

    let Some(scope_info) = scope_info else {
        return Err(MiraError::InvalidInput(format!(
            "Memory not found (id: {}). Use memory(action=\"recall\", query=\"...\") to search.",
            id
        )));
    };

    let project_id = ctx.project_id().await;
    let user_id = ctx.get_user_identity();
    let team_id: Option<i64> = ctx.get_team_membership().map(|m| m.team_id);
    verify_memory_access(&scope_info, project_id, user_id.as_deref(), team_id)?;

    // Delete from both SQL and vector table, then clean up orphaned entities
    let deleted = ctx
        .pool()
        .run(move |conn| {
            let d = delete_memory_sync(conn, id)?;
            if d {
                // CASCADE removes memory_entity_links; now remove entities with no remaining links
                conn.execute(
                    "DELETE FROM memory_entities WHERE id NOT IN (SELECT DISTINCT entity_id FROM memory_entity_links)",
                    [],
                )?;
            }
            Ok::<bool, rusqlite::Error>(d)
        })
        .await?;

    if deleted {
        if let Some(cache) = ctx.fuzzy_cache() {
            cache.invalidate_memory(project_id).await;
        }
        Ok(Json(MemoryOutput {
            action: "forget".into(),
            message: format!("Memory {} deleted.", id),
            data: None,
        }))
    } else {
        Err(MiraError::InvalidInput(format!(
            "Memory not found (id: {}). Use memory(action=\"recall\", query=\"...\") to search.",
            id
        )))
    }
}

/// Archive a memory (sets status to 'archived', excluding it from auto-export)
pub async fn archive<C: ToolContext>(ctx: &C, id: i64) -> Result<Json<MemoryOutput>, MiraError> {
    use crate::db::get_memory_scope_sync;

    if id <= 0 {
        return Err(MiraError::InvalidInput(
            "Invalid memory ID: must be positive".to_string(),
        ));
    }

    // Verify scope/ownership before archiving
    let scope_info = ctx
        .pool()
        .run(move |conn| get_memory_scope_sync(conn, id))
        .await?;

    let Some(scope_info) = scope_info else {
        return Err(MiraError::InvalidInput(format!(
            "Memory not found (id: {}). Use memory(action=\"recall\", query=\"...\") to search.",
            id
        )));
    };

    let project_id = ctx.project_id().await;
    let user_id = ctx.get_user_identity();
    let team_id: Option<i64> = ctx.get_team_membership().map(|m| m.team_id);
    verify_memory_access(&scope_info, project_id, user_id.as_deref(), team_id)?;

    let archived = ctx
        .pool()
        .run(move |conn| {
            let rows = conn
                .execute(
                    "UPDATE memory_facts SET status = 'archived', updated_at = datetime('now') WHERE id = ?",
                    [id],
                )?;
            Ok::<bool, rusqlite::Error>(rows > 0)
        })
        .await?;

    if archived {
        if let Some(cache) = ctx.fuzzy_cache() {
            cache.invalidate_memory(project_id).await;
        }
        Ok(Json(MemoryOutput {
            action: "archive".into(),
            message: format!(
                "Memory {} archived. It will no longer appear in auto-exports.",
                id
            ),
            data: None,
        }))
    } else {
        Err(MiraError::InvalidInput(format!(
            "Memory not found (id: {}). Use memory(action=\"recall\", query=\"...\") to search.",
            id
        )))
    }
}

/// Export all project memories as structured JSON
pub async fn export_memories<C: ToolContext>(ctx: &C) -> Result<Json<MemoryOutput>, MiraError> {
    let pi = get_project_info(ctx).await;
    let project_id = pi.id;
    let project_name = pi.context.as_ref().and_then(|c| c.name.clone());
    let user_id = ctx.get_user_identity();
    let team_id: Option<i64> = ctx.get_team_membership().map(|m| m.team_id);

    let uid = user_id.clone();
    let memories: Vec<ExportMemoryItem> = ctx
        .pool()
        .run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, content, fact_type, category,
                        COALESCE(scope, 'project') as scope, key, branch,
                        confidence, created_at, updated_at
                 FROM memory_facts
                 WHERE (project_id IS ?1 OR project_id IS NULL)
                   AND COALESCE(suspicious, 0) = 0
                   AND COALESCE(status, 'active') != 'archived'
                   AND (
                     COALESCE(scope, 'project') = 'project'
                     OR (scope = 'personal' AND COALESCE(user_id, '') = COALESCE(?2, ''))
                     OR (scope = 'team' AND COALESCE(team_id, 0) = COALESCE(?3, 0))
                   )
                 ORDER BY created_at DESC",
            )?;

            let rows = stmt
                .query_map(
                    rusqlite::params![project_id, uid.as_deref(), team_id],
                    |row| {
                        Ok(ExportMemoryItem {
                            id: row.get(0)?,
                            content: row.get(1)?,
                            fact_type: row.get(2)?,
                            category: row.get(3)?,
                            scope: row.get(4)?,
                            key: row.get(5)?,
                            branch: row.get(6)?,
                            confidence: row.get(7)?,
                            created_at: row.get(8)?,
                            updated_at: row.get(9)?,
                        })
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;

            Ok::<Vec<ExportMemoryItem>, rusqlite::Error>(rows)
        })
        .await?;

    let total = memories.len();
    let exported_at = chrono::Utc::now().to_rfc3339();

    Ok(Json(MemoryOutput {
        action: "export".into(),
        message: format!("Exported {} memories.", total),
        data: Some(MemoryData::Export(ExportData {
            memories,
            total,
            project_name,
            exported_at,
        })),
    }))
}

/// Delete all memories for the current project
pub async fn purge_memories<C: ToolContext>(
    ctx: &C,
    confirm: Option<bool>,
) -> Result<Json<MemoryOutput>, MiraError> {
    let project_id = ctx.project_id().await;

    let Some(pid) = project_id else {
        return Err(MiraError::InvalidInput(
            "Cannot purge: no active project. Use project(action=\"start\") first.".to_string(),
        ));
    };

    // Count memories first
    let count: usize = ctx
        .pool()
        .run(move |conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM memory_facts WHERE project_id = ?1",
                [pid],
                |row| row.get::<_, usize>(0),
            )
        })
        .await?;

    if confirm != Some(true) {
        return Err(MiraError::InvalidInput(format!(
            "Use confirm=true to delete all {} memories for this project",
            count
        )));
    }

    // Delete from vec_memory first (references memory_facts rowids),
    // then from memory_facts
    let deleted_count: usize = ctx
        .pool()
        .run(move |conn| {
            let tx = conn.unchecked_transaction()?;

            // Delete vector embeddings for matching facts
            tx.execute(
                "DELETE FROM vec_memory WHERE rowid IN (SELECT id FROM memory_facts WHERE project_id = ?1)",
                [pid],
            )?;

            // Delete the facts themselves (CASCADE removes memory_entity_links)
            let deleted = tx.execute(
                "DELETE FROM memory_facts WHERE project_id = ?1",
                [pid],
            )?;

            // Clean up orphaned entities (no remaining links)
            tx.execute(
                "DELETE FROM memory_entities WHERE project_id IS ?1
                 AND id NOT IN (SELECT DISTINCT entity_id FROM memory_entity_links)",
                [pid],
            )?;

            tx.commit()?;
            Ok::<usize, rusqlite::Error>(deleted)
        })
        .await?;

    // Invalidate fuzzy cache
    if let Some(cache) = ctx.fuzzy_cache() {
        cache.invalidate_memory(project_id).await;
    }

    Ok(Json(MemoryOutput {
        action: "purge".into(),
        message: format!("Purged {} memories for this project.", deleted_count),
        data: Some(MemoryData::Purge(PurgeData { deleted_count })),
    }))
}

/// Query entity graph for the current project
pub async fn list_entities<C: ToolContext>(
    ctx: &C,
    query: Option<String>,
    limit: Option<i64>,
) -> Result<Json<MemoryOutput>, MiraError> {
    let project_id = ctx.project_id().await;
    let limit = (limit.unwrap_or(50).clamp(1, 200)) as usize;

    let entities: Vec<EntityItem> = ctx
        .pool()
        .run(move |conn| {
            let (sql, params): (String, Vec<Box<dyn rusqlite::ToSql>>) = if let Some(ref q) = query
            {
                let escaped = q
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_");
                let pattern = format!("%{}%", escaped);
                (
                    "SELECT me.id, me.canonical_name, me.entity_type, me.display_name,
                            COUNT(mel.fact_id) as linked_facts
                     FROM memory_entities me
                     JOIN memory_entity_links mel ON mel.entity_id = me.id
                     WHERE me.project_id IS ?1
                       AND me.canonical_name LIKE ?2 ESCAPE '\\'
                     GROUP BY me.id
                     ORDER BY linked_facts DESC
                     LIMIT ?3"
                        .to_string(),
                    vec![
                        Box::new(project_id) as Box<dyn rusqlite::ToSql>,
                        Box::new(pattern),
                        Box::new(limit as i64),
                    ],
                )
            } else {
                (
                    "SELECT me.id, me.canonical_name, me.entity_type, me.display_name,
                            COUNT(mel.fact_id) as linked_facts
                     FROM memory_entities me
                     JOIN memory_entity_links mel ON mel.entity_id = me.id
                     WHERE me.project_id IS ?1
                     GROUP BY me.id
                     ORDER BY linked_facts DESC
                     LIMIT ?2"
                        .to_string(),
                    vec![
                        Box::new(project_id) as Box<dyn rusqlite::ToSql>,
                        Box::new(limit as i64),
                    ],
                )
            };

            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(params), |row| {
                    Ok(EntityItem {
                        id: row.get(0)?,
                        canonical_name: row.get(1)?,
                        entity_type: row.get(2)?,
                        display_name: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                        linked_facts: row.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            Ok::<Vec<EntityItem>, rusqlite::Error>(rows)
        })
        .await?;

    let total = entities.len();
    let mut response = format!("Found {} entities:\n", total);
    for e in &entities {
        response.push_str(&format!(
            "  [{}] {} ({}) — {} linked memories\n",
            e.id, e.canonical_name, e.entity_type, e.linked_facts
        ));
    }

    Ok(Json(MemoryOutput {
        action: "entities".into(),
        message: response,
        data: Some(MemoryData::Entities(EntitiesData { entities, total })),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════════════════════════════════════════
    // verify_memory_access scope isolation tests
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn access_project_scope_same_project() {
        let scope: crate::db::MemoryScopeInfo = (Some(1), "project".into(), None, None);
        assert!(verify_memory_access(&scope, Some(1), None, None).is_ok());
    }

    #[test]
    fn access_project_scope_different_project_denied() {
        let scope: crate::db::MemoryScopeInfo = (Some(1), "project".into(), None, None);
        assert!(verify_memory_access(&scope, Some(2), None, None).is_err());
    }

    #[test]
    fn access_global_memory_always_passes() {
        // NULL project_id = global memory, accessible from any project
        let scope: crate::db::MemoryScopeInfo = (None, "project".into(), None, None);
        assert!(verify_memory_access(&scope, Some(99), None, None).is_ok());
        assert!(verify_memory_access(&scope, None, None, None).is_ok());
    }

    #[test]
    fn access_personal_scope_matching_user() {
        let scope: crate::db::MemoryScopeInfo =
            (Some(1), "personal".into(), Some("alice".into()), None);
        assert!(verify_memory_access(&scope, Some(1), Some("alice"), None).is_ok());
    }

    #[test]
    fn access_personal_scope_different_user_denied() {
        let scope: crate::db::MemoryScopeInfo =
            (Some(1), "personal".into(), Some("alice".into()), None);
        assert!(verify_memory_access(&scope, Some(1), Some("bob"), None).is_err());
    }

    #[test]
    fn access_personal_scope_no_caller_user_denied() {
        let scope: crate::db::MemoryScopeInfo =
            (Some(1), "personal".into(), Some("alice".into()), None);
        assert!(verify_memory_access(&scope, Some(1), None, None).is_err());
    }

    #[test]
    fn access_team_scope_matching_team() {
        let scope: crate::db::MemoryScopeInfo = (Some(1), "team".into(), None, Some(10));
        assert!(verify_memory_access(&scope, Some(1), None, Some(10)).is_ok());
    }

    #[test]
    fn access_team_scope_different_team_denied() {
        let scope: crate::db::MemoryScopeInfo = (Some(1), "team".into(), None, Some(10));
        assert!(verify_memory_access(&scope, Some(1), None, Some(20)).is_err());
    }

    #[test]
    fn access_team_scope_no_caller_team_denied() {
        let scope: crate::db::MemoryScopeInfo = (Some(1), "team".into(), None, Some(10));
        assert!(verify_memory_access(&scope, Some(1), None, None).is_err());
    }

    #[test]
    fn access_project_scope_ignores_caller_identity() {
        // Project-scoped memory accessible regardless of caller user/team
        let scope: crate::db::MemoryScopeInfo = (Some(1), "project".into(), None, None);
        assert!(verify_memory_access(&scope, Some(1), Some("anyone"), Some(99)).is_ok());
        assert!(verify_memory_access(&scope, Some(1), None, None).is_ok());
    }
}
