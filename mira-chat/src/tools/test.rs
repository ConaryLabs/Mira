//! Test runner integration
//!
//! Provides test execution with structured output:
//! - run_tests: Execute tests for cargo/python/node projects
//! - Parses output for pass/fail/skip counts
//! - Returns formatted results

use anyhow::Result;
use serde_json::Value;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

/// Test tool implementations
pub struct TestTools<'a> {
    pub cwd: &'a Path,
}

/// Test run result summary
#[derive(Debug)]
pub struct TestResult {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub duration_ms: u64,
    pub output: String,
    pub success: bool,
}

impl<'a> TestTools<'a> {
    /// Run tests with auto-detected or specified runner
    pub async fn run_tests(&self, args: &Value) -> Result<String> {
        let runner = args["runner"].as_str();
        let filter = args["filter"].as_str();
        let verbose = args["verbose"].as_bool().unwrap_or(false);

        // Auto-detect runner if not specified
        let detected_runner = match runner {
            Some(r) => r.to_string(),
            None => self.detect_runner()?,
        };

        let start = Instant::now();
        let result = match detected_runner.as_str() {
            "cargo" => self.run_cargo_test(filter, verbose).await?,
            "pytest" => self.run_pytest(filter, verbose).await?,
            "npm" | "node" => self.run_npm_test(filter, verbose).await?,
            "go" => self.run_go_test(filter, verbose).await?,
            _ => return Ok(format!("Unknown test runner: {}", detected_runner)),
        };

        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(self.format_result(&result, duration_ms))
    }

    /// Detect which test runner to use based on project files
    fn detect_runner(&self) -> Result<String> {
        // Check for Cargo.toml (Rust)
        if self.cwd.join("Cargo.toml").exists() {
            return Ok("cargo".into());
        }
        // Check for pytest/python
        if self.cwd.join("pytest.ini").exists()
            || self.cwd.join("pyproject.toml").exists()
            || self.cwd.join("setup.py").exists()
        {
            return Ok("pytest".into());
        }
        // Check for package.json (Node)
        if self.cwd.join("package.json").exists() {
            return Ok("npm".into());
        }
        // Check for go.mod
        if self.cwd.join("go.mod").exists() {
            return Ok("go".into());
        }

        Ok("cargo".into()) // Default to cargo
    }

    /// Run cargo test
    async fn run_cargo_test(&self, filter: Option<&str>, verbose: bool) -> Result<TestResult> {
        let mut cmd = Command::new("cargo");
        cmd.current_dir(self.cwd);
        cmd.args(["test", "--color=always"]);

        if !verbose {
            cmd.arg("--quiet");
        }

        if let Some(f) = filter {
            cmd.arg(f);
        }

        let output = cmd.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{}\n{}", stdout, stderr);

        // Parse cargo test output
        let (passed, failed, skipped) = self.parse_cargo_output(&combined);

        Ok(TestResult {
            passed,
            failed,
            skipped,
            duration_ms: 0,
            output: self.truncate_output(&combined),
            success: output.status.success(),
        })
    }

    /// Parse cargo test output for counts
    fn parse_cargo_output(&self, output: &str) -> (usize, usize, usize) {
        let mut passed = 0;
        let mut failed = 0;
        let mut ignored = 0;

        for line in output.lines() {
            // Look for summary line like "test result: ok. 5 passed; 0 failed; 1 ignored"
            if line.contains("test result:") {
                let parts: Vec<&str> = line.split(';').collect();
                for part in parts {
                    let trimmed = part.trim();
                    if trimmed.contains("passed") {
                        passed = Self::extract_number(trimmed);
                    } else if trimmed.contains("failed") {
                        failed = Self::extract_number(trimmed);
                    } else if trimmed.contains("ignored") {
                        ignored = Self::extract_number(trimmed);
                    }
                }
            }
        }

        (passed, failed, ignored)
    }

    /// Run pytest
    async fn run_pytest(&self, filter: Option<&str>, verbose: bool) -> Result<TestResult> {
        let mut cmd = Command::new("pytest");
        cmd.current_dir(self.cwd);

        if verbose {
            cmd.arg("-v");
        } else {
            cmd.arg("-q");
        }

        if let Some(f) = filter {
            cmd.arg("-k").arg(f);
        }

        let output = cmd.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{}\n{}", stdout, stderr);

        // Parse pytest output
        let (passed, failed, skipped) = self.parse_pytest_output(&combined);

        Ok(TestResult {
            passed,
            failed,
            skipped,
            duration_ms: 0,
            output: self.truncate_output(&combined),
            success: output.status.success(),
        })
    }

    /// Parse pytest output for counts
    fn parse_pytest_output(&self, output: &str) -> (usize, usize, usize) {
        let mut passed = 0;
        let mut failed = 0;
        let mut skipped = 0;

        for line in output.lines() {
            // Look for summary like "5 passed, 1 failed, 2 skipped"
            if line.contains("passed") || line.contains("failed") || line.contains("skipped") {
                let parts: Vec<&str> = line.split(',').collect();
                for part in parts {
                    let trimmed = part.trim();
                    if trimmed.contains("passed") {
                        passed = Self::extract_number(trimmed);
                    } else if trimmed.contains("failed") {
                        failed = Self::extract_number(trimmed);
                    } else if trimmed.contains("skipped") {
                        skipped = Self::extract_number(trimmed);
                    }
                }
            }
        }

        (passed, failed, skipped)
    }

    /// Run npm test
    async fn run_npm_test(&self, filter: Option<&str>, verbose: bool) -> Result<TestResult> {
        let mut cmd = Command::new("npm");
        cmd.current_dir(self.cwd);
        cmd.args(["test", "--"]);

        if !verbose {
            cmd.arg("--silent");
        }

        if let Some(f) = filter {
            cmd.arg("--grep").arg(f);
        }

        let output = cmd.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{}\n{}", stdout, stderr);

        // Parse npm/jest output
        let (passed, failed, skipped) = self.parse_npm_output(&combined);

        Ok(TestResult {
            passed,
            failed,
            skipped,
            duration_ms: 0,
            output: self.truncate_output(&combined),
            success: output.status.success(),
        })
    }

    /// Parse npm test output
    fn parse_npm_output(&self, output: &str) -> (usize, usize, usize) {
        // Jest output: "Tests: 2 failed, 3 passed, 5 total"
        let mut passed = 0;
        let mut failed = 0;
        let mut skipped = 0;

        for line in output.lines() {
            if line.starts_with("Tests:") || line.contains("Tests:") {
                let parts: Vec<&str> = line.split(',').collect();
                for part in parts {
                    let trimmed = part.trim();
                    if trimmed.contains("passed") {
                        passed = Self::extract_number(trimmed);
                    } else if trimmed.contains("failed") {
                        failed = Self::extract_number(trimmed);
                    } else if trimmed.contains("skipped") {
                        skipped = Self::extract_number(trimmed);
                    }
                }
            }
        }

        (passed, failed, skipped)
    }

    /// Run go test
    async fn run_go_test(&self, filter: Option<&str>, verbose: bool) -> Result<TestResult> {
        let mut cmd = Command::new("go");
        cmd.current_dir(self.cwd);
        cmd.args(["test", "./..."]);

        if verbose {
            cmd.arg("-v");
        }

        if let Some(f) = filter {
            cmd.arg("-run").arg(f);
        }

        let output = cmd.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{}\n{}", stdout, stderr);

        // Parse go test output
        let (passed, failed, skipped) = self.parse_go_output(&combined);

        Ok(TestResult {
            passed,
            failed,
            skipped,
            duration_ms: 0,
            output: self.truncate_output(&combined),
            success: output.status.success(),
        })
    }

    /// Parse go test output
    fn parse_go_output(&self, output: &str) -> (usize, usize, usize) {
        let mut passed = 0;
        let mut failed = 0;
        let skipped = 0;

        for line in output.lines() {
            if line.starts_with("--- PASS:") {
                passed += 1;
            } else if line.starts_with("--- FAIL:") {
                failed += 1;
            } else if line.starts_with("ok ") {
                // Package passed
                passed += 1;
            } else if line.starts_with("FAIL") && !line.starts_with("--- FAIL:") {
                failed += 1;
            }
        }

        (passed, failed, skipped)
    }

    /// Extract number from a string like "5 passed"
    fn extract_number(s: &str) -> usize {
        s.split_whitespace()
            .find_map(|word| word.parse::<usize>().ok())
            .unwrap_or(0)
    }

    /// Truncate output if too long
    fn truncate_output(&self, output: &str) -> String {
        let lines: Vec<&str> = output.lines().collect();
        if lines.len() > 50 {
            let head: String = lines[..20].join("\n");
            let tail: String = lines[lines.len() - 20..].join("\n");
            format!("{}\n... ({} lines omitted) ...\n{}", head, lines.len() - 40, tail)
        } else {
            output.to_string()
        }
    }

    /// Format test result for display
    fn format_result(&self, result: &TestResult, duration_ms: u64) -> String {
        let status = if result.success { "✓ PASS" } else { "✗ FAIL" };
        let summary = format!(
            "{}: {} passed, {} failed, {} skipped ({} ms)\n\n{}",
            status,
            result.passed,
            result.failed,
            result.skipped,
            duration_ms,
            result.output
        );
        summary
    }
}
