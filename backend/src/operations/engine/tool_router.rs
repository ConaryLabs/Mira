// src/operations/engine/tool_router.rs
// Tool Router - Routes GPT-5 meta-tools to DeepSeek file operations
// This is the key component enabling the strategic dual-model architecture

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use tracing::{info, warn};

use crate::llm::provider::deepseek::DeepSeekProvider;
use crate::llm::provider::Message;
use crate::memory::features::code_intelligence::CodeIntelligenceService;
use crate::operations::get_file_operation_tools;
use crate::sudo::SudoPermissionService;
use super::{code_handlers::CodeHandlers, external_handlers::ExternalHandlers, file_handlers::FileHandlers, git_handlers::GitHandlers};
use std::sync::Arc;

/// Routes GPT-5 meta-tool calls to DeepSeek file operation execution
pub struct ToolRouter {
    deepseek: DeepSeekProvider,
    file_handlers: FileHandlers,
    external_handlers: ExternalHandlers,
    git_handlers: GitHandlers,
    code_handlers: CodeHandlers,
}

impl ToolRouter {
    /// Create a new tool router
    pub fn new(
        deepseek: DeepSeekProvider,
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
            deepseek,
            file_handlers: FileHandlers::new(project_dir.clone()),
            external_handlers,
            git_handlers: GitHandlers::new(project_dir),
            code_handlers: CodeHandlers::new(code_intelligence),
        }
    }

    /// Route a GPT-5 meta-tool call to appropriate handler
    ///
    /// Flow:
    /// 1. GPT-5 calls meta-tool (e.g., "read_project_file")
    /// 2. Router translates to DeepSeek tool call(s)
    /// 3. DeepSeek executes file operations via FileHandlers
    /// 4. Results returned to GPT-5
    pub async fn route_tool_call(&self, tool_name: &str, arguments: Value) -> Result<Value> {
        info!("[ROUTER] Routing tool: {}", tool_name);

        match tool_name {
            // File operations
            "read_project_file" => self.route_read_file(arguments).await,
            "write_project_file" => self.route_write_file(arguments).await,
            "write_file" => self.route_write_file_unrestricted(arguments).await, // NEW: Unrestricted file writing
            "edit_project_file" => self.route_edit_file(arguments).await,
            "search_codebase" => self.route_search(arguments).await,
            "list_project_files" => self.route_list_files(arguments).await,
            "get_file_summary" => self.route_file_summary(arguments).await,
            "get_file_structure" => self.route_file_structure(arguments).await,

            // Git operations
            "git_history" => self.route_git_history(arguments).await,
            "git_blame" => self.route_git_blame(arguments).await,
            "git_diff" => self.route_git_diff(arguments).await,
            "git_file_history" => self.route_git_file_history(arguments).await,
            "git_branches" => self.route_git_branches(arguments).await,
            "git_show_commit" => self.route_git_show_commit(arguments).await,
            "git_file_at_commit" => self.route_git_file_at_commit(arguments).await,
            "git_recent_changes" => self.route_git_recent_changes(arguments).await,
            "git_contributors" => self.route_git_contributors(arguments).await,
            "git_status" => self.route_git_status(arguments).await,

            // Code intelligence operations
            "find_function" => self.route_find_function(arguments).await,
            "find_class_or_struct" => self.route_find_class_or_struct(arguments).await,
            "search_code_semantic" => self.route_search_code_semantic(arguments).await,
            "find_imports" => self.route_find_imports(arguments).await,
            "analyze_dependencies" => self.route_analyze_dependencies(arguments).await,
            "get_complexity_hotspots" => self.route_get_complexity_hotspots(arguments).await,
            "get_quality_issues" => self.route_get_quality_issues(arguments).await,
            "get_file_symbols" => self.route_get_file_symbols(arguments).await,
            "find_tests_for_code" => self.route_find_tests_for_code(arguments).await,
            "get_codebase_stats" => self.route_get_codebase_stats(arguments).await,
            "find_callers" => self.route_find_callers(arguments).await,
            "get_element_definition" => self.route_get_element_definition(arguments).await,

            // External operations
            "web_search" => self.route_web_search(arguments).await,
            "fetch_url" => self.route_fetch_url(arguments).await,
            "execute_command" => self.route_execute_command(arguments).await,

            _ => Err(anyhow::anyhow!("Unknown meta-tool: {}", tool_name)),
        }
    }

    /// Route read_project_file to DeepSeek's read_file tool
    ///
    /// Supports reading multiple files in one call (optimized for token usage)
    async fn route_read_file(&self, args: Value) -> Result<Value> {
        let paths = args
            .get("paths")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing 'paths' array"))?;

        let purpose = args
            .get("purpose")
            .and_then(|v| v.as_str())
            .unwrap_or("Reading project files");

        info!(
            "[ROUTER] Reading {} file(s) for: {}",
            paths.len(),
            purpose
        );

        // Build DeepSeek prompt with file reading tools
        let system_prompt = format!(
            "You are a file reading assistant. Use the read_file tool to read the requested files. \
            Purpose: {}\n\n\
            Read all requested files and return a summary with the file contents.",
            purpose
        );

        let user_prompt = format!(
            "Please read the following files:\n{}\n\n\
            For each file, use the read_file tool.",
            paths
                .iter()
                .filter_map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join("\n- ")
        );

        let messages = vec![
            Message::system(system_prompt),
            Message::user(user_prompt),
        ];

        // Call DeepSeek with file operation tools
        let tools = get_file_operation_tools();
        let mut response = self
            .deepseek
            .call_with_tools(messages.clone(), tools.clone())
            .await
            .context("DeepSeek file read failed")?;

        // Execute tool calls and continue conversation
        let mut all_files = Vec::new();
        let mut conversation = messages;

        while !response.tool_calls.is_empty() {
            info!(
                "[ROUTER] DeepSeek requested {} tool call(s)",
                response.tool_calls.len()
            );

            // Execute all tool calls
            let mut tool_results = Vec::new();
            for tool_call in &response.tool_calls {
                let result = self
                    .file_handlers
                    .execute_tool(&tool_call.name, tool_call.arguments.clone())
                    .await;

                match result {
                    Ok(res) => {
                        // Extract file content for aggregation
                        if let Some(content) = res.get("content").and_then(|c| c.as_str()) {
                            if let Some(path) = res.get("path").and_then(|p| p.as_str()) {
                                all_files.push(json!({
                                    "path": path,
                                    "content": content,
                                    "lines": res.get("line_count"),
                                    "chars": res.get("char_count")
                                }));
                            }
                        }
                        tool_results.push((tool_call.id.clone(), res));
                    }
                    Err(e) => {
                        warn!("[ROUTER] Tool execution failed: {}", e);
                        tool_results.push((
                            tool_call.id.clone(),
                            json!({
                                "success": false,
                                "error": e.to_string()
                            }),
                        ));
                    }
                }
            }

            // Add assistant message with tool calls
            conversation.push(Message::assistant(
                response.content.clone().unwrap_or_default(),
            ));

            // Add tool results as user messages (DeepSeek format)
            for (tool_id, result) in tool_results {
                conversation.push(Message::user(format!(
                    "[Tool Result for {}]\n{}",
                    tool_id,
                    serde_json::to_string_pretty(&result).unwrap_or_default()
                )));
            }

            // Continue conversation with DeepSeek
            response = self
                .deepseek
                .call_with_tools(conversation.clone(), tools.clone())
                .await
                .context("DeepSeek continuation failed")?;

            // Break if DeepSeek returns text instead of more tool calls
            if response.tool_calls.is_empty() {
                break;
            }
        }

        // Return aggregated results to GPT-5
        Ok(json!({
            "success": true,
            "files_read": all_files.len(),
            "files": all_files,
            "summary": response.content.unwrap_or_else(|| format!("Read {} files successfully", all_files.len())),
            "tokens_used": {
                "input": response.tokens_input,
                "output": response.tokens_output
            }
        }))
    }

    /// Route search_codebase to DeepSeek's grep_files tool
    async fn route_search(&self, args: Value) -> Result<Value> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter"))?;

        let file_pattern = args.get("file_pattern").and_then(|v| v.as_str());
        let case_sensitive = args
            .get("case_sensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        info!("[ROUTER] Searching codebase for: '{}'", query);

        // Build DeepSeek prompt for searching
        let system_prompt = "You are a code search assistant. Use the grep_files tool to search for the requested pattern.";

        let mut user_prompt = format!("Search for: {}", query);
        if let Some(pattern) = file_pattern {
            user_prompt.push_str(&format!("\nLimit to files matching: {}", pattern));
        }

        let messages = vec![
            Message::system(system_prompt.to_string()),
            Message::user(user_prompt),
        ];

        let tools = get_file_operation_tools();
        let response = self
            .deepseek
            .call_with_tools(messages, tools)
            .await
            .context("DeepSeek search failed")?;

        // Execute grep tool if DeepSeek called it
        if let Some(tool_call) = response.tool_calls.first() {
            if tool_call.name == "grep_files" {
                let result = self
                    .file_handlers
                    .execute_tool(&tool_call.name, tool_call.arguments.clone())
                    .await?;

                return Ok(result);
            }
        }

        // Fallback: execute grep directly
        let grep_args = json!({
            "pattern": query,
            "file_pattern": file_pattern,
            "case_insensitive": !case_sensitive
        });

        self.file_handlers.execute_tool("grep_files", grep_args).await
    }

    /// Route list_project_files to DeepSeek's list_files tool
    async fn route_list_files(&self, args: Value) -> Result<Value> {
        let directory = args
            .get("directory")
            .and_then(|v| v.as_str())
            .unwrap_or(".");
        let pattern = args.get("pattern").and_then(|v| v.as_str());
        let recursive = args
            .get("recursive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        info!("[ROUTER] Listing files in: {}", directory);

        // Build DeepSeek prompt for listing
        let system_prompt = "You are a file listing assistant. Use the list_files tool to list the requested directory.";

        let mut user_prompt = format!("List files in directory: {}", directory);
        if let Some(p) = pattern {
            user_prompt.push_str(&format!("\nFilter by pattern: {}", p));
        }
        if recursive {
            user_prompt.push_str("\nInclude subdirectories recursively");
        }

        let messages = vec![
            Message::system(system_prompt.to_string()),
            Message::user(user_prompt),
        ];

        let tools = get_file_operation_tools();
        let response = self
            .deepseek
            .call_with_tools(messages, tools)
            .await
            .context("DeepSeek list failed")?;

        // Execute list_files tool if DeepSeek called it
        if let Some(tool_call) = response.tool_calls.first() {
            if tool_call.name == "list_files" {
                let result = self
                    .file_handlers
                    .execute_tool(&tool_call.name, tool_call.arguments.clone())
                    .await?;

                return Ok(result);
            }
        }

        // Fallback: execute list_files directly
        let list_args = json!({
            "directory": directory,
            "pattern": pattern,
            "recursive": recursive
        });

        self.file_handlers
            .execute_tool("list_files", list_args)
            .await
    }

    /// Route get_file_summary to DeepSeek's summarize_file tool
    ///
    /// Token-optimized: Returns only preview + stats instead of full content
    async fn route_file_summary(&self, args: Value) -> Result<Value> {
        let paths = args
            .get("paths")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing 'paths' array"))?;

        let preview_lines = args
            .get("preview_lines")
            .and_then(|v| v.as_str())
            .unwrap_or("10");

        info!("[ROUTER] Summarizing {} file(s)", paths.len());

        let mut summaries = Vec::new();

        for path_val in paths {
            if let Some(path) = path_val.as_str() {
                let summary_args = json!({
                    "path": path,
                    "preview_lines": preview_lines
                });

                match self.file_handlers.execute_tool("summarize_file", summary_args).await {
                    Ok(result) => summaries.push(result),
                    Err(e) => {
                        warn!("[ROUTER] Failed to summarize {}: {}", path, e);
                        summaries.push(json!({
                            "success": false,
                            "path": path,
                            "error": e.to_string()
                        }));
                    }
                }
            }
        }

        Ok(json!({
            "success": true,
            "file_count": summaries.len(),
            "summaries": summaries
        }))
    }

    /// Route get_file_structure to DeepSeek's extract_symbols tool
    ///
    /// Token-optimized: Returns only symbol list instead of full code
    async fn route_file_structure(&self, args: Value) -> Result<Value> {
        let paths = args
            .get("paths")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing 'paths' array"))?;

        info!("[ROUTER] Extracting structure from {} file(s)", paths.len());

        let mut structures = Vec::new();

        for path_val in paths {
            if let Some(path) = path_val.as_str() {
                let extract_args = json!({
                    "path": path
                });

                match self.file_handlers.execute_tool("extract_symbols", extract_args).await {
                    Ok(result) => structures.push(result),
                    Err(e) => {
                        warn!("[ROUTER] Failed to extract symbols from {}: {}", path, e);
                        structures.push(json!({
                            "success": false,
                            "path": path,
                            "error": e.to_string()
                        }));
                    }
                }
            }
        }

        Ok(json!({
            "success": true,
            "file_count": structures.len(),
            "structures": structures
        }))
    }

    /// Route write_project_file directly to file handler
    ///
    /// Writes content to a file, creating parent directories if needed
    async fn route_write_file(&self, args: Value) -> Result<Value> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' argument"))?;

        info!("[ROUTER] Writing file: {}", path);

        let write_args = json!({
            "path": path,
            "content": content
        });

        self.file_handlers.execute_tool("write_file", write_args).await
    }

    /// Route write_file (unrestricted) directly to file handler
    /// This allows writing to ANY file on the system, not just project files
    async fn route_write_file_unrestricted(&self, args: Value) -> Result<Value> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' argument"))?;

        info!("[ROUTER] Writing unrestricted file: {}", path);

        let write_args = json!({
            "path": path,
            "content": content,
            "unrestricted": true // Flag to bypass project restrictions
        });

        self.file_handlers.execute_tool("write_file", write_args).await
    }

    /// Route edit_project_file directly to file handler
    ///
    /// Performs search/replace edits on an existing file
    async fn route_edit_file(&self, args: Value) -> Result<Value> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

        let search = args
            .get("search")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'search' argument"))?;

        let replace = args
            .get("replace")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'replace' argument"))?;

        info!("[ROUTER] Editing file: {} (search/replace)", path);

        let edit_args = json!({
            "path": path,
            "search": search,
            "replace": replace
        });

        self.file_handlers.execute_tool("edit_file", edit_args).await
    }

    // ========================================================================
    // External Operations Routing (Web, Commands)
    // ========================================================================

    /// Route web_search to DeepSeek + external handler
    ///
    /// Unlike file operations, web search is executed directly (not via DeepSeek)
    /// for better reliability and speed
    async fn route_web_search(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing web_search");

        // Execute web search directly via external handler
        self.external_handlers
            .execute_tool("web_search_internal", args)
            .await
    }

    /// Route fetch_url to external handler
    async fn route_fetch_url(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing fetch_url");

        // Execute URL fetch directly
        self.external_handlers
            .execute_tool("fetch_url_internal", args)
            .await
    }

    /// Route execute_command to external handler
    async fn route_execute_command(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing execute_command");

        // Execute command directly
        self.external_handlers
            .execute_tool("execute_command_internal", args)
            .await
    }

    // ========================================================================
    // Git Operations Routing
    // ========================================================================

    /// Route git_history to git handler
    async fn route_git_history(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing git_history");
        self.git_handlers
            .execute_tool("git_history_internal", args)
            .await
    }

    /// Route git_blame to git handler
    async fn route_git_blame(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing git_blame");
        self.git_handlers
            .execute_tool("git_blame_internal", args)
            .await
    }

    /// Route git_diff to git handler
    async fn route_git_diff(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing git_diff");
        self.git_handlers
            .execute_tool("git_diff_internal", args)
            .await
    }

    /// Route git_file_history to git handler
    async fn route_git_file_history(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing git_file_history");
        self.git_handlers
            .execute_tool("git_file_history_internal", args)
            .await
    }

    /// Route git_branches to git handler
    async fn route_git_branches(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing git_branches");
        self.git_handlers
            .execute_tool("git_branches_internal", args)
            .await
    }

    /// Route git_show_commit to git handler
    async fn route_git_show_commit(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing git_show_commit");
        self.git_handlers
            .execute_tool("git_show_commit_internal", args)
            .await
    }

    /// Route git_file_at_commit to git handler
    async fn route_git_file_at_commit(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing git_file_at_commit");
        self.git_handlers
            .execute_tool("git_file_at_commit_internal", args)
            .await
    }

    /// Route git_recent_changes to git handler
    async fn route_git_recent_changes(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing git_recent_changes");
        self.git_handlers
            .execute_tool("git_recent_changes_internal", args)
            .await
    }

    /// Route git_contributors to git handler
    async fn route_git_contributors(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing git_contributors");
        self.git_handlers
            .execute_tool("git_contributors_internal", args)
            .await
    }

    /// Route git_status to git handler
    async fn route_git_status(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing git_status");
        self.git_handlers
            .execute_tool("git_status_internal", args)
            .await
    }

    // ========================================================================
    // Code Intelligence Operations Routing
    // ========================================================================

    /// Route find_function to code handler
    async fn route_find_function(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing find_function");
        self.code_handlers
            .execute_tool("find_function_internal", args)
            .await
    }

    /// Route find_class_or_struct to code handler
    async fn route_find_class_or_struct(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing find_class_or_struct");
        self.code_handlers
            .execute_tool("find_class_or_struct_internal", args)
            .await
    }

    /// Route search_code_semantic to code handler
    async fn route_search_code_semantic(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing search_code_semantic");
        self.code_handlers
            .execute_tool("search_code_semantic_internal", args)
            .await
    }

    /// Route find_imports to code handler
    async fn route_find_imports(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing find_imports");
        self.code_handlers
            .execute_tool("find_imports_internal", args)
            .await
    }

    /// Route analyze_dependencies to code handler
    async fn route_analyze_dependencies(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing analyze_dependencies");
        self.code_handlers
            .execute_tool("analyze_dependencies_internal", args)
            .await
    }

    /// Route get_complexity_hotspots to code handler
    async fn route_get_complexity_hotspots(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing get_complexity_hotspots");
        self.code_handlers
            .execute_tool("get_complexity_hotspots_internal", args)
            .await
    }

    /// Route get_quality_issues to code handler
    async fn route_get_quality_issues(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing get_quality_issues");
        self.code_handlers
            .execute_tool("get_quality_issues_internal", args)
            .await
    }

    /// Route get_file_symbols to code handler
    async fn route_get_file_symbols(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing get_file_symbols");
        self.code_handlers
            .execute_tool("get_file_symbols_internal", args)
            .await
    }

    /// Route find_tests_for_code to code handler
    async fn route_find_tests_for_code(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing find_tests_for_code");
        self.code_handlers
            .execute_tool("find_tests_for_code_internal", args)
            .await
    }

    /// Route get_codebase_stats to code handler
    async fn route_get_codebase_stats(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing get_codebase_stats");
        self.code_handlers
            .execute_tool("get_codebase_stats_internal", args)
            .await
    }

    /// Route find_callers to code handler
    async fn route_find_callers(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing find_callers");
        self.code_handlers
            .execute_tool("find_callers_internal", args)
            .await
    }

    /// Route get_element_definition to code handler
    async fn route_get_element_definition(&self, args: Value) -> Result<Value> {
        info!("[ROUTER] Routing get_element_definition");
        self.code_handlers
            .execute_tool("get_element_definition_internal", args)
            .await
    }
}
