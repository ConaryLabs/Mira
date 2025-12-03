// src/operations/engine/tool_router/mod.rs
// Tool Router - Routes tool calls to appropriate handlers
// GPT 5.1 single-model architecture

mod context_routes;
mod file_routes;
mod llm_conversation;
mod registry;

use anyhow::Result;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

use crate::llm::provider::Gemini3Provider;
use crate::memory::features::code_intelligence::CodeIntelligenceService;
use crate::project::guidelines::ProjectGuidelinesService;
use crate::project::ProjectTaskService;
use crate::sudo::SudoPermissionService;
use super::{
    code_handlers::CodeHandlers, external_handlers::ExternalHandlers,
    file_handlers::FileHandlers, git_handlers::GitHandlers,
};

use registry::{HandlerType, ToolRegistry};

/// Routes tool calls to appropriate handlers
pub struct ToolRouter {
    llm: Gemini3Provider,
    file_handlers: FileHandlers,
    external_handlers: ExternalHandlers,
    git_handlers: GitHandlers,
    code_handlers: CodeHandlers,
    project_task_service: Option<Arc<ProjectTaskService>>,
    guidelines_service: Option<Arc<ProjectGuidelinesService>>,
    registry: ToolRegistry,
}

impl ToolRouter {
    /// Create a new tool router
    pub fn new(
        llm: Gemini3Provider,
        project_dir: PathBuf,
        code_intelligence: Arc<CodeIntelligenceService>,
        sudo_service: Option<Arc<SudoPermissionService>>,
    ) -> Self {
        // Create external handlers with optional sudo service
        let external_handlers = if let Some(sudo) = sudo_service {
            ExternalHandlers::new(project_dir.clone()).with_sudo_service(sudo)
        } else {
            ExternalHandlers::new(project_dir.clone())
        };

        Self {
            llm,
            file_handlers: FileHandlers::new(project_dir.clone()),
            external_handlers,
            git_handlers: GitHandlers::new(project_dir),
            code_handlers: CodeHandlers::new(code_intelligence),
            project_task_service: None,
            guidelines_service: None,
            registry: ToolRegistry::new(),
        }
    }

    /// Set the project task service for task management
    pub fn with_project_task_service(mut self, service: Arc<ProjectTaskService>) -> Self {
        self.project_task_service = Some(service);
        self
    }

    /// Set the guidelines service for project guidelines management
    pub fn with_guidelines_service(mut self, service: Arc<ProjectGuidelinesService>) -> Self {
        self.guidelines_service = Some(service);
        self
    }

    /// Route a tool call to appropriate handler
    ///
    /// Flow:
    /// 1. GPT 5.1 calls tool (e.g., "read_project_file")
    /// 2. Router executes via appropriate handler
    /// 3. Results returned to GPT 5.1
    pub async fn route_tool_call(&self, tool_name: &str, arguments: Value) -> Result<Value> {
        info!("[ROUTER] Routing tool: {}", tool_name);

        // Check registry for simple pass-through tools (git, code, external)
        if let Some(route) = self.registry.get_route(tool_name) {
            return self.execute_registered_route(route, arguments).await;
        }

        // Handle complex file operations
        match tool_name {
            "read_project_file" => {
                file_routes::route_read_file(&self.llm, &self.file_handlers, arguments).await
            }
            "write_project_file" => {
                file_routes::route_write_file(&self.file_handlers, arguments).await
            }
            "write_file" => {
                file_routes::route_write_file_unrestricted(&self.file_handlers, arguments).await
            }
            "edit_project_file" => {
                file_routes::route_edit_file(&self.file_handlers, arguments).await
            }
            "search_codebase" => {
                file_routes::route_search(&self.llm, &self.file_handlers, arguments).await
            }
            "list_project_files" => {
                file_routes::route_list_files(&self.llm, &self.file_handlers, arguments).await
            }
            "get_file_summary" => {
                file_routes::route_file_summary(&self.file_handlers, arguments).await
            }
            "get_file_structure" => {
                file_routes::route_file_structure(&self.file_handlers, arguments).await
            }

            // Context-dependent tools - require route_tool_call_with_context
            "manage_project_task" => Err(anyhow::anyhow!(
                "manage_project_task requires context - use route_tool_call_with_context"
            )),
            "manage_project_guidelines" => Err(anyhow::anyhow!(
                "manage_project_guidelines requires context - use route_tool_call_with_context"
            )),

            _ => Err(anyhow::anyhow!("Unknown meta-tool: {}", tool_name)),
        }
    }

    /// Route a tool call with project/session context
    ///
    /// Some tools (like manage_project_task) need context that isn't in the arguments.
    /// Use this method when you have project_id and session_id available.
    pub async fn route_tool_call_with_context(
        &self,
        tool_name: &str,
        arguments: Value,
        project_id: Option<&str>,
        session_id: &str,
    ) -> Result<Value> {
        // Handle context-dependent tools
        match tool_name {
            "manage_project_task" => {
                let service = self.project_task_service.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("ProjectTaskService not configured")
                })?;
                return context_routes::route_manage_project_task(
                    service, arguments, project_id, session_id,
                ).await;
            }
            "manage_project_guidelines" => {
                let service = self.guidelines_service.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("ProjectGuidelinesService not configured")
                })?;
                return context_routes::route_manage_project_guidelines(
                    service, arguments, project_id,
                ).await;
            }
            _ => {}
        }

        // Delegate to regular routing for other tools
        self.route_tool_call(tool_name, arguments).await
    }

    /// Execute a route from the registry
    async fn execute_registered_route(
        &self,
        route: &registry::ToolRoute,
        arguments: Value,
    ) -> Result<Value> {
        match route.handler_type {
            HandlerType::Git => {
                self.git_handlers.execute_tool(&route.internal_name, arguments).await
            }
            HandlerType::Code => {
                self.code_handlers.execute_tool(&route.internal_name, arguments).await
            }
            HandlerType::External => {
                self.external_handlers.execute_tool(&route.internal_name, arguments).await
            }
        }
    }
}
