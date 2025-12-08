// src/testing/harness/runner.rs
// Scenario execution engine

use anyhow::{Context, Result};
use futures::stream::{self, StreamExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

use crate::cli::ws_client::OperationEvent;
use crate::testing::harness::assertions::TestContext;
use crate::testing::harness::client::{TestClient, CapturedEvents};
use crate::testing::scenarios::types::{
    CleanupConfig, ScenarioResult, SetupConfig, StepResult, TestScenario, TestStep,
};

/// Configuration for the scenario runner
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    /// Backend WebSocket URL
    pub backend_url: String,

    /// Default timeout for operations
    pub default_timeout: Duration,

    /// Whether to use mock LLM (when implemented)
    pub mock_mode: bool,

    /// Stop on first failure
    pub fail_fast: bool,

    /// Verbose output
    pub verbose: bool,

    /// Run scenarios in parallel
    pub parallel: bool,

    /// Maximum concurrent scenarios (0 = unlimited)
    pub max_parallel: usize,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            backend_url: "ws://localhost:3001/ws".to_string(),
            default_timeout: Duration::from_secs(60),
            mock_mode: false,
            fail_fast: false,
            verbose: false,
            parallel: false,
            max_parallel: 4,
        }
    }
}

/// Executes test scenarios
pub struct ScenarioRunner {
    config: RunnerConfig,
}

impl ScenarioRunner {
    pub fn new(config: RunnerConfig) -> Self {
        Self { config }
    }

    /// Run a single scenario
    pub async fn run_scenario(&self, scenario: &TestScenario) -> ScenarioResult {
        info!("Running scenario: {}", scenario.name);
        let start = Instant::now();
        let mut result = ScenarioResult::new(&scenario.name);

        // Setup phase
        let project_dir = match self.setup(&scenario.setup).await {
            Ok(dir) => dir,
            Err(e) => {
                error!("Setup failed: {}", e);
                result.fail_with_error(format!("Setup failed: {}", e));
                return result;
            }
        };

        // In mock mode, we don't connect to backend
        let mut client: Option<TestClient> = None;

        if !self.config.mock_mode {
            // Connect to backend
            let timeout = Duration::from_secs(scenario.timeout_seconds);
            match TestClient::connect_with_timeout(&self.config.backend_url, timeout).await {
                Ok(c) => {
                    client = Some(c);
                }
                Err(e) => {
                    error!("Failed to connect to backend: {}", e);
                    result.fail_with_error(format!("Connection failed: {}", e));
                    self.cleanup(&scenario.cleanup, &project_dir).await;
                    return result;
                }
            };

            // Create an isolated session for this test run
            let project_path = project_dir.to_string_lossy().to_string();
            let session_name = format!("test-{}", uuid::Uuid::new_v4());
            if let Some(ref mut c) = client {
                match c.create_session(Some(&session_name), Some(&project_path)).await {
                    Ok(session_id) => {
                        info!("Created isolated session: {} ({})", session_name, session_id);
                    }
                    Err(e) => {
                        error!("Failed to create session: {}", e);
                        result.fail_with_error(format!("Session creation failed: {}", e));
                        self.cleanup(&scenario.cleanup, &project_dir).await;
                        return result;
                    }
                }

                // Register the project directory so tools can access it
                match c.register_project(&project_path, Some("Test Project")).await {
                    Ok(project_id) => {
                        info!("Registered project with ID: {}", project_id);
                    }
                    Err(e) => {
                        error!("Failed to register project: {}", e);
                        result.fail_with_error(format!("Project registration failed: {}", e));
                        self.cleanup(&scenario.cleanup, &project_dir).await;
                        return result;
                    }
                }
            }
        } else {
            info!("[Mock] Running in mock mode - skipping backend connection");
        }

        // Run each step
        for step in &scenario.steps {
            let step_result = self.run_step(&mut client, step, &project_dir).await;

            let step_passed = step_result.passed;
            result.add_step_result(step_result);

            if !step_passed && self.config.fail_fast {
                warn!("Step '{}' failed, stopping due to fail_fast", step.name);
                break;
            }
        }

        // Cleanup phase
        if let Some(mut c) = client {
            let _ = c.close().await;
        }
        self.cleanup(&scenario.cleanup, &project_dir).await;

        result.duration_ms = start.elapsed().as_millis() as u64;
        info!(
            "Scenario '{}' completed: {} ({}ms)",
            scenario.name,
            if result.passed { "PASSED" } else { "FAILED" },
            result.duration_ms
        );

        result
    }

    /// Run a single step
    async fn run_step(
        &self,
        client: &mut Option<TestClient>,
        step: &TestStep,
        project_dir: &Path,
    ) -> StepResult {
        // Handle skipped steps
        if step.skip {
            info!("Skipping step: {} ({})", step.name, step.skip_reason.as_deref().unwrap_or("no reason given"));
            return StepResult::skipped(&step.name, step.skip_reason.clone());
        }

        info!("Running step: {}", step.name);
        let start = Instant::now();
        let mut result = StepResult::new(&step.name);

        // Check for mock mode
        let events = if self.config.mock_mode {
            // Use mock response if available
            if let Some(ref mock) = step.mock_response {
                info!("[Mock] Using mock response for step '{}'", step.name);
                self.generate_mock_events(mock, &step.name)
            } else {
                // No mock response defined - generate minimal success events
                info!("[Mock] No mock_response defined, using minimal mock for step '{}'", step.name);
                self.generate_minimal_mock_events(&step.name)
            }
        } else if let Some(c) = client {
            // Real mode - send to backend
            let timeout = Duration::from_secs(step.timeout_seconds);
            c.set_timeout(timeout);

            // Inject project directory context into the prompt
            let prompt_with_context = format!(
                "[Project directory: {}]\n\n{}",
                project_dir.display(),
                step.prompt
            );

            // Send prompt and capture events
            match c.send_and_capture(&prompt_with_context).await {
                Ok(e) => e,
                Err(e) => {
                    error!("Step '{}' failed to execute: {}", step.name, e);
                    result.fail_with_error(format!("Execution failed: {}", e));
                    return result;
                }
            }
        } else {
            error!("No client available and not in mock mode");
            result.fail_with_error("No client available");
            return result;
        };

        result.event_count = events.len();
        result.duration_ms = start.elapsed().as_millis() as u64;

        // Collect tool executions for reporting
        result.tool_executions = events
            .tool_executions()
            .iter()
            .filter_map(|op| {
                if let OperationEvent::ToolExecuted { tool_name, success, .. } = op {
                    Some(format!("{}({})", tool_name, if *success { "ok" } else { "fail" }))
                } else {
                    None
                }
            })
            .collect();

        // Check expected events (if specified)
        if !step.expect_events.is_empty() {
            let event_check = self.check_expected_events(&events, step);
            if !event_check.passed {
                result.assertion_results.push(event_check);
                result.passed = false;
            }
        }

        // Build test context for assertions
        let context = TestContext::new(events, project_dir.to_path_buf());

        // Run assertions
        for assertion in &step.assertions {
            let assertion_result = assertion.check(&context);
            if self.config.verbose || !assertion_result.passed {
                info!(
                    "  Assertion {}: {} - {}",
                    assertion_result.assertion_type,
                    if assertion_result.passed { "PASS" } else { "FAIL" },
                    assertion_result.message
                );
            }
            if !assertion_result.passed {
                result.passed = false;
            }
            result.assertion_results.push(assertion_result);
        }

        // Handle expect_failure flag
        if step.expect_failure {
            if result.passed {
                result.passed = false;
                result.error = Some("Step was expected to fail but passed".to_string());
            } else {
                result.passed = true;
                result.error = None;
            }
        }

        info!(
            "Step '{}' completed: {} ({} events, {}ms)",
            step.name,
            if result.passed { "PASS" } else { "FAIL" },
            result.event_count,
            result.duration_ms
        );

        result
    }

    /// Generate mock events from a MockResponse
    fn generate_mock_events(&self, mock: &crate::testing::scenarios::types::MockResponse, _step_name: &str) -> CapturedEvents {
        use crate::cli::ws_client::{BackendEvent, OperationEvent};
        use crate::testing::harness::client::CapturedEvent;

        let mut events = Vec::new();
        let now = Instant::now();
        let mut seq = 0;
        let op_id = format!("mock-{}", uuid::Uuid::new_v4());

        // Generate operation started event
        events.push(CapturedEvent {
            event: BackendEvent::OperationEvent(OperationEvent::Started {
                operation_id: op_id.clone(),
            }),
            timestamp: now,
            sequence: seq,
        });
        seq += 1;

        // Generate tool execution events
        for tool in &mock.tool_calls {
            events.push(CapturedEvent {
                event: BackendEvent::OperationEvent(OperationEvent::ToolExecuted {
                    operation_id: op_id.clone(),
                    tool_name: tool.name.clone(),
                    tool_type: "mock".to_string(),
                    summary: tool.result.clone(),
                    success: tool.success,
                    duration_ms: 1,
                }),
                timestamp: now,
                sequence: seq,
            });
            seq += 1;
        }

        // Generate text chunk if there's text
        if !mock.text.is_empty() {
            events.push(CapturedEvent {
                event: BackendEvent::StreamToken(mock.text.clone()),
                timestamp: now,
                sequence: seq,
            });
            seq += 1;
        }

        // Generate completion event
        events.push(CapturedEvent {
            event: BackendEvent::OperationEvent(OperationEvent::Completed {
                operation_id: op_id,
                result: Some(mock.text.clone()),
            }),
            timestamp: now,
            sequence: seq,
        });

        CapturedEvents::new(events)
    }

    /// Generate minimal mock events for steps without mock_response
    fn generate_minimal_mock_events(&self, step_name: &str) -> CapturedEvents {
        use crate::cli::ws_client::{BackendEvent, OperationEvent};
        use crate::testing::harness::client::CapturedEvent;

        let now = Instant::now();
        let op_id = format!("mock-{}", step_name);
        let events = vec![
            CapturedEvent {
                event: BackendEvent::OperationEvent(OperationEvent::Started {
                    operation_id: op_id.clone(),
                }),
                timestamp: now,
                sequence: 0,
            },
            CapturedEvent {
                event: BackendEvent::StreamToken("[Mock response - no mock_response defined]".to_string()),
                timestamp: now,
                sequence: 1,
            },
            CapturedEvent {
                event: BackendEvent::OperationEvent(OperationEvent::Completed {
                    operation_id: op_id,
                    result: None,
                }),
                timestamp: now,
                sequence: 2,
            },
        ];

        CapturedEvents::new(events)
    }

    /// Check expected events against captured events
    fn check_expected_events(
        &self,
        events: &CapturedEvents,
        step: &TestStep,
    ) -> crate::testing::harness::assertions::AssertionResult {
        use crate::testing::harness::assertions::AssertionResult;

        // For now, just check that all expected event types are present
        for expected in &step.expect_events {
            let found = events.of_type(&expected.event_type);
            if found.is_empty() {
                return AssertionResult::fail(
                    "expected_events",
                    format!("Expected event '{}' not found", expected.event_type),
                );
            }
        }

        AssertionResult::pass("expected_events", "All expected events received")
    }

    /// Setup phase: create temp directory, files, etc.
    async fn setup(&self, setup: &SetupConfig) -> Result<PathBuf> {
        // Determine project directory
        let project_dir = if let Some(ref path) = setup.project_path {
            PathBuf::from(path)
        } else {
            // Create temp directory
            let temp_dir = std::env::temp_dir().join(format!("mira-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&temp_dir)
                .context("Failed to create temp directory")?;
            temp_dir
        };

        info!("Using project directory: {}", project_dir.display());

        // Create directories
        for dir in &setup.create_dirs {
            let path = project_dir.join(dir);
            std::fs::create_dir_all(&path)
                .with_context(|| format!("Failed to create directory: {}", path.display()))?;
        }

        // Create files
        for file in &setup.create_files {
            let path = project_dir.join(&file.path);

            // Ensure parent directory exists
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            std::fs::write(&path, &file.content)
                .with_context(|| format!("Failed to create file: {}", path.display()))?;

            #[cfg(unix)]
            if file.executable {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&path)?.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&path, perms)?;
            }

            info!("Created file: {}", path.display());
        }

        // Set environment variables
        // SAFETY: This is safe because test scenarios run sequentially
        // and we don't access these env vars from other threads during setup
        for (key, value) in &setup.env_vars {
            unsafe { std::env::set_var(key, value) };
        }

        Ok(project_dir)
    }

    /// Cleanup phase: remove temp files, etc.
    async fn cleanup(&self, cleanup: &CleanupConfig, project_dir: &Path) {
        // Remove specific files
        for file in &cleanup.remove_files {
            let path = project_dir.join(file);
            if let Err(e) = std::fs::remove_file(&path) {
                warn!("Failed to remove file {}: {}", path.display(), e);
            }
        }

        // Remove specific directories
        for dir in &cleanup.remove_dirs {
            let path = project_dir.join(dir);
            if let Err(e) = std::fs::remove_dir_all(&path) {
                warn!("Failed to remove directory {}: {}", path.display(), e);
            }
        }

        // Remove entire project directory
        if cleanup.remove_project {
            if let Err(e) = std::fs::remove_dir_all(project_dir) {
                warn!("Failed to remove project directory: {}", e);
            } else {
                info!("Removed project directory: {}", project_dir.display());
            }
        }
    }

    /// Run multiple scenarios (sequentially or in parallel)
    pub async fn run_scenarios(&self, scenarios: &[TestScenario]) -> Vec<ScenarioResult> {
        if self.config.parallel && !self.config.fail_fast {
            self.run_scenarios_parallel(scenarios).await
        } else {
            self.run_scenarios_sequential(scenarios).await
        }
    }

    /// Run scenarios sequentially
    async fn run_scenarios_sequential(&self, scenarios: &[TestScenario]) -> Vec<ScenarioResult> {
        let mut results = Vec::new();

        for scenario in scenarios {
            let result = self.run_scenario(scenario).await;
            let passed = result.passed;
            results.push(result);

            if !passed && self.config.fail_fast {
                warn!("Stopping due to fail_fast");
                break;
            }
        }

        results
    }

    /// Run scenarios in parallel with concurrency limit
    async fn run_scenarios_parallel(&self, scenarios: &[TestScenario]) -> Vec<ScenarioResult> {
        let concurrency = if self.config.max_parallel == 0 {
            scenarios.len()
        } else {
            self.config.max_parallel
        };

        info!("Running {} scenarios in parallel (max concurrency: {})", scenarios.len(), concurrency);

        // Clone config for sharing across tasks
        let config = Arc::new(self.config.clone());

        // Create futures for all scenarios
        let results: Vec<ScenarioResult> = stream::iter(scenarios.iter().cloned())
            .map(|scenario| {
                let config = Arc::clone(&config);
                async move {
                    let runner = ScenarioRunner { config: (*config).clone() };
                    runner.run_scenario(&scenario).await
                }
            })
            .buffer_unordered(concurrency)
            .collect()
            .await;

        results
    }
}

/// Summary of multiple scenario results
#[derive(Debug)]
pub struct RunSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub total_duration_ms: u64,
}

impl RunSummary {
    pub fn from_results(results: &[ScenarioResult]) -> Self {
        let mut summary = Self {
            total: results.len(),
            passed: 0,
            failed: 0,
            skipped: 0,
            total_duration_ms: 0,
        };

        for result in results {
            summary.total_duration_ms += result.duration_ms;
            if result.passed {
                summary.passed += 1;
            } else {
                summary.failed += 1;
            }
        }

        summary
    }

    pub fn print(&self) {
        println!();
        println!("========================================");
        println!("TEST SUMMARY");
        println!("========================================");
        println!("Total:    {}", self.total);
        println!("Passed:   {} ({}%)", self.passed, if self.total > 0 { self.passed * 100 / self.total } else { 0 });
        println!("Failed:   {}", self.failed);
        println!("Duration: {}ms", self.total_duration_ms);
        println!("========================================");

        if self.failed > 0 {
            println!("RESULT: FAILED");
        } else {
            println!("RESULT: PASSED");
        }
    }
}
