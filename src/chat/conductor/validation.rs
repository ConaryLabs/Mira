//! Tool Validation - JSON schema validation and auto-repair
//!
//! Validates tool calls and attempts to repair common issues
//! before execution. Critical for DeepSeek reliability.

use serde_json::{json, Value};
use std::collections::HashMap;

/// Result of validating a tool call
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Whether the validation passed
    pub valid: bool,

    /// Repaired arguments (if repair was successful)
    pub repaired_args: Option<Value>,

    /// List of issues found
    pub issues: Vec<ValidationIssue>,
}

/// A validation issue
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub severity: IssueSeverity,
    pub field: String,
    pub message: String,
    pub repaired: bool,
}

/// Severity of validation issues
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    Error,   // Must be fixed
    Warning, // Repaired automatically
    Info,    // Informational
}

/// Tool parameter schema for validation
#[derive(Debug, Clone)]
pub struct ParamSchema {
    pub name: String,
    pub param_type: ParamType,
    pub required: bool,
    pub description: Option<String>,
    pub default: Option<Value>,
    pub enum_values: Option<Vec<String>>,
}

/// Parameter types
#[derive(Debug, Clone, PartialEq)]
pub enum ParamType {
    String,
    Integer,
    Number,
    Boolean,
    Array,
    Object,
}

/// Tool schema registry
pub struct ToolSchemas {
    schemas: HashMap<String, Vec<ParamSchema>>,
}

impl Default for ToolSchemas {
    fn default() -> Self {
        let mut schemas = HashMap::new();

        // Read tool
        schemas.insert(
            "Read".into(),
            vec![ParamSchema {
                name: "file_path".into(),
                param_type: ParamType::String,
                required: true,
                description: Some("Absolute path to file".into()),
                default: None,
                enum_values: None,
            }],
        );

        // Edit tool
        schemas.insert(
            "Edit".into(),
            vec![
                ParamSchema {
                    name: "file_path".into(),
                    param_type: ParamType::String,
                    required: true,
                    description: Some("Absolute path to file".into()),
                    default: None,
                    enum_values: None,
                },
                ParamSchema {
                    name: "old_string".into(),
                    param_type: ParamType::String,
                    required: true,
                    description: Some("Text to replace".into()),
                    default: None,
                    enum_values: None,
                },
                ParamSchema {
                    name: "new_string".into(),
                    param_type: ParamType::String,
                    required: true,
                    description: Some("Replacement text".into()),
                    default: None,
                    enum_values: None,
                },
                ParamSchema {
                    name: "replace_all".into(),
                    param_type: ParamType::Boolean,
                    required: false,
                    description: Some("Replace all occurrences".into()),
                    default: Some(json!(false)),
                    enum_values: None,
                },
            ],
        );

        // Write tool
        schemas.insert(
            "Write".into(),
            vec![
                ParamSchema {
                    name: "file_path".into(),
                    param_type: ParamType::String,
                    required: true,
                    description: Some("Absolute path to file".into()),
                    default: None,
                    enum_values: None,
                },
                ParamSchema {
                    name: "content".into(),
                    param_type: ParamType::String,
                    required: true,
                    description: Some("File content".into()),
                    default: None,
                    enum_values: None,
                },
            ],
        );

        // Bash tool
        schemas.insert(
            "Bash".into(),
            vec![
                ParamSchema {
                    name: "command".into(),
                    param_type: ParamType::String,
                    required: true,
                    description: Some("Command to execute".into()),
                    default: None,
                    enum_values: None,
                },
                ParamSchema {
                    name: "timeout".into(),
                    param_type: ParamType::Integer,
                    required: false,
                    description: Some("Timeout in milliseconds".into()),
                    default: Some(json!(120000)),
                    enum_values: None,
                },
            ],
        );

        // Glob tool
        schemas.insert(
            "Glob".into(),
            vec![
                ParamSchema {
                    name: "pattern".into(),
                    param_type: ParamType::String,
                    required: true,
                    description: Some("Glob pattern".into()),
                    default: None,
                    enum_values: None,
                },
                ParamSchema {
                    name: "path".into(),
                    param_type: ParamType::String,
                    required: false,
                    description: Some("Base path".into()),
                    default: None,
                    enum_values: None,
                },
            ],
        );

        // Grep tool
        schemas.insert(
            "Grep".into(),
            vec![
                ParamSchema {
                    name: "pattern".into(),
                    param_type: ParamType::String,
                    required: true,
                    description: Some("Search pattern".into()),
                    default: None,
                    enum_values: None,
                },
                ParamSchema {
                    name: "path".into(),
                    param_type: ParamType::String,
                    required: false,
                    description: Some("Path to search".into()),
                    default: None,
                    enum_values: None,
                },
            ],
        );

        Self { schemas }
    }
}

impl ToolSchemas {
    /// Validate a tool call
    pub fn validate(&self, tool_name: &str, args: &Value) -> ValidationResult {
        let schema = match self.schemas.get(tool_name) {
            Some(s) => s,
            None => {
                // Unknown tool - can't validate
                return ValidationResult {
                    valid: true,
                    repaired_args: None,
                    issues: vec![ValidationIssue {
                        severity: IssueSeverity::Info,
                        field: String::new(),
                        message: format!("Unknown tool '{}', skipping validation", tool_name),
                        repaired: false,
                    }],
                };
            }
        };

        let mut issues = Vec::new();
        let mut repaired = args.clone();
        let mut needs_repair = false;

        // Get args as object
        let obj = match args.as_object() {
            Some(o) => o,
            None => {
                return ValidationResult {
                    valid: false,
                    repaired_args: None,
                    issues: vec![ValidationIssue {
                        severity: IssueSeverity::Error,
                        field: String::new(),
                        message: "Arguments must be a JSON object".into(),
                        repaired: false,
                    }],
                };
            }
        };

        // Check each parameter
        for param in schema {
            let value = obj.get(&param.name);

            // Check required
            if param.required && value.is_none() {
                // Try to repair with default
                if let Some(default) = &param.default {
                    repaired[&param.name] = default.clone();
                    needs_repair = true;
                    issues.push(ValidationIssue {
                        severity: IssueSeverity::Warning,
                        field: param.name.clone(),
                        message: format!("Missing required field, using default: {:?}", default),
                        repaired: true,
                    });
                } else {
                    issues.push(ValidationIssue {
                        severity: IssueSeverity::Error,
                        field: param.name.clone(),
                        message: "Missing required field".into(),
                        repaired: false,
                    });
                }
                continue;
            }

            // Type check
            if let Some(val) = value {
                if let Some(type_issue) = self.check_type(val, &param.param_type, &param.name) {
                    // Try to repair type
                    if let Some(fixed) = self.repair_type(val, &param.param_type) {
                        repaired[&param.name] = fixed;
                        needs_repair = true;
                        issues.push(ValidationIssue {
                            severity: IssueSeverity::Warning,
                            field: param.name.clone(),
                            message: format!("{} (auto-repaired)", type_issue),
                            repaired: true,
                        });
                    } else {
                        issues.push(ValidationIssue {
                            severity: IssueSeverity::Error,
                            field: param.name.clone(),
                            message: type_issue,
                            repaired: false,
                        });
                    }
                }

                // Enum check
                if let Some(ref enum_vals) = param.enum_values {
                    if let Some(s) = val.as_str() {
                        if !enum_vals.contains(&s.to_string()) {
                            // Try fuzzy match
                            if let Some(matched) = fuzzy_match_enum(s, enum_vals) {
                                repaired[&param.name] = json!(matched);
                                needs_repair = true;
                                issues.push(ValidationIssue {
                                    severity: IssueSeverity::Warning,
                                    field: param.name.clone(),
                                    message: format!(
                                        "Invalid enum value '{}', corrected to '{}'",
                                        s, matched
                                    ),
                                    repaired: true,
                                });
                            } else {
                                issues.push(ValidationIssue {
                                    severity: IssueSeverity::Error,
                                    field: param.name.clone(),
                                    message: format!(
                                        "Invalid enum value '{}', expected one of: {:?}",
                                        s, enum_vals
                                    ),
                                    repaired: false,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Check for extra fields (warning only)
        for key in obj.keys() {
            if !schema.iter().any(|p| &p.name == key) {
                issues.push(ValidationIssue {
                    severity: IssueSeverity::Info,
                    field: key.clone(),
                    message: "Unknown field (will be ignored)".into(),
                    repaired: false,
                });
            }
        }

        let has_errors = issues.iter().any(|i| i.severity == IssueSeverity::Error);

        ValidationResult {
            valid: !has_errors,
            repaired_args: if needs_repair && !has_errors {
                Some(repaired)
            } else {
                None
            },
            issues,
        }
    }

    /// Check if value matches expected type
    fn check_type(&self, value: &Value, expected: &ParamType, _field: &str) -> Option<String> {
        match (expected, value) {
            (ParamType::String, Value::String(_)) => None,
            (ParamType::Integer, Value::Number(n)) if n.is_i64() || n.is_u64() => None,
            (ParamType::Number, Value::Number(_)) => None,
            (ParamType::Boolean, Value::Bool(_)) => None,
            (ParamType::Array, Value::Array(_)) => None,
            (ParamType::Object, Value::Object(_)) => None,
            _ => Some(format!(
                "Expected {:?}, got {:?}",
                expected,
                json_type_name(value)
            )),
        }
    }

    /// Try to repair a type mismatch
    fn repair_type(&self, value: &Value, expected: &ParamType) -> Option<Value> {
        match (expected, value) {
            // String from number
            (ParamType::String, Value::Number(n)) => Some(json!(n.to_string())),

            // String from bool
            (ParamType::String, Value::Bool(b)) => Some(json!(b.to_string())),

            // Integer from string
            (ParamType::Integer, Value::String(s)) => {
                s.parse::<i64>().ok().map(|n| json!(n))
            }

            // Integer from float
            (ParamType::Integer, Value::Number(n)) => {
                n.as_f64().map(|f| json!(f as i64))
            }

            // Number from string
            (ParamType::Number, Value::String(s)) => {
                s.parse::<f64>().ok().map(|n| json!(n))
            }

            // Boolean from string
            (ParamType::Boolean, Value::String(s)) => {
                match s.to_lowercase().as_str() {
                    "true" | "yes" | "1" => Some(json!(true)),
                    "false" | "no" | "0" => Some(json!(false)),
                    _ => None,
                }
            }

            // Boolean from number
            (ParamType::Boolean, Value::Number(n)) => {
                n.as_i64().map(|i| json!(i != 0))
            }

            _ => None,
        }
    }

    /// Add or update a tool schema
    pub fn register(&mut self, tool_name: &str, schema: Vec<ParamSchema>) {
        self.schemas.insert(tool_name.into(), schema);
    }
}

/// Get JSON type name
fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Fuzzy match an enum value
fn fuzzy_match_enum(value: &str, valid: &[String]) -> Option<String> {
    let value_lower = value.to_lowercase();

    // Exact match (case-insensitive)
    for v in valid {
        if v.to_lowercase() == value_lower {
            return Some(v.clone());
        }
    }

    // Prefix match
    for v in valid {
        if v.to_lowercase().starts_with(&value_lower) {
            return Some(v.clone());
        }
    }

    // Levenshtein distance (simple implementation)
    let mut best_match = None;
    let mut best_distance = usize::MAX;

    for v in valid {
        let dist = levenshtein(&value_lower, &v.to_lowercase());
        if dist < best_distance && dist <= 2 {
            // Max 2 edits
            best_distance = dist;
            best_match = Some(v.clone());
        }
    }

    best_match
}

/// Simple Levenshtein distance
fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

/// Repair common JSON issues in tool arguments
pub fn repair_json(input: &str) -> Result<Value, String> {
    // First try normal parse
    if let Ok(v) = serde_json::from_str::<Value>(input) {
        return Ok(v);
    }

    let mut fixed = input.to_string();

    // Common repairs:

    // 1. Trailing commas
    fixed = remove_trailing_commas(&fixed);

    // 2. Single quotes to double quotes
    fixed = fixed.replace('\'', "\"");

    // 3. Unquoted keys
    fixed = quote_unquoted_keys(&fixed);

    // 4. Missing closing braces
    let open_braces = fixed.matches('{').count();
    let close_braces = fixed.matches('}').count();
    for _ in 0..(open_braces.saturating_sub(close_braces)) {
        fixed.push('}');
    }

    // 5. Missing closing brackets
    let open_brackets = fixed.matches('[').count();
    let close_brackets = fixed.matches(']').count();
    for _ in 0..(open_brackets.saturating_sub(close_brackets)) {
        fixed.push(']');
    }

    // Try parsing again
    serde_json::from_str(&fixed).map_err(|e| format!("Failed to repair JSON: {}", e))
}

/// Remove trailing commas from JSON
fn remove_trailing_commas(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();

    let mut i = 0;
    while i < len {
        let c = chars[i];

        if c == ',' {
            // Look ahead for closing brace/bracket
            let mut j = i + 1;
            while j < len && chars[j].is_whitespace() {
                j += 1;
            }
            if j < len && (chars[j] == '}' || chars[j] == ']') {
                // Skip the comma
                i += 1;
                continue;
            }
        }

        result.push(c);
        i += 1;
    }

    result
}

/// Quote unquoted keys in JSON
fn quote_unquoted_keys(input: &str) -> String {
    // Simple regex-like replacement
    let mut result = String::with_capacity(input.len() * 2);
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();

    let mut i = 0;
    while i < len {
        let c = chars[i];

        // After { or , look for unquoted key
        if (c == '{' || c == ',') {
            result.push(c);
            i += 1;

            // Skip whitespace
            while i < len && chars[i].is_whitespace() {
                result.push(chars[i]);
                i += 1;
            }

            // Check if we have an unquoted identifier
            if i < len && (chars[i].is_alphabetic() || chars[i] == '_') {
                let start = i;
                while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }

                // Skip whitespace after key
                let key_end = i;
                while i < len && chars[i].is_whitespace() {
                    i += 1;
                }

                // If followed by :, it's a key - quote it
                if i < len && chars[i] == ':' {
                    result.push('"');
                    for c in &chars[start..key_end] {
                        result.push(*c);
                    }
                    result.push('"');

                    // Add whitespace we skipped
                    for c in &chars[key_end..i] {
                        result.push(*c);
                    }
                } else {
                    // Not a key, just copy as-is
                    for c in &chars[start..i] {
                        result.push(*c);
                    }
                }
            }
        } else {
            result.push(c);
            i += 1;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_read_tool() {
        let schemas = ToolSchemas::default();
        let args = json!({
            "file_path": "/home/user/test.rs"
        });

        let result = schemas.validate("Read", &args);
        assert!(result.valid);
        assert!(result.issues.is_empty());
    }

    #[test]
    fn test_missing_required() {
        let schemas = ToolSchemas::default();
        let args = json!({});

        let result = schemas.validate("Read", &args);
        assert!(!result.valid);
        assert!(result.issues.iter().any(|i| i.field == "file_path"));
    }

    #[test]
    fn test_type_repair() {
        let schemas = ToolSchemas::default();
        let args = json!({
            "command": "ls",
            "timeout": "5000"  // String instead of integer
        });

        let result = schemas.validate("Bash", &args);
        assert!(result.valid);
        assert!(result.repaired_args.is_some());

        let repaired = result.repaired_args.unwrap();
        assert_eq!(repaired["timeout"], 5000);
    }

    #[test]
    fn test_repair_json_trailing_comma() {
        let input = r#"{"file_path": "/test",}"#;
        let result = repair_json(input).unwrap();
        assert_eq!(result["file_path"], "/test");
    }

    #[test]
    fn test_repair_json_single_quotes() {
        let input = "{'file_path': '/test'}";
        let result = repair_json(input).unwrap();
        assert_eq!(result["file_path"], "/test");
    }

    #[test]
    fn test_repair_json_unquoted_keys() {
        let input = r#"{file_path: "/test"}"#;
        let result = repair_json(input).unwrap();
        assert_eq!(result["file_path"], "/test");
    }

    #[test]
    fn test_levenshtein() {
        assert_eq!(levenshtein("hello", "hello"), 0);
        assert_eq!(levenshtein("hello", "helo"), 1);
        assert_eq!(levenshtein("hello", "world"), 4);
    }

    #[test]
    fn test_fuzzy_enum() {
        let valid = vec!["low".into(), "medium".into(), "high".into()];

        assert_eq!(fuzzy_match_enum("LOW", &valid), Some("low".into()));
        assert_eq!(fuzzy_match_enum("med", &valid), Some("medium".into()));
        assert_eq!(fuzzy_match_enum("hig", &valid), Some("high".into()));
        assert_eq!(fuzzy_match_enum("xyz", &valid), None);
    }
}
