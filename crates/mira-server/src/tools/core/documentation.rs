// crates/mira-server/src/tools/core/documentation.rs
// Documentation tools - detect gaps, let Claude Code write docs directly

use crate::background::documentation::clear_documentation_scan_marker_sync;
use crate::db::documentation::{
    DocInventory, DocTask, get_doc_inventory, get_doc_task, mark_doc_task_applied,
    mark_doc_task_skipped,
};
use crate::mcp::requests::DocumentationAction;
use crate::mcp::responses::Json;
use crate::mcp::responses::{
    DocData, DocGetData, DocInventoryData, DocInventoryItem, DocListData, DocOutput, DocTaskItem,
};
use crate::tools::core::ToolContext;

/// List documentation that needs to be written or updated
pub async fn list_doc_tasks(
    ctx: &(impl ToolContext + ?Sized),
    status: Option<String>,
    doc_type: Option<String>,
    priority: Option<String>,
) -> Result<Json<DocOutput>, String> {
    let project_id = ctx
        .project_id()
        .await
        .ok_or("No active project. Use project(action=\"start\") first.")?;

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

    let mut output = String::from("## Documentation Tasks\n\n");

    for task in &tasks {
        let status_indicator = match task.status.as_str() {
            "pending" => "[P]",
            "applied" => "[A]",
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
    let total = items.len();

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
    let current_project_id = ctx
        .project_id()
        .await
        .ok_or("No active project. Use project(action=\"start\") first.")?;

    // Get task
    let task = ctx
        .pool()
        .run(move |conn| get_doc_task(conn, task_id))
        .await?
        .ok_or(format!("Task {} not found", task_id))?;

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
        .run(move |conn| {
            conn.query_row(
                "SELECT path FROM projects WHERE id = ?",
                [project_id],
                |row| row.get::<_, String>(0),
            )
        })
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
    let current_project_id = ctx
        .project_id()
        .await
        .ok_or("No active project. Use project(action=\"start\") first.")?;

    // Verify task exists and is pending
    let task = ctx
        .pool()
        .run(move |conn| get_doc_task(conn, task_id))
        .await?
        .ok_or(format!("Task {} not found", task_id))?;

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

    // Mark as applied
    ctx.pool()
        .run(move |conn| mark_doc_task_applied(conn, task_id))
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
    let current_project_id = ctx
        .project_id()
        .await
        .ok_or("No active project. Use project(action=\"start\") first.")?;

    // Verify task exists and belongs to current project
    let task = ctx
        .pool()
        .run(move |conn| get_doc_task(conn, task_id))
        .await?
        .ok_or(format!("Task {} not found", task_id))?;

    if task.project_id != Some(current_project_id) {
        return Err(format!("Task {} belongs to a different project.", task_id));
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

/// Show documentation inventory with staleness indicators
pub async fn show_doc_inventory(
    ctx: &(impl ToolContext + ?Sized),
) -> Result<Json<DocOutput>, String> {
    let project_id_opt = ctx.project_id().await;
    let project_id = project_id_opt.ok_or("No active project")?;

    let inventory = ctx
        .pool()
        .run(move |conn| get_doc_inventory(conn, project_id))
        .await?;

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

    // Group by type
    let mut by_type: std::collections::HashMap<&str, Vec<&DocInventory>> = Default::default();
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
        }

        output.push('\n');
    }

    let items: Vec<DocInventoryItem> = inventory
        .iter()
        .map(|item| DocInventoryItem {
            doc_path: item.doc_path.clone(),
            doc_type: item.doc_type.clone(),
            is_stale: item.is_stale,
            title: item.title.clone(),
            staleness_reason: item.staleness_reason.clone(),
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

/// Trigger manual documentation scan
pub async fn scan_documentation(
    ctx: &(impl ToolContext + ?Sized),
) -> Result<Json<DocOutput>, String> {
    let project_id_opt = ctx.project_id().await;
    let project_id = project_id_opt.ok_or("No active project")?;

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
/// Actions: list, get, complete, skip, inventory, scan
pub async fn documentation<C: ToolContext>(
    ctx: &C,
    action: DocumentationAction,
    task_id: Option<i64>,
    reason: Option<String>,
    doc_type: Option<String>,
    priority: Option<String>,
    status: Option<String>,
) -> Result<Json<DocOutput>, String> {
    match action {
        DocumentationAction::List => list_doc_tasks(ctx, status, doc_type, priority).await,
        DocumentationAction::Get => {
            let id = task_id.ok_or("task_id is required for action 'get'")?;
            get_doc_task_details(ctx, id).await
        }
        DocumentationAction::Complete => {
            let id = task_id.ok_or("task_id is required for action 'complete'")?;
            complete_doc_task(ctx, id).await
        }
        DocumentationAction::Skip => {
            let id = task_id.ok_or("task_id is required for action 'skip'")?;
            skip_doc_task(ctx, id, reason).await
        }
        DocumentationAction::Inventory => show_doc_inventory(ctx).await,
        DocumentationAction::Scan => scan_documentation(ctx).await,
        DocumentationAction::ExportClaudeLocal => {
            let message = crate::tools::core::claude_local::export_claude_local(ctx).await?;
            Ok(Json(DocOutput {
                action: "export_claude_local".into(),
                message,
                data: None,
            }))
        }
    }
}
