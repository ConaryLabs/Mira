// src/testing/harness/assertions.rs
// Assertion framework for Mira testing

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

use crate::cli::ws_client::OperationEvent;
use super::client::CapturedEvents;

/// Result of running an assertion
#[derive(Debug, Clone)]
pub struct AssertionResult {
    pub passed: bool,
    pub assertion_type: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

impl AssertionResult {
    pub fn pass(assertion_type: &str, message: impl Into<String>) -> Self {
        Self {
            passed: true,
            assertion_type: assertion_type.to_string(),
            message: message.into(),
            details: None,
        }
    }

    pub fn fail(assertion_type: &str, message: impl Into<String>) -> Self {
        Self {
            passed: false,
            assertion_type: assertion_type.to_string(),
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

/// Context available during assertion checking
pub struct TestContext {
    pub events: CapturedEvents,
    pub project_dir: PathBuf,
    pub response_content: Option<String>,
}

impl TestContext {
    pub fn new(events: CapturedEvents, project_dir: PathBuf) -> Self {
        let response_content = events.final_response();
        Self {
            events,
            project_dir,
            response_content,
        }
    }
}

/// Types of assertions that can be made
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Assertion {
    // Event assertions
    EventReceived {
        event_type: String,
    },
    EventCount {
        event_type: String,
        count: usize,
    },
    EventSequence {
        events: Vec<String>,
    },
    NoErrors,

    // Tool execution assertions
    ToolExecuted {
        tool_name: String,
        #[serde(default = "default_true")]
        success: bool,
    },
    ToolSequence {
        tools: Vec<String>,
    },
    ToolCount {
        tool_name: String,
        count: usize,
    },
    NoToolErrors,

    // Response content assertions
    ResponseContains {
        text: String,
        #[serde(default)]
        case_sensitive: bool,
    },
    ResponseMatches {
        pattern: String,
    },
    ResponseNotContains {
        text: String,
    },

    // File system assertions (checked after operation)
    FileExists {
        path: String,
    },
    FileNotExists {
        path: String,
    },
    FileContains {
        path: String,
        text: String,
    },
    FileContentEquals {
        path: String,
        content: String,
    },

    // Timing assertions
    CompletedWithin {
        seconds: f64,
    },

    // Completion assertions
    CompletedSuccessfully,
    OperationFailed {
        #[serde(default)]
        error_contains: Option<String>,
    },
}

fn default_true() -> bool {
    true
}

impl Assertion {
    /// Check this assertion against the test context
    pub fn check(&self, context: &TestContext) -> AssertionResult {
        match self {
            // Event assertions
            Assertion::EventReceived { event_type } => {
                let found = !context.events.of_type(event_type).is_empty();
                if found {
                    AssertionResult::pass("event_received", format!("Event '{}' was received", event_type))
                } else {
                    AssertionResult::fail("event_received", format!("Event '{}' was not received", event_type))
                }
            }

            Assertion::EventCount { event_type, count } => {
                let actual = context.events.of_type(event_type).len();
                if actual == *count {
                    AssertionResult::pass("event_count", format!("Event '{}' received {} times", event_type, count))
                } else {
                    AssertionResult::fail("event_count", format!("Event '{}' received {} times, expected {}", event_type, actual, count))
                }
            }

            Assertion::EventSequence { events } => {
                let all_events: Vec<_> = context.events.all().iter().collect();
                let mut event_idx = 0;

                for expected in events {
                    let mut found = false;
                    while event_idx < all_events.len() {
                        if context.events.of_type(expected).iter().any(|e| e.sequence == all_events[event_idx].sequence) {
                            found = true;
                            event_idx += 1;
                            break;
                        }
                        event_idx += 1;
                    }
                    if !found {
                        return AssertionResult::fail("event_sequence", format!("Event '{}' not found in expected sequence", expected));
                    }
                }
                AssertionResult::pass("event_sequence", "All events received in expected order")
            }

            Assertion::NoErrors => {
                let errors = context.events.of_type("error");
                let failures = context.events.of_type("operation.failed");
                if errors.is_empty() && failures.is_empty() {
                    AssertionResult::pass("no_errors", "No errors occurred")
                } else {
                    let error_msg = context.events.error_message().unwrap_or_default();
                    AssertionResult::fail("no_errors", format!("Errors occurred: {}", error_msg))
                }
            }

            // Tool execution assertions
            Assertion::ToolExecuted { tool_name, success } => {
                let executions = context.events.tool_executions_by_name(tool_name);
                if executions.is_empty() {
                    return AssertionResult::fail("tool_executed", format!("Tool '{}' was not executed", tool_name));
                }

                let matching = executions.iter().any(|op| {
                    if let OperationEvent::ToolExecuted { success: s, .. } = op {
                        *s == *success
                    } else {
                        false
                    }
                });

                if matching {
                    AssertionResult::pass("tool_executed", format!("Tool '{}' executed with success={}", tool_name, success))
                } else {
                    AssertionResult::fail("tool_executed", format!("Tool '{}' executed but success was not {}", tool_name, success))
                }
            }

            Assertion::ToolSequence { tools } => {
                let executed: Vec<_> = context.events.tool_executions()
                    .iter()
                    .filter_map(|op| {
                        if let OperationEvent::ToolExecuted { tool_name, .. } = op {
                            Some(tool_name.clone())
                        } else {
                            None
                        }
                    })
                    .collect();

                let mut exec_idx = 0;
                for expected in tools {
                    let mut found = false;
                    while exec_idx < executed.len() {
                        if &executed[exec_idx] == expected {
                            found = true;
                            exec_idx += 1;
                            break;
                        }
                        exec_idx += 1;
                    }
                    if !found {
                        return AssertionResult::fail("tool_sequence", format!("Tool '{}' not found in expected sequence. Actual: {:?}", expected, executed));
                    }
                }
                AssertionResult::pass("tool_sequence", format!("Tools executed in order: {:?}", tools))
            }

            Assertion::ToolCount { tool_name, count } => {
                let actual = context.events.tool_executions_by_name(tool_name).len();
                if actual == *count {
                    AssertionResult::pass("tool_count", format!("Tool '{}' executed {} times", tool_name, count))
                } else {
                    AssertionResult::fail("tool_count", format!("Tool '{}' executed {} times, expected {}", tool_name, actual, count))
                }
            }

            Assertion::NoToolErrors => {
                let tool_execs = context.events.tool_executions();
                let failed_tools: Vec<_> = tool_execs
                    .iter()
                    .filter(|op| {
                        if let OperationEvent::ToolExecuted { success, .. } = op {
                            !success
                        } else {
                            false
                        }
                    })
                    .collect();

                if failed_tools.is_empty() {
                    AssertionResult::pass("no_tool_errors", "All tools executed successfully")
                } else {
                    let names: Vec<_> = failed_tools.iter().filter_map(|op| {
                        if let OperationEvent::ToolExecuted { tool_name, .. } = op {
                            Some(tool_name.clone())
                        } else {
                            None
                        }
                    }).collect();
                    AssertionResult::fail("no_tool_errors", format!("Tools failed: {:?}", names))
                }
            }

            // Response content assertions
            Assertion::ResponseContains { text, case_sensitive } => {
                if let Some(ref content) = context.response_content {
                    let found = if *case_sensitive {
                        content.contains(text)
                    } else {
                        content.to_lowercase().contains(&text.to_lowercase())
                    };
                    if found {
                        AssertionResult::pass("response_contains", format!("Response contains '{}'", text))
                    } else {
                        AssertionResult::fail("response_contains", format!("Response does not contain '{}'. Content: {}...", text, &content[..content.len().min(200)]))
                    }
                } else {
                    AssertionResult::fail("response_contains", "No response content available")
                }
            }

            Assertion::ResponseMatches { pattern } => {
                if let Some(ref content) = context.response_content {
                    match regex::Regex::new(pattern) {
                        Ok(re) => {
                            if re.is_match(content) {
                                AssertionResult::pass("response_matches", format!("Response matches pattern '{}'", pattern))
                            } else {
                                AssertionResult::fail("response_matches", format!("Response does not match pattern '{}'", pattern))
                            }
                        }
                        Err(e) => AssertionResult::fail("response_matches", format!("Invalid regex pattern: {}", e)),
                    }
                } else {
                    AssertionResult::fail("response_matches", "No response content available")
                }
            }

            Assertion::ResponseNotContains { text } => {
                if let Some(ref content) = context.response_content {
                    if !content.contains(text) {
                        AssertionResult::pass("response_not_contains", format!("Response does not contain '{}'", text))
                    } else {
                        AssertionResult::fail("response_not_contains", format!("Response contains '{}' but should not", text))
                    }
                } else {
                    AssertionResult::pass("response_not_contains", "No response content (trivially true)")
                }
            }

            // File system assertions
            Assertion::FileExists { path } => {
                let full_path = context.project_dir.join(path);
                if full_path.exists() {
                    AssertionResult::pass("file_exists", format!("File '{}' exists", path))
                } else {
                    AssertionResult::fail("file_exists", format!("File '{}' does not exist", path))
                }
            }

            Assertion::FileNotExists { path } => {
                let full_path = context.project_dir.join(path);
                if !full_path.exists() {
                    AssertionResult::pass("file_not_exists", format!("File '{}' does not exist", path))
                } else {
                    AssertionResult::fail("file_not_exists", format!("File '{}' exists but should not", path))
                }
            }

            Assertion::FileContains { path, text } => {
                let full_path = context.project_dir.join(path);
                match std::fs::read_to_string(&full_path) {
                    Ok(content) => {
                        if content.contains(text) {
                            AssertionResult::pass("file_contains", format!("File '{}' contains expected text", path))
                        } else {
                            AssertionResult::fail("file_contains", format!("File '{}' does not contain '{}'. Content: {}...", path, text, &content[..content.len().min(200)]))
                        }
                    }
                    Err(e) => AssertionResult::fail("file_contains", format!("Could not read file '{}': {}", path, e)),
                }
            }

            Assertion::FileContentEquals { path, content } => {
                let full_path = context.project_dir.join(path);
                match std::fs::read_to_string(&full_path) {
                    Ok(actual) => {
                        if actual.trim() == content.trim() {
                            AssertionResult::pass("file_content_equals", format!("File '{}' has expected content", path))
                        } else {
                            AssertionResult::fail("file_content_equals", format!("File '{}' content differs. Expected: {}..., Actual: {}...", path, &content[..content.len().min(100)], &actual[..actual.len().min(100)]))
                        }
                    }
                    Err(e) => AssertionResult::fail("file_content_equals", format!("Could not read file '{}': {}", path, e)),
                }
            }

            // Timing assertions
            Assertion::CompletedWithin { seconds } => {
                let duration = context.events.duration();
                let limit = Duration::from_secs_f64(*seconds);
                if duration <= limit {
                    AssertionResult::pass("completed_within", format!("Completed in {:?} (limit: {:?})", duration, limit))
                } else {
                    AssertionResult::fail("completed_within", format!("Took {:?}, expected within {:?}", duration, limit))
                }
            }

            // Completion assertions
            Assertion::CompletedSuccessfully => {
                if context.events.completed_successfully() {
                    AssertionResult::pass("completed_successfully", "Operation completed successfully")
                } else {
                    let error = context.events.error_message().unwrap_or_else(|| "Unknown error".to_string());
                    AssertionResult::fail("completed_successfully", format!("Operation did not complete successfully: {}", error))
                }
            }

            Assertion::OperationFailed { error_contains } => {
                if let Some(error_msg) = context.events.error_message() {
                    if let Some(expected) = error_contains {
                        if error_msg.contains(expected) {
                            AssertionResult::pass("operation_failed", format!("Operation failed with expected error containing '{}'", expected))
                        } else {
                            AssertionResult::fail("operation_failed", format!("Operation failed but error '{}' does not contain '{}'", error_msg, expected))
                        }
                    } else {
                        AssertionResult::pass("operation_failed", format!("Operation failed as expected: {}", error_msg))
                    }
                } else {
                    AssertionResult::fail("operation_failed", "Operation did not fail as expected")
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::harness::client::CapturedEvent;
    use std::time::Instant;

    fn make_context(events: Vec<BackendEvent>) -> TestContext {
        let captured: Vec<CapturedEvent> = events
            .into_iter()
            .enumerate()
            .map(|(i, e)| CapturedEvent {
                event: e,
                timestamp: Instant::now(),
                sequence: i,
            })
            .collect();

        TestContext::new(
            CapturedEvents::new(captured),
            PathBuf::from("/tmp/test"),
        )
    }

    #[test]
    fn test_event_received() {
        let context = make_context(vec![
            BackendEvent::Connected,
            BackendEvent::OperationEvent(OperationEvent::Started {
                operation_id: "op1".to_string(),
            }),
        ]);

        let assertion = Assertion::EventReceived {
            event_type: "operation.started".to_string(),
        };
        assert!(assertion.check(&context).passed);

        let assertion = Assertion::EventReceived {
            event_type: "operation.failed".to_string(),
        };
        assert!(!assertion.check(&context).passed);
    }

    #[test]
    fn test_no_errors() {
        let context = make_context(vec![
            BackendEvent::Connected,
            BackendEvent::OperationEvent(OperationEvent::Completed {
                operation_id: "op1".to_string(),
                result: Some("Done".to_string()),
            }),
        ]);

        let assertion = Assertion::NoErrors;
        assert!(assertion.check(&context).passed);
    }

    #[test]
    fn test_completed_successfully() {
        let context = make_context(vec![
            BackendEvent::ChatComplete {
                content: "Hello!".to_string(),
                artifacts: vec![],
                thinking: None,
            },
        ]);

        let assertion = Assertion::CompletedSuccessfully;
        assert!(assertion.check(&context).passed);
    }
}
