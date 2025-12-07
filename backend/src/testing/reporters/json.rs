// src/testing/reporters/json.rs
// JSON output for test results (machine-readable)

use serde::Serialize;
use crate::testing::scenarios::types::ScenarioResult;
use crate::testing::harness::runner::RunSummary;
use super::Reporter;

/// JSON reporter for machine-readable output
pub struct JsonReporter;

/// JSON output structure
#[derive(Serialize)]
struct JsonReport {
    summary: JsonSummary,
    scenarios: Vec<JsonScenarioResult>,
}

#[derive(Serialize)]
struct JsonSummary {
    total: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    duration_ms: u64,
}

#[derive(Serialize)]
struct JsonScenarioResult {
    name: String,
    passed: bool,
    duration_ms: u64,
    steps: Vec<JsonStepResult>,
    error: Option<String>,
}

#[derive(Serialize)]
struct JsonStepResult {
    name: String,
    passed: bool,
    skipped: bool,
    duration_ms: u64,
    event_count: usize,
    tool_executions: Vec<String>,
    assertions: Vec<JsonAssertionResult>,
    error: Option<String>,
}

#[derive(Serialize)]
struct JsonAssertionResult {
    passed: bool,
    assertion_type: String,
    message: String,
}

impl Reporter for JsonReporter {
    fn report(&self, results: &[ScenarioResult], _verbose: bool) -> String {
        let summary = RunSummary::from_results(results);

        let report = JsonReport {
            summary: JsonSummary {
                total: summary.total,
                passed: summary.passed,
                failed: summary.failed,
                skipped: summary.skipped,
                duration_ms: summary.total_duration_ms,
            },
            scenarios: results.iter().map(|r| JsonScenarioResult {
                name: r.scenario_name.clone(),
                passed: r.passed,
                duration_ms: r.duration_ms,
                steps: r.step_results.iter().map(|s| JsonStepResult {
                    name: s.step_name.clone(),
                    passed: s.passed,
                    skipped: s.skipped,
                    duration_ms: s.duration_ms,
                    event_count: s.event_count,
                    tool_executions: s.tool_executions.clone(),
                    assertions: s.assertion_results.iter().map(|a| JsonAssertionResult {
                        passed: a.passed,
                        assertion_type: a.assertion_type.clone(),
                        message: a.message.clone(),
                    }).collect(),
                    error: s.error.clone(),
                }).collect(),
                error: r.error.clone(),
            }).collect(),
        };

        serde_json::to_string_pretty(&report).unwrap_or_else(|e| {
            format!("{{\"error\": \"Failed to serialize results: {}\"}}", e)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::scenarios::types::StepResult;
    use crate::testing::harness::assertions::AssertionResult;

    fn make_test_result(name: &str, passed: bool) -> ScenarioResult {
        ScenarioResult {
            scenario_name: name.to_string(),
            passed,
            duration_ms: 100,
            step_results: vec![StepResult {
                step_name: "Test step".to_string(),
                passed,
                skipped: false,
                duration_ms: 50,
                event_count: 5,
                tool_executions: vec!["test_tool".to_string()],
                assertion_results: vec![AssertionResult {
                    passed,
                    assertion_type: "test".to_string(),
                    message: "Test assertion".to_string(),
                    details: None,
                }],
                error: None,
            }],
            error: None,
        }
    }

    #[test]
    fn test_json_reporter() {
        let reporter = JsonReporter;
        let results = vec![
            make_test_result("Test 1", true),
            make_test_result("Test 2", false),
        ];
        let output = reporter.report(&results, false);

        // Parse and verify
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["summary"]["total"], 2);
        assert_eq!(parsed["summary"]["passed"], 1);
        assert_eq!(parsed["summary"]["failed"], 1);
        assert_eq!(parsed["scenarios"][0]["name"], "Test 1");
        assert_eq!(parsed["scenarios"][0]["passed"], true);
        assert_eq!(parsed["scenarios"][1]["name"], "Test 2");
        assert_eq!(parsed["scenarios"][1]["passed"], false);
    }

    #[test]
    fn test_json_structure() {
        let reporter = JsonReporter;
        let results = vec![make_test_result("Test", true)];
        let output = reporter.report(&results, false);

        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        // Verify structure
        assert!(parsed.get("summary").is_some());
        assert!(parsed.get("scenarios").is_some());
        assert!(parsed["scenarios"][0].get("steps").is_some());
        assert!(parsed["scenarios"][0]["steps"][0].get("assertions").is_some());
    }
}
