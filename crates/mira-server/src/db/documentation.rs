// crates/mira-server/src/db/documentation.rs
// Database layer for documentation tracking and generation

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// Columns selected for DocTask queries (excludes vestigial draft columns)
const DOC_TASK_COLUMNS: &str = "id, project_id, doc_type, doc_category, source_file_path, target_doc_path, \
     priority, status, reason, skip_reason, created_at, updated_at, git_commit, \
     source_signature_hash, applied_at";

/// Documentation task for tracking missing or stale docs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocTask {
    pub id: i64,
    pub project_id: Option<i64>,
    pub doc_type: String,
    pub doc_category: String,
    pub source_file_path: Option<String>,
    pub target_doc_path: String,
    pub priority: String,
    pub status: String,
    pub reason: Option<String>,
    pub skip_reason: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub git_commit: Option<String>,
    /// Safety rails: hash of source signatures at generation time
    pub source_signature_hash: Option<String>,
    pub applied_at: Option<String>,
}

/// Documentation inventory entry for existing docs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocInventory {
    pub id: i64,
    pub project_id: i64,
    pub doc_path: String,
    pub doc_type: String,
    pub doc_category: Option<String>,
    pub title: Option<String>,
    pub source_signature_hash: Option<String>,
    pub source_symbols: Option<String>,
    pub last_seen_commit: Option<String>,
    pub is_stale: bool,
    pub staleness_reason: Option<String>,
    pub verified_at: String,
    pub created_at: String,
}

/// Documentation gap detected during scanning
#[derive(Debug, Clone)]
pub struct DocGap {
    pub project_id: i64,
    pub doc_type: String,
    pub doc_category: String,
    pub source_file_path: Option<String>,
    pub target_doc_path: String,
    pub priority: String,
    pub reason: String,
    pub source_signature_hash: Option<String>,
}

/// Create a new documentation task.
/// Silently skips creation if a row for this (project, target) already exists
/// (any status) thanks to the unconditional unique index.
pub fn create_doc_task(
    conn: &rusqlite::Connection,
    gap: &DocGap,
    git_commit: Option<&str>,
) -> Result<i64, String> {
    let changed = conn
        .execute(
            "INSERT OR IGNORE INTO documentation_tasks (
                project_id, doc_type, doc_category, source_file_path,
                target_doc_path, priority, status, reason, git_commit,
                source_signature_hash
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?8, ?9)",
            params![
                gap.project_id,
                gap.doc_type,
                gap.doc_category,
                gap.source_file_path,
                gap.target_doc_path,
                gap.priority,
                gap.reason,
                git_commit,
                gap.source_signature_hash,
            ],
        )
        .map_err(|e| e.to_string())?;

    if changed == 0 {
        return Err(format!("Task for {} already exists", gap.target_doc_path));
    }
    Ok(conn.last_insert_rowid())
}

/// Get pending documentation tasks, ordered by priority
pub fn get_pending_doc_tasks(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    limit: usize,
) -> rusqlite::Result<Vec<DocTask>> {
    let sql = format!(
        "SELECT {} FROM documentation_tasks
         WHERE {}status = 'pending'
         ORDER BY {}, created_at DESC
         LIMIT ?",
        DOC_TASK_COLUMNS,
        if project_id.is_some() {
            "project_id = ? AND "
        } else {
            ""
        },
        super::PRIORITY_ORDER_SQL
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = if let Some(pid) = project_id {
        stmt.query_map(params![pid, limit as i64], parse_doc_task)?
    } else {
        stmt.query_map(params![limit as i64], parse_doc_task)?
    };
    rows.collect()
}

/// Get all tasks with optional filters
pub fn list_doc_tasks(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    status: Option<&str>,
    doc_type: Option<&str>,
    priority: Option<&str>,
) -> rusqlite::Result<Vec<DocTask>> {
    let mut sql = format!(
        "SELECT {} FROM documentation_tasks WHERE 1=1",
        DOC_TASK_COLUMNS
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(pid) = project_id {
        sql.push_str(" AND project_id = ?");
        params.push(Box::new(pid));
    }
    if let Some(s) = status {
        sql.push_str(" AND status = ?");
        params.push(Box::new(s.to_string()));
    }
    if let Some(t) = doc_type {
        sql.push_str(" AND doc_type = ?");
        params.push(Box::new(t.to_string()));
    }
    if let Some(p) = priority {
        sql.push_str(" AND priority = ?");
        params.push(Box::new(p.to_string()));
    }

    sql.push_str(&format!(
        " ORDER BY {}, created_at DESC",
        super::PRIORITY_ORDER_SQL
    ));

    let mut stmt = conn.prepare(&sql)?;
    stmt.query_map(rusqlite::params_from_iter(params), parse_doc_task)?
        .collect()
}

/// Get a single task by ID
pub fn get_doc_task(
    conn: &rusqlite::Connection,
    task_id: i64,
) -> rusqlite::Result<Option<DocTask>> {
    conn.query_row(
        &format!(
            "SELECT {} FROM documentation_tasks WHERE id = ?",
            DOC_TASK_COLUMNS
        ),
        [task_id],
        parse_doc_task,
    )
    .optional()
}

/// Mark a task as completed (documentation written)
pub fn mark_doc_task_completed(conn: &rusqlite::Connection, task_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE documentation_tasks
         SET status = 'completed', applied_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP
         WHERE id = ?",
        [task_id],
    )
    .map(|_| ())
}

/// Mark a task as skipped (preserves original reason, stores skip reason separately)
pub fn mark_doc_task_skipped(
    conn: &rusqlite::Connection,
    task_id: i64,
    reason: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE documentation_tasks
         SET status = 'skipped', skip_reason = ?1, updated_at = CURRENT_TIMESTAMP
         WHERE id = ?2",
        params![reason, task_id],
    )
    .map(|_| ())
}

/// Reset orphaned tasks whose target files no longer exist
/// Returns the number of tasks reset
pub fn reset_orphaned_doc_tasks(
    conn: &rusqlite::Connection,
    project_id: i64,
    project_path: &str,
) -> rusqlite::Result<usize> {
    use std::path::{Path, PathBuf};

    let canonical_project = Path::new(project_path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(project_path));

    // Get all completed tasks for this project
    let mut stmt = conn.prepare(
        "SELECT id, target_doc_path FROM documentation_tasks
             WHERE project_id = ? AND status = 'completed'",
    )?;

    let tasks: Vec<(i64, String)> = stmt
        .query_map([project_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(super::log_and_discard)
        .collect();

    let mut reset_count = 0;
    for (task_id, target_path) in tasks {
        let full_path = Path::new(project_path).join(&target_path);

        // Validate path stays within project root (prevent directory traversal)
        // Use canonicalize when possible, but for non-existent paths fall back to
        // lexical normalization that strips '..' components
        let canonical_full = full_path
            .canonicalize()
            .unwrap_or_else(|_| normalize_lexical(&full_path));
        if !canonical_full.starts_with(&canonical_project) {
            continue;
        }

        if !full_path.exists() {
            conn.execute(
                "UPDATE documentation_tasks
                 SET status = 'pending', applied_at = NULL, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?",
                [task_id],
            )?;
            reset_count += 1;
            tracing::info!(
                "Reset orphaned doc task {} (file missing: {})",
                task_id,
                target_path
            );
        }
    }

    Ok(reset_count)
}

/// Lexically normalize a path by resolving `.` and `..` components without
/// filesystem access. Used as a fallback when `canonicalize()` fails (path
/// doesn't exist) to prevent directory traversal via `../` in stored paths.
fn normalize_lexical(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// Parameters for upserting documentation inventory
pub struct DocInventoryParams<'a> {
    pub project_id: i64,
    pub doc_path: &'a str,
    pub doc_type: &'a str,
    pub doc_category: Option<&'a str>,
    pub title: Option<&'a str>,
    pub source_signature_hash: Option<&'a str>,
    pub source_symbols: Option<&'a str>,
    pub git_commit: Option<&'a str>,
}

/// Add or update documentation inventory entry
pub fn upsert_doc_inventory(
    conn: &rusqlite::Connection,
    p: &DocInventoryParams,
) -> rusqlite::Result<i64> {
    conn.query_row(
        "INSERT INTO documentation_inventory (
            project_id, doc_path, doc_type, doc_category, title,
            source_signature_hash, source_symbols, last_seen_commit
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ON CONFLICT(project_id, doc_path) DO UPDATE SET
            doc_type = excluded.doc_type,
            doc_category = excluded.doc_category,
            title = excluded.title,
            source_signature_hash = excluded.source_signature_hash,
            source_symbols = excluded.source_symbols,
            last_seen_commit = excluded.last_seen_commit,
            is_stale = 0,
            staleness_reason = NULL,
            change_impact = NULL,
            change_summary = NULL,
            impact_analyzed_at = NULL,
            verified_at = CURRENT_TIMESTAMP
        RETURNING id",
        params![
            p.project_id,
            p.doc_path,
            p.doc_type,
            p.doc_category,
            p.title,
            p.source_signature_hash,
            p.source_symbols,
            p.git_commit,
        ],
        |row| row.get(0),
    )
}

/// Mark documentation as stale
pub fn mark_doc_stale(
    conn: &rusqlite::Connection,
    project_id: i64,
    doc_path: &str,
    reason: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE documentation_inventory
         SET is_stale = 1, staleness_reason = ?1
         WHERE project_id = ?2 AND doc_path = ?3",
        params![reason, project_id, doc_path],
    )
    .map(|_| ())
}

/// Get all documentation inventory for a project
pub fn get_doc_inventory(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> rusqlite::Result<Vec<DocInventory>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM documentation_inventory
             WHERE project_id = ?
             ORDER BY doc_type, doc_path",
    )?;

    stmt.query_map(params![project_id], parse_doc_inventory)?
        .collect()
}

/// Get inventory items eligible for staleness check
/// Returns items with source_signature_hash that are not already marked stale
pub fn get_inventory_for_stale_check(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> rusqlite::Result<Vec<DocInventory>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM documentation_inventory
             WHERE project_id = ? AND source_signature_hash IS NOT NULL
             AND is_stale = 0",
    )?;

    stmt.query_map(params![project_id], parse_doc_inventory)?
        .collect()
}

/// Get stale documentation for a project
pub fn get_stale_docs(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> rusqlite::Result<Vec<DocInventory>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM documentation_inventory
             WHERE project_id = ? AND is_stale = 1
             ORDER BY doc_type, doc_path",
    )?;

    stmt.query_map(params![project_id], parse_doc_inventory)?
        .collect()
}

/// Parse a DocTask row from explicit column list
fn parse_doc_task(row: &rusqlite::Row) -> Result<DocTask, rusqlite::Error> {
    Ok(DocTask {
        id: row.get("id")?,
        project_id: row.get("project_id")?,
        doc_type: row.get("doc_type")?,
        doc_category: row.get("doc_category")?,
        source_file_path: row.get("source_file_path")?,
        target_doc_path: row.get("target_doc_path")?,
        priority: row.get("priority")?,
        status: row.get("status")?,
        reason: row.get("reason")?,
        skip_reason: row.get("skip_reason")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        git_commit: row.get("git_commit")?,
        source_signature_hash: row.get("source_signature_hash")?,
        applied_at: row.get("applied_at")?,
    })
}

/// Parse a DocInventory row (public for use in background workers)
pub fn parse_doc_inventory(row: &rusqlite::Row) -> Result<DocInventory, rusqlite::Error> {
    Ok(DocInventory {
        id: row.get("id")?,
        project_id: row.get("project_id")?,
        doc_path: row.get("doc_path")?,
        doc_type: row.get("doc_type")?,
        doc_category: row.get("doc_category")?,
        title: row.get("title")?,
        source_signature_hash: row.get("source_signature_hash")?,
        source_symbols: row.get("source_symbols")?,
        last_seen_commit: row.get("last_seen_commit")?,
        is_stale: row.get::<_, i32>("is_stale")? == 1,
        staleness_reason: row.get("staleness_reason")?,
        verified_at: row.get("verified_at")?,
        created_at: row.get("created_at")?,
    })
}

/// Count documentation tasks by status for a project
pub fn count_doc_tasks_by_status(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
) -> rusqlite::Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT status, COUNT(*) as count FROM documentation_tasks
             WHERE ?1 IS NULL OR project_id = ?1
             GROUP BY status",
    )?;
    stmt.query_map(params![project_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?
    .collect()
}

/// Get stale docs that need impact analysis (stale but not yet analyzed)
pub fn get_stale_docs_needing_analysis(
    conn: &rusqlite::Connection,
    project_id: i64,
    limit: usize,
) -> rusqlite::Result<Vec<DocInventory>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM documentation_inventory
             WHERE project_id = ?
               AND is_stale = 1
               AND change_impact IS NULL
             ORDER BY verified_at DESC
             LIMIT ?",
    )?;

    stmt.query_map(params![project_id, limit as i64], parse_doc_inventory)?
        .collect()
}

/// Update impact analysis results for a stale doc
pub fn update_doc_impact_analysis(
    conn: &rusqlite::Connection,
    doc_id: i64,
    change_impact: &str,
    change_summary: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE documentation_inventory
         SET change_impact = ?,
             change_summary = ?,
             impact_analyzed_at = CURRENT_TIMESTAMP
         WHERE id = ?",
        params![change_impact, change_summary, doc_id],
    )
    .map(|_| ())
}

/// Clear impact analysis when doc is no longer stale (e.g., after update)
pub fn clear_doc_impact_analysis(
    conn: &rusqlite::Connection,
    project_id: i64,
    doc_path: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE documentation_inventory
         SET change_impact = NULL,
             change_summary = NULL,
             impact_analyzed_at = NULL,
             is_stale = 0,
             staleness_reason = NULL
         WHERE project_id = ? AND doc_path = ?",
        params![project_id, doc_path],
    )
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::setup_test_connection;

    fn setup_doc_db() -> (rusqlite::Connection, i64) {
        let conn = setup_test_connection();
        let (pid, _) =
            crate::db::get_or_create_project_sync(&conn, "/test/project", Some("test")).unwrap();
        (conn, pid)
    }

    fn make_gap(project_id: i64) -> DocGap {
        DocGap {
            project_id,
            doc_type: "api".to_string(),
            doc_category: "module".to_string(),
            source_file_path: Some("src/main.rs".to_string()),
            target_doc_path: "docs/api.md".to_string(),
            priority: "medium".to_string(),
            reason: "Missing API docs".to_string(),
            source_signature_hash: None,
        }
    }

    // ========================================================================
    // Happy-path: create, get, list, complete doc tasks
    // ========================================================================

    #[test]
    fn test_create_doc_task_returns_id() {
        let (conn, pid) = setup_doc_db();
        let gap = make_gap(pid);
        let id = create_doc_task(&conn, &gap, Some("abc123")).expect("create should succeed");
        assert!(id > 0);
    }

    #[test]
    fn test_get_doc_task_all_fields() {
        let (conn, pid) = setup_doc_db();
        let gap = make_gap(pid);
        let id = create_doc_task(&conn, &gap, Some("commit1")).unwrap();

        let task = get_doc_task(&conn, id).unwrap().unwrap();
        assert_eq!(task.id, id);
        assert_eq!(task.project_id, Some(pid));
        assert_eq!(task.doc_type, "api");
        assert_eq!(task.doc_category, "module");
        assert_eq!(task.source_file_path.as_deref(), Some("src/main.rs"));
        assert_eq!(task.target_doc_path, "docs/api.md");
        assert_eq!(task.priority, "medium");
        assert_eq!(task.status, "pending");
        assert_eq!(task.reason.as_deref(), Some("Missing API docs"));
        assert_eq!(task.git_commit.as_deref(), Some("commit1"));
        assert!(task.skip_reason.is_none());
        assert!(task.applied_at.is_none());
    }

    #[test]
    fn test_get_pending_doc_tasks_ordered_by_priority() {
        let (conn, pid) = setup_doc_db();

        let low_gap = DocGap {
            project_id: pid,
            doc_type: "guide".to_string(),
            doc_category: "overview".to_string(),
            source_file_path: None,
            target_doc_path: "docs/low.md".to_string(),
            priority: "low".to_string(),
            reason: "Low priority".to_string(),
            source_signature_hash: None,
        };
        let high_gap = DocGap {
            project_id: pid,
            doc_type: "api".to_string(),
            doc_category: "module".to_string(),
            source_file_path: None,
            target_doc_path: "docs/high.md".to_string(),
            priority: "high".to_string(),
            reason: "High priority".to_string(),
            source_signature_hash: None,
        };

        create_doc_task(&conn, &low_gap, None).unwrap();
        create_doc_task(&conn, &high_gap, None).unwrap();

        let tasks = get_pending_doc_tasks(&conn, Some(pid), 10).unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].priority, "high");
        assert_eq!(tasks[1].priority, "low");
    }

    #[test]
    fn test_list_doc_tasks_with_status_filter() {
        let (conn, pid) = setup_doc_db();
        let gap = make_gap(pid);
        let id = create_doc_task(&conn, &gap, None).unwrap();

        // Initially pending
        let pending = list_doc_tasks(&conn, Some(pid), Some("pending"), None, None).unwrap();
        assert_eq!(pending.len(), 1);

        // Complete it
        mark_doc_task_completed(&conn, id).unwrap();

        // Now filter by completed
        let completed = list_doc_tasks(&conn, Some(pid), Some("completed"), None, None).unwrap();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].status, "completed");
        assert!(completed[0].applied_at.is_some());

        // Pending should be empty
        let pending = list_doc_tasks(&conn, Some(pid), Some("pending"), None, None).unwrap();
        assert!(pending.is_empty());
    }

    #[test]
    fn test_mark_doc_task_completed_sets_applied_at() {
        let (conn, pid) = setup_doc_db();
        let gap = make_gap(pid);
        let id = create_doc_task(&conn, &gap, None).unwrap();

        mark_doc_task_completed(&conn, id).unwrap();

        let task = get_doc_task(&conn, id).unwrap().unwrap();
        assert_eq!(task.status, "completed");
        assert!(task.applied_at.is_some());
    }

    #[test]
    fn test_count_doc_tasks_by_status_with_data() {
        let (conn, pid) = setup_doc_db();

        let gap1 = make_gap(pid);
        let id1 = create_doc_task(&conn, &gap1, None).unwrap();

        let gap2 = DocGap {
            target_doc_path: "docs/other.md".to_string(),
            ..make_gap(pid)
        };
        create_doc_task(&conn, &gap2, None).unwrap();

        mark_doc_task_completed(&conn, id1).unwrap();

        let counts = count_doc_tasks_by_status(&conn, Some(pid)).unwrap();
        let pending_count = counts
            .iter()
            .find(|(s, _)| s == "pending")
            .map(|(_, c)| *c)
            .unwrap_or(0);
        let completed_count = counts
            .iter()
            .find(|(s, _)| s == "completed")
            .map(|(_, c)| *c)
            .unwrap_or(0);
        assert_eq!(pending_count, 1);
        assert_eq!(completed_count, 1);
    }

    // ========================================================================
    // Happy-path: inventory operations
    // ========================================================================

    #[test]
    fn test_upsert_doc_inventory_and_get() {
        let (conn, pid) = setup_doc_db();

        let id = upsert_doc_inventory(
            &conn,
            &DocInventoryParams {
                project_id: pid,
                doc_path: "docs/readme.md",
                doc_type: "guide",
                doc_category: Some("overview"),
                title: Some("README"),
                source_signature_hash: Some("hash123"),
                source_symbols: Some("fn main"),
                git_commit: Some("abcdef"),
            },
        )
        .unwrap();
        assert!(id > 0);

        let inventory = get_doc_inventory(&conn, pid).unwrap();
        assert_eq!(inventory.len(), 1);
        assert_eq!(inventory[0].doc_path, "docs/readme.md");
        assert_eq!(inventory[0].doc_type, "guide");
        assert_eq!(inventory[0].doc_category.as_deref(), Some("overview"));
        assert_eq!(inventory[0].title.as_deref(), Some("README"));
        assert!(!inventory[0].is_stale);
    }

    #[test]
    fn test_upsert_doc_inventory_updates_on_conflict() {
        let (conn, pid) = setup_doc_db();

        upsert_doc_inventory(
            &conn,
            &DocInventoryParams {
                project_id: pid,
                doc_path: "docs/api.md",
                doc_type: "api",
                doc_category: None,
                title: Some("Old title"),
                source_signature_hash: Some("old_hash"),
                source_symbols: None,
                git_commit: None,
            },
        )
        .unwrap();

        // Upsert same path => should update
        upsert_doc_inventory(
            &conn,
            &DocInventoryParams {
                project_id: pid,
                doc_path: "docs/api.md",
                doc_type: "api",
                doc_category: Some("module"),
                title: Some("New title"),
                source_signature_hash: Some("new_hash"),
                source_symbols: None,
                git_commit: Some("commit2"),
            },
        )
        .unwrap();

        let inventory = get_doc_inventory(&conn, pid).unwrap();
        assert_eq!(inventory.len(), 1);
        assert_eq!(inventory[0].title.as_deref(), Some("New title"));
        assert_eq!(
            inventory[0].source_signature_hash.as_deref(),
            Some("new_hash")
        );
    }

    #[test]
    fn test_mark_doc_stale_and_get_stale_docs() {
        let (conn, pid) = setup_doc_db();

        upsert_doc_inventory(
            &conn,
            &DocInventoryParams {
                project_id: pid,
                doc_path: "docs/api.md",
                doc_type: "api",
                doc_category: None,
                title: Some("API"),
                source_signature_hash: Some("hash1"),
                source_symbols: None,
                git_commit: None,
            },
        )
        .unwrap();

        mark_doc_stale(&conn, pid, "docs/api.md", "source changed").unwrap();

        let stale = get_stale_docs(&conn, pid).unwrap();
        assert_eq!(stale.len(), 1);
        assert!(stale[0].is_stale);
        assert_eq!(stale[0].staleness_reason.as_deref(), Some("source changed"));
    }

    #[test]
    fn test_clear_doc_impact_analysis_clears_staleness() {
        let (conn, pid) = setup_doc_db();

        upsert_doc_inventory(
            &conn,
            &DocInventoryParams {
                project_id: pid,
                doc_path: "docs/api.md",
                doc_type: "api",
                doc_category: None,
                title: Some("API"),
                source_signature_hash: Some("hash1"),
                source_symbols: None,
                git_commit: None,
            },
        )
        .unwrap();

        mark_doc_stale(&conn, pid, "docs/api.md", "changed").unwrap();
        clear_doc_impact_analysis(&conn, pid, "docs/api.md").unwrap();

        let stale = get_stale_docs(&conn, pid).unwrap();
        assert!(stale.is_empty());
    }

    #[test]
    fn test_update_doc_impact_analysis() {
        let (conn, pid) = setup_doc_db();

        let id = upsert_doc_inventory(
            &conn,
            &DocInventoryParams {
                project_id: pid,
                doc_path: "docs/api.md",
                doc_type: "api",
                doc_category: None,
                title: Some("API"),
                source_signature_hash: Some("hash1"),
                source_symbols: None,
                git_commit: None,
            },
        )
        .unwrap();

        mark_doc_stale(&conn, pid, "docs/api.md", "source changed").unwrap();
        update_doc_impact_analysis(&conn, id, "high", "Major API change").unwrap();

        // Verify the stale doc now has analysis
        let needing = get_stale_docs_needing_analysis(&conn, pid, 10).unwrap();
        assert!(
            needing.is_empty(),
            "analyzed doc should not appear in needing-analysis list"
        );
    }

    // ========================================================================
    // mark_doc_task_skipped: skip with reason
    // ========================================================================

    #[test]
    fn test_mark_doc_task_skipped_with_reason() {
        let (conn, pid) = setup_doc_db();
        let gap = make_gap(pid);
        let task_id = create_doc_task(&conn, &gap, None).expect("create should succeed");

        mark_doc_task_skipped(&conn, task_id, "Not relevant anymore").expect("skip should succeed");

        let task = get_doc_task(&conn, task_id)
            .expect("get should succeed")
            .expect("task should exist");
        assert_eq!(task.status, "skipped");
        assert_eq!(task.skip_reason.as_deref(), Some("Not relevant anymore"));
        // Original reason should be preserved
        assert_eq!(task.reason.as_deref(), Some("Missing API docs"));
    }

    // ========================================================================
    // reset_orphaned_doc_tasks edge case
    // ========================================================================

    #[test]
    fn test_reset_orphaned_doc_tasks_no_completed_tasks() {
        let (conn, pid) = setup_doc_db();

        // Only pending tasks exist, no completed ones to check
        let gap = make_gap(pid);
        create_doc_task(&conn, &gap, None).expect("create should succeed");

        let reset =
            reset_orphaned_doc_tasks(&conn, pid, "/test/project").expect("reset should succeed");
        assert_eq!(reset, 0, "no completed tasks means nothing to reset");
    }

    #[test]
    fn test_reset_orphaned_doc_tasks_empty() {
        let (conn, pid) = setup_doc_db();

        let reset =
            reset_orphaned_doc_tasks(&conn, pid, "/test/project").expect("reset should succeed");
        assert_eq!(reset, 0, "no tasks at all means nothing to reset");
    }

    // ========================================================================
    // get_stale_docs with zero-age docs (just inserted, not stale)
    // ========================================================================

    #[test]
    fn test_get_stale_docs_none_stale() {
        let (conn, pid) = setup_doc_db();

        // Insert a non-stale inventory item
        upsert_doc_inventory(
            &conn,
            &DocInventoryParams {
                project_id: pid,
                doc_path: "docs/readme.md",
                doc_type: "guide",
                doc_category: None,
                title: Some("README"),
                source_signature_hash: Some("abc123"),
                source_symbols: None,
                git_commit: Some("deadbeef"),
            },
        )
        .expect("upsert should succeed");

        let stale = get_stale_docs(&conn, pid).expect("get_stale should succeed");
        assert!(stale.is_empty(), "freshly inserted doc should not be stale");
    }

    #[test]
    fn test_get_inventory_for_stale_check_filters_already_stale() {
        let (conn, pid) = setup_doc_db();

        // Insert a doc and mark it stale
        upsert_doc_inventory(
            &conn,
            &DocInventoryParams {
                project_id: pid,
                doc_path: "docs/api.md",
                doc_type: "api",
                doc_category: None,
                title: Some("API"),
                source_signature_hash: Some("hash1"),
                source_symbols: None,
                git_commit: None,
            },
        )
        .expect("upsert should succeed");
        mark_doc_stale(&conn, pid, "docs/api.md", "source changed")
            .expect("mark stale should succeed");

        // This should not return already-stale docs
        let candidates =
            get_inventory_for_stale_check(&conn, pid).expect("stale check should succeed");
        assert!(
            candidates.is_empty(),
            "already-stale docs should not be returned for stale check"
        );
    }

    // ========================================================================
    // get_doc_task for nonexistent ID
    // ========================================================================

    #[test]
    fn test_get_doc_task_nonexistent() {
        let (conn, _pid) = setup_doc_db();

        let task = get_doc_task(&conn, 99999).expect("get should succeed");
        assert!(task.is_none(), "nonexistent task should return None");
    }

    // ========================================================================
    // count_doc_tasks_by_status empty
    // ========================================================================

    #[test]
    fn test_count_doc_tasks_by_status_empty() {
        let (conn, pid) = setup_doc_db();

        let counts = count_doc_tasks_by_status(&conn, Some(pid)).expect("count should succeed");
        assert!(counts.is_empty(), "no tasks should return empty counts");
    }

    // ========================================================================
    // create_doc_task duplicate (IGNORE behavior)
    // ========================================================================

    #[test]
    fn test_create_doc_task_duplicate_returns_error_message() {
        let (conn, pid) = setup_doc_db();
        let gap = make_gap(pid);

        let first = create_doc_task(&conn, &gap, None);
        assert!(first.is_ok(), "first create should succeed");

        let second = create_doc_task(&conn, &gap, None);
        assert!(second.is_err(), "duplicate should return error");
        let err = second.unwrap_err();
        assert!(
            err.contains("already exists"),
            "error should mention already exists: {}",
            err
        );
    }
}
