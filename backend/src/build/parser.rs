// src/build/parser.rs
// Error parsing for various build tools (cargo, npm, pytest, tsc)

use regex::Regex;
use tracing::debug;

use super::types::*;

/// Parsed error from build output
#[derive(Debug, Clone)]
pub struct ParsedError {
    pub severity: ErrorSeverity,
    pub error_code: Option<String>,
    pub message: String,
    pub file_path: Option<String>,
    pub line_number: Option<i32>,
    pub column_number: Option<i32>,
    pub suggestion: Option<String>,
    pub code_snippet: Option<String>,
    pub category: ErrorCategory,
}

/// Error parser for build output
pub struct ErrorParser {
    // Cargo patterns
    cargo_error_re: Regex,
    cargo_warning_re: Regex,
    cargo_note_re: Regex,
    cargo_help_re: Regex,
    cargo_location_re: Regex,

    // TypeScript/npm patterns
    tsc_error_re: Regex,
    eslint_error_re: Regex,

    // Pytest patterns
    pytest_error_re: Regex,
    pytest_failure_re: Regex,

    // Generic patterns
    generic_error_re: Regex,
}

impl ErrorParser {
    pub fn new() -> Self {
        Self {
            // Cargo: "error[E0308]: mismatched types"
            cargo_error_re: Regex::new(r"^error(?:\[([A-Z]\d+)\])?: (.+)$").unwrap(),
            // Cargo: "warning: unused variable"
            cargo_warning_re: Regex::new(r"^warning(?:\[([A-Z]\d+)\])?: (.+)$").unwrap(),
            // Cargo: "note: ..."
            cargo_note_re: Regex::new(r"^note: (.+)$").unwrap(),
            // Cargo: "help: ..."
            cargo_help_re: Regex::new(r"^help: (.+)$").unwrap(),
            // Cargo location: "  --> src/main.rs:42:10"
            cargo_location_re: Regex::new(r"^\s*--> ([^:]+):(\d+):(\d+)").unwrap(),

            // TypeScript: "src/file.ts(10,5): error TS2304: Cannot find name 'foo'"
            tsc_error_re: Regex::new(
                r"^([^(]+)\((\d+),(\d+)\): (error|warning) (TS\d+): (.+)$",
            )
            .unwrap(),
            // ESLint: "src/file.ts:10:5: error ..."
            eslint_error_re: Regex::new(r"^([^:]+):(\d+):(\d+): (error|warning) (.+)$").unwrap(),

            // Pytest: "FAILED test_file.py::test_name - AssertionError"
            pytest_error_re: Regex::new(r"^FAILED ([^:]+)::([^ ]+) - (.+)$").unwrap(),
            // Pytest: "E       AssertionError: ..."
            pytest_failure_re: Regex::new(r"^E\s+(.+)$").unwrap(),

            // Generic: "error: ..." or "Error: ..."
            generic_error_re: Regex::new(r"(?i)^(error|warning|note):\s*(.+)$").unwrap(),
        }
    }

    /// Parse build output for errors
    pub fn parse(&self, output: &str, build_type: BuildType) -> Vec<ParsedError> {
        match build_type {
            BuildType::CargoBuild
            | BuildType::CargoCheck
            | BuildType::CargoTest
            | BuildType::CargoClippy => self.parse_cargo(output),
            BuildType::NpmBuild | BuildType::NpmTest => self.parse_npm(output),
            BuildType::TypeScriptCompile => self.parse_typescript(output),
            BuildType::Pytest => self.parse_pytest(output),
            BuildType::Mypy => self.parse_mypy(output),
            _ => self.parse_generic(output),
        }
    }

    /// Parse cargo output
    fn parse_cargo(&self, output: &str) -> Vec<ParsedError> {
        let mut errors = Vec::new();
        let lines: Vec<&str> = output.lines().collect();

        let mut i = 0;
        while i < lines.len() {
            let line = lines[i].trim();

            // Check for error
            if let Some(caps) = self.cargo_error_re.captures(line) {
                let error_code = caps.get(1).map(|m| m.as_str().to_string());
                let message = caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();

                // Look for location on next lines
                let (file_path, line_num, col_num) = self.find_cargo_location(&lines, i + 1);

                // Look for help/suggestion
                let suggestion = self.find_cargo_suggestion(&lines, i + 1);

                // Categorize error
                let category = self.categorize_cargo_error(&error_code, &message);

                errors.push(ParsedError {
                    severity: ErrorSeverity::Error,
                    error_code,
                    message,
                    file_path,
                    line_number: line_num,
                    column_number: col_num,
                    suggestion,
                    code_snippet: None,
                    category,
                });
            }
            // Check for warning
            else if let Some(caps) = self.cargo_warning_re.captures(line) {
                let error_code = caps.get(1).map(|m| m.as_str().to_string());
                let message = caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();

                let (file_path, line_num, col_num) = self.find_cargo_location(&lines, i + 1);
                let suggestion = self.find_cargo_suggestion(&lines, i + 1);
                let category = self.categorize_cargo_error(&error_code, &message);

                errors.push(ParsedError {
                    severity: ErrorSeverity::Warning,
                    error_code,
                    message,
                    file_path,
                    line_number: line_num,
                    column_number: col_num,
                    suggestion,
                    code_snippet: None,
                    category,
                });
            }

            i += 1;
        }

        debug!("Parsed {} cargo errors/warnings", errors.len());
        errors
    }

    /// Find location info after an error line
    fn find_cargo_location(&self, lines: &[&str], start: usize) -> (Option<String>, Option<i32>, Option<i32>) {
        for i in start..std::cmp::min(start + 5, lines.len()) {
            if let Some(caps) = self.cargo_location_re.captures(lines[i]) {
                let file = caps.get(1).map(|m| m.as_str().to_string());
                let line = caps.get(2).and_then(|m| m.as_str().parse().ok());
                let col = caps.get(3).and_then(|m| m.as_str().parse().ok());
                return (file, line, col);
            }
        }
        (None, None, None)
    }

    /// Find suggestion/help after an error
    fn find_cargo_suggestion(&self, lines: &[&str], start: usize) -> Option<String> {
        for i in start..std::cmp::min(start + 10, lines.len()) {
            let line = lines[i].trim();
            if let Some(caps) = self.cargo_help_re.captures(line) {
                return caps.get(1).map(|m| m.as_str().to_string());
            }
        }
        None
    }

    /// Categorize cargo error based on code and message
    fn categorize_cargo_error(&self, error_code: &Option<String>, message: &str) -> ErrorCategory {
        let msg_lower = message.to_lowercase();

        if let Some(code) = error_code {
            match code.as_str() {
                // Type errors
                "E0308" | "E0277" | "E0369" | "E0382" | "E0507" => return ErrorCategory::Type,
                // Borrow checker
                "E0502" | "E0499" | "E0503" | "E0505" | "E0506" => return ErrorCategory::Borrow,
                // Lifetimes
                "E0106" | "E0621" | "E0623" | "E0759" => return ErrorCategory::Lifetime,
                // Imports
                "E0432" | "E0433" | "E0412" => return ErrorCategory::Import,
                // Undefined
                "E0425" | "E0599" => return ErrorCategory::Undefined,
                _ => {}
            }
        }

        // Check message content
        if msg_lower.contains("borrow") || msg_lower.contains("moved") {
            ErrorCategory::Borrow
        } else if msg_lower.contains("lifetime") {
            ErrorCategory::Lifetime
        } else if msg_lower.contains("type") || msg_lower.contains("expected") {
            ErrorCategory::Type
        } else if msg_lower.contains("cannot find") || msg_lower.contains("not found") {
            ErrorCategory::Undefined
        } else if msg_lower.contains("unused") || msg_lower.contains("dead code") {
            ErrorCategory::Unused
        } else if msg_lower.contains("syntax") || msg_lower.contains("parse") {
            ErrorCategory::Syntax
        } else if msg_lower.contains("test") {
            ErrorCategory::TestFailure
        } else {
            ErrorCategory::Other
        }
    }

    /// Parse npm/JavaScript output
    fn parse_npm(&self, output: &str) -> Vec<ParsedError> {
        let mut errors = Vec::new();

        for line in output.lines() {
            let line = line.trim();

            // ESLint format
            if let Some(caps) = self.eslint_error_re.captures(line) {
                let file = caps.get(1).map(|m| m.as_str().to_string());
                let line_num = caps.get(2).and_then(|m| m.as_str().parse().ok());
                let col_num = caps.get(3).and_then(|m| m.as_str().parse().ok());
                let severity_str = caps.get(4).map(|m| m.as_str()).unwrap_or("error");
                let message = caps.get(5).map(|m| m.as_str().to_string()).unwrap_or_default();

                errors.push(ParsedError {
                    severity: ErrorSeverity::from_str(severity_str),
                    error_code: None,
                    message,
                    file_path: file,
                    line_number: line_num,
                    column_number: col_num,
                    suggestion: None,
                    code_snippet: None,
                    category: ErrorCategory::Other,
                });
            }
        }

        debug!("Parsed {} npm errors", errors.len());
        errors
    }

    /// Parse TypeScript compiler output
    fn parse_typescript(&self, output: &str) -> Vec<ParsedError> {
        let mut errors = Vec::new();

        for line in output.lines() {
            let line = line.trim();

            if let Some(caps) = self.tsc_error_re.captures(line) {
                let file = caps.get(1).map(|m| m.as_str().to_string());
                let line_num = caps.get(2).and_then(|m| m.as_str().parse().ok());
                let col_num = caps.get(3).and_then(|m| m.as_str().parse().ok());
                let severity_str = caps.get(4).map(|m| m.as_str()).unwrap_or("error");
                let error_code = caps.get(5).map(|m| m.as_str().to_string());
                let message = caps.get(6).map(|m| m.as_str().to_string()).unwrap_or_default();

                let category = self.categorize_typescript_error(&error_code, &message);

                errors.push(ParsedError {
                    severity: ErrorSeverity::from_str(severity_str),
                    error_code,
                    message,
                    file_path: file,
                    line_number: line_num,
                    column_number: col_num,
                    suggestion: None,
                    code_snippet: None,
                    category,
                });
            }
        }

        debug!("Parsed {} TypeScript errors", errors.len());
        errors
    }

    /// Categorize TypeScript error
    fn categorize_typescript_error(&self, error_code: &Option<String>, message: &str) -> ErrorCategory {
        let msg_lower = message.to_lowercase();

        if let Some(code) = error_code {
            match code.as_str() {
                "TS2304" | "TS2552" | "TS2339" => return ErrorCategory::Undefined,
                "TS2307" | "TS2305" => return ErrorCategory::Import,
                "TS2322" | "TS2345" | "TS2741" => return ErrorCategory::Type,
                "TS1005" | "TS1003" | "TS1128" => return ErrorCategory::Syntax,
                _ => {}
            }
        }

        if msg_lower.contains("cannot find") {
            ErrorCategory::Undefined
        } else if msg_lower.contains("type") || msg_lower.contains("assignable") {
            ErrorCategory::Type
        } else if msg_lower.contains("module") || msg_lower.contains("import") {
            ErrorCategory::Import
        } else {
            ErrorCategory::Other
        }
    }

    /// Parse pytest output
    fn parse_pytest(&self, output: &str) -> Vec<ParsedError> {
        let mut errors = Vec::new();

        for line in output.lines() {
            let line = line.trim();

            // FAILED test_file.py::test_name - Error
            if let Some(caps) = self.pytest_error_re.captures(line) {
                let file = caps.get(1).map(|m| m.as_str().to_string());
                let test_name = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                let error_msg = caps.get(3).map(|m| m.as_str()).unwrap_or("");

                let message = format!("Test '{}' failed: {}", test_name, error_msg);
                let category = if error_msg.contains("AssertionError") {
                    ErrorCategory::Assertion
                } else {
                    ErrorCategory::TestFailure
                };

                errors.push(ParsedError {
                    severity: ErrorSeverity::Error,
                    error_code: None,
                    message,
                    file_path: file,
                    line_number: None,
                    column_number: None,
                    suggestion: None,
                    code_snippet: None,
                    category,
                });
            }
        }

        debug!("Parsed {} pytest errors", errors.len());
        errors
    }

    /// Parse mypy output
    fn parse_mypy(&self, output: &str) -> Vec<ParsedError> {
        let mut errors = Vec::new();

        // mypy format: "file.py:10: error: Message"
        let mypy_re = Regex::new(r"^([^:]+):(\d+): (error|warning|note): (.+)$").unwrap();

        for line in output.lines() {
            let line = line.trim();

            if let Some(caps) = mypy_re.captures(line) {
                let file = caps.get(1).map(|m| m.as_str().to_string());
                let line_num = caps.get(2).and_then(|m| m.as_str().parse().ok());
                let severity_str = caps.get(3).map(|m| m.as_str()).unwrap_or("error");
                let message = caps.get(4).map(|m| m.as_str().to_string()).unwrap_or_default();

                errors.push(ParsedError {
                    severity: ErrorSeverity::from_str(severity_str),
                    error_code: None,
                    message,
                    file_path: file,
                    line_number: line_num,
                    column_number: None,
                    suggestion: None,
                    code_snippet: None,
                    category: ErrorCategory::Type,
                });
            }
        }

        debug!("Parsed {} mypy errors", errors.len());
        errors
    }

    /// Parse generic output
    fn parse_generic(&self, output: &str) -> Vec<ParsedError> {
        let mut errors = Vec::new();

        for line in output.lines() {
            let line = line.trim();

            if let Some(caps) = self.generic_error_re.captures(line) {
                let severity_str = caps.get(1).map(|m| m.as_str()).unwrap_or("error");
                let message = caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();

                errors.push(ParsedError {
                    severity: ErrorSeverity::from_str(severity_str),
                    error_code: None,
                    message,
                    file_path: None,
                    line_number: None,
                    column_number: None,
                    suggestion: None,
                    code_snippet: None,
                    category: ErrorCategory::Other,
                });
            }
        }

        debug!("Parsed {} generic errors", errors.len());
        errors
    }
}

impl Default for ErrorParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cargo_error() {
        let parser = ErrorParser::new();
        let output = r#"
error[E0308]: mismatched types
  --> src/main.rs:42:10
   |
42 |     let x: i32 = "hello";
   |            ---   ^^^^^^^ expected `i32`, found `&str`
   |            |
   |            expected due to this
   |
help: try using a conversion method
   |
42 |     let x: i32 = "hello".parse().unwrap();
   |                         +++++++++++++++++
"#;

        let errors = parser.parse(output, BuildType::CargoBuild);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].error_code, Some("E0308".to_string()));
        assert_eq!(errors[0].severity, ErrorSeverity::Error);
        assert_eq!(errors[0].file_path, Some("src/main.rs".to_string()));
        assert_eq!(errors[0].line_number, Some(42));
        assert_eq!(errors[0].category, ErrorCategory::Type);
    }

    #[test]
    fn test_parse_cargo_warning() {
        let parser = ErrorParser::new();
        let output = r#"
warning: unused variable: `x`
  --> src/main.rs:10:9
   |
10 |     let x = 5;
   |         ^ help: if this is intentional, prefix it with an underscore: `_x`
"#;

        let errors = parser.parse(output, BuildType::CargoBuild);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].severity, ErrorSeverity::Warning);
        assert_eq!(errors[0].category, ErrorCategory::Unused);
    }

    #[test]
    fn test_parse_typescript_error() {
        let parser = ErrorParser::new();
        let output = "src/app.ts(10,5): error TS2304: Cannot find name 'foo'.";

        let errors = parser.parse(output, BuildType::TypeScriptCompile);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].error_code, Some("TS2304".to_string()));
        assert_eq!(errors[0].file_path, Some("src/app.ts".to_string()));
        assert_eq!(errors[0].line_number, Some(10));
        assert_eq!(errors[0].column_number, Some(5));
        assert_eq!(errors[0].category, ErrorCategory::Undefined);
    }

    #[test]
    fn test_parse_pytest_failure() {
        let parser = ErrorParser::new();
        let output = "FAILED tests/test_app.py::test_login - AssertionError: expected True";

        let errors = parser.parse(output, BuildType::Pytest);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file_path, Some("tests/test_app.py".to_string()));
        assert!(errors[0].message.contains("test_login"));
        assert_eq!(errors[0].category, ErrorCategory::Assertion);
    }

    #[test]
    fn test_categorize_borrow_error() {
        let parser = ErrorParser::new();
        let category = parser.categorize_cargo_error(&Some("E0502".to_string()), "cannot borrow");
        assert_eq!(category, ErrorCategory::Borrow);
    }
}
