// crates/mira-server/src/db/documentation.rs
// Database layer for documentation tracking and generation

use crate::utils::ResultExt;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

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
    pub created_at: String,
    pub updated_at: String,
    pub git_commit: Option<String>,
    /// Safety rails: hash of source signatures at generation time
    pub source_signature_hash: Option<String>,
    /// Safety rails: checksum of target doc when draft was generated
    pub target_doc_checksum_at_generation: Option<String>,
    /// Generated draft content
    pub draft_content: Option<String>,
    /// Preview for list views (first 200 chars)
    pub draft_preview: Option<String>,
    /// SHA256 of draft content
    pub draft_sha256: Option<String>,
    pub draft_generated_at: Option<String>,
    pub reviewed_at: Option<String>,
    pub applied_at: Option<String>,
    pub retry_count: i32,
    pub last_error: Option<String>,
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

/// Create a new documentation task
pub fn create_doc_task(
    conn: &rusqlite::Connection,
    gap: &DocGap,
    git_commit: Option<&str>,
) -> Result<i64, String> {
    conn.execute(
        "INSERT INTO documentation_tasks (
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
    .map(|_| conn.last_insert_rowid())
    .str_err()
}

/// Get pending documentation tasks, ordered by priority
pub fn get_pending_doc_tasks(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    limit: usize,
) -> Result<Vec<DocTask>, String> {
    let sql = format!(
        "SELECT * FROM documentation_tasks
         WHERE (?1 IS NULL OR project_id = ?1) AND status = 'pending'
         ORDER BY {}, created_at DESC
         LIMIT ?2",
        super::PRIORITY_ORDER_SQL
    );

    let mut stmt = conn.prepare(&sql).str_err()?;
    let rows = stmt
        .query_map(params![project_id, limit as i64], parse_doc_task)
        .str_err()?;
    rows.collect::<Result<Vec<_>, _>>().str_err()
}

/// Get all tasks with optional filters
pub fn list_doc_tasks(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    status: Option<&str>,
    doc_type: Option<&str>,
    priority: Option<&str>,
) -> Result<Vec<DocTask>, String> {
    let mut sql = "SELECT * FROM documentation_tasks WHERE 1=1".to_string();
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

    let mut stmt = conn.prepare(&sql).str_err()?;
    stmt.query_map(rusqlite::params_from_iter(params), parse_doc_task)
        .str_err()?
        .collect::<Result<Vec<_>, _>>()
        .str_err()
}

/// Get a single task by ID
pub fn get_doc_task(conn: &rusqlite::Connection, task_id: i64) -> Result<Option<DocTask>, String> {
    conn.query_row(
        "SELECT * FROM documentation_tasks WHERE id = ?",
        [task_id],
        parse_doc_task,
    )
    .optional()
    .str_err()
}

/// Mark a task as applied (documentation written)
pub fn mark_doc_task_applied(conn: &rusqlite::Connection, task_id: i64) -> Result<(), String> {
    conn.execute(
        "UPDATE documentation_tasks
         SET status = 'applied', applied_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP
         WHERE id = ?",
        [task_id],
    )
    .map(|_| ())
    .str_err()
}

/// Mark a task as skipped
pub fn mark_doc_task_skipped(
    conn: &rusqlite::Connection,
    task_id: i64,
    reason: &str,
) -> Result<(), String> {
    conn.execute(
        "UPDATE documentation_tasks
         SET status = 'skipped', reason = ?1, updated_at = CURRENT_TIMESTAMP
         WHERE id = ?2",
        params![reason, task_id],
    )
    .map(|_| ())
    .str_err()
}

/// Reset orphaned tasks whose target files no longer exist
/// Returns the number of tasks reset
pub fn reset_orphaned_doc_tasks(
    conn: &rusqlite::Connection,
    project_id: i64,
    project_path: &str,
) -> Result<usize, String> {
    use std::path::Path;

    // Get all applied tasks for this project
    let mut stmt = conn
        .prepare(
            "SELECT id, target_doc_path FROM documentation_tasks
             WHERE project_id = ? AND status = 'applied'",
        )
        .str_err()?;

    let tasks: Vec<(i64, String)> = stmt
        .query_map([project_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .str_err()?
        .filter_map(|r| r.ok())
        .collect();

    let mut reset_count = 0;
    for (task_id, target_path) in tasks {
        let full_path = Path::new(project_path).join(&target_path);
        if !full_path.exists() {
            conn.execute(
                "UPDATE documentation_tasks
                 SET status = 'pending', applied_at = NULL, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?",
                [task_id],
            )
            .str_err()?;
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
) -> Result<i64, String> {
    conn.execute(
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
            verified_at = CURRENT_TIMESTAMP",
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
    )
    .map(|_| conn.last_insert_rowid())
    .str_err()
}

/// Mark documentation as stale
pub fn mark_doc_stale(
    conn: &rusqlite::Connection,
    project_id: i64,
    doc_path: &str,
    reason: &str,
) -> Result<(), String> {
    conn.execute(
        "UPDATE documentation_inventory
         SET is_stale = 1, staleness_reason = ?1
         WHERE project_id = ?2 AND doc_path = ?3",
        params![reason, project_id, doc_path],
    )
    .map(|_| ())
    .str_err()
}

/// Get all documentation inventory for a project
pub fn get_doc_inventory(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> Result<Vec<DocInventory>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT * FROM documentation_inventory
             WHERE project_id = ?
             ORDER BY doc_type, doc_path",
        )
        .str_err()?;

    stmt.query_map(params![project_id], parse_doc_inventory)
        .str_err()?
        .collect::<Result<Vec<_>, _>>()
        .str_err()
}

/// Get inventory items eligible for staleness check
/// Returns items with source_signature_hash that are not already marked stale
pub fn get_inventory_for_stale_check(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> Result<Vec<DocInventory>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT * FROM documentation_inventory
             WHERE project_id = ? AND source_signature_hash IS NOT NULL
             AND is_stale = 0",
        )
        .str_err()?;

    stmt.query_map(params![project_id], parse_doc_inventory)
        .str_err()?
        .collect::<Result<Vec<_>, _>>()
        .str_err()
}

/// Get stale documentation for a project
pub fn get_stale_docs(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> Result<Vec<DocInventory>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT * FROM documentation_inventory
             WHERE project_id = ? AND is_stale = 1
             ORDER BY doc_type, doc_path",
        )
        .str_err()?;

    stmt.query_map(params![project_id], parse_doc_inventory)
        .str_err()?
        .collect::<Result<Vec<_>, _>>()
        .str_err()
}

/// Parse a DocTask row
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
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        git_commit: row.get("git_commit")?,
        source_signature_hash: row.get("source_signature_hash")?,
        target_doc_checksum_at_generation: row.get("target_doc_checksum_at_generation")?,
        draft_content: row.get("draft_content")?,
        draft_preview: row.get("draft_preview")?,
        draft_sha256: row.get("draft_sha256")?,
        draft_generated_at: row.get("draft_generated_at")?,
        reviewed_at: row.get("reviewed_at")?,
        applied_at: row.get("applied_at")?,
        retry_count: row.get("retry_count")?,
        last_error: row.get("last_error")?,
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
) -> Result<Vec<(String, i64)>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT status, COUNT(*) as count FROM documentation_tasks
             WHERE ?1 IS NULL OR project_id = ?1
             GROUP BY status",
        )
        .str_err()?;
    stmt.query_map(params![project_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })
    .str_err()?
    .collect::<Result<Vec<_>, _>>()
    .str_err()
}

/// Get stale docs that need impact analysis (stale but not yet analyzed)
pub fn get_stale_docs_needing_analysis(
    conn: &rusqlite::Connection,
    project_id: i64,
    limit: usize,
) -> Result<Vec<DocInventory>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT * FROM documentation_inventory
             WHERE project_id = ?
               AND is_stale = 1
               AND change_impact IS NULL
             ORDER BY verified_at DESC
             LIMIT ?",
        )
        .str_err()?;

    stmt.query_map(params![project_id, limit as i64], parse_doc_inventory)
        .str_err()?
        .collect::<Result<Vec<_>, _>>()
        .str_err()
}

/// Update impact analysis results for a stale doc
pub fn update_doc_impact_analysis(
    conn: &rusqlite::Connection,
    doc_id: i64,
    change_impact: &str,
    change_summary: &str,
) -> Result<(), String> {
    conn.execute(
        "UPDATE documentation_inventory
         SET change_impact = ?,
             change_summary = ?,
             impact_analyzed_at = CURRENT_TIMESTAMP
         WHERE id = ?",
        params![change_impact, change_summary, doc_id],
    )
    .map(|_| ())
    .str_err()
}

/// Clear impact analysis when doc is no longer stale (e.g., after update)
pub fn clear_doc_impact_analysis(
    conn: &rusqlite::Connection,
    project_id: i64,
    doc_path: &str,
) -> Result<(), String> {
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
    .str_err()
}
