// backend/src/cli/display/streaming.rs
// Streaming display for real-time token output

use crate::cli::args::OutputFormat;
use crate::cli::ws_client::{BackendEvent, OperationEvent};
use serde::Serialize;
use std::io::{self, Write};

use super::terminal::TerminalDisplay;

/// Streaming display handler
pub struct StreamingDisplay {
    terminal: TerminalDisplay,
    output_format: OutputFormat,
    buffer: String,
    in_response: bool,
    current_operation_id: Option<String>,
}

impl StreamingDisplay {
    /// Create a new streaming display
    pub fn new(terminal: TerminalDisplay, output_format: OutputFormat) -> Self {
        Self {
            terminal,
            output_format,
            buffer: String::new(),
            in_response: false,
            current_operation_id: None,
        }
    }

    /// Handle a backend event
    pub fn handle_event(&mut self, event: &BackendEvent) -> io::Result<()> {
        match self.output_format {
            OutputFormat::Text => self.handle_text_event(event),
            OutputFormat::Json => self.handle_json_event(event),
            OutputFormat::StreamJson => self.handle_stream_json_event(event),
        }
    }

    /// Handle event in text format
    fn handle_text_event(&mut self, event: &BackendEvent) -> io::Result<()> {
        match event {
            BackendEvent::StreamToken(token) => {
                if !self.in_response {
                    self.terminal.stop_spinner();
                    self.terminal.print_assistant_start()?;
                    self.in_response = true;
                }
                self.terminal.print_token(token)?;
                self.buffer.push_str(token);
            }
            BackendEvent::ChatComplete {
                content,
                thinking,
                ..
            } => {
                if !self.in_response {
                    self.terminal.stop_spinner();
                    self.terminal.print_assistant_start()?;
                    self.terminal.print_token(content)?;
                }
                self.terminal.print_assistant_end()?;

                if let Some(thinking_content) = thinking {
                    self.terminal.print_thinking(thinking_content)?;
                }

                self.in_response = false;
                self.buffer.clear();
            }
            BackendEvent::OperationEvent(op_event) => {
                self.handle_operation_event_text(op_event)?;
            }
            BackendEvent::Status { message, detail } => {
                self.terminal.print_status(message, detail.as_deref())?;
            }
            BackendEvent::Error { message, .. } => {
                if self.in_response {
                    self.terminal.print_assistant_end()?;
                    self.in_response = false;
                }
                self.terminal.stop_spinner();
                self.terminal.print_error(message)?;
            }
            BackendEvent::Connected => {
                // Silent in text mode
            }
            BackendEvent::Disconnected => {
                self.terminal.print_warning("Disconnected from backend")?;
            }
            BackendEvent::SessionData { .. } => {
                // Session data is handled separately by session management
            }
        }
        Ok(())
    }

    /// Handle operation event in text format
    fn handle_operation_event_text(&mut self, event: &OperationEvent) -> io::Result<()> {
        match event {
            OperationEvent::Started { operation_id } => {
                self.current_operation_id = Some(operation_id.clone());
                self.terminal.start_spinner("Processing...");
            }
            OperationEvent::Streaming { content, .. } => {
                if !self.in_response {
                    self.terminal.stop_spinner();
                    self.terminal.print_assistant_start()?;
                    self.in_response = true;
                }
                self.terminal.print_token(content)?;
                self.buffer.push_str(content);
            }
            OperationEvent::PlanGenerated { plan_text, .. } => {
                self.terminal.update_spinner(&format!("Planning: {}", truncate(plan_text, 50)));
            }
            OperationEvent::ToolExecuted {
                tool_name,
                summary,
                success,
                duration_ms,
                ..
            } => {
                self.terminal.print_tool_execution(tool_name, summary, *success, *duration_ms)?;
            }
            OperationEvent::AgentSpawned {
                agent_name, task, ..
            } => {
                self.terminal.print_agent_spawn(agent_name, task)?;
            }
            OperationEvent::AgentProgress {
                agent_name,
                iteration,
                max_iterations,
                current_activity,
                ..
            } => {
                self.terminal.print_agent_progress(
                    agent_name,
                    *iteration,
                    *max_iterations,
                    current_activity,
                )?;
            }
            OperationEvent::AgentStreaming { content, .. } => {
                if !self.in_response {
                    self.terminal.stop_spinner();
                    self.terminal.print_assistant_start()?;
                    self.in_response = true;
                }
                self.terminal.print_token(content)?;
            }
            OperationEvent::AgentCompleted { agent_name, .. } => {
                self.terminal.print_agent_complete(agent_name)?;
            }
            OperationEvent::Completed { .. } => {
                self.terminal.stop_spinner();
                if self.in_response {
                    self.terminal.print_assistant_end()?;
                    self.in_response = false;
                }
                self.current_operation_id = None;
            }
            OperationEvent::Failed { error, .. } => {
                self.terminal.stop_spinner();
                if self.in_response {
                    self.terminal.print_assistant_end()?;
                    self.in_response = false;
                }
                self.terminal.print_error(error)?;
                self.current_operation_id = None;
            }
            OperationEvent::SudoApprovalRequired {
                command, reason, ..
            } => {
                self.terminal.stop_spinner();
                println!();
                self.terminal.print_warning(&format!(
                    "Sudo approval required for: {}",
                    command
                ))?;
                if let Some(r) = reason {
                    println!("  Reason: {}", r);
                }
                // TODO: Implement approval prompt
            }
            OperationEvent::ArtifactPreview { path, preview, .. } => {
                // Check if preview looks like a diff
                if preview.contains("@@") && (preview.contains("+") || preview.contains("-")) {
                    self.terminal.print_diff(preview)?;
                } else if let Some(p) = path {
                    self.terminal.print_file_content(p, preview, 1)?;
                }
            }
            OperationEvent::TaskCreated { task_id, title, .. } => {
                // Display task creation
                println!("  [ ] {}: {}", truncate(task_id, 8), title);
            }
            OperationEvent::TaskStarted { task_id, .. } => {
                println!("  [>] {} started", truncate(task_id, 8));
            }
            OperationEvent::TaskCompleted { task_id, .. } => {
                println!("  [x] {} completed", truncate(task_id, 8));
            }
            OperationEvent::Thinking {
                message,
                tokens_in,
                tokens_out,
                active_tool,
                ..
            } => {
                // Update spinner with thinking status and token count
                let tokens_total = tokens_in + tokens_out;
                let status_msg = if let Some(tool) = active_tool {
                    format!("{} ({})", message, tool)
                } else if tokens_total > 0 {
                    format!("{} [{} tokens]", message, tokens_total)
                } else {
                    message.clone()
                };
                self.terminal.update_spinner(&status_msg);
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle event in JSON format (complete response only)
    fn handle_json_event(&mut self, event: &BackendEvent) -> io::Result<()> {
        match event {
            BackendEvent::StreamToken(token) => {
                self.buffer.push_str(token);
            }
            BackendEvent::OperationEvent(op_event) => match op_event {
                OperationEvent::Streaming { content, .. } => {
                    self.buffer.push_str(content);
                }
                OperationEvent::Completed { result, .. } => {
                    let content = if self.buffer.is_empty() {
                        result.clone().unwrap_or_default()
                    } else {
                        self.buffer.clone()
                    };
                    let output = JsonOutput {
                        content,
                        artifacts: vec![],
                        thinking: None,
                    };
                    println!("{}", serde_json::to_string_pretty(&output).unwrap());
                    self.buffer.clear();
                }
                OperationEvent::Failed { error, .. } => {
                    // Output any accumulated content before the error
                    if !self.buffer.is_empty() {
                        let output = JsonOutput {
                            content: self.buffer.clone(),
                            artifacts: vec![],
                            thinking: None,
                        };
                        println!("{}", serde_json::to_string_pretty(&output).unwrap());
                        self.buffer.clear();
                    }
                    let output = JsonError {
                        error: error.clone(),
                        code: "operation_failed".to_string(),
                    };
                    eprintln!("{}", serde_json::to_string(&output).unwrap());
                }
                _ => {}
            },
            BackendEvent::ChatComplete {
                content,
                artifacts,
                thinking,
                ..
            } => {
                let output = JsonOutput {
                    content: content.clone(),
                    artifacts: artifacts.clone(),
                    thinking: thinking.clone(),
                };
                println!("{}", serde_json::to_string_pretty(&output).unwrap());
            }
            BackendEvent::Error { message, code } => {
                let output = JsonError {
                    error: message.clone(),
                    code: code.clone(),
                };
                eprintln!("{}", serde_json::to_string(&output).unwrap());
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle event in stream-json format (NDJSON)
    fn handle_stream_json_event(&mut self, event: &BackendEvent) -> io::Result<()> {
        let stream_event = match event {
            BackendEvent::Connected => Some(StreamJsonEvent::Start),
            BackendEvent::StreamToken(delta) => Some(StreamJsonEvent::Token {
                delta: delta.clone(),
            }),
            BackendEvent::OperationEvent(op_event) => match op_event {
                OperationEvent::Streaming { content, .. } => Some(StreamJsonEvent::Token {
                    delta: content.clone(),
                }),
                OperationEvent::ToolExecuted {
                    tool_name,
                    summary,
                    success,
                    duration_ms,
                    ..
                } => Some(StreamJsonEvent::Tool {
                    name: tool_name.clone(),
                    summary: summary.clone(),
                    success: *success,
                    duration_ms: *duration_ms,
                }),
                OperationEvent::AgentSpawned {
                    agent_id,
                    agent_name,
                    task,
                    ..
                } => Some(StreamJsonEvent::AgentSpawned {
                    agent_id: agent_id.clone(),
                    name: agent_name.clone(),
                    task: task.clone(),
                }),
                OperationEvent::Completed { result, .. } => Some(StreamJsonEvent::Complete {
                    response: result.clone(),
                }),
                OperationEvent::Failed { error, .. } => Some(StreamJsonEvent::Error {
                    message: error.clone(),
                }),
                _ => None,
            },
            BackendEvent::ChatComplete { content, .. } => Some(StreamJsonEvent::Complete {
                response: Some(content.clone()),
            }),
            BackendEvent::Error { message, .. } => Some(StreamJsonEvent::Error {
                message: message.clone(),
            }),
            _ => None,
        };

        if let Some(e) = stream_event {
            println!("{}", serde_json::to_string(&e).unwrap());
            io::stdout().flush()?;
        }

        Ok(())
    }

    /// Get the accumulated buffer
    pub fn get_buffer(&self) -> &str {
        &self.buffer
    }

    /// Clear the buffer
    pub fn clear_buffer(&mut self) {
        self.buffer.clear();
    }

    /// Check if currently in a response
    pub fn is_in_response(&self) -> bool {
        self.in_response
    }

    /// Get terminal display reference
    pub fn terminal(&self) -> &TerminalDisplay {
        &self.terminal
    }

    /// Get mutable terminal display reference
    pub fn terminal_mut(&mut self) -> &mut TerminalDisplay {
        &mut self.terminal
    }
}

/// JSON output format
#[derive(Debug, Serialize)]
struct JsonOutput {
    content: String,
    artifacts: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<String>,
}

/// JSON error format
#[derive(Debug, Serialize)]
struct JsonError {
    error: String,
    code: String,
}

/// Stream JSON event types (NDJSON)
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StreamJsonEvent {
    Start,
    Token { delta: String },
    Tool {
        name: String,
        summary: String,
        success: bool,
        duration_ms: u64,
    },
    AgentSpawned {
        agent_id: String,
        name: String,
        task: String,
    },
    Complete {
        response: Option<String>,
    },
    Error {
        message: String,
    },
}

/// Truncate a string to a maximum length
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("this is a longer string", 10), "this is...");
    }

    #[test]
    fn test_stream_json_event_serialization() {
        let event = StreamJsonEvent::Token {
            delta: "Hello".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("token"));
        assert!(json.contains("Hello"));
    }
}
