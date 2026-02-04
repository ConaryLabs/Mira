// crates/mira-server/src/mcp/handler.rs
// MCP ServerHandler implementation — protocol lifecycle methods

use super::MiraServer;
use super::tasks;

use rmcp::{
    ErrorData, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResult, CancelTaskParams, CreateTaskResult,
        GetTaskInfoParams, GetTaskInfoResult, GetTaskResultParams, ListTasksResult,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo,
        Task, TaskResult as ModelTaskResult, TaskStatus, TasksCapability,
    },
    service::{RequestContext, RoleServer},
    task_manager::{self, OperationDescriptor, OperationMessage, ToolCallTaskResult},
};

/// Extract the "action" field from tool arguments and look up the task TTL.
fn extract_action_ttl(request: &CallToolRequestParams) -> (Option<String>, Option<u64>) {
    let action = request
        .arguments
        .as_ref()
        .and_then(|a| a.get("action"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let ttl = tasks::task_ttl(&request.name, action.as_deref());
    (action, ttl)
}

impl ServerHandler for MiraServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: Default::default(),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_tasks_with(TasksCapability::server_default())
                .build(),
            server_info: rmcp::model::Implementation {
                name: "mira".into(),
                title: Some("Mira - Memory and Intelligence Layer for Claude Code".into()),
                version: env!("CARGO_PKG_VERSION").into(),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Mira provides semantic memory, code intelligence, and persistent context for Claude Code.".into(),
            ),
        }
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult {
            tools: self.tool_router.list_all(),
            next_cursor: None,
            meta: None,
        }))
    }

    #[allow(clippy::manual_async_fn)]
    fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, ErrorData>> + Send + '_ {
        async move {
            // Auto-enqueue task-eligible tools that arrive via synchronous call_tool
            // (i.e. not already going through the native task protocol).
            let maybe_enqueue = if request.task.is_none() {
                extract_action_ttl(&request).1
            } else {
                None
            };

            if let Some(ttl) = maybe_enqueue {
                let tool_name = request.name.to_string();
                return self
                    .auto_enqueue_task(request, context, &tool_name, ttl)
                    .await;
            }

            self.run_tool_call(request, context).await
        }
    }

    #[allow(clippy::manual_async_fn)]
    fn enqueue_task(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CreateTaskResult, ErrorData>> + Send + '_ {
        async move {
            let tool_name = request.name.to_string();

            // Extract action + check eligibility by tool name + action
            let (action, maybe_ttl) = extract_action_ttl(&request);
            let ttl = match maybe_ttl {
                Some(ttl) => ttl,
                None => {
                    return Err(ErrorData::internal_error(
                        format!(
                            "Tool '{}' (action: {:?}) does not support async tasks",
                            tool_name, action
                        ),
                        None,
                    ));
                }
            };

            // Generate task ID
            let task_id = uuid::Uuid::new_v4().to_string();
            let now = task_manager::current_timestamp();

            // Strip the `task` field to prevent re-enqueue loops
            let mut clean_request = request;
            clean_request.task = None;

            // Build the async future that calls run_tool_call
            let server = self.clone();
            let ctx = context.clone();
            let tid = task_id.clone();
            let future: task_manager::OperationFuture = Box::pin(async move {
                let result = server.run_tool_call(clean_request, ctx).await;
                let transport = ToolCallTaskResult::new(tid, result);
                Ok(Box::new(transport) as Box<dyn task_manager::OperationResultTransport>)
            });

            // Build descriptor and submit
            let descriptor =
                OperationDescriptor::new(task_id.clone(), tool_name.clone()).with_ttl(ttl);
            let message = OperationMessage::new(descriptor, future);

            let mut proc = self.processor.lock().await;
            proc.submit_operation(message).map_err(|e| {
                ErrorData::internal_error(format!("Failed to enqueue task: {}", e), None)
            })?;

            tracing::info!(
                task_id = %task_id,
                tool = %tool_name,
                ttl_secs = ttl,
                "Enqueued async task"
            );

            Ok(CreateTaskResult {
                task: Task {
                    task_id,
                    status: TaskStatus::Working,
                    status_message: Some(format!("Running {} asynchronously", tool_name)),
                    created_at: now,
                    last_updated_at: None,
                    ttl: Some(ttl * 1000), // Protocol uses milliseconds
                    poll_interval: Some(2000),
                },
            })
        }
    }

    #[allow(clippy::manual_async_fn)]
    fn list_tasks(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListTasksResult, ErrorData>> + Send + '_ {
        async move {
            let mut proc = self.processor.lock().await;
            proc.check_timeouts();

            let running_ids = proc.list_running();
            let mut all_tasks: Vec<Task> = running_ids
                .iter()
                .filter_map(|id| {
                    proc.task_descriptor(id).map(|desc| Task {
                        task_id: id.clone(),
                        status: TaskStatus::Working,
                        status_message: Some(format!("Running {}", desc.name)),
                        created_at: String::new(), // Not tracked in descriptor
                        last_updated_at: None,
                        ttl: desc.ttl.map(|t| t * 1000),
                        poll_interval: Some(2000),
                    })
                })
                .collect();

            // Include completed results that haven't been collected yet
            for result in proc.peek_completed() {
                let status = match &result.result {
                    Ok(_) => TaskStatus::Completed,
                    Err(e) if e.to_string().contains("cancelled") => TaskStatus::Cancelled,
                    Err(_) => TaskStatus::Failed,
                };
                all_tasks.push(Task {
                    task_id: result.descriptor.operation_id.clone(),
                    status,
                    status_message: Some(result.descriptor.name.clone()),
                    created_at: String::new(),
                    last_updated_at: None,
                    ttl: None,
                    poll_interval: None,
                });
            }

            Ok(ListTasksResult {
                tasks: all_tasks,
                next_cursor: None,
                total: None,
            })
        }
    }

    #[allow(clippy::manual_async_fn)]
    fn get_task_info(
        &self,
        request: GetTaskInfoParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<GetTaskInfoResult, ErrorData>> + Send + '_ {
        async move {
            let proc = self.processor.lock().await;

            // Check running tasks first
            if let Some(desc) = proc.task_descriptor(&request.task_id) {
                return Ok(GetTaskInfoResult {
                    task: Some(Task {
                        task_id: request.task_id,
                        status: TaskStatus::Working,
                        status_message: Some(format!("Running {}", desc.name)),
                        created_at: String::new(),
                        last_updated_at: None,
                        ttl: desc.ttl.map(|t| t * 1000),
                        poll_interval: Some(2000),
                    }),
                });
            }

            // Check completed results
            for result in proc.peek_completed() {
                if result.descriptor.operation_id == request.task_id {
                    let status = match &result.result {
                        Ok(_) => TaskStatus::Completed,
                        Err(e) if e.to_string().contains("cancelled") => TaskStatus::Cancelled,
                        Err(_) => TaskStatus::Failed,
                    };
                    return Ok(GetTaskInfoResult {
                        task: Some(Task {
                            task_id: request.task_id,
                            status,
                            status_message: Some(result.descriptor.name.clone()),
                            created_at: String::new(),
                            last_updated_at: None,
                            ttl: None,
                            poll_interval: None,
                        }),
                    });
                }
            }

            // Not found
            Ok(GetTaskInfoResult { task: None })
        }
    }

    #[allow(clippy::manual_async_fn)]
    fn get_task_result(
        &self,
        request: GetTaskResultParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ModelTaskResult, ErrorData>> + Send + '_ {
        async move {
            let mut proc = self.processor.lock().await;
            // Collect any newly completed results
            proc.collect_completed_results();

            match proc.take_completed_result(&request.task_id) {
                Some(task_result) => match task_result.result {
                    Ok(boxed) => {
                        // Downcast to ToolCallTaskResult
                        if let Some(tcr) = boxed.as_any().downcast_ref::<ToolCallTaskResult>() {
                            let value = match &tcr.result {
                                Ok(call_result) => {
                                    serde_json::to_value(call_result).unwrap_or_default()
                                }
                                Err(e) => serde_json::json!({ "error": e.message }),
                            };
                            let summary = match &tcr.result {
                                Ok(r) => r.content.first().and_then(|c| c.as_text()).map(|t| {
                                    if t.text.len() > 200 {
                                        format!("{}...", &t.text[..200])
                                    } else {
                                        t.text.to_string()
                                    }
                                }),
                                Err(e) => Some(e.message.to_string()),
                            };
                            Ok(ModelTaskResult {
                                content_type: "application/json".to_string(),
                                value,
                                summary,
                            })
                        } else {
                            Err(ErrorData::internal_error(
                                "Task result has unexpected type".to_string(),
                                None,
                            ))
                        }
                    }
                    Err(e) => Err(ErrorData::internal_error(
                        format!("Task failed: {}", e),
                        None,
                    )),
                },
                None => {
                    // Check if still running
                    if proc.task_descriptor(&request.task_id).is_some() {
                        Err(ErrorData::internal_error(
                            "Task is still running".to_string(),
                            None,
                        ))
                    } else {
                        Err(ErrorData::internal_error(
                            format!("Task '{}' not found", request.task_id),
                            None,
                        ))
                    }
                }
            }
        }
    }

    #[allow(clippy::manual_async_fn)]
    fn cancel_task(
        &self,
        request: CancelTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<(), ErrorData>> + Send + '_ {
        async move {
            let mut proc = self.processor.lock().await;
            if proc.cancel_task(&request.task_id) {
                tracing::info!(task_id = %request.task_id, "Task cancelled");
                Ok(())
            } else {
                Err(ErrorData::internal_error(
                    format!("Task '{}' not found or already completed", request.task_id),
                    None,
                ))
            }
        }
    }
}
