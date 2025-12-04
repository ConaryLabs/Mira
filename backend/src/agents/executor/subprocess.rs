// backend/src/agents/executor/subprocess.rs
// Executor for custom agents (subprocess via MCP-like protocol)

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::{debug, info, warn};

use crate::agents::protocol::{AgentRequest, AgentResponse};
use crate::agents::types::{AgentConfig, AgentDefinition, AgentResult, AgentToolCall};
use crate::operations::engine::tool_router::ToolRouter;

use super::{AgentEvent, AgentExecutor};

/// Executor for custom agents running as subprocesses
pub struct SubprocessAgentExecutor {
    tool_router: Arc<ToolRouter>,
}

impl SubprocessAgentExecutor {
    pub fn new(tool_router: Arc<ToolRouter>) -> Self {
        Self { tool_router }
    }

    /// Send event if channel is available
    async fn emit_event(event_tx: &Option<mpsc::Sender<AgentEvent>>, event: AgentEvent) {
        if let Some(tx) = event_tx {
            let _ = tx.send(event).await;
        }
    }

    /// Execute a tool call requested by the subprocess agent
    async fn execute_tool_for_subprocess(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
        config: &AgentConfig,
    ) -> Result<serde_json::Value> {
        // Use context-aware routing if available
        if config.project_id.is_some() || config.session_id.is_some() {
            self.tool_router
                .route_tool_call_with_context(
                    tool_name,
                    arguments,
                    config.project_id.as_deref(),
                    config.session_id.as_deref().unwrap_or("subprocess-agent"),
                )
                .await
        } else {
            self.tool_router.route_tool_call(tool_name, arguments).await
        }
    }
}

#[async_trait]
impl AgentExecutor for SubprocessAgentExecutor {
    async fn execute(
        &self,
        definition: &AgentDefinition,
        config: AgentConfig,
        event_tx: Option<mpsc::Sender<AgentEvent>>,
    ) -> Result<AgentResult> {
        let start_time = Instant::now();
        let agent_execution_id = uuid::Uuid::new_v4().to_string();

        let command = definition
            .command
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Subprocess agent requires command"))?;

        info!(
            "[AGENT:{}] Spawning subprocess: {} (id: {})",
            definition.id, command, agent_execution_id
        );

        // Emit started event
        Self::emit_event(
            &event_tx,
            AgentEvent::Started {
                agent_execution_id: agent_execution_id.clone(),
                agent_name: definition.name.clone(),
                task: config.task.clone(),
            },
        )
        .await;

        // Spawn the subprocess
        let mut cmd = Command::new(command);
        cmd.args(&definition.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // Set environment variables
        for (key, value) in &definition.env {
            cmd.env(key, value);
        }

        // Add agent-specific environment
        cmd.env("MIRA_AGENT_ID", &definition.id);
        cmd.env("MIRA_AGENT_NAME", &definition.name);
        cmd.env("MIRA_EXECUTION_ID", &agent_execution_id);

        let mut child = cmd.spawn().context("Failed to spawn agent process")?;

        let stdin = child.stdin.take().context("Failed to get stdin")?;
        let stdout = child.stdout.take().context("Failed to get stdout")?;
        let stderr = child.stderr.take();

        let mut stdin = tokio::io::BufWriter::new(stdin);
        let mut stdout = BufReader::new(stdout);

        // Spawn stderr logger
        if let Some(stderr) = stderr {
            let agent_id = definition.id.clone();
            tokio::spawn(async move {
                let mut stderr = BufReader::new(stderr);
                let mut line = String::new();
                while stderr.read_line(&mut line).await.unwrap_or(0) > 0 {
                    debug!("[AGENT:{}:stderr] {}", agent_id, line.trim());
                    line.clear();
                }
            });
        }

        // Send initial request to agent
        let request = AgentRequest {
            task: config.task.clone(),
            context: config.context.clone(),
            context_files: config.context_files.clone(),
            thought_signature: config.thought_signature.clone(),
            allowed_tools: definition.tool_access.allowed_tools().iter().map(|s| s.to_string()).collect(),
            max_iterations: definition.max_iterations,
            timeout_ms: definition.timeout_ms,
        };

        let request_json = serde_json::to_string(&request)?;
        stdin.write_all(request_json.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;

        // Communication loop
        let mut tool_calls_made: Vec<AgentToolCall> = Vec::new();
        let mut final_response = None;
        let timeout_duration = Duration::from_millis(definition.timeout_ms);
        let mut iteration = 0usize;

        loop {
            let mut line = String::new();
            let read_result = timeout(timeout_duration, stdout.read_line(&mut line)).await;

            match read_result {
                Ok(Ok(0)) => {
                    // EOF - process ended
                    debug!("[AGENT:{}] Process ended (EOF)", definition.id);
                    break;
                }
                Ok(Ok(_)) => {
                    let response: AgentResponse = serde_json::from_str(line.trim())
                        .context("Failed to parse agent response")?;

                    match response {
                        AgentResponse::ToolRequest { id, name, arguments } => {
                            debug!("[AGENT:{}] Tool request: {}", definition.id, name);

                            // Check if tool is allowed
                            if !definition.tool_access.is_allowed(&name) {
                                let error_response = serde_json::json!({
                                    "type": "tool_result",
                                    "id": id,
                                    "success": false,
                                    "result": null,
                                    "error": format!("Tool '{}' not allowed for this agent", name)
                                });
                                stdin
                                    .write_all(serde_json::to_string(&error_response)?.as_bytes())
                                    .await?;
                                stdin.write_all(b"\n").await?;
                                stdin.flush().await?;
                                continue;
                            }

                            let tool_start = Instant::now();
                            let result = self
                                .execute_tool_for_subprocess(&name, arguments.clone(), &config)
                                .await;

                            let (result_value, success) = match result {
                                Ok(v) => (Some(v), true),
                                Err(_) => (None, false),
                            };

                            let duration_ms = tool_start.elapsed().as_millis() as u64;

                            tool_calls_made.push(AgentToolCall {
                                tool_name: name.clone(),
                                arguments: arguments.clone(),
                                result: result_value
                                    .clone()
                                    .unwrap_or(serde_json::json!({"error": "failed"})),
                                success,
                                duration_ms,
                            });

                            // Emit tool executed event
                            Self::emit_event(
                                &event_tx,
                                AgentEvent::ToolExecuted {
                                    agent_execution_id: agent_execution_id.clone(),
                                    tool_name: name.clone(),
                                    success,
                                    duration_ms,
                                },
                            )
                            .await;

                            // Send tool result back to agent
                            let tool_result = serde_json::json!({
                                "type": "tool_result",
                                "id": id,
                                "success": success,
                                "result": result_value,
                                "error": if !success { Some("Tool execution failed") } else { None }
                            });
                            stdin
                                .write_all(serde_json::to_string(&tool_result)?.as_bytes())
                                .await?;
                            stdin.write_all(b"\n").await?;
                            stdin.flush().await?;
                        }
                        AgentResponse::Progress {
                            iteration: iter,
                            max_iterations,
                            activity,
                        } => {
                            iteration = iter;
                            Self::emit_event(
                                &event_tx,
                                AgentEvent::Progress {
                                    agent_execution_id: agent_execution_id.clone(),
                                    agent_name: definition.name.clone(),
                                    iteration: iter,
                                    max_iterations,
                                    current_activity: activity,
                                },
                            )
                            .await;
                        }
                        AgentResponse::Streaming { content } => {
                            Self::emit_event(
                                &event_tx,
                                AgentEvent::Streaming {
                                    agent_execution_id: agent_execution_id.clone(),
                                    content,
                                },
                            )
                            .await;
                        }
                        AgentResponse::Complete {
                            response,
                            thought_signature,
                            artifacts,
                        } => {
                            final_response = Some((response, thought_signature, artifacts));
                            break;
                        }
                        AgentResponse::Error { message } => {
                            warn!("[AGENT:{}] Error: {}", definition.id, message);

                            Self::emit_event(
                                &event_tx,
                                AgentEvent::Failed {
                                    agent_execution_id: agent_execution_id.clone(),
                                    agent_name: definition.name.clone(),
                                    error: message.clone(),
                                },
                            )
                            .await;

                            return Ok(AgentResult {
                                agent_id: definition.id.clone(),
                                success: false,
                                response: String::new(),
                                thought_signature: None,
                                artifacts: vec![],
                                tool_calls: tool_calls_made,
                                tokens_input: 0,
                                tokens_output: 0,
                                execution_time_ms: start_time.elapsed().as_millis() as u64,
                                error: Some(message),
                            });
                        }
                    }
                }
                Ok(Err(e)) => {
                    return Err(anyhow::anyhow!("Error reading from agent: {}", e));
                }
                Err(_) => {
                    Self::emit_event(
                        &event_tx,
                        AgentEvent::Failed {
                            agent_execution_id: agent_execution_id.clone(),
                            agent_name: definition.name.clone(),
                            error: format!("Agent timed out after {}ms", definition.timeout_ms),
                        },
                    )
                    .await;

                    return Err(anyhow::anyhow!(
                        "Agent timed out after {}ms",
                        definition.timeout_ms
                    ));
                }
            }
        }

        let execution_time_ms = start_time.elapsed().as_millis() as u64;

        let (response, thought_signature, artifacts) = final_response.unwrap_or_else(|| {
            (
                "Agent completed without explicit response".to_string(),
                None,
                vec![],
            )
        });

        info!(
            "[AGENT:{}] Completed in {}ms, {} tool calls",
            definition.id,
            execution_time_ms,
            tool_calls_made.len()
        );

        Self::emit_event(
            &event_tx,
            AgentEvent::Completed {
                agent_execution_id: agent_execution_id.clone(),
                agent_name: definition.name.clone(),
                summary: if response.len() > 200 {
                    format!("{}...", &response[..200])
                } else {
                    response.clone()
                },
                iterations_used: iteration,
            },
        )
        .await;

        Ok(AgentResult {
            agent_id: definition.id.clone(),
            success: true,
            response,
            thought_signature,
            artifacts,
            tool_calls: tool_calls_made,
            tokens_input: 0, // Subprocess agents track their own
            tokens_output: 0,
            execution_time_ms,
            error: None,
        })
    }
}
