// src/testing/reporters/console.rs
// Human-readable console output for test results

use crate::testing::scenarios::types::ScenarioResult;
use crate::testing::harness::runner::RunSummary;
use super::Reporter;

/// Console reporter for human-readable output
pub struct ConsoleReporter;

impl Reporter for ConsoleReporter {
    fn report(&self, results: &[ScenarioResult], verbose: bool) -> String {
        let mut output = String::new();

        output.push_str("\nRESULTS\n");
        output.push_str("-------\n");

        for result in results {
            let status = if result.passed { "PASS" } else { "FAIL" };
            output.push_str(&format!(
                "[{}] {} ({}ms)\n",
                status, result.scenario_name, result.duration_ms
            ));

            for step in &result.step_results {
                let step_status = if step.skipped {
                    "SKIP"
                } else if step.passed {
                    "PASS"
                } else {
                    "FAIL"
                };

                output.push_str(&format!("  [{}] {}\n", step_status, step.step_name));

                if !step.tool_executions.is_empty() && verbose {
                    output.push_str(&format!("    Tools: {}\n", step.tool_executions.join(", ")));
                }

                for assertion in &step.assertion_results {
                    if !assertion.passed || verbose {
                        let a_status = if assertion.passed { "PASS" } else { "FAIL" };
                        output.push_str(&format!(
                            "    [{}] {}: {}\n",
                            a_status, assertion.assertion_type, assertion.message
                        ));
                    }
                }

                if let Some(ref error) = step.error {
                    output.push_str(&format!("    Error: {}\n", error));
                }
            }

            if let Some(ref error) = result.error {
                output.push_str(&format!("  Error: {}\n", error));
            }
        }

        // Summary
        let summary = RunSummary::from_results(results);
        output.push_str(&format!(
            "\n================\n\
             SUMMARY\n\
             ================\n\
             Total:   {}\n\
             Passed:  {}\n\
             Failed:  {}\n\
             Skipped: {}\n\
             Duration: {}ms\n",
            summary.total,
            summary.passed,
            summary.failed,
            summary.skipped,
            summary.total_duration_ms
        ));

        if summary.failed > 0 {
            output.push_str("\nFailed scenarios:\n");
            for result in results.iter().filter(|r| !r.passed) {
                output.push_str(&format!("  - {}\n", result.scenario_name));
            }
        }

        output
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
                error: if passed { None } else { Some("Test error".to_string()) },
            }],
            error: None,
        }
    }

    #[test]
    fn test_console_reporter_pass() {
        let reporter = ConsoleReporter;
        let results = vec![make_test_result("Test Scenario", true)];
        let output = reporter.report(&results, false);

        assert!(output.contains("[PASS] Test Scenario"));
        assert!(output.contains("Passed:  1"));
        assert!(output.contains("Failed:  0"));
    }

    #[test]
    fn test_console_reporter_fail() {
        let reporter = ConsoleReporter;
        let results = vec![make_test_result("Failing Scenario", false)];
        let output = reporter.report(&results, false);

        assert!(output.contains("[FAIL] Failing Scenario"));
        assert!(output.contains("Failed:  1"));
        assert!(output.contains("Failed scenarios:"));
    }

    #[test]
    fn test_console_reporter_verbose() {
        let reporter = ConsoleReporter;
        let results = vec![make_test_result("Test Scenario", true)];
        let output = reporter.report(&results, true);

        assert!(output.contains("Tools: test_tool"));
        assert!(output.contains("[PASS] test: Test assertion"));
    }
}
