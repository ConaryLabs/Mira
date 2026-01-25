// crates/mira-server/src/tools/core/documentation.rs
// Simplified documentation tools - detect gaps, write docs directly

use crate::background::documentation::clear_documentation_scan_marker_sync;
use crate::db::documentation::{
    get_doc_inventory, get_doc_task, mark_doc_task_applied, mark_doc_task_skipped, DocInventory,
    DocTask,
};
use crate::tools::core::ToolContext;
use std::path::Path;

/// List documentation that needs to be written or updated
pub async fn list_doc_tasks(
    ctx: &(impl ToolContext + ?Sized),
    status: Option<String>,
    doc_type: Option<String>,
    priority: Option<String>,
) -> Result<String, String> {
    let project_id_opt = ctx.project_id().await;

    let tasks = ctx
        .pool()
        .interact(move |conn| {
            list_db_doc_tasks(
                conn,
                project_id_opt,
                status.as_deref(),
                doc_type.as_deref(),
                priority.as_deref(),
            )
            .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .map_err(|e| e.to_string())?;

    if tasks.is_empty() {
        return Ok("No documentation tasks found.".to_string());
    }

    let mut output = String::from("## Documentation Needed\n\n");

    for task in tasks {
        let status_indicator = match task.status.as_str() {
            "pending" => "[needs docs]",
            "applied" => "[done]",
            "skipped" => "[skipped]",
            _ => "[?]",
        };

        output.push_str(&format!(
            "{} `{}` -> `{}`\n",
            status_indicator, task.doc_category, task.target_doc_path
        ));
        output.push_str(&format!("  ID: {} | Priority: {}\n", task.id, task.priority));

        if let Some(reason) = &task.reason {
            output.push_str(&format!("  Reason: {}\n", reason));
        }

        if task.status == "pending" {
            output.push_str(&format!("  Write with: `write_documentation({})`\n", task.id));
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
    crate::db::documentation::list_doc_tasks(conn, project_id, status, doc_type, priority)
}

/// Skip a documentation task (mark as not needed)
pub async fn skip_doc_task(
    ctx: &(impl ToolContext + ?Sized),
    task_id: i64,
    reason: Option<String>,
) -> Result<String, String> {
    let skip_reason = reason.unwrap_or_else(|| "Skipped by user".to_string());
    let skip_reason_clone = skip_reason.clone();

    ctx.pool()
        .interact(move |conn| {
            mark_doc_task_skipped(conn, task_id, &skip_reason_clone)
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .map_err(|e| e.to_string())?;

    Ok(format!("Task {} skipped: {}", task_id, skip_reason))
}

/// Show documentation inventory with staleness indicators
pub async fn show_doc_inventory(ctx: &(impl ToolContext + ?Sized)) -> Result<String, String> {
    let project_id_opt = ctx.project_id().await;
    let project_id = project_id_opt.ok_or("No active project")?;

    let inventory = ctx
        .pool()
        .interact(move |conn| {
            get_doc_inventory(conn, project_id).map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .map_err(|e| e.to_string())?;

    if inventory.is_empty() {
        return Ok("No documentation inventory found. Run scan to build inventory.".to_string());
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
pub async fn scan_documentation(ctx: &(impl ToolContext + ?Sized)) -> Result<String, String> {
    let project_id_opt = ctx.project_id().await;
    let project_id = project_id_opt.ok_or("No active project")?;

    // Clear the scan marker to force new scan
    ctx.pool()
        .interact(move |conn| -> Result<(), anyhow::Error> {
            clear_documentation_scan_marker_sync(conn, project_id)
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .map_err(|e| e.to_string())?;

    Ok(
        "Documentation scan triggered. Check `list_doc_tasks()` for results after scan completes."
            .to_string(),
    )
}

/// Write documentation for a detected gap - expert generates and writes directly
pub async fn write_documentation<C: ToolContext>(
    ctx: &C,
    task_id: i64,
) -> Result<String, String> {
    use crate::tools::core::experts::{consult_expert, ExpertRole};

    // Get task details
    let task = ctx
        .pool()
        .interact(move |conn| {
            get_doc_task(conn, task_id).map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .map_err(|e| e.to_string())?
        .ok_or(format!("Task {} not found", task_id))?;

    // Only allow writing for pending tasks
    if task.status != "pending" {
        return Err(format!(
            "Task {} is not pending (status: {}). Only pending tasks can be written.",
            task_id, task.status
        ));
    }

    // Get project path
    let project_id = task.project_id.ok_or("No project_id on task")?;
    let project_path = ctx
        .pool()
        .interact(move |conn| {
            conn.query_row(
                "SELECT path FROM projects WHERE id = ?",
                [project_id],
                |row| row.get::<_, String>(0),
            )
            .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .map_err(|e| e.to_string())?;

    // Build context for the expert
    let context = build_expert_context(ctx, &task, &project_path).await?;

    // Derive source identifier
    let source_identifier = task
        .source_file_path
        .as_deref()
        .unwrap_or(&task.target_doc_path);

    // Build the instruction
    let question = format!(
        "Generate comprehensive markdown documentation for `{}`. \
         The documentation will be written to `{}`. \
         Explore the codebase to understand the actual behavior, not just the signatures. \
         Return ONLY the markdown content, no explanations.",
        source_identifier, task.target_doc_path
    );

    // Call the documentation expert
    let draft = consult_expert(ctx, ExpertRole::DocumentationWriter, context, Some(question)).await?;

    // Extract just the markdown content
    let markdown_content = extract_markdown_from_response(&draft);

    // Write directly to file
    let target_path = Path::new(&project_path).join(&task.target_doc_path);

    // Create parent directories if needed
    if let Some(parent) = target_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    // Write the file
    tokio::fs::write(&target_path, &markdown_content)
        .await
        .map_err(|e| format!("Failed to write file: {}", e))?;

    // Mark task as applied
    ctx.pool()
        .interact(move |conn| {
            mark_doc_task_applied(conn, task_id).map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await
        .map_err(|e| e.to_string())?;

    Ok(format!(
        "Documentation written to `{}`\nTask {} marked complete.",
        task.target_doc_path, task_id
    ))
}

/// Build context for the expert based on the documentation task type
async fn build_expert_context<C: ToolContext>(
    _ctx: &C,
    task: &DocTask,
    project_path: &str,
) -> Result<String, String> {
    let mut context = String::new();

    let source_identifier = task
        .source_file_path
        .as_deref()
        .unwrap_or(&task.target_doc_path);

    context.push_str("# Documentation Task\n\n");
    context.push_str(&format!("**Type:** {} / {}\n", task.doc_type, task.doc_category));
    context.push_str(&format!("**Target:** {}\n", source_identifier));
    context.push_str(&format!("**Output Path:** {}\n\n", task.target_doc_path));

    if let Some(reason) = &task.reason {
        context.push_str(&format!("**Reason:** {}\n\n", reason));
    }

    // Add source file content if available
    if let Some(source_path) = &task.source_file_path {
        let full_path = Path::new(project_path).join(source_path);
        if let Ok(content) = tokio::fs::read_to_string(&full_path).await {
            let lang = detect_language(source_path);
            context.push_str("## Source File\n\n");
            context.push_str(&format!("```{}\n{}\n```\n\n", lang, content));
        }
    }

    // Add guidance based on doc type
    match task.doc_category.as_str() {
        "mcp_tool" => {
            context.push_str("## Guidelines for MCP Tool Documentation\n\n");
            context.push_str("Include: Purpose, Parameters (with types/defaults), Return Value, Examples, Errors, Related tools.\n");
        }
        "module" => {
            context.push_str("## Guidelines for Module Documentation\n\n");
            context.push_str("Include: Overview, Key Components, Usage patterns, Architecture notes.\n");
        }
        _ => {}
    }

    Ok(context)
}

/// Detect programming language from file extension
fn detect_language(path: &str) -> &'static str {
    if path.ends_with(".rs") {
        "rust"
    } else if path.ends_with(".py") {
        "python"
    } else if path.ends_with(".ts") {
        "typescript"
    } else if path.ends_with(".js") {
        "javascript"
    } else if path.ends_with(".go") {
        "go"
    } else {
        ""
    }
}

/// Extract markdown content from expert response
fn extract_markdown_from_response(response: &str) -> String {
    let mut content = response.to_string();

    // Strip token stats at the end (e.g., "*Tokens: 27196 prompt, 1121 completion*")
    if let Some(pos) = content.rfind("\n---\n*Tokens:") {
        content = content[..pos].to_string();
    } else if let Some(pos) = content.rfind("*Tokens:") {
        if let Some(line_start) = content[..pos].rfind('\n') {
            content = content[..line_start].to_string();
        }
    }

    // Strip <details> blocks (reasoning sections)
    while let Some(start) = content.find("<details>") {
        if let Some(end) = content.find("</details>") {
            let before = &content[..start];
            let after = &content[end + 10..];
            content = format!("{}{}", before.trim(), after);
        } else {
            break;
        }
    }

    // If response contains a code block with markdown, extract it
    if let Some(start) = content.find("```md") {
        if let Some(end) = content[start + 5..].find("```") {
            return content[start + 5..start + 5 + end].trim().to_string();
        }
    }

    if let Some(start) = content.find("```markdown") {
        if let Some(end) = content[start + 11..].find("```") {
            return content[start + 11..start + 11 + end].trim().to_string();
        }
    }

    // Strip analytical sections that shouldn't be in reference docs
    let analytical_patterns = [
        "\n## Key Findings",
        "\n## Actionable Insights",
        "\n## Actionable Recommendations",
        "\n## Recommendations",
        "\n## Quality Assessment",
        "\n## Conclusion",
        "\n## Assessment",
        "\n## Analysis",
        "\n## Overall Assessment",
        "\n## Future Enhancement",
        "\n### For Documentation",
        "\n### For Users",
        "\n### For Developers",
        "\n### For Implementation",
    ];

    for pattern in analytical_patterns {
        // Find the section and remove it along with its content until the next heading
        while let Some(start) = content.find(pattern) {
            // Find the next section heading (## or end of content)
            let after_heading = start + pattern.len();
            let next_section = content[after_heading..]
                .find("\n## ")
                .map(|p| after_heading + p)
                .unwrap_or(content.len());

            let before = &content[..start];
            let after = &content[next_section..];
            content = format!("{}{}", before.trim_end(), after);
        }
    }

    // Look for the first proper heading, skipping meta-headings
    let meta_prefixes = [
        "# Documentation Writer",
        "# Analysis",
        "# Expert",
        "# Summary",
    ];

    for pattern in ["# ", "\n# "] {
        if let Some(pos) = content.find(pattern) {
            let start = if pattern.starts_with('\n') {
                pos + 1
            } else {
                pos
            };
            let heading_line = &content[start..];
            // Skip meta headings
            let is_meta = meta_prefixes
                .iter()
                .any(|prefix| heading_line.starts_with(prefix));
            if !is_meta {
                return content[start..].trim().to_string();
            }
        }
    }

    // Look for ## heading after skipping meta content
    if let Some(pos) = content.find("\n## ") {
        let heading_line = &content[pos + 1..];
        if !heading_line.starts_with("## Documentation Writer")
            && !heading_line.starts_with("## Analysis")
            && !heading_line.starts_with("## Summary")
        {
            return content[pos + 1..].trim().to_string();
        }
    }

    // Fallback: return cleaned content
    content.trim().to_string()
}
