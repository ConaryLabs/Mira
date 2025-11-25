// src/build/types.rs
// Core types for build system integration

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Build type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuildType {
    /// Rust cargo build
    CargoBuild,
    /// Rust cargo check (faster, no codegen)
    CargoCheck,
    /// Rust cargo test
    CargoTest,
    /// Rust cargo clippy
    CargoClippy,
    /// Node.js npm/yarn build
    NpmBuild,
    /// Node.js npm/yarn test
    NpmTest,
    /// TypeScript tsc
    TypeScriptCompile,
    /// Python pytest
    Pytest,
    /// Python mypy type checking
    Mypy,
    /// Generic make
    Make,
    /// Generic shell command
    Generic,
}

impl BuildType {
    pub fn as_str(&self) -> &'static str {
        match self {
            BuildType::CargoBuild => "cargo_build",
            BuildType::CargoCheck => "cargo_check",
            BuildType::CargoTest => "cargo_test",
            BuildType::CargoClippy => "cargo_clippy",
            BuildType::NpmBuild => "npm_build",
            BuildType::NpmTest => "npm_test",
            BuildType::TypeScriptCompile => "tsc",
            BuildType::Pytest => "pytest",
            BuildType::Mypy => "mypy",
            BuildType::Make => "make",
            BuildType::Generic => "generic",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "cargo_build" => BuildType::CargoBuild,
            "cargo_check" => BuildType::CargoCheck,
            "cargo_test" => BuildType::CargoTest,
            "cargo_clippy" => BuildType::CargoClippy,
            "npm_build" => BuildType::NpmBuild,
            "npm_test" => BuildType::NpmTest,
            "tsc" => BuildType::TypeScriptCompile,
            "pytest" => BuildType::Pytest,
            "mypy" => BuildType::Mypy,
            "make" => BuildType::Make,
            _ => BuildType::Generic,
        }
    }

    /// Detect build type from command
    pub fn from_command(command: &str) -> Self {
        let cmd = command.to_lowercase();
        if cmd.starts_with("cargo build") {
            BuildType::CargoBuild
        } else if cmd.starts_with("cargo check") {
            BuildType::CargoCheck
        } else if cmd.starts_with("cargo test") {
            BuildType::CargoTest
        } else if cmd.starts_with("cargo clippy") {
            BuildType::CargoClippy
        } else if cmd.contains("npm run build") || cmd.contains("yarn build") {
            BuildType::NpmBuild
        } else if cmd.contains("npm test") || cmd.contains("yarn test") || cmd.contains("vitest") || cmd.contains("jest") {
            BuildType::NpmTest
        } else if cmd.starts_with("tsc") {
            BuildType::TypeScriptCompile
        } else if cmd.starts_with("pytest") || cmd.contains("python -m pytest") {
            BuildType::Pytest
        } else if cmd.starts_with("mypy") {
            BuildType::Mypy
        } else if cmd.starts_with("make") {
            BuildType::Make
        } else {
            BuildType::Generic
        }
    }
}

/// Error severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorSeverity {
    Error,
    Warning,
    Note,
    Help,
    Info,
}

impl ErrorSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorSeverity::Error => "error",
            ErrorSeverity::Warning => "warning",
            ErrorSeverity::Note => "note",
            ErrorSeverity::Help => "help",
            ErrorSeverity::Info => "info",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "error" => ErrorSeverity::Error,
            "warning" | "warn" => ErrorSeverity::Warning,
            "note" => ErrorSeverity::Note,
            "help" | "hint" => ErrorSeverity::Help,
            _ => ErrorSeverity::Info,
        }
    }
}

/// Error category for grouping similar errors
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCategory {
    /// Type system errors
    Type,
    /// Borrow checker errors
    Borrow,
    /// Lifetime errors
    Lifetime,
    /// Syntax errors
    Syntax,
    /// Import/module resolution errors
    Import,
    /// Undefined variable/function
    Undefined,
    /// Unused code warnings
    Unused,
    /// Test failures
    TestFailure,
    /// Assertion failures
    Assertion,
    /// Runtime errors
    Runtime,
    /// Configuration errors
    Config,
    /// Dependency errors
    Dependency,
    /// Generic/unknown
    Other,
}

impl ErrorCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCategory::Type => "type",
            ErrorCategory::Borrow => "borrow",
            ErrorCategory::Lifetime => "lifetime",
            ErrorCategory::Syntax => "syntax",
            ErrorCategory::Import => "import",
            ErrorCategory::Undefined => "undefined",
            ErrorCategory::Unused => "unused",
            ErrorCategory::TestFailure => "test_failure",
            ErrorCategory::Assertion => "assertion",
            ErrorCategory::Runtime => "runtime",
            ErrorCategory::Config => "config",
            ErrorCategory::Dependency => "dependency",
            ErrorCategory::Other => "other",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "type" => ErrorCategory::Type,
            "borrow" => ErrorCategory::Borrow,
            "lifetime" => ErrorCategory::Lifetime,
            "syntax" => ErrorCategory::Syntax,
            "import" => ErrorCategory::Import,
            "undefined" => ErrorCategory::Undefined,
            "unused" => ErrorCategory::Unused,
            "test_failure" => ErrorCategory::TestFailure,
            "assertion" => ErrorCategory::Assertion,
            "runtime" => ErrorCategory::Runtime,
            "config" => ErrorCategory::Config,
            "dependency" => ErrorCategory::Dependency,
            _ => ErrorCategory::Other,
        }
    }
}

/// Resolution type for how an error was fixed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolutionType {
    /// Fixed by code change
    CodeChange,
    /// Fixed by configuration change
    ConfigChange,
    /// Fixed by dependency update
    DependencyUpdate,
    /// Fixed by reverting changes
    Revert,
    /// Marked as won't fix
    WontFix,
    /// Auto-resolved (e.g., by subsequent successful build)
    AutoResolved,
}

impl ResolutionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResolutionType::CodeChange => "code_change",
            ResolutionType::ConfigChange => "config_change",
            ResolutionType::DependencyUpdate => "dependency_update",
            ResolutionType::Revert => "revert",
            ResolutionType::WontFix => "wont_fix",
            ResolutionType::AutoResolved => "auto_resolved",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "code_change" => ResolutionType::CodeChange,
            "config_change" => ResolutionType::ConfigChange,
            "dependency_update" => ResolutionType::DependencyUpdate,
            "revert" => ResolutionType::Revert,
            "wont_fix" => ResolutionType::WontFix,
            "auto_resolved" => ResolutionType::AutoResolved,
            _ => ResolutionType::CodeChange,
        }
    }
}

/// Build run record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildRun {
    pub id: Option<i64>,
    pub project_id: String,
    pub operation_id: Option<String>,
    pub build_type: BuildType,
    pub command: String,
    pub exit_code: i32,
    pub duration_ms: i64,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub error_count: i32,
    pub warning_count: i32,
    pub triggered_by: Option<String>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}

impl BuildRun {
    pub fn new(project_id: String, command: String) -> Self {
        let now = Utc::now();
        Self {
            id: None,
            project_id,
            operation_id: None,
            build_type: BuildType::from_command(&command),
            command,
            exit_code: 0,
            duration_ms: 0,
            started_at: now,
            completed_at: now,
            error_count: 0,
            warning_count: 0,
            triggered_by: None,
            stdout: None,
            stderr: None,
        }
    }

    pub fn is_success(&self) -> bool {
        self.exit_code == 0
    }

    pub fn with_operation(mut self, operation_id: &str) -> Self {
        self.operation_id = Some(operation_id.to_string());
        self
    }
}

/// Build error record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildError {
    pub id: Option<i64>,
    pub build_run_id: i64,
    pub error_hash: String,
    pub severity: ErrorSeverity,
    pub error_code: Option<String>,
    pub message: String,
    pub file_path: Option<String>,
    pub line_number: Option<i32>,
    pub column_number: Option<i32>,
    pub suggestion: Option<String>,
    pub code_snippet: Option<String>,
    pub category: ErrorCategory,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub occurrence_count: i32,
    pub resolved_at: Option<DateTime<Utc>>,
}

impl BuildError {
    /// Compute hash for deduplication
    pub fn compute_hash(
        error_code: Option<&str>,
        message: &str,
        file_path: Option<&str>,
        line_number: Option<i32>,
    ) -> String {
        use sha2::{Digest, Sha256};

        // Normalize message by removing specific numbers/paths that might vary
        let normalized_message = normalize_error_message(message);

        let mut hasher = Sha256::new();
        if let Some(code) = error_code {
            hasher.update(code.as_bytes());
        }
        hasher.update(normalized_message.as_bytes());
        if let Some(path) = file_path {
            // Use just the filename, not full path
            let filename = std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path);
            hasher.update(filename.as_bytes());
        }
        // Don't include line number in hash - same error at different lines is the same error
        format!("{:x}", hasher.finalize())
    }
}

/// Error resolution record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResolution {
    pub id: Option<i64>,
    pub error_hash: String,
    pub resolution_type: ResolutionType,
    pub files_changed: Vec<String>,
    pub commit_hash: Option<String>,
    pub resolution_time_ms: Option<i64>,
    pub resolved_at: DateTime<Utc>,
    pub notes: Option<String>,
}

/// Build context injection record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildContextInjection {
    pub id: Option<i64>,
    pub operation_id: String,
    pub build_run_id: i64,
    pub error_ids: Vec<i64>,
    pub injected_at: DateTime<Utc>,
}

/// Build result summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildResult {
    pub run: BuildRun,
    pub errors: Vec<BuildError>,
    pub success: bool,
}

impl BuildResult {
    pub fn new(run: BuildRun, errors: Vec<BuildError>) -> Self {
        let success = run.is_success();
        Self { run, errors, success }
    }
}

/// Statistics for a project's builds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildStats {
    pub project_id: String,
    pub total_builds: i64,
    pub successful_builds: i64,
    pub failed_builds: i64,
    pub success_rate: f64,
    pub total_errors: i64,
    pub resolved_errors: i64,
    pub unresolved_errors: i64,
    pub average_duration_ms: f64,
    pub most_common_errors: Vec<(String, i64)>,
}

/// Normalize error message for better deduplication
fn normalize_error_message(message: &str) -> String {
    let mut normalized = message.to_string();

    // Remove line/column numbers from message
    let line_col_re = regex::Regex::new(r":\d+:\d+").unwrap();
    normalized = line_col_re.replace_all(&normalized, ":N:N").to_string();

    // Normalize specific type names that might vary
    let type_re = regex::Regex::new(r"`[^`]+`").unwrap();
    normalized = type_re.replace_all(&normalized, "`T`").to_string();

    // Normalize file paths
    let path_re = regex::Regex::new(r"(/[a-zA-Z0-9_\-./]+)+").unwrap();
    normalized = path_re.replace_all(&normalized, "/PATH").to_string();

    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_type_from_command() {
        assert_eq!(BuildType::from_command("cargo build"), BuildType::CargoBuild);
        assert_eq!(BuildType::from_command("cargo test"), BuildType::CargoTest);
        assert_eq!(BuildType::from_command("cargo check --all"), BuildType::CargoCheck);
        assert_eq!(BuildType::from_command("npm run build"), BuildType::NpmBuild);
        assert_eq!(BuildType::from_command("pytest tests/"), BuildType::Pytest);
        assert_eq!(BuildType::from_command("unknown"), BuildType::Generic);
    }

    #[test]
    fn test_error_hash_deduplication() {
        let hash1 = BuildError::compute_hash(
            Some("E0308"),
            "mismatched types",
            Some("/src/main.rs"),
            Some(42),
        );

        let hash2 = BuildError::compute_hash(
            Some("E0308"),
            "mismatched types",
            Some("/other/path/main.rs"),
            Some(100),
        );

        // Same error code + message + filename = same hash
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_normalize_error_message() {
        let msg1 = "error at /src/main.rs:42:10";
        let msg2 = "error at /other/main.rs:100:5";

        let norm1 = normalize_error_message(msg1);
        let norm2 = normalize_error_message(msg2);

        assert_eq!(norm1, norm2);
    }
}
