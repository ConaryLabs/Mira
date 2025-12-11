// src/operations/engine/tool_router/mod.rs
// Tool Router - Routes tool calls to appropriate handlers
// OpenAI GPT-5.1 multi-model architecture (December 2025)

mod context_routes;
mod file_routes;
mod llm_conversation;
mod registry;

use anyhow::{Context, Result};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

use crate::api::ws::message::SystemAccessMode;
use crate::llm::provider::LlmProvider;
use crate::mcp::McpManager;
use crate::memory::features::code_intelligence::CodeIntelligenceService;
use crate::project::guidelines::ProjectGuidelinesService;
use crate::project::{ProjectStore, ProjectTaskService};
use crate::sudo::SudoPermissionService;
use super::{
    code_handlers::CodeHandlers, external_handlers::ExternalHandlers,
    file_handlers::FileHandlers, git_handlers::GitHandlers,
};

use registry::{HandlerType, ToolRegistry};

/// Routes tool calls to appropriate handlers
pub struct ToolRouter {
    llm: Arc<dyn LlmProvider>,
    file_handlers: FileHandlers,
    external_handlers: ExternalHandlers,
    git_handlers: GitHandlers,
    code_handlers: CodeHandlers,
    project_task_service: Option<Arc<ProjectTaskService>>,
    guidelines_service: Option<Arc<ProjectGuidelinesService>>,
    project_store: Option<Arc<ProjectStore>>,
    mcp_manager: Option<Arc<McpManager>>,
    registry: ToolRegistry,
}

impl ToolRouter {
    /// Create a new tool router
    pub fn new(
        llm: Arc<dyn LlmProvider>,
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
            project_store: None,
            mcp_manager: None,
            registry: ToolRegistry::new(),
        }
    }

    /// Set the MCP manager for external tool server integration
    pub fn with_mcp_manager(mut self, manager: Arc<McpManager>) -> Self {
        self.mcp_manager = Some(manager);
        self
    }

    /// Get the MCP manager if available
    pub fn mcp_manager(&self) -> Option<&Arc<McpManager>> {
        self.mcp_manager.as_ref()
    }

    /// Set the project task service for task management
    pub fn with_project_task_service(mut self, service: Arc<ProjectTaskService>) -> Self {
        self.project_task_service = Some(service);
        self
    }

    /// Set the project store for dynamic path resolution
    pub fn with_project_store(mut self, store: Arc<ProjectStore>) -> Self {
        self.project_store = Some(store);
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
    /// 1. LLM calls tool (e.g., "read_project_file")
    /// 2. Router executes via appropriate handler
    /// 3. Results returned to LLM
    pub async fn route_tool_call(&self, tool_name: &str, arguments: Value) -> Result<Value> {
        info!("[ROUTER] Routing tool: {}", tool_name);

        // Handle MCP tools first (prefix: mcp__{server}__{tool})
        if tool_name.starts_with("mcp__") {
            return self.route_mcp_tool(tool_name, arguments).await;
        }

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

            // Native GPT-5.1 tools (Responses API built-in tools)
            "__native_apply_patch" => {
                file_routes::route_native_apply_patch(&self.file_handlers, arguments).await
            }
            "__native_shell" => {
                self.route_native_shell(arguments).await
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

        // For external handlers (command execution), inject project path as working directory
        if let Some(route) = self.registry.get_route(tool_name) {
            if matches!(route.handler_type, HandlerType::External) {
                let enhanced_args = self.inject_project_path(arguments, project_id).await;
                return self.execute_registered_route(route, enhanced_args).await;
            }
        }

        // Set file and external handler's project directory if we have a project_id
        // This allows file operations and commands to use the project's root path
        if let (Some(pid), Some(store)) = (project_id, &self.project_store) {
            if let Ok(Some(project)) = store.get_project(pid).await {
                let project_path = PathBuf::from(&project.path);
                self.file_handlers.set_project_dir(project_path.clone());
                self.external_handlers.set_project_dir(project_path);
            }
        }

        // Delegate to regular routing for other tools
        self.route_tool_call(tool_name, arguments).await
    }

    /// Route a tool call with access mode control
    ///
    /// This is the primary entry point that enforces filesystem access restrictions
    /// based on the user's selected access mode (project, home, or system).
    pub async fn route_tool_call_with_access_mode(
        &self,
        tool_name: &str,
        arguments: Value,
        project_id: Option<&str>,
        system_access_mode: &SystemAccessMode,
        session_id: &str,
    ) -> Result<Value> {
        // Configure file and external handlers based on access mode
        match system_access_mode {
            SystemAccessMode::Project => {
                // Default behavior: restrict to project directory only
                // Set project dir if available
                if let (Some(pid), Some(store)) = (project_id, &self.project_store) {
                    if let Ok(Some(project)) = store.get_project(pid).await {
                        let project_path = PathBuf::from(&project.path);
                        self.file_handlers.set_project_dir(project_path.clone());
                        self.file_handlers.set_access_mode(SystemAccessMode::Project);
                        // Also set external handlers project dir for command execution
                        self.external_handlers.set_project_dir(project_path);
                    }
                }
            }
            SystemAccessMode::Home => {
                // Allow access to home directory and below
                if let Some(home) = dirs::home_dir() {
                    self.file_handlers.set_project_dir(home.clone());
                    self.file_handlers.set_access_mode(SystemAccessMode::Home);
                    self.external_handlers.set_project_dir(home);
                }
            }
            SystemAccessMode::System => {
                // Allow unrestricted filesystem access
                let root = PathBuf::from("/");
                self.file_handlers.set_project_dir(root.clone());
                self.file_handlers.set_access_mode(SystemAccessMode::System);
                self.external_handlers.set_project_dir(root);
            }
        }

        info!(
            "[ROUTER] Routing {} with access_mode={:?}",
            tool_name, system_access_mode
        );

        // Delegate to context-aware routing
        self.route_tool_call_with_context(tool_name, arguments, project_id, session_id).await
    }

    /// Inject project path into arguments for external tools
    async fn inject_project_path(&self, mut arguments: Value, project_id: Option<&str>) -> Value {
        // Only inject if we have a project_id and project_store
        if let (Some(pid), Some(store)) = (project_id, &self.project_store) {
            // Look up project path
            match store.get_project(pid).await {
                Ok(Some(project)) => {
                    // Only inject if working_directory not already specified
                    if arguments.get("working_directory").is_none() {
                        if let Some(obj) = arguments.as_object_mut() {
                            obj.insert(
                                "working_directory".to_string(),
                                serde_json::Value::String(project.path.clone()),
                            );
                            info!(
                                "[ROUTER] Injected project path as working_directory: {}",
                                project.path
                            );
                        }
                    }
                }
                Ok(None) => {
                    info!(
                        "[ROUTER] Project {} not found, using default working directory",
                        pid
                    );
                }
                Err(e) => {
                    info!(
                        "[ROUTER] Failed to look up project {}: {}, using default",
                        pid, e
                    );
                }
            }
        }
        arguments
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
            HandlerType::Mcp => {
                // MCP tools should be routed via route_mcp_tool, not registry
                // This is a fallback that shouldn't normally be reached
                Err(anyhow::anyhow!("MCP tools should use route_mcp_tool"))
            }
        }
    }

    /// Route an MCP tool call to the appropriate server
    ///
    /// MCP tools are named as `mcp__{server}__{tool}` where:
    /// - server: The MCP server name from config
    /// - tool: The tool name within that server
    async fn route_mcp_tool(&self, tool_name: &str, arguments: Value) -> Result<Value> {
        let mcp = self.mcp_manager.as_ref()
            .context("MCP manager not configured - cannot route MCP tool calls")?;

        // Parse tool name: mcp__{server}__{tool}
        let without_prefix = tool_name.strip_prefix("mcp__")
            .context("Invalid MCP tool name - missing prefix")?;

        let parts: Vec<&str> = without_prefix.splitn(2, "__").collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!(
                "Invalid MCP tool name format. Expected 'mcp__{{server}}__{{tool}}', got '{}'",
                tool_name
            ));
        }

        let server_name = parts[0];
        let actual_tool_name = parts[1];

        info!(
            "[ROUTER] MCP tool call: server='{}', tool='{}', args={}",
            server_name, actual_tool_name,
            serde_json::to_string(&arguments).unwrap_or_default()
        );

        // Call the tool on the MCP server
        let result = mcp.call_tool(server_name, actual_tool_name, arguments).await
            .with_context(|| format!(
                "Failed to execute MCP tool '{}' on server '{}'",
                actual_tool_name, server_name
            ))?;

        Ok(result)
    }

    /// Route native shell command from GPT-5.1
    ///
    /// Converts the native shell format to our execute_command handler:
    /// - command: Vec<String> (first element is binary, rest are args)
    /// - workdir: Optional working directory
    /// - timeout: Timeout in seconds (default 120)
    async fn route_native_shell(&self, arguments: Value) -> Result<Value> {
        use serde_json::json;

        let command = arguments
            .get("command")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing 'command' array"))?;

        if command.is_empty() {
            return Err(anyhow::anyhow!("Empty command array"));
        }

        // Join command parts into a single string for execute_command
        let command_str = command
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        let workdir = arguments
            .get("workdir")
            .and_then(|v| v.as_str())
            .map(String::from);

        let timeout = arguments
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(120);

        info!(
            "[ROUTER] Native shell: {} (timeout: {}s)",
            command_str, timeout
        );

        let exec_args = json!({
            "command": command_str,
            "working_directory": workdir,
            "timeout_ms": timeout * 1000
        });

        self.external_handlers
            .execute_tool("execute_command", exec_args)
            .await
    }
}
