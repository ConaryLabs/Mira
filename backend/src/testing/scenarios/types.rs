// src/testing/scenarios/types.rs
// Test scenario type definitions

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::testing::harness::assertions::Assertion;

/// A complete test scenario
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestScenario {
    /// Unique name for the scenario
    pub name: String,

    /// Human-readable description
    #[serde(default)]
    pub description: String,

    /// Tags for filtering/grouping tests
    #[serde(default)]
    pub tags: Vec<String>,

    /// Setup configuration
    #[serde(default)]
    pub setup: SetupConfig,

    /// Test steps to execute
    pub steps: Vec<TestStep>,

    /// Cleanup configuration
    #[serde(default)]
    pub cleanup: CleanupConfig,

    /// Global timeout for entire scenario (seconds)
    #[serde(default = "default_scenario_timeout")]
    pub timeout_seconds: u64,
}

fn default_scenario_timeout() -> u64 {
    120 // 2 minutes default
}

/// Setup configuration for a test scenario
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SetupConfig {
    /// Project directory (will create temp dir if not specified)
    pub project_path: Option<String>,

    /// Files to create before test
    #[serde(default)]
    pub create_files: Vec<FileSpec>,

    /// Directories to create before test
    #[serde(default)]
    pub create_dirs: Vec<String>,

    /// Environment variables to set
    #[serde(default)]
    pub env_vars: HashMap<String, String>,

    /// Whether to use mock LLM
    #[serde(default)]
    pub mock_mode: bool,

    /// Session ID to use (creates new if not specified)
    pub session_id: Option<String>,

    /// Project ID to associate with
    pub project_id: Option<String>,
}

/// File specification for setup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSpec {
    /// Relative path within project
    pub path: String,

    /// File content
    pub content: String,

    /// Whether to make executable
    #[serde(default)]
    pub executable: bool,
}

/// Cleanup configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CleanupConfig {
    /// Remove the project directory after test
    #[serde(default)]
    pub remove_project: bool,

    /// Specific files to remove
    #[serde(default)]
    pub remove_files: Vec<String>,

    /// Specific directories to remove
    #[serde(default)]
    pub remove_dirs: Vec<String>,
}

/// A single test step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestStep {
    /// Step name for reporting
    pub name: String,

    /// Prompt to send to Mira
    pub prompt: String,

    /// Expected events (in order)
    #[serde(default)]
    pub expect_events: Vec<ExpectedEvent>,

    /// Assertions to check after step completes
    #[serde(default)]
    pub assertions: Vec<Assertion>,

    /// Timeout for this specific step (seconds)
    #[serde(default = "default_step_timeout")]
    pub timeout_seconds: u64,

    /// Whether this step is expected to fail
    #[serde(default)]
    pub expect_failure: bool,

    /// Skip this step if true
    #[serde(default)]
    pub skip: bool,

    /// Reason for skipping
    #[serde(default)]
    pub skip_reason: Option<String>,

    /// Mock response for this step (used in mock mode)
    #[serde(default)]
    pub mock_response: Option<MockResponse>,

    /// Force the LLM to call a specific tool by name (for deterministic testing)
    /// Uses OpenAI's tool_choice parameter to guarantee the specified tool is called
    #[serde(default)]
    pub force_tool: Option<String>,
}

/// Mock response for a test step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockResponse {
    /// Text response from the LLM
    #[serde(default)]
    pub text: String,

    /// Tool calls to simulate
    #[serde(default)]
    pub tool_calls: Vec<MockToolCall>,
}

/// A simulated tool call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockToolCall {
    /// Tool name
    pub name: String,

    /// Tool arguments
    #[serde(default)]
    pub args: serde_json::Value,

    /// Tool result
    #[serde(default)]
    pub result: String,

    /// Whether the tool succeeded
    #[serde(default = "default_true")]
    pub success: bool,
}

fn default_true() -> bool {
    true
}

fn default_step_timeout() -> u64 {
    60 // 1 minute default per step
}

/// Expected event specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedEvent {
    /// Event type (e.g., "operation.started", "operation.tool_executed")
    #[serde(rename = "type")]
    pub event_type: String,

    /// Additional matchers for this event
    #[serde(flatten)]
    pub matchers: HashMap<String, serde_json::Value>,
}

impl ExpectedEvent {
    /// Check if an operation event matches this expectation
    pub fn matches(&self, event_type: &str, event_data: &serde_json::Value) -> bool {
        // Check type matches
        if event_type != self.event_type {
            return false;
        }

        // Check all matchers
        for (key, expected_value) in &self.matchers {
            if let Some(actual_value) = event_data.get(key) {
                if !Self::values_match(expected_value, actual_value) {
                    return false;
                }
            } else {
                // Key not found in event data
                return false;
            }
        }

        true
    }

    fn values_match(expected: &serde_json::Value, actual: &serde_json::Value) -> bool {
        match (expected, actual) {
            (serde_json::Value::String(e), serde_json::Value::String(a)) => {
                // Support wildcard matching
                if e == "*" {
                    return true;
                }
                // Support prefix matching
                if e.ends_with('*') {
                    return a.starts_with(&e[..e.len() - 1]);
                }
                e == a
            }
            (serde_json::Value::Bool(e), serde_json::Value::Bool(a)) => e == a,
            (serde_json::Value::Number(e), serde_json::Value::Number(a)) => e == a,
            _ => expected == actual,
        }
    }
}

/// Result of running a test scenario
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    pub scenario_name: String,
    pub passed: bool,
    pub step_results: Vec<StepResult>,
    pub duration_ms: u64,
    pub error: Option<String>,
}

impl ScenarioResult {
    pub fn new(scenario_name: &str) -> Self {
        Self {
            scenario_name: scenario_name.to_string(),
            passed: true,
            step_results: Vec::new(),
            duration_ms: 0,
            error: None,
        }
    }

    pub fn add_step_result(&mut self, result: StepResult) {
        if !result.passed {
            self.passed = false;
        }
        self.step_results.push(result);
    }

    pub fn fail_with_error(&mut self, error: impl Into<String>) {
        self.passed = false;
        self.error = Some(error.into());
    }
}

/// Result of running a single step
#[derive(Debug, Clone)]
pub struct StepResult {
    pub step_name: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub assertion_results: Vec<crate::testing::harness::assertions::AssertionResult>,
    pub event_count: usize,
    pub tool_executions: Vec<String>,
    pub error: Option<String>,
    pub skipped: bool,
}

impl StepResult {
    pub fn skipped(step_name: &str, reason: Option<String>) -> Self {
        Self {
            step_name: step_name.to_string(),
            passed: true,
            duration_ms: 0,
            assertion_results: Vec::new(),
            event_count: 0,
            tool_executions: Vec::new(),
            error: reason,
            skipped: true,
        }
    }

    pub fn new(step_name: &str) -> Self {
        Self {
            step_name: step_name.to_string(),
            passed: true,
            duration_ms: 0,
            assertion_results: Vec::new(),
            event_count: 0,
            tool_executions: Vec::new(),
            error: None,
            skipped: false,
        }
    }

    pub fn fail_with_error(&mut self, error: impl Into<String>) {
        self.passed = false;
        self.error = Some(error.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expected_event_matching() {
        let event = ExpectedEvent {
            event_type: "operation.tool_executed".to_string(),
            matchers: {
                let mut m = HashMap::new();
                m.insert("tool_name".to_string(), serde_json::Value::String("write_project_file".to_string()));
                m.insert("success".to_string(), serde_json::Value::Bool(true));
                m
            },
        };

        let data = serde_json::json!({
            "tool_name": "write_project_file",
            "success": true,
            "summary": "Wrote file test.txt"
        });

        assert!(event.matches("operation.tool_executed", &data));

        // Wrong tool name
        let data2 = serde_json::json!({
            "tool_name": "read_project_file",
            "success": true
        });
        assert!(!event.matches("operation.tool_executed", &data2));
    }

    #[test]
    fn test_wildcard_matching() {
        let event = ExpectedEvent {
            event_type: "operation.tool_executed".to_string(),
            matchers: {
                let mut m = HashMap::new();
                m.insert("tool_name".to_string(), serde_json::Value::String("write_*".to_string()));
                m
            },
        };

        let data = serde_json::json!({
            "tool_name": "write_project_file",
        });

        assert!(event.matches("operation.tool_executed", &data));
    }
}
