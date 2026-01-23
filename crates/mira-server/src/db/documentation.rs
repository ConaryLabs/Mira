// crates/mira-server/src/db/documentation.rs
// Database layer for documentation tracking and generation

use rusqlite::{params, OptionalExtension};
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
    .map_err(|e| e.to_string())
}

/// Get pending documentation tasks, ordered by priority
pub fn get_pending_doc_tasks(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    limit: usize,
) -> Result<Vec<DocTask>, String> {
    let sql = if project_id.is_some() {
        "SELECT * FROM documentation_tasks
         WHERE project_id = ?1 AND status = 'pending'
         ORDER BY
             CASE priority
                 WHEN 'urgent' THEN 1
                 WHEN 'high' THEN 2
                 WHEN 'medium' THEN 3
                 ELSE 4
             END,
             created_at DESC
         LIMIT ?2"
    } else {
        "SELECT * FROM documentation_tasks
         WHERE status = 'pending'
         ORDER BY
             CASE priority
                 WHEN 'urgent' THEN 1
                 WHEN 'high' THEN 2
                 WHEN 'medium' THEN 3
                 ELSE 4
             END,
             created_at DESC
         LIMIT ?1"
    };

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;

    let rows = if let Some(pid) = project_id {
        stmt.query_map(params![pid, limit as i64], parse_doc_task)
    } else {
        stmt.query_map(params![limit as i64], parse_doc_task)
    }
    .map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
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
    let mut params = Vec::new();

    if let Some(pid) = project_id {
        sql.push_str(" AND project_id = ?");
        params.push(pid.to_string());
    }
    if let Some(s) = status {
        sql.push_str(" AND status = ?");
        params.push(s.to_string());
    }
    if let Some(t) = doc_type {
        sql.push_str(" AND doc_type = ?");
        params.push(t.to_string());
    }
    if let Some(p) = priority {
        sql.push_str(" AND priority = ?");
        params.push(p.to_string());
    }

    sql.push_str(
        " ORDER BY CASE priority
            WHEN 'urgent' THEN 1
            WHEN 'high' THEN 2
            WHEN 'medium' THEN 3
            ELSE 4
        END, created_at DESC",
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p as &dyn rusqlite::ToSql).collect();

    stmt.query_map(params_refs.as_slice(), parse_doc_task)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

/// Get a single task by ID
pub fn get_doc_task(conn: &rusqlite::Connection, task_id: i64) -> Result<Option<DocTask>, String> {
    conn.query_row(
        "SELECT * FROM documentation_tasks WHERE id = ?",
        [task_id],
        |row| parse_doc_task(row),
    )
    .optional()
    .map_err(|e| e.to_string())
}

/// Store a generated draft for a task
pub fn store_doc_draft(
    conn: &rusqlite::Connection,
    task_id: i64,
    draft_content: &str,
    target_doc_checksum: &str,
) -> Result<(), String> {
    let preview = if draft_content.len() > 200 {
        format!("{}...", &draft_content[..200])
    } else {
        draft_content.to_string()
    };

    let sha256 = sha256::digest(draft_content);

    conn.execute(
        "UPDATE documentation_tasks
         SET draft_content = ?1, draft_preview = ?2, draft_sha256 = ?3,
             target_doc_checksum_at_generation = ?4, draft_generated_at = CURRENT_TIMESTAMP,
             status = 'draft_ready', updated_at = CURRENT_TIMESTAMP
         WHERE id = ?5",
        params![draft_content, preview, sha256, target_doc_checksum, task_id],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
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
    .map_err(|e| e.to_string())
}

/// Mark a task as approved (ready to apply)
pub fn mark_doc_task_approved(conn: &rusqlite::Connection, task_id: i64) -> Result<(), String> {
    conn.execute(
        "UPDATE documentation_tasks
         SET status = 'approved', reviewed_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP
         WHERE id = ?",
        [task_id],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
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
    .map_err(|e| e.to_string())
}

/// Update task error and increment retry count
pub fn mark_doc_task_error(
    conn: &rusqlite::Connection,
    task_id: i64,
    error: &str,
) -> Result<(), String> {
    conn.execute(
        "UPDATE documentation_tasks
         SET last_error = ?1, retry_count = retry_count + 1, updated_at = CURRENT_TIMESTAMP
         WHERE id = ?2",
        params![error, task_id],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// Add or update documentation inventory entry
pub fn upsert_doc_inventory(
    conn: &rusqlite::Connection,
    project_id: i64,
    doc_path: &str,
    doc_type: &str,
    doc_category: Option<&str>,
    title: Option<&str>,
    source_signature_hash: Option<&str>,
    source_symbols: Option<&str>,
    git_commit: Option<&str>,
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
            project_id,
            doc_path,
            doc_type,
            doc_category,
            title,
            source_signature_hash,
            source_symbols,
            git_commit,
        ],
    )
    .map(|_| conn.last_insert_rowid())
    .map_err(|e| e.to_string())
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
    .map_err(|e| e.to_string())
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
        .map_err(|e| e.to_string())?;

    stmt.query_map(params![project_id], parse_doc_inventory)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
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
        .map_err(|e| e.to_string())?;

    stmt.query_map(params![project_id], parse_doc_inventory)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
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
    let sql = match project_id {
        Some(pid) => {
            let mut stmt = conn
                .prepare(
                    "SELECT status, COUNT(*) as count FROM documentation_tasks
                     WHERE project_id = ?
                     GROUP BY status",
                )
                .map_err(|e| e.to_string())?;
            stmt.query_map([pid], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())
        }
        None => {
            let mut stmt = conn
                .prepare(
                    "SELECT status, COUNT(*) as count FROM documentation_tasks
                     GROUP BY status",
                )
                .map_err(|e| e.to_string())?;
            stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())
        }
    };
    sql
}

/// Simple sha256 wrapper for the module
mod sha256 {
    use sha2::Digest;
    use sha2::Sha256;

    pub fn digest(input: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        format!("{:x}", hasher.finalize())
    }
}
