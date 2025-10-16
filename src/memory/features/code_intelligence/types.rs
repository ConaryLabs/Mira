// src/memory/features/code_intelligence/types.rs
use serde::{Serialize, Deserialize};
use anyhow::Result;
use std::future::Future;

/// A single code element extracted from source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeElement {
    pub element_type: String,    // 'function', 'struct', 'enum', etc.
    pub name: String,
    pub full_path: String,       // 'module::path::element_name'
    pub visibility: String,      // 'public', 'private', 'protected'
    pub start_line: i64,         // Changed from u32 - matches SQLite INTEGER
    pub end_line: i64,           // Changed from u32 - matches SQLite INTEGER
    pub content: String,         // Full source code
    pub signature_hash: String,  // For change detection
    pub complexity_score: i64,   // Changed from u32 - matches SQLite INTEGER
    pub is_test: bool,
    pub is_async: bool,
    pub documentation: Option<String>,
    pub metadata: Option<String>, // JSON for language-specific data
}

/// Quality issue found in code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityIssue {
    pub issue_type: String,      // 'complexity', 'duplication', etc.
    pub severity: String,        // 'info', 'low', 'medium', 'high', 'critical'
    pub title: String,
    pub description: String,
    pub suggested_fix: Option<String>,
    pub fix_confidence: f64,
    pub is_auto_fixable: bool,
}

/// External dependency (import/use statement)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalDependency {
    pub import_path: String,      // 'std::collections::HashMap'
    pub imported_symbols: Vec<String>, // ["HashMap", "BTreeMap"]
    pub dependency_type: String,  // 'crate', 'npm_package', 'local_import'
}

/// Complete analysis result for a file
#[derive(Debug, Clone)]
pub struct FileAnalysis {
    pub elements: Vec<CodeElement>,
    pub dependencies: Vec<ExternalDependency>,
    pub quality_issues: Vec<QualityIssue>,
    pub complexity_score: i64,   // Changed from u32 - matches SQLite INTEGER
    pub test_count: i64,         // Changed from u32 - matches SQLite INTEGER
    pub doc_coverage: f64,
    // REMOVED: websocket_calls (Phase 1 - WebSocket tracking deleted)
}

/// Result of analyzing a file
#[derive(Debug)]
pub struct FileAnalysisResult {
    pub file_id: i64,
    pub language: String,
    pub elements_count: usize,
    pub complexity_score: i64,   // Changed from u32 - matches SQLite INTEGER
    pub quality_issues_count: usize,
    pub test_coverage: f64,
    pub doc_coverage: f64,
}

/// Complete context for a file
#[derive(Debug)]
pub struct FileContext {
    pub elements: Vec<CodeElement>,
    pub quality_issues: Vec<QualityIssue>,
}

/// Language parser trait - extensible for multiple languages
pub trait LanguageParser: Send + Sync {
    /// Parse a file and return analysis
    /// Returns a Send future to ensure thread-safety in async contexts
    fn parse_file(&self, content: &str, file_path: &str) -> impl Future<Output = Result<FileAnalysis>> + Send;
    
    /// Check if this parser can handle the content
    fn can_parse(&self, content: &str, file_path: Option<&str>) -> bool;
    
    /// Get language identifier
    fn language(&self) -> &'static str;
}
