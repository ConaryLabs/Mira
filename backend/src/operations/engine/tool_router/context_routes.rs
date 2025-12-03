// src/operations/engine/tool_router/context_routes.rs
// Context-aware routes for tools that need project/session context

use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use tracing::info;

use crate::project::guidelines::ProjectGuidelinesService;
use crate::project::ProjectTaskService;
use super::super::{guidelines_handlers, task_handlers};

/// Route manage_project_task to task handler
pub async fn route_manage_project_task(
    service: &Arc<ProjectTaskService>,
    args: Value,
    project_id: Option<&str>,
    session_id: &str,
) -> Result<Value> {
    info!("[ROUTER] Routing manage_project_task");
    task_handlers::handle_manage_project_task(service, &args, project_id, session_id).await
}

/// Route manage_project_guidelines to guidelines handler
pub async fn route_manage_project_guidelines(
    service: &Arc<ProjectGuidelinesService>,
    args: Value,
    project_id: Option<&str>,
) -> Result<Value> {
    info!("[ROUTER] Routing manage_project_guidelines");
    guidelines_handlers::handle_manage_project_guidelines(service, &args, project_id).await
}
