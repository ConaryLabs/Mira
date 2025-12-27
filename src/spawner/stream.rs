//! Stream-JSON parser for Claude Code output
//!
//! Parses the streaming JSON output from `claude --output-format stream-json`

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::ChildStdout;
use tokio::sync::mpsc;
use tracing::{debug, trace, warn};

use super::types::StreamEvent;

/// Parse stream-json output from Claude Code
pub struct StreamParser {
    /// Channel to send parsed events
    tx: mpsc::Sender<StreamEvent>,
}

impl StreamParser {
    pub fn new(tx: mpsc::Sender<StreamEvent>) -> Self {
        Self { tx }
    }

    /// Spawn a task to read and parse stdout
    pub fn spawn_reader(self, stdout: ChildStdout) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            self.read_stream(stdout).await;
        })
    }

    async fn read_stream(self, stdout: ChildStdout) {
        let mut reader = BufReader::new(stdout).lines();

        while let Ok(Some(line)) = reader.next_line().await {
            if line.is_empty() {
                continue;
            }

            match self.parse_line(&line) {
                Ok(event) => {
                    trace!(?event, "Parsed stream event");
                    if self.tx.send(event).await.is_err() {
                        debug!("Stream receiver dropped, stopping parser");
                        break;
                    }
                }
                Err(e) => {
                    warn!(line = %line, error = %e, "Failed to parse stream-json line");
                }
            }
        }

        debug!("Stream parser finished");
    }

    fn parse_line(&self, line: &str) -> Result<StreamEvent> {
        serde_json::from_str(line).context("Failed to parse stream-json")
    }
}

/// Detect if output contains an AskUserQuestion tool call
pub fn detect_question(event: &StreamEvent) -> Option<DetectedQuestion> {
    if let StreamEvent::ToolUse { name, id, input } = event {
        if name == "AskUserQuestion" {
            // Parse the AskUserQuestion input
            let questions = input.get("questions")?.as_array()?;
            let first_q = questions.first()?;

            return Some(DetectedQuestion {
                tool_id: id.clone(),
                question: first_q.get("question")?.as_str()?.to_string(),
                options: first_q
                    .get("options")
                    .and_then(|o| o.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|opt| {
                                Some(super::types::QuestionOption {
                                    label: opt.get("label")?.as_str()?.to_string(),
                                    description: opt
                                        .get("description")
                                        .and_then(|d| d.as_str())
                                        .map(String::from),
                                })
                            })
                            .collect()
                    }),
            });
        }
    }
    None
}

/// A detected question from Claude Code output
#[derive(Debug, Clone)]
pub struct DetectedQuestion {
    pub tool_id: String,
    pub question: String,
    pub options: Option<Vec<super::types::QuestionOption>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_assistant_message() {
        let json = r#"{"type":"assistant","message":{"content":"Hello world","stop_reason":"end_turn"}}"#;
        let event: StreamEvent = serde_json::from_str(json).unwrap();

        if let StreamEvent::Assistant { message } = event {
            assert_eq!(message.content, Some("Hello world".to_string()));
        } else {
            panic!("Expected Assistant event");
        }
    }

    #[test]
    fn test_parse_tool_use() {
        let json = r#"{"type":"tool_use","name":"Edit","id":"tool_123","input":{"file_path":"/test.rs"}}"#;
        let event: StreamEvent = serde_json::from_str(json).unwrap();

        if let StreamEvent::ToolUse { name, id, .. } = event {
            assert_eq!(name, "Edit");
            assert_eq!(id, "tool_123");
        } else {
            panic!("Expected ToolUse event");
        }
    }

    #[test]
    fn test_detect_question() {
        let json = r#"{"type":"tool_use","name":"AskUserQuestion","id":"q1","input":{"questions":[{"question":"Which approach?","options":[{"label":"A"},{"label":"B"}]}]}}"#;
        let event: StreamEvent = serde_json::from_str(json).unwrap();

        let q = detect_question(&event).expect("Should detect question");
        assert_eq!(q.question, "Which approach?");
        assert_eq!(q.options.as_ref().unwrap().len(), 2);
    }
}
