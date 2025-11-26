// src/build/runner.rs
// Build execution with output capture

use anyhow::{Context, Result};
use chrono::Utc;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{info, warn};

use super::parser::ErrorParser;
use super::tracker::BuildTracker;
use super::types::*;

/// Configuration for build runner
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    /// Maximum output size to capture (bytes)
    pub max_output_size: usize,
    /// Timeout for build commands (seconds)
    pub timeout_seconds: u64,
    /// Whether to parse errors from output
    pub parse_errors: bool,
    /// Whether to store build results
    pub store_results: bool,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            max_output_size: 1024 * 1024, // 1MB
            timeout_seconds: 600,          // 10 minutes
            parse_errors: true,
            store_results: true,
        }
    }
}

/// Build runner executes builds and captures results
pub struct BuildRunner {
    tracker: Arc<BuildTracker>,
    parser: ErrorParser,
    config: RunnerConfig,
}

impl BuildRunner {
    pub fn new(tracker: Arc<BuildTracker>) -> Self {
        Self {
            tracker,
            parser: ErrorParser::new(),
            config: RunnerConfig::default(),
        }
    }

    pub fn with_config(mut self, config: RunnerConfig) -> Self {
        self.config = config;
        self
    }

    /// Run a build command and capture results
    pub async fn run(
        &self,
        project_id: &str,
        command: &str,
        working_dir: &Path,
        operation_id: Option<&str>,
    ) -> Result<BuildResult> {
        info!(
            "Running build command in {}: {}",
            working_dir.display(),
            command
        );

        let started_at = Utc::now();
        let start_instant = Instant::now();

        // Parse command into program and args
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return Err(anyhow::anyhow!("Empty command"));
        }

        let program = parts[0];
        let args = &parts[1..];

        // Create build run record
        let mut build_run = BuildRun::new(project_id.to_string(), command.to_string());
        if let Some(op_id) = operation_id {
            build_run = build_run.with_operation(op_id);
        }
        build_run.started_at = started_at;

        // Execute command
        let mut child = Command::new(program)
            .args(args)
            .current_dir(working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn build process")?;

        // Capture stdout and stderr
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let mut stdout_buffer = String::new();
        let mut stderr_buffer = String::new();

        // Read stdout
        if let Some(stdout) = stdout {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if stdout_buffer.len() < self.config.max_output_size {
                    stdout_buffer.push_str(&line);
                    stdout_buffer.push('\n');
                }
            }
        }

        // Read stderr
        if let Some(stderr) = stderr {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if stderr_buffer.len() < self.config.max_output_size {
                    stderr_buffer.push_str(&line);
                    stderr_buffer.push('\n');
                }
            }
        }

        // Wait for process to complete
        let status = child.wait().await.context("Failed to wait for build process")?;

        let duration = start_instant.elapsed();
        let completed_at = Utc::now();

        // Update build run
        build_run.exit_code = status.code().unwrap_or(-1);
        build_run.duration_ms = duration.as_millis() as i64;
        build_run.completed_at = completed_at;
        build_run.stdout = if stdout_buffer.is_empty() {
            None
        } else {
            Some(stdout_buffer.clone())
        };
        build_run.stderr = if stderr_buffer.is_empty() {
            None
        } else {
            Some(stderr_buffer.clone())
        };

        // Parse errors if enabled
        let mut errors = Vec::new();
        if self.config.parse_errors {
            let combined_output = format!("{}\n{}", stdout_buffer, stderr_buffer);
            let parsed = self.parser.parse(&combined_output, build_run.build_type);

            for parsed_error in parsed {
                let error_hash = BuildError::compute_hash(
                    parsed_error.error_code.as_deref(),
                    &parsed_error.message,
                    parsed_error.file_path.as_deref(),
                    parsed_error.line_number,
                );

                let error = BuildError {
                    id: None,
                    build_run_id: 0, // Will be set after storing build run
                    error_hash,
                    severity: parsed_error.severity,
                    error_code: parsed_error.error_code,
                    message: parsed_error.message,
                    file_path: parsed_error.file_path,
                    line_number: parsed_error.line_number,
                    column_number: parsed_error.column_number,
                    suggestion: parsed_error.suggestion,
                    code_snippet: parsed_error.code_snippet,
                    category: parsed_error.category,
                    first_seen_at: completed_at,
                    last_seen_at: completed_at,
                    occurrence_count: 1,
                    resolved_at: None,
                };

                errors.push(error);
            }
        }

        // Update counts
        build_run.error_count = errors
            .iter()
            .filter(|e| e.severity == ErrorSeverity::Error)
            .count() as i32;
        build_run.warning_count = errors
            .iter()
            .filter(|e| e.severity == ErrorSeverity::Warning)
            .count() as i32;

        info!(
            "Build completed: exit_code={}, errors={}, warnings={}, duration={}ms",
            build_run.exit_code, build_run.error_count, build_run.warning_count, build_run.duration_ms
        );

        // Store results if enabled
        if self.config.store_results {
            match self.tracker.store_build_run(&build_run).await {
                Ok(build_id) => {
                    // Update errors with build_run_id and store them
                    for error in &mut errors {
                        error.build_run_id = build_id;
                    }

                    for error in &errors {
                        if let Err(e) = self.tracker.store_error(error).await {
                            warn!("Failed to store error: {}", e);
                        }
                    }

                    build_run.id = Some(build_id);
                }
                Err(e) => {
                    warn!("Failed to store build run: {}", e);
                }
            }
        }

        Ok(BuildResult::new(build_run, errors))
    }

    /// Run cargo build
    pub async fn cargo_build(
        &self,
        project_id: &str,
        working_dir: &Path,
        release: bool,
        operation_id: Option<&str>,
    ) -> Result<BuildResult> {
        let command = if release {
            "cargo build --release"
        } else {
            "cargo build"
        };
        self.run(project_id, command, working_dir, operation_id).await
    }

    /// Run cargo check
    pub async fn cargo_check(
        &self,
        project_id: &str,
        working_dir: &Path,
        operation_id: Option<&str>,
    ) -> Result<BuildResult> {
        self.run(project_id, "cargo check", working_dir, operation_id).await
    }

    /// Run cargo test
    pub async fn cargo_test(
        &self,
        project_id: &str,
        working_dir: &Path,
        test_name: Option<&str>,
        operation_id: Option<&str>,
    ) -> Result<BuildResult> {
        let command = match test_name {
            Some(name) => format!("cargo test {}", name),
            None => "cargo test".to_string(),
        };
        self.run(project_id, &command, working_dir, operation_id).await
    }

    /// Run cargo clippy
    pub async fn cargo_clippy(
        &self,
        project_id: &str,
        working_dir: &Path,
        operation_id: Option<&str>,
    ) -> Result<BuildResult> {
        self.run(
            project_id,
            "cargo clippy -- -W clippy::all",
            working_dir,
            operation_id,
        )
        .await
    }

    /// Run npm build
    pub async fn npm_build(
        &self,
        project_id: &str,
        working_dir: &Path,
        operation_id: Option<&str>,
    ) -> Result<BuildResult> {
        self.run(project_id, "npm run build", working_dir, operation_id).await
    }

    /// Run npm test
    pub async fn npm_test(
        &self,
        project_id: &str,
        working_dir: &Path,
        operation_id: Option<&str>,
    ) -> Result<BuildResult> {
        self.run(project_id, "npm test", working_dir, operation_id).await
    }

    /// Run pytest
    pub async fn pytest(
        &self,
        project_id: &str,
        working_dir: &Path,
        test_path: Option<&str>,
        operation_id: Option<&str>,
    ) -> Result<BuildResult> {
        let command = match test_path {
            Some(path) => format!("pytest {}", path),
            None => "pytest".to_string(),
        };
        self.run(project_id, &command, working_dir, operation_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn test_build_type_detection() {
        let run = BuildRun::new("test".to_string(), "cargo build".to_string());
        assert_eq!(run.build_type, BuildType::CargoBuild);

        let run = BuildRun::new("test".to_string(), "npm test".to_string());
        assert_eq!(run.build_type, BuildType::NpmTest);
    }
}
