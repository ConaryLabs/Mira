// src/testing/reporters/junit.rs
// JUnit XML output for CI integration

use crate::testing::scenarios::types::ScenarioResult;
use crate::testing::harness::runner::RunSummary;
use super::Reporter;

/// JUnit XML reporter for CI systems
pub struct JunitReporter;

impl Reporter for JunitReporter {
    fn report(&self, results: &[ScenarioResult], _verbose: bool) -> String {
        let summary = RunSummary::from_results(results);

        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");

        // Test suites wrapper
        xml.push_str(&format!(
            "<testsuites name=\"mira-test\" tests=\"{}\" failures=\"{}\" errors=\"0\" skipped=\"{}\" time=\"{:.3}\">\n",
            summary.total,
            summary.failed,
            summary.skipped,
            summary.total_duration_ms as f64 / 1000.0
        ));

        // Each scenario is a test suite
        for result in results {
            let test_count = result.step_results.len();
            let failure_count = result.step_results.iter().filter(|s| !s.passed && !s.skipped).count();
            let skip_count = result.step_results.iter().filter(|s| s.skipped).count();

            xml.push_str(&format!(
                "  <testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" errors=\"0\" skipped=\"{}\" time=\"{:.3}\">\n",
                escape_xml(&result.scenario_name),
                test_count,
                failure_count,
                skip_count,
                result.duration_ms as f64 / 1000.0
            ));

            // Each step is a test case
            for step in &result.step_results {
                xml.push_str(&format!(
                    "    <testcase name=\"{}\" classname=\"{}\" time=\"{:.3}\"",
                    escape_xml(&step.step_name),
                    escape_xml(&result.scenario_name),
                    step.duration_ms as f64 / 1000.0
                ));

                if step.skipped {
                    xml.push_str(">\n      <skipped/>\n    </testcase>\n");
                } else if !step.passed {
                    xml.push_str(">\n");

                    // Add failure information
                    let failure_message = step.error.as_deref().unwrap_or("Test failed");
                    let failed_assertions: Vec<String> = step.assertion_results
                        .iter()
                        .filter(|a| !a.passed)
                        .map(|a| format!("{}: {}", a.assertion_type, a.message))
                        .collect();

                    let failure_details = if failed_assertions.is_empty() {
                        failure_message.to_string()
                    } else {
                        format!("{}\n\nFailed assertions:\n{}", failure_message, failed_assertions.join("\n"))
                    };

                    xml.push_str(&format!(
                        "      <failure message=\"{}\" type=\"AssertionError\">\n<![CDATA[{}]]>\n      </failure>\n",
                        escape_xml(failure_message),
                        failure_details
                    ));
                    xml.push_str("    </testcase>\n");
                } else {
                    xml.push_str("/>\n");
                }
            }

            // Add scenario-level error if present
            if let Some(ref error) = result.error {
                xml.push_str(&format!(
                    "    <testcase name=\"scenario_setup\" classname=\"{}\" time=\"0\">\n",
                    escape_xml(&result.scenario_name)
                ));
                xml.push_str(&format!(
                    "      <error message=\"{}\" type=\"ScenarioError\">\n<![CDATA[{}]]>\n      </error>\n",
                    escape_xml(error),
                    error
                ));
                xml.push_str("    </testcase>\n");
            }

            xml.push_str("  </testsuite>\n");
        }

        xml.push_str("</testsuites>\n");
        xml
    }
}

/// Escape special XML characters
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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
            duration_ms: 1500,
            step_results: vec![StepResult {
                step_name: "Test step".to_string(),
                passed,
                skipped: false,
                duration_ms: 500,
                event_count: 5,
                tool_executions: vec![],
                assertion_results: vec![AssertionResult {
                    passed,
                    assertion_type: "test".to_string(),
                    message: if passed { "Passed" } else { "Expected true, got false" }.to_string(),
                    details: None,
                }],
                error: if passed { None } else { Some("Assertion failed".to_string()) },
            }],
            error: None,
        }
    }

    #[test]
    fn test_junit_xml_structure() {
        let reporter = JunitReporter;
        let results = vec![make_test_result("Test Suite", true)];
        let output = reporter.report(&results, false);

        assert!(output.starts_with("<?xml version=\"1.0\""));
        assert!(output.contains("<testsuites"));
        assert!(output.contains("<testsuite name=\"Test Suite\""));
        assert!(output.contains("<testcase name=\"Test step\""));
        assert!(output.contains("</testsuites>"));
    }

    #[test]
    fn test_junit_failure() {
        let reporter = JunitReporter;
        let results = vec![make_test_result("Failing Test", false)];
        let output = reporter.report(&results, false);

        assert!(output.contains("<failure"));
        assert!(output.contains("Assertion failed"));
        assert!(output.contains("failures=\"1\""));
    }

    #[test]
    fn test_junit_pass() {
        let reporter = JunitReporter;
        let results = vec![make_test_result("Passing Test", true)];
        let output = reporter.report(&results, false);

        assert!(output.contains("failures=\"0\""));
        assert!(!output.contains("<failure"));
    }

    #[test]
    fn test_junit_xml_escaping() {
        let reporter = JunitReporter;
        let result = ScenarioResult {
            scenario_name: "Test <with> \"special\" & 'chars'".to_string(),
            passed: true,
            duration_ms: 100,
            step_results: vec![],
            error: None,
        };
        let output = reporter.report(&[result], false);

        assert!(output.contains("&lt;with&gt;"));
        assert!(output.contains("&quot;special&quot;"));
        assert!(output.contains("&amp;"));
        assert!(output.contains("&apos;chars&apos;"));
    }

    #[test]
    fn test_junit_time_format() {
        let reporter = JunitReporter;
        let results = vec![make_test_result("Test", true)];
        let output = reporter.report(&results, false);

        // 1500ms should be 1.500 seconds
        assert!(output.contains("time=\"1.500\""));
    }
}
