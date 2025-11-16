// src/prompt/types.rs
// Type definitions for prompt building

/// Code element metadata for context formatting
#[derive(Debug, Clone)]
pub struct CodeElement {
    pub element_type: String,
    pub name: String,
    pub start_line: i64,
    pub end_line: i64,
    pub complexity: Option<i64>,
    pub is_async: Option<bool>,
    pub is_public: Option<bool>,
    pub documentation: Option<String>,
}

/// Quality issue metadata for code review
#[derive(Debug, Clone)]
pub struct QualityIssue {
    pub severity: String,
    pub category: String,
    pub description: String,
    pub element_name: Option<String>,
    pub suggestion: Option<String>,
}

/// Error context for code fix operations
#[derive(Debug, Clone)]
pub struct ErrorContext {
    pub error_message: String,
    pub file_path: String,
    pub error_type: String,
    pub error_severity: String,
    pub original_line_count: usize,
}
