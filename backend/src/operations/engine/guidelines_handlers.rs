// backend/src/operations/engine/guidelines_handlers.rs
// Tool handlers for project guidelines management

use crate::project::guidelines::ProjectGuidelinesService;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::info;

/// Handle the manage_project_guidelines tool
pub async fn handle_manage_project_guidelines(
    guidelines_service: &Arc<ProjectGuidelinesService>,
    args: &Value,
    project_id: Option<&str>,
) -> Result<Value> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing required 'action' parameter"))?;

    let project_id =
        project_id.ok_or_else(|| anyhow!("No project context - cannot manage guidelines"))?;

    info!(action = action, project_id = project_id, "Handling manage_project_guidelines");

    match action {
        "get" => handle_get(guidelines_service, project_id).await,
        "set" => handle_set(guidelines_service, args, project_id).await,
        "append" => handle_append(guidelines_service, args, project_id).await,
        _ => Err(anyhow!("Unknown action: {}", action)),
    }
}

/// Get current guidelines
async fn handle_get(
    guidelines_service: &Arc<ProjectGuidelinesService>,
    project_id: &str,
) -> Result<Value> {
    let guidelines = guidelines_service.get_guidelines(project_id).await?;

    Ok(json!({
        "success": true,
        "exists": guidelines.is_some(),
        "content": guidelines.as_ref().map(|g| &g.content),
        "file_path": guidelines.as_ref().map(|g| &g.file_path),
        "message": if guidelines.is_some() {
            "Guidelines retrieved"
        } else {
            "No guidelines set for this project"
        }
    }))
}

/// Set (replace) guidelines content
async fn handle_set(
    guidelines_service: &Arc<ProjectGuidelinesService>,
    args: &Value,
    project_id: &str,
) -> Result<Value> {
    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing required 'content' parameter for set action"))?;

    guidelines_service
        .save_guidelines(project_id, "MIRA_GUIDELINES.md", content)
        .await?;

    info!(project_id = project_id, "Saved project guidelines");

    Ok(json!({
        "success": true,
        "message": "Guidelines saved successfully",
        "content_length": content.len()
    }))
}

/// Append to existing guidelines
async fn handle_append(
    guidelines_service: &Arc<ProjectGuidelinesService>,
    args: &Value,
    project_id: &str,
) -> Result<Value> {
    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing required 'content' parameter for append action"))?;

    let section = args.get("section").and_then(|v| v.as_str());

    // Get existing guidelines
    let existing = guidelines_service
        .get_guidelines(project_id)
        .await?
        .map(|g| g.content)
        .unwrap_or_default();

    // Build new content
    let new_content = if let Some(section_name) = section {
        if existing.is_empty() {
            format!("## {}\n\n{}", section_name, content)
        } else {
            format!("{}\n\n## {}\n\n{}", existing, section_name, content)
        }
    } else if existing.is_empty() {
        content.to_string()
    } else {
        format!("{}\n\n{}", existing, content)
    };

    guidelines_service
        .save_guidelines(project_id, "MIRA_GUIDELINES.md", &new_content)
        .await?;

    info!(
        project_id = project_id,
        section = section,
        "Appended to project guidelines"
    );

    Ok(json!({
        "success": true,
        "message": if section.is_some() {
            format!("Added section '{}' to guidelines", section.unwrap())
        } else {
            "Content appended to guidelines".to_string()
        },
        "content_length": new_content.len()
    }))
}
