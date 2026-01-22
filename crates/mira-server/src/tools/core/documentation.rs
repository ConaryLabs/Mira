// crates/mira-server/src/tools/core/documentation.rs
// MCP tools for documentation review and approval workflow

use crate::db::documentation::{
    get_doc_task, mark_doc_task_applied, mark_doc_task_skipped,
    get_doc_inventory, mark_doc_task_approved, DocTask, DocInventory,
};
use crate::tools::core::ToolContext;
use rusqlite::params;
use sha2::Digest;
use std::path::Path;

/// List pending documentation tasks with optional filters
pub async fn list_doc_tasks(
    ctx: &(impl ToolContext + ?Sized),
    status: Option<String>,
    doc_type: Option<String>,
    priority: Option<String>,
) -> Result<String, String> {
    let project_id_opt = ctx.project_id().await;

    let db = ctx.db();
    let conn = db.conn();

    let tasks = list_db_doc_tasks(
        &conn,
        project_id_opt,
        status.as_deref(),
        doc_type.as_deref(),
        priority.as_deref(),
    )?;

    if tasks.is_empty() {
        return Ok("No documentation tasks found.".to_string());
    }

    let mut output = String::from("## Documentation Tasks\n\n");

    for task in tasks {
        let status_icon = match task.status.as_str() {
            "pending" => "⏳",
            "draft_ready" => "📝",
            "approved" => "✅",
            "applied" => "✨",
            "skipped" => "⏭️",
            _ => "❓",
        };

        output.push_str(&format!(
            "{} **[{}]** `{} -> {}` ({})\n",
            status_icon, task.priority, task.doc_category, task.target_doc_path, task.status
        ));

        if let Some(reason) = &task.reason {
            output.push_str(&format!("   Reason: {}\n", reason));
        }

        if task.status == "draft_ready" {
            if let Some(preview) = &task.draft_preview {
                output.push_str(&format!("   Preview: {}\n", preview));
            }
            output.push_str(&format!("   Review with: `review_doc_draft({})`\n", task.id));
        }

        output.push('\n');
    }

    Ok(output)
}

/// Wrapper to avoid name collision with db module function
fn list_db_doc_tasks(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    status: Option<&str>,
    doc_type: Option<&str>,
    priority: Option<&str>,
) -> Result<Vec<DocTask>, String> {
    crate::db::documentation::list_doc_tasks(
        conn,
        project_id,
        status,
        doc_type,
        priority,
    )
}

/// Review a generated documentation draft
pub async fn review_doc_draft(
    ctx: &(impl ToolContext + ?Sized),
    task_id: i64,
) -> Result<String, String> {
    let db = ctx.db();
    let conn = db.conn();

    let task = get_doc_task(&conn, task_id)?
        .ok_or(format!("Task {} not found", task_id))?;

    if task.status != "draft_ready" {
        return Ok(format!(
            "Task {} is not ready for review (current status: {})",
            task_id, task.status
        ));
    }

    let draft_content = task.draft_content
        .as_ref()
        .ok_or("No draft content available")?;

    let mut output = String::new();

    output.push_str(&format!("## Documentation Draft Review\n\n"));
    output.push_str(&format!("**Task ID:** {}\n", task.id));
    output.push_str(&format!("**Target:** `{}`\n", task.target_doc_path));
    output.push_str(&format!("**Type:** {} / {}\n", task.doc_type, task.doc_category));
    output.push_str(&format!("**Priority:** {}\n\n", task.priority));

    if let Some(reason) = &task.reason {
        output.push_str(&format!("**Reason:** {}\n\n", reason));
    }

    output.push_str("---\n\n");
    output.push_str(draft_content);
    output.push_str("\n\n---\n\n");

    output.push_str("**Actions:**\n");
    output.push_str(&format!("- Apply draft: `apply_doc_draft({}, force=false)`\n", task.id));
    output.push_str(&format!("- Apply (overwrite): `apply_doc_draft({}, force=true)`\n", task.id));
    output.push_str(&format!("- Skip: `skip_doc_task({})`\n", task.id));

    // Safety check info
    if let Some(checksum) = &task.target_doc_checksum_at_generation {
        if checksum != "none" {
            output.push_str(&format!("\n**Safety:** Target file checksum at generation: `{}`", checksum));
        }
    }

    Ok(output)
}

/// Apply an approved documentation draft
pub async fn apply_doc_draft(
    ctx: &(impl ToolContext + ?Sized),
    task_id: i64,
    force: bool,
) -> Result<String, String> {
    let project_id_opt = ctx.project_id().await;
    let project_id = project_id_opt.ok_or("No active project")?;

    // Get task info and project path in a sync block
    let (task, project_path) = {
        let db = ctx.db();
        let conn = db.conn();

        let task = get_doc_task(&conn, task_id)?
            .ok_or(format!("Task {} not found", task_id))?;

        // Get project path
        let project_path: String = conn.query_row(
            "SELECT path FROM projects WHERE id = ?",
            [project_id],
            |row| row.get(0),
        ).map_err(|e| e.to_string())?;

        (task, project_path)
    };

    let target_path = Path::new(&project_path).join(&task.target_doc_path);

    // Safety check: verify file hasn't changed since draft generation
    if !force && target_path.exists() {
        let current_checksum = file_checksum(&target_path)
            .ok_or("Failed to calculate current file checksum")?;

        let default_checksum = "none".to_string();
        let stored_checksum = task.target_doc_checksum_at_generation
            .as_ref()
            .unwrap_or(&default_checksum);

        if stored_checksum.as_str() != "none" && current_checksum != stored_checksum.as_str() {
            return Ok(format!(
                "❌ **Safety Check Failed**\n\n\
                Target file `{}` has been modified since the draft was generated.\n\n\
                - Stored checksum: `{}`\n\
                - Current checksum: `{}`\n\n\
                The draft may be outdated. To apply anyway, use `force=true`.\n\
                Alternatively, regenerate the draft with `scan_documentation()`.",
                task.target_doc_path, stored_checksum, current_checksum
            ));
        }
    }

    // Get draft content
    let draft_content = task.draft_content
        .as_ref()
        .ok_or("No draft content available")?;

    // Create parent directories if needed (async)
    if let Some(parent) = target_path.parent() {
        tokio::fs::create_dir_all(parent).await
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    // Write the file (async)
    tokio::fs::write(&target_path, draft_content).await
        .map_err(|e| format!("Failed to write file: {}", e))?;

    // Mark task as applied (sync, after file write)
    {
        let db = ctx.db();
        let conn = db.conn();
        mark_doc_task_applied(&conn, task_id)?;
    }

    Ok(format!(
        "✅ **Documentation Applied**\n\n\
        Draft written to: `{}`\n\
        Task {} marked as applied.",
        task.target_doc_path, task_id
    ))
}

/// Skip a documentation task
pub async fn skip_doc_task(
    ctx: &(impl ToolContext + ?Sized),
    task_id: i64,
    reason: Option<String>,
) -> Result<String, String> {
    let db = ctx.db();
    let conn = db.conn();

    let skip_reason = reason.unwrap_or_else(|| "Skipped by user".to_string());

    mark_doc_task_skipped(&conn, task_id, &skip_reason)?;

    Ok(format!("Task {} skipped: {}", task_id, skip_reason))
}

/// Show documentation inventory with staleness indicators
pub async fn show_doc_inventory(
    ctx: &(impl ToolContext + ?Sized),
) -> Result<String, String> {
    let project_id_opt = ctx.project_id().await;
    let project_id = project_id_opt.ok_or("No active project")?;

    let db = ctx.db();
    let conn = db.conn();

    let inventory = get_doc_inventory(&conn, project_id)?;

    if inventory.is_empty() {
        return Ok("No documentation inventory found. Run scan to build inventory.".to_string());
    }

    let mut output = String::from("## Documentation Inventory\n\n");

    let stale_count = inventory.iter().filter(|i| i.is_stale).count();
    output.push_str(&format!("Total: {} documents", inventory.len()));
    if stale_count > 0 {
        output.push_str(&format!(" ({} stale ⚠️)", stale_count));
    }
    output.push_str("\n\n---\n\n");

    // Group by type
    let mut by_type: std::collections::HashMap<&str, Vec<&DocInventory>> = Default::default();
    for item in &inventory {
        by_type.entry(&item.doc_type).or_default().push(item);
    }

    for (doc_type, items) in by_type.iter() {
        output.push_str(&format!("### {}\n\n", doc_type));

        for item in items {
            let stale_indicator = if item.is_stale { " ⚠️ STALE" } else { "" };
            output.push_str(&format!("- `{}`{}\n", item.doc_path, stale_indicator));

            if let Some(title) = &item.title {
                output.push_str(&format!("  - {}\n", title));
            }

            if item.is_stale {
                if let Some(reason) = &item.staleness_reason {
                    output.push_str(&format!("  - Reason: {}\n", reason));
                }
            }
        }

        output.push('\n');
    }

    Ok(output)
}

/// Trigger manual documentation scan
pub async fn scan_documentation(
    ctx: &(impl ToolContext + ?Sized),
) -> Result<String, String> {
    let project_id_opt = ctx.project_id().await;
    let project_id = project_id_opt.ok_or("No active project")?;

    let db = ctx.db();
    let conn = db.conn();

    // Clear the scan marker to force new scan
    conn.execute(
        "DELETE FROM memory_facts WHERE project_id = ? AND key = 'documentation_last_scan'",
        params![project_id],
    ).map_err(|e| e.to_string())?;

    Ok(format!(
        "✅ **Documentation scan triggered**\n\n\
        A new documentation scan will run on the next background worker cycle.\n\
        This will detect:\n\
        - Missing MCP tool documentation\n\
        - Undocumented public APIs\n\
        - Missing module docs\n\
        - Stale/outdated documentation\n\n\
        Check progress with `list_doc_tasks()`."
    ))
}

/// Approve a documentation draft (marks it as ready to apply)
pub async fn approve_doc_draft(
    ctx: &(impl ToolContext + ?Sized),
    task_id: i64,
) -> Result<String, String> {
    let db = ctx.db();
    let conn = db.conn();

    let task = get_doc_task(&conn, task_id)?
        .ok_or(format!("Task {} not found", task_id))?;

    if task.status != "draft_ready" {
        return Ok(format!(
            "Task {} is not ready for approval (current status: {})",
            task_id, task.status
        ));
    }

    mark_doc_task_approved(&conn, task_id)?;

    Ok(format!(
        "✅ Task {} approved. Use `apply_doc_draft({}, force=false)` to apply.",
        task_id, task_id
    ))
}

/// Calculate SHA256 checksum of a file
fn file_checksum(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = Vec::new();

    std::io::Read::read_to_end(&mut file, &mut buffer).ok()?;
    hasher.update(&buffer);

    Some(format!("{:x}", hasher.finalize()))
}
