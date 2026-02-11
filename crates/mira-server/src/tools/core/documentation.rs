// crates/mira-server/src/tools/core/documentation.rs
// Documentation tools - detect gaps, let Claude Code write docs directly

use crate::background::documentation::clear_documentation_scan_marker_sync;
use crate::db::documentation::{
    DocInventory, DocTask, get_doc_inventory, get_doc_task, mark_doc_task_completed,
    mark_doc_task_skipped,
};
use crate::mcp::requests::DocumentationAction;
use crate::mcp::responses::Json;
use crate::mcp::responses::{
    DocData, DocGetData, DocInventoryData, DocInventoryItem, DocListData, DocOutput, DocTaskItem,
};
use crate::tools::core::{NO_ACTIVE_PROJECT_ERROR, ToolContext};
use std::collections::BTreeMap;

/// List documentation that needs to be written or updated
pub async fn list_doc_tasks(
    ctx: &(impl ToolContext + ?Sized),
    status: Option<String>,
    doc_type: Option<String>,
    priority: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Json<DocOutput>, String> {
    let project_id = ctx.project_id().await.ok_or(NO_ACTIVE_PROJECT_ERROR)?;

    let tasks = ctx
        .pool()
        .run(move |conn| {
            list_db_doc_tasks(
                conn,
                Some(project_id),
                status.as_deref(),
                doc_type.as_deref(),
                priority.as_deref(),
            )
        })
        .await?;

    if tasks.is_empty() {
        return Ok(Json(DocOutput {
            action: "list".into(),
            message: "No documentation tasks found for this project.".into(),
            data: Some(DocData::List(DocListData {
                tasks: vec![],
                total: 0,
            })),
        }));
    }

    // Apply pagination
    let offset = offset.unwrap_or(0).max(0) as usize;
    let limit = limit.unwrap_or(50).clamp(1, 500) as usize;
    let total_unfiltered = tasks.len();
    let tasks: Vec<&DocTask> = tasks.iter().skip(offset).take(limit).collect();

    let mut output = String::from("## Documentation Tasks\n\n");

    if total_unfiltered > tasks.len() && !tasks.is_empty() {
        output.push_str(&format!(
            "Showing {}-{} of {} tasks\n\n",
            offset + 1,
            offset + tasks.len(),
            total_unfiltered
        ));
    }

    for task in &tasks {
        let status_indicator = match task.status.as_str() {
            "pending" => "[P]",
            "completed" => "[C]",
            "skipped" => "[S]",
            _ => "[?]",
        };

        output.push_str(&format!(
            "{} **{}** `{}`\n",
            status_indicator, task.doc_category, task.target_doc_path
        ));
        output.push_str(&format!(
            "   ID: {} | Priority: {} | Status: {}\n",
            task.id, task.priority, task.status
        ));

        if let Some(source) = &task.source_file_path {
            output.push_str(&format!("   Source: `{}`\n", source));
        }

        if let Some(reason) = &task.reason {
            output.push_str(&format!("   Reason: {}\n", reason));
        }

        if task.status == "pending" {
            output.push_str(&format!(
                "   → Get details: `documentation(action=\"get\", task_id={})`\n",
                task.id
            ));
        }

        output.push('\n');
    }

    let items: Vec<DocTaskItem> = tasks
        .iter()
        .map(|task| DocTaskItem {
            id: task.id,
            doc_category: task.doc_category.clone(),
            target_doc_path: task.target_doc_path.clone(),
            priority: task.priority.clone(),
            status: task.status.clone(),
            source_file_path: task.source_file_path.clone(),
            reason: task.reason.clone(),
        })
        .collect();
    let total = total_unfiltered;

    Ok(Json(DocOutput {
        action: "list".into(),
        message: output,
        data: Some(DocData::List(DocListData {
            tasks: items,
            total,
        })),
    }))
}

/// Wrapper to avoid name collision with db module function
fn list_db_doc_tasks(
    conn: &rusqlite::Connection,
    project_id: Option<i64>,
    status: Option<&str>,
    doc_type: Option<&str>,
    priority: Option<&str>,
) -> Result<Vec<DocTask>, String> {
    crate::db::documentation::list_doc_tasks(conn, project_id, status, doc_type, priority)
}

/// Get full task details with writing guidelines for Claude to use
pub async fn get_doc_task_details(
    ctx: &(impl ToolContext + ?Sized),
    task_id: i64,
) -> Result<Json<DocOutput>, String> {
    // Require active project
    let current_project_id = ctx.project_id().await.ok_or(NO_ACTIVE_PROJECT_ERROR)?;

    // Get task
    let task = ctx
        .pool()
        .run(move |conn| get_doc_task(conn, task_id))
        .await?
        .ok_or(format!("Task '{}' not found", task_id))?;

    // Verify task belongs to current project
    let task_project_id = task.project_id.ok_or("No project_id on task")?;
    if task_project_id != current_project_id {
        return Err(format!(
            "Task {} belongs to a different project. Switch projects first.",
            task_id
        ));
    }

    // Only allow getting pending tasks
    if task.status != "pending" {
        return Err(format!(
            "Task {} is not pending (status: {}). Only pending tasks can be written.",
            task_id, task.status
        ));
    }

    // Get project path
    let project_id = task_project_id;
    let project_path: String = ctx
        .pool()
        .run(move |conn| crate::db::get_project_path_sync(conn, project_id))
        .await?;

    // Build response with all info Claude needs
    let mut output = format!("## Documentation Task #{}\n\n", task.id);

    output.push_str(&format!("**Target Path:** `{}`\n", task.target_doc_path));
    output.push_str(&format!(
        "**Full Target:** `{}/{}`\n",
        project_path, task.target_doc_path
    ));

    if let Some(source) = &task.source_file_path {
        output.push_str(&format!("**Source File:** `{}`\n", source));
        output.push_str(&format!("**Full Source:** `{}/{}`\n", project_path, source));
    }

    output.push_str(&format!(
        "**Type:** {} / {}\n",
        task.doc_type, task.doc_category
    ));
    output.push_str(&format!("**Priority:** {}\n", task.priority));

    if let Some(reason) = &task.reason {
        output.push_str(&format!("**Reason:** {}\n", reason));
    }

    output.push_str("\n---\n\n");

    // Add category-specific guidelines
    output.push_str("## Writing Guidelines\n\n");

    let guidelines = match task.doc_category.as_str() {
        "mcp_tool" => {
            output.push_str("For MCP tool documentation, include:\n\n");
            output.push_str("1. **Title** - Tool name as heading\n");
            output.push_str("2. **Description** - One paragraph explaining what it does\n");
            output.push_str(
                "3. **Parameters** - Table with columns: Name, Type, Required, Description\n",
            );
            output.push_str("4. **Returns** - What the tool returns on success\n");
            output.push_str("5. **Examples** - 2-3 realistic usage examples\n");
            output.push_str("6. **Errors** - Common failure modes\n");
            output.push_str("7. **See Also** - Related tools (if any)\n");
            "For MCP tool documentation, include: Title, Description, Parameters table, Returns, Examples, Errors, See Also"
        }
        "module" => {
            output.push_str("For module documentation, include:\n\n");
            output.push_str("1. **Overview** - What this module does and why it exists\n");
            output.push_str("2. **Key Components** - Main structs, functions, traits\n");
            output.push_str("3. **Usage Patterns** - How to use this module\n");
            output.push_str("4. **Architecture Notes** - Design decisions, dependencies\n");
            "For module documentation, include: Overview, Key Components, Usage Patterns, Architecture Notes"
        }
        "public_api" => {
            output.push_str("For public API documentation, include:\n\n");
            output.push_str("1. **Overview** - What this API provides\n");
            output.push_str("2. **Functions/Methods** - Signature, parameters, return values\n");
            output.push_str("3. **Examples** - Code snippets showing usage\n");
            output.push_str("4. **Error Handling** - What errors can occur\n");
            "For public API documentation, include: Overview, Functions/Methods, Examples, Error Handling"
        }
        _ => {
            output.push_str("Write clear, concise documentation that explains:\n\n");
            output.push_str("1. What this code does\n");
            output.push_str("2. How to use it\n");
            output.push_str("3. Any important caveats or edge cases\n");
            "Write clear, concise documentation explaining what the code does, how to use it, and important caveats"
        }
    };

    output.push_str("\n---\n\n");
    output.push_str("## Instructions\n\n");
    output.push_str("1. Read the source file to understand the implementation\n");
    output.push_str("2. Write the documentation to the target path\n");
    output.push_str(&format!(
        "3. Mark complete: `documentation(action=\"complete\", task_id={})`\n",
        task.id
    ));

    Ok(Json(DocOutput {
        action: "get".into(),
        message: output,
        data: Some(DocData::Get(DocGetData {
            task_id: task.id,
            target_doc_path: task.target_doc_path.clone(),
            full_target_path: format!("{}/{}", project_path, task.target_doc_path),
            doc_type: task.doc_type.clone(),
            doc_category: task.doc_category.clone(),
            priority: task.priority.clone(),
            source_file_path: task.source_file_path.clone(),
            full_source_path: task
                .source_file_path
                .as_ref()
                .map(|s| format!("{}/{}", project_path, s)),
            reason: task.reason.clone(),
            guidelines: guidelines.to_string(),
        })),
    }))
}

/// Mark a documentation task as complete (after Claude has written the doc)
pub async fn complete_doc_task(
    ctx: &(impl ToolContext + ?Sized),
    task_id: i64,
) -> Result<Json<DocOutput>, String> {
    // Require active project
    let current_project_id = ctx.project_id().await.ok_or(NO_ACTIVE_PROJECT_ERROR)?;

    // Verify task exists and is pending
    let task = ctx
        .pool()
        .run(move |conn| get_doc_task(conn, task_id))
        .await?
        .ok_or(format!("Task '{}' not found", task_id))?;

    // Verify task belongs to current project
    if task.project_id != Some(current_project_id) {
        return Err(format!("Task {} belongs to a different project.", task_id));
    }

    if task.status != "pending" {
        return Err(format!(
            "Task {} is not pending (status: {}). Cannot mark as complete.",
            task_id, task.status
        ));
    }

    // Mark as completed
    ctx.pool()
        .run(move |conn| mark_doc_task_completed(conn, task_id))
        .await?;

    Ok(Json(DocOutput {
        action: "complete".into(),
        message: format!(
            "Task {} marked complete. Documentation written to `{}`.",
            task_id, task.target_doc_path
        ),
        data: None,
    }))
}

/// Skip a documentation task (mark as not needed)
pub async fn skip_doc_task(
    ctx: &(impl ToolContext + ?Sized),
    task_id: i64,
    reason: Option<String>,
) -> Result<Json<DocOutput>, String> {
    // Require active project
    let current_project_id = ctx.project_id().await.ok_or(NO_ACTIVE_PROJECT_ERROR)?;

    // Verify task exists and belongs to current project
    let task = ctx
        .pool()
        .run(move |conn| get_doc_task(conn, task_id))
        .await?
        .ok_or(format!("Task '{}' not found", task_id))?;

    if task.project_id != Some(current_project_id) {
        return Err(format!("Task {} belongs to a different project.", task_id));
    }

    if task.status != "pending" {
        return Err(format!(
            "Task {} is not pending (status: {}). Cannot skip.",
            task_id, task.status
        ));
    }

    let skip_reason = reason.unwrap_or_else(|| "Skipped by user".to_string());
    let skip_reason_clone = skip_reason.clone();

    ctx.pool()
        .run(move |conn| mark_doc_task_skipped(conn, task_id, &skip_reason_clone))
        .await?;

    Ok(Json(DocOutput {
        action: "skip".into(),
        message: format!("Task {} skipped: {}", task_id, skip_reason),
        data: None,
    }))
}

/// Batch skip documentation tasks by IDs or filter
pub async fn batch_skip_doc_tasks(
    ctx: &(impl ToolContext + ?Sized),
    task_ids: Option<Vec<i64>>,
    reason: Option<String>,
    doc_type: Option<String>,
    priority: Option<String>,
) -> Result<Json<DocOutput>, String> {
    let current_project_id = ctx.project_id().await.ok_or(NO_ACTIVE_PROJECT_ERROR)?;

    let skip_reason = reason.unwrap_or_else(|| "Batch skipped by user".to_string());

    // Determine which tasks to skip
    let tasks_to_skip: Vec<DocTask> = if let Some(ids) = task_ids {
        if ids.is_empty() {
            return Err("task_ids list is empty".to_string());
        }
        // Fetch each task by ID
        let mut tasks = Vec::new();
        for id in ids {
            let task = ctx
                .pool()
                .run(move |conn| get_doc_task(conn, id))
                .await?
                .ok_or(format!("Task '{}' not found", id))?;
            tasks.push(task);
        }
        tasks
    } else if doc_type.is_some() || priority.is_some() {
        // Use filter to find matching pending tasks
        let dt = doc_type.clone();
        let pr = priority.clone();
        ctx.pool()
            .run(move |conn| {
                list_db_doc_tasks(
                    conn,
                    Some(current_project_id),
                    Some("pending"),
                    dt.as_deref(),
                    pr.as_deref(),
                )
            })
            .await?
    } else {
        return Err(
            "batch_skip requires either task_ids or a filter (doc_type/priority)".to_string(),
        );
    };

    // Filter to only pending tasks belonging to current project
    let eligible: Vec<&DocTask> = tasks_to_skip
        .iter()
        .filter(|t| t.project_id == Some(current_project_id) && t.status == "pending")
        .collect();

    if eligible.is_empty() {
        return Ok(Json(DocOutput {
            action: "batch_skip".into(),
            message: "No eligible pending tasks found to skip.".into(),
            data: None,
        }));
    }

    let mut skipped_ids = Vec::new();
    let mut errors = Vec::new();

    for task in &eligible {
        let task_id = task.id;
        let reason_clone = skip_reason.clone();
        match ctx
            .pool()
            .run(move |conn| mark_doc_task_skipped(conn, task_id, &reason_clone))
            .await
        {
            Ok(()) => skipped_ids.push(task_id),
            Err(e) => errors.push(format!("Task {}: {}", task_id, e)),
        }
    }

    let mut message = format!("Skipped {} tasks: {:?}", skipped_ids.len(), skipped_ids);
    if !errors.is_empty() {
        message.push_str(&format!("\nErrors: {}", errors.join("; ")));
    }

    Ok(Json(DocOutput {
        action: "batch_skip".into(),
        message,
        data: None,
    }))
}

/// Show documentation inventory with staleness indicators
pub async fn show_doc_inventory(
    ctx: &(impl ToolContext + ?Sized),
) -> Result<Json<DocOutput>, String> {
    let project_id = ctx.project_id().await.ok_or(NO_ACTIVE_PROJECT_ERROR)?;

    let inventory = ctx
        .pool()
        .run(move |conn| get_doc_inventory(conn, project_id))
        .await?;

    // Fetch impact data for stale docs
    let impact_data = ctx
        .pool()
        .run(move |conn| get_doc_impact_data(conn, project_id))
        .await
        .unwrap_or_default();

    if inventory.is_empty() {
        return Ok(Json(DocOutput {
            action: "inventory".into(),
            message: "No documentation inventory found. Run scan to build inventory.".into(),
            data: Some(DocData::Inventory(DocInventoryData {
                docs: vec![],
                total: 0,
                stale_count: 0,
            })),
        }));
    }

    let mut output = String::from("## Documentation Inventory\n\n");

    let stale_count = inventory.iter().filter(|i| i.is_stale).count();
    output.push_str(&format!("Total: {} documents", inventory.len()));
    if stale_count > 0 {
        output.push_str(&format!(" ({} stale)", stale_count));
    }
    output.push_str("\n\n---\n\n");

    // Group by type using BTreeMap for stable ordering
    let mut by_type: BTreeMap<&str, Vec<&DocInventory>> = BTreeMap::new();
    for item in &inventory {
        by_type.entry(&item.doc_type).or_default().push(item);
    }

    for (doc_type, items) in by_type.iter() {
        output.push_str(&format!("### {}\n\n", doc_type));

        for item in items {
            let stale_indicator = if item.is_stale { " [STALE]" } else { "" };
            output.push_str(&format!("- `{}`{}\n", item.doc_path, stale_indicator));

            if let Some(title) = &item.title {
                output.push_str(&format!("  - {}\n", title));
            }

            if item.is_stale
                && let Some(reason) = &item.staleness_reason
            {
                output.push_str(&format!("  - Reason: {}\n", reason));
            }

            // Show impact data if available
            if let Some((impact, summary)) = impact_data.get(&item.id) {
                output.push_str(&format!("  - Impact: {}\n", impact));
                output.push_str(&format!("  - Summary: {}\n", summary));
            }
        }

        output.push('\n');
    }

    let items: Vec<DocInventoryItem> = inventory
        .iter()
        .map(|item| {
            let (change_impact, change_summary) = impact_data
                .get(&item.id)
                .map(|(i, s)| (Some(i.clone()), Some(s.clone())))
                .unwrap_or((None, None));
            DocInventoryItem {
                doc_path: item.doc_path.clone(),
                doc_type: item.doc_type.clone(),
                is_stale: item.is_stale,
                title: item.title.clone(),
                staleness_reason: item.staleness_reason.clone(),
                change_impact,
                change_summary,
            }
        })
        .collect();
    let total = items.len();

    Ok(Json(DocOutput {
        action: "inventory".into(),
        message: output,
        data: Some(DocData::Inventory(DocInventoryData {
            docs: items,
            total,
            stale_count,
        })),
    }))
}

/// Fetch change_impact and change_summary for inventory items
fn get_doc_impact_data(
    conn: &rusqlite::Connection,
    project_id: i64,
) -> Result<std::collections::HashMap<i64, (String, String)>, String> {
    use crate::utils::ResultExt;
    let mut stmt = conn
        .prepare(
            "SELECT id, change_impact, change_summary FROM documentation_inventory
             WHERE project_id = ? AND is_stale = 1 AND change_impact IS NOT NULL AND change_summary IS NOT NULL",
        )
        .str_err()?;

    let rows = stmt
        .query_map(rusqlite::params![project_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .str_err()?;

    let mut map = std::collections::HashMap::new();
    for (id, impact, summary) in rows.flatten() {
        map.insert(id, (impact, summary));
    }
    Ok(map)
}

/// Trigger manual documentation scan
pub async fn scan_documentation(
    ctx: &(impl ToolContext + ?Sized),
) -> Result<Json<DocOutput>, String> {
    let project_id = ctx.project_id().await.ok_or(NO_ACTIVE_PROJECT_ERROR)?;

    // Clear the scan marker to force new scan
    ctx.pool()
        .run(move |conn| clear_documentation_scan_marker_sync(conn, project_id))
        .await?;

    Ok(Json(DocOutput {
        action: "scan".into(),
        message: "Documentation scan triggered. Check `documentation(action=\"list\")` for results after scan completes.".into(),
        data: None,
    }))
}

/// Unified documentation tool with action parameter
/// Actions: list, get, complete, skip, batch_skip, inventory, scan
pub async fn documentation<C: ToolContext>(
    ctx: &C,
    req: crate::mcp::requests::DocumentationRequest,
) -> Result<Json<DocOutput>, String> {
    match req.action {
        DocumentationAction::List => {
            list_doc_tasks(
                ctx,
                req.status,
                req.doc_type,
                req.priority,
                req.limit,
                req.offset,
            )
            .await
        }
        DocumentationAction::Get => {
            let id = req
                .task_id
                .ok_or("task_id is required for documentation(action=get)")?;
            get_doc_task_details(ctx, id).await
        }
        DocumentationAction::Complete => {
            let id = req
                .task_id
                .ok_or("task_id is required for documentation(action=complete)")?;
            complete_doc_task(ctx, id).await
        }
        DocumentationAction::Skip => {
            let id = req
                .task_id
                .ok_or("task_id is required for documentation(action=skip)")?;
            skip_doc_task(ctx, id, req.reason).await
        }
        DocumentationAction::BatchSkip => {
            batch_skip_doc_tasks(ctx, req.task_ids, req.reason, req.doc_type, req.priority).await
        }
        DocumentationAction::Inventory => show_doc_inventory(ctx).await,
        DocumentationAction::Scan => scan_documentation(ctx).await,
    }
}
